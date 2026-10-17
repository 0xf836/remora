#!/usr/bin/env python3
"""Unprivileged owned-loopback US2 qualification. Never changes host networking."""

import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
from benchmark import install_termination_handler
import secrets
import select
import socket
import subprocess
import tempfile
import time
import tomllib

from benchmark import ROOT, Owned, resource, sha, wait_udp_bound, write


def packet(kind, index, sequence):
    header = kind + index.to_bytes(2, "big") + sequence.to_bytes(8, "big")
    return header + hashlib.sha256(header).digest() + bytes(85)


def udp():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("127.0.0.1", 0))
    s.setblocking(False)
    return s


class Lab:
    def __init__(self, root, logs, binary, scenario):
        self.root, self.logs, self.binary = root, logs, binary
        self.processes, self.senders, self.targets, self.extra = [], [], [], []
        self.relays = {}
        self.cross = self.corrupt = self.generator_errors = 0
        self.forward = self.reverse = 0
        self.samples = []
        self.sockets = []
        self.scenario = scenario
        subprocess.run(
            [str(ROOT / "scripts/validate/fixtures.sh"), "--output", str(root)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        self.targets = [udp(), udp()]
        self.sockets.extend(self.targets)
        ports = json.loads((root / "ports.json").read_text())
        self.server_address = ("127.0.0.1", ports["proxy"])
        self.idle = 500 if scenario == "churn" else 2000
        limits = f"\n[limits]\nidle_session_ms = {self.idle}\n"
        if scenario == "overload":
            limits += "max_connections = 4\nmax_sessions = 33\nmax_sessions_per_identity = 17\nmax_sessions_per_connection = 32\nmax_pending_setup = 16\nmax_relay_sockets = 33\nsession_queue_packets = 2\nquiche_dgram_packets = 64\n"
        tokens = [secrets.token_hex(32), secrets.token_hex(32)]
        credentials = ""
        for i, token in enumerate(tokens):
            credentials += f'[[identities]]\nid = "concurrency-{i}"\ntoken_sha256 = "{hashlib.sha256(token.encode()).hexdigest()}"\n'
            for target in self.targets:
                credentials += f'[[identities.targets]]\nhost = "127.0.0.1"\nport = {target.getsockname()[1]}\naddresses = ["127.0.0.1/32"]\n'
        (root / "credentials.toml").write_text(credentials)
        (root / "server.toml").write_text((root / "server.toml").read_text() + limits)
        self.limits = tomllib.loads((root / "server.toml").read_text()).get(
            "limits", {}
        )
        self.processes.append(
            Owned(
                [str(binary), "server", "--config", str(root / "server.toml")],
                logs / "server",
            )
        )
        wait_udp_bound(self.processes[0], f'127.0.0.1:{ports["proxy"]}')
        base = (root / "client.toml").read_text()
        self.listens = []
        for i in range(4):
            chosen = udp()
            port = chosen.getsockname()[1]
            chosen.close()
            text = (
                base.replace(f'127.0.0.1:{ports["client"]}', f"127.0.0.1:{port}")
                .replace(
                    f'target_port = {ports["target"]}',
                    f"target_port = {self.targets[i // 2].getsockname()[1]}",
                )
                .replace("client.token", f"client-{i}.token")
            )
            cfg = root / f"client-{i}.toml"
            cfg.write_text(text + limits)
            secret = root / f"client-{i}.token"
            secret.write_text(tokens[i % 2])
            secret.chmod(0o600)
            self.listens.append(("127.0.0.1", port))
            self.processes.append(
                Owned(
                    [str(binary), "client", "--config", str(cfg)], logs / f"client-{i}"
                )
            )
            wait_udp_bound(self.processes[-1], f"127.0.0.1:{port}")
        write(
            logs / "configs.json",
            {
                "server": tomllib.loads((root / "server.toml").read_text()),
                "clients": [
                    tomllib.loads((root / f"client-{i}.toml").read_text())
                    for i in range(4)
                ],
            },
        )
        self.expected = {}
        self.received = set()
        self.pending = {}
        self.rtts = [[] for _ in range(32)]
        self.offered = [0] * 32
        self.ontime = [0] * 32
        self.reverse_offered = [0] * 32
        self.reverse_ontime = [0] * 32
        self.last_sample = 0
        self.max_fds = [0] * 5
        self.fd_floor = [
            len(list(Path(f"/proc/{p.p.pid}/fd").iterdir())) for p in self.processes
        ]
        for _ in range(32):
            s = udp()
            self.senders.append(s)
            self.sockets.append(s)

    def close(self):
        for s in self.sockets:
            s.close()
        for p in reversed(self.processes):
            p.stop()

    def send(self, s, payload, to):
        try:
            n = s.sendto(payload, to)
            if n != len(payload):
                self.generator_errors += 1
        except (BlockingIOError, OSError):
            self.generator_errors += 1

    def exchange(self, deadline):
        while time.monotonic() < deadline:
            ready, _, _ = select.select(
                self.targets + self.senders,
                [],
                [],
                min(0.01, max(0, deadline - time.monotonic())),
            )
            for s in ready:
                for _ in range(64):
                    try:
                        data, peer = s.recvfrom(65536)
                    except BlockingIOError:
                        break
                    if len(data) != 128 or data[:1] not in (b"P", b"R", b"L"):
                        self.corrupt += 1
                        raise AssertionError("unexpected application payload")
                    kind, idx, seq = (
                        data[:1],
                        int.from_bytes(data[1:3], "big"),
                        int.from_bytes(data[3:11], "big"),
                    )
                    if data != packet(kind, idx, seq) or idx >= 32:
                        self.corrupt += 1
                        raise AssertionError("payload integrity failed")
                    if s in self.targets:
                        if s is not self.targets[idx // 16]:
                            self.cross += 1
                            raise AssertionError("cross-destination delivery")
                        if kind == b"L":
                            self.forward += 1
                            self.send(s, data, peer)
                            continue
                        old = self.relays.get(idx)
                        if old and old != peer and self.scenario != "churn":
                            # Recreated/expired association is observable; never reroute retained data.
                            self.relays.pop(idx, None)
                        self.relays[idx] = peer
                        if len(set(self.relays.values())) != len(self.relays):
                            self.cross += 1
                            raise AssertionError("shared active relay tuple")
                        self.forward += 1
                        self.send(s, data, peer)
                    else:
                        if s is not self.senders[idx] or peer != self.listens[idx // 8]:
                            self.cross += 1
                            raise AssertionError("cross-client/listener delivery")
                        self.reverse += 1
                        key = kind, idx, seq
                        if key in self.pending:
                            started = self.pending.pop(key)
                            rtt = time.monotonic() - started
                            self.received.add(key)
                            if seq != 2**64 - 1:
                                self.rtts[idx].append(rtt)
                                if kind == b"P":
                                    self.ontime[idx] += int(rtt <= 1)
                                elif kind == b"R":
                                    self.reverse_ontime[idx] += int(rtt <= 1)
            if self.generator_errors or self.cross or self.corrupt:
                raise AssertionError("traffic generator or isolation failure")

    def ready(self, indices=range(32)):
        warmed_indices = []
        for idx in indices:
            end = time.monotonic() + 5
            key = b"P", idx, 2**64 - 1
            self.received.discard(key)
            while key not in self.received:
                if time.monotonic() >= end:
                    raise TimeoutError(f"session {idx} setup")
                for prior in warmed_indices:
                    self.send(
                        self.senders[prior],
                        packet(b"P", prior, 2**64 - 1),
                        self.listens[prior // 8],
                    )
                self.pending[key] = time.monotonic()
                self.send(self.senders[idx], packet(*key), self.listens[idx // 8])
                self.exchange(time.monotonic() + 0.02)
            warmed_indices.append(idx)
        # No echo peer sharing among the 32 simultaneously warm associations.
        if len(list(indices)) == 32 and len(set(self.relays.values())) != 32:
            raise AssertionError("32 distinct relays required")

    def sample(self):
        now = time.monotonic()
        rows = []
        for index, p in enumerate(self.processes):
            if p.exited():
                raise AssertionError("role exited")
            r = resource(p.p)
            fds = list(Path(f"/proc/{p.p.pid}/fd").iterdir())
            self.max_fds[index] = max(self.max_fds[index], len(fds))
            inodes = set()
            for fd in fds:
                try:
                    link = os.readlink(fd)
                except FileNotFoundError:
                    continue
                if link.startswith("socket:["):
                    inodes.add(link[8:-1])
            rx = tx = sockets = 0
            for line in Path(f"/proc/{p.p.pid}/net/udp").read_text().splitlines()[1:]:
                fields = line.split()
                if fields[9] in inodes:
                    sockets += 1
                    t, v = fields[4].split(":")
                    tx += int(t, 16)
                    rx += int(v, 16)
            r.update(
                fd_count=len(fds),
                udp_sockets=sockets,
                kernel_tx_queue_bytes=tx,
                kernel_rx_queue_bytes=rx,
            )
            rows.append(r)
        self.samples.append({"monotonic": now, "roles": rows})
        return rows

    def check_status(self):
        statuses = []
        for i, path in enumerate(
            [self.logs / "server.stdout.ndjson"]
            + [self.logs / f"client-{n}.stdout.ndjson" for n in range(4)]
        ):
            records = [
                json.loads(line)
                for line in path.read_text().splitlines()
                if line.endswith("}")
            ]
            if not records:
                raise AssertionError("missing resource snapshots")
            for record in records:
                r = record.get("resources", {})
                bounds = dict(connections=64, sessions=128, setup=16, relay_fds=256)
                for k in bounds:
                    maximum = self.limits.get(
                        (
                            "max_pending_setup"
                            if k == "setup"
                            else "max_relay_sockets" if k == "relay_fds" else "max_" + k
                        ),
                        bounds[k],
                    )
                    if r[k] > maximum or r[k + "_peak"] > maximum:
                        raise AssertionError("admission ceiling exceeded")
                sessions = r["sessions"]
                if r["queued_packets"] > sessions * 2 * self.limits.get(
                    "session_queue_packets", 32
                ) or r["queued_bytes"] > sessions * self.limits.get(
                    "session_queue_bytes", 65536
                ):
                    raise AssertionError("queue ceiling exceeded")
                if r["native_send_bytes"] + r["native_receive_bytes"] > r[
                    "connections"
                ] * self.limits.get("connection_dgram_bytes", 262144):
                    raise AssertionError("native queue ceiling exceeded")
                for k, maximum in [
                    ("application_retained_bytes", 8388608),
                    ("ciphertext_retained_bytes", 4194304),
                    ("scratch_retained_bytes", 2097152),
                ]:
                    if r[k] > maximum:
                        raise AssertionError("pool ceiling exceeded")
                if record.get("status_write_failures", 0) or record.get(
                    "counter_overflow", False
                ):
                    raise AssertionError("diagnostic output or arithmetic failed")
                if record.get("coverage_lost", 0) and self.scenario != "overload":
                    raise AssertionError("diagnostic coverage lost")
            statuses.append(records[-1])
        return statuses

    def run(self, seconds, cycles):
        self.ready()
        warmed = self.sample()
        start = time.monotonic()
        if self.scenario == "churn":
            # Idle retention is intentionally 500ms solely to accelerate closed-resource tests.
            self.exchange(time.monotonic() + 0.7)
            cycle = 0
            while cycle < cycles:
                indices = list(range(min(32, cycles - cycle)))
                self.relays.clear()
                for idx in range(32):
                    self.senders[idx].close()
                    s = udp()
                    self.senders[idx] = s
                    self.sockets.append(s)
                self.ready(indices)
                cycle += len(indices)
                self.exchange(time.monotonic() + 0.7)
                self.sample()
            closed_at = time.monotonic()
            while True:
                self.exchange(time.monotonic() + 0.1)
                samples = self.sample()
                status = self.check_status()
                if all(
                    s["resources"]["sessions"] == 0
                    and s["resources"]["setup"] == 0
                    and s["resources"]["relay_fds"] == 0
                    for s in status
                ):
                    break
                if time.monotonic() - closed_at > 60:
                    raise AssertionError("closed-resource reclamation exceeded 60s")
            growth = [r["rss_bytes"] - w["rss_bytes"] for r, w in zip(samples, warmed)]
            if any(g > 10 * 1024 * 1024 for g in growth):
                raise AssertionError(
                    "churn RSS growth exceeds unchanged 10 MiB criterion"
                )
            # The server has 32 warmed relay FDs; all must be gone after cleanup.
            if samples[0]["fd_count"] > warmed[0]["fd_count"] - 32:
                raise AssertionError("leaked relay FDs")
            return dict(
                diagnostic_retention_ceiling_bytes=2 * 1024 * 1024,
                kernel_memory_accounting="per-role owned UDP inode tx/rx queue allocations; requested/effective buffer ceilings in snapshots, separate from RSS",
                cycles=cycle,
                settled_seconds=time.monotonic() - closed_at,
                rss_growth_bytes=growth,
                resources=status,
            )
        flood = udp()
        self.sockets.append(flood)
        if self.scenario == "overload":
            self.extra = [udp() for _ in range(64)]
            self.sockets.extend(self.extra)
        load = udp()
        self.sockets.append(load)
        if self.scenario == "overload":
            for _ in range(10):
                self.send(load, packet(b"L", 0, 0), self.listens[0])
                self.exchange(time.monotonic() + 0.02)
        start = time.monotonic()
        seq = 0
        next_tick = start
        next_flood = start
        max_tick_lateness = 0
        while time.monotonic() - start < seconds:
            now = time.monotonic()
            if now >= next_tick:
                max_tick_lateness = max(max_tick_lateness, now - next_tick)
                for idx in range(32):
                    # Exercise individual flow expiry/recreation while others continue.
                    if self.scenario == "isolation" and 2 <= seq < 5 and idx == 0:
                        continue
                    key = b"P", idx, seq
                    self.pending[key] = time.monotonic()
                    self.offered[idx] += 1
                    self.send(self.senders[idx], packet(*key), self.listens[idx // 8])
                    if idx in self.relays:
                        key = b"R", idx, seq
                        self.pending[key] = time.monotonic()
                        self.reverse_offered[idx] += 1
                        self.send(
                            self.targets[idx // 16], packet(*key), self.relays[idx]
                        )
                seq += 1
                next_tick += 1
                self.sample()
            if self.scenario == "overload" and now >= next_flood:
                # ~5 Mbit/s payload on loopback: configured queues, never link saturation.
                for n in range(256):
                    self.send(load, packet(b"L", 0, n), self.listens[0])
                for idx, s in enumerate(self.extra):
                    self.send(s, packet(b"L", idx % 32, seq), self.listens[idx % 4])
                for _ in range(64):
                    self.send(flood, b"\xff" * 48, self.server_address)
                next_flood = now + 0.05
            self.exchange(min(next_tick, time.monotonic() + 0.01))
            # Bound generator bookkeeping for deliberately dropped probes/recreated flows.
            self.pending = {
                k: t for k, t in self.pending.items() if time.monotonic() - t < 2
            }
        self.exchange(time.monotonic() + 1)
        statuses = self.check_status()
        ratios = [
            ok / total if total else 0 for ok, total in zip(self.ontime, self.offered)
        ]
        reverse_ratios = [
            ok / total if total else 0
            for ok, total in zip(self.reverse_ontime, self.reverse_offered)
        ]
        if (
            min(reverse_ratios if self.scenario == "overload" else reverse_ratios[1:])
            < 0.99
        ):
            raise AssertionError(f"reverse progress failed {reverse_ratios}")
        # Isolation includes deliberate expiry/setup drops of sender 0; assert all
        # other flows and successful recreation separately. Saturation has no exception.
        required = ratios if self.scenario == "overload" else ratios[1:]
        if min(required) < 0.99:
            raise AssertionError(f"probe progress failed {ratios}")
        if self.scenario == "isolation" and (
            self.offered[0] > 5 and self.ontime[0] < self.offered[0] - 3
        ):
            raise AssertionError("expired flow failed to recreate")
        if max_tick_lateness > 0.25:
            raise AssertionError("generator timing invalid")
        if self.scenario == "overload":
            drops = sum(
                s["counters"]["local_drops"]["queue_full"]["to_target"]
                + s["counters"]["local_drops"]["queue_full"]["to_client"]
                for s in statuses
            )
            if not drops:
                raise AssertionError("budget saturation was not observed")
        return dict(
            diagnostic_coverage_lost=[s.get("coverage_lost", 0) for s in statuses],
            diagnostic_correlation_claim=False,
            seconds=time.monotonic() - start,
            offered=self.offered,
            ontime=self.ontime,
            within_one_second_ratio=ratios,
            reverse_offered=self.reverse_offered,
            reverse_ontime=self.reverse_ontime,
            reverse_within_one_second_ratio=reverse_ratios,
            max_generator_tick_lateness=max_tick_lateness,
            resources=statuses,
        )


def main():
    install_termination_handler()
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--scenario", choices=["isolation", "overload", "churn"], required=True
    )
    p.add_argument("--seconds", type=int)
    p.add_argument("--cycles", type=int, default=1000)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--binary", type=Path, default=ROOT / "target/release/remora")
    a = p.parse_args()
    seconds = (
        a.seconds
        if a.seconds is not None
        else (600 if a.scenario == "overload" else 1800)
    )
    if seconds < 1 or not 1 <= a.cycles <= 1000:
        p.error("invalid interval/cycles")
    a.output.mkdir(parents=True, exist_ok=False)
    manifest = dict(
        schema_version=1,
        scenario=a.scenario,
        revision=subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
        runner_sha256=sha(Path(__file__)),
        binary_sha256=sha(a.binary),
        lock_sha256=sha(ROOT / "Cargo.lock"),
        source_sha256={
            str(f.relative_to(ROOT)): sha(f)
            for f in sorted((ROOT / "src").rglob("*.rs"))
        },
        seconds=seconds,
        cycles=a.cycles,
        kernel=platform.release(),
        arch=platform.machine(),
        cpu_affinity=sorted(os.sched_getaffinity(0)),
        topology="owned IPv4 loopback, two targets, four client processes, two credentials, eight senders/client",
        sessions=32,
        probe_bytes=128,
        probe_period_seconds=1,
        idle_ms=500 if a.scenario == "churn" else 2000,
        short_idle_rationale="test-only acceleration of expiry/recreation; RFC SHOULD deviation, no production default change",
    )
    write(a.output / "manifest.json", manifest)
    result = dict(status="failed", cross_delivery=0, integrity_errors=0)
    lab = None
    with tempfile.TemporaryDirectory(prefix="remora-concurrency-") as tmp:
        try:
            # Ensure startup failures also clean every successfully owned child.
            lab = Lab.__new__(Lab)
            lab.processes = []
            lab.sockets = []
            Lab.__init__(lab, Path(tmp) / "fixtures", a.output, a.binary, a.scenario)
            result.update(lab.run(seconds, a.cycles))
            result["status"] = "passed"
        except Exception as e:
            result["failure"] = f"{type(e).__name__}: {e}"
        finally:
            if lab:
                result.update(
                    cross_delivery=lab.cross if hasattr(lab, "cross") else 0,
                    integrity_errors=lab.corrupt if hasattr(lab, "corrupt") else 0,
                    generator_errors=(
                        lab.generator_errors if hasattr(lab, "generator_errors") else 0
                    ),
                    forward_datagrams=lab.forward if hasattr(lab, "forward") else 0,
                    reverse_datagrams=lab.reverse if hasattr(lab, "reverse") else 0,
                    max_fds=getattr(lab, "max_fds", []),
                )
                write(a.output / "samples.json", getattr(lab, "samples", []))
                lab.close()
                released = []
                try:
                    for address in getattr(lab, "listens", []) + (
                        [lab.server_address] if hasattr(lab, "server_address") else []
                    ):
                        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                        s.bind(address)
                        released.append(s)
                    result["listeners_released"] = True
                except OSError:
                    result["status"] = "failed"
                    result["failure"] = "listener leaked"
                finally:
                    for s in released:
                        s.close()
                result["exit_codes"] = [p.p.returncode for p in lab.processes]
                if any(p.p.returncode != 0 for p in lab.processes):
                    result["status"] = "failed"
                    result.setdefault("failure", "nonzero role exit")
            write(a.output / "summary.json", result)
    print(json.dumps({k: v for k, v in result.items() if k != "resources"}), flush=True)
    if result["status"] != "passed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
