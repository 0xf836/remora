#!/usr/bin/env python3
"""Owned reference-path recovery scenarios with explicit SC-006 timing origins."""

import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
from benchmark import install_termination_handler
import re
import shutil
import subprocess
import sys
import tempfile
import time
from benchmark import ROOT, Owned, sha, wait_udp_bound, write
from network import ns, run
from recovery_probe import publish

SCENARIOS = [
    "blackhole",
    "restart",
    "address-change",
    "long-outage",
    "revoked",
    "policy",
    "trust",
    "capability",
    "idle",
]


def shutdown_matrix(out, peer_binary):
    if not peer_binary or not peer_binary.is_file():
        raise RuntimeError("pinned independent peer required for held-setup shutdown")
    commands = [
        [
            "cargo",
            "test",
            "--locked",
            "--test",
            "e2e",
            "shutdown::both_signals_stop",
            "--",
            "--nocapture",
        ],
        [
            "cargo",
            "test",
            "--locked",
            "--test",
            "e2e",
            "shutdown::both_signals_cancel",
            "--",
            "--ignored",
            "--nocapture",
        ],
    ]
    import os

    env = {**os.environ, "REMORA_INTEROP_BIN": str(peer_binary.resolve())}
    # Cargo reports the exact executable path, including a custom target dir.
    # Bind both process-test cases to that executable, not the release binary.
    build = subprocess.run(
        [
            "cargo",
            "test",
            "--locked",
            "--test",
            "e2e",
            "--no-run",
            "--message-format=json",
        ],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=True,
        timeout=120,
    )
    (out / "shutdown-build.jsonl").write_text(build.stdout)
    (out / "shutdown-build.log").write_text(build.stderr)
    binaries = {
        Path(record["executable"])
        for line in build.stdout.splitlines()
        if (record := json.loads(line)).get("reason") == "compiler-artifact"
        and record.get("target", {}).get("name") == "remora"
        and "bin" in record.get("target", {}).get("kind", [])
        and record.get("executable")
    }
    if len(binaries) != 1:
        raise RuntimeError("exact shutdown process executable unavailable")
    executable = binaries.pop()
    executable_hash = sha(executable)
    events = []
    for index, command in enumerate(commands):
        path = out / f"shutdown-{index}.log"
        with path.open("x") as log:
            subprocess.run(
                command,
                cwd=ROOT,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=True,
                timeout=120,
            )
        for line in path.read_text().splitlines():
            if line.startswith('{"event":"shutdown_process"'):
                events.append(json.loads(line))
        if sha(executable) != executable_hash:
            raise RuntimeError("shutdown executable changed during matrix")
    expected = {
        (state, signal)
        for state in ["starting", "active", "recovering", "setup"]
        for signal in ["SIGINT", "SIGTERM"]
    }
    if {(r["state"], r["signal"]) for r in events} != expected or any(
        r["stop_ms"] > 5000 or not r["listener_released"] for r in events
    ):
        raise RuntimeError("incomplete/failed shutdown coverage")
    write(
        out / "shutdown-summary.json",
        dict(
            status="passed",
            cases=events,
            independent_peer_sha256=sha(peer_binary),
            process_executable=str(executable),
            process_executable_sha256=executable_hash,
            release_acceptance_claim=False,
        ),
    )


def read_report(path):
    return (
        json.loads(path.read_text())
        if path.exists()
        else dict(received=[], offered=[], corrupt=0, duplicates=0)
    )


def records(path):
    rows = []
    if path.exists():
        for line in path.read_text().splitlines():
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                # Only a not-yet-complete final line can be in flight.
                if line != path.read_text().splitlines()[-1]:
                    raise
    return rows


def wait_condition(check, seconds, owned):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if any(p.exited() for p in owned):
            raise RuntimeError("owned recovery process exited")
        value = check()
        if value:
            return value
        time.sleep(0.025)
    raise TimeoutError("recovery condition deadline")


def listener_identity(proc, port):
    inodes = set()
    for path in Path(f"/proc/{proc.p.pid}/fd").iterdir():
        try:
            link = path.readlink().as_posix()
        except FileNotFoundError:
            continue
        if link.startswith("socket:["):
            inodes.add(link[8:-1])
    found = []
    for family in ["udp", "udp6"]:
        for line in (
            Path(f"/proc/{proc.p.pid}/net/{family}").read_text().splitlines()[1:]
        ):
            fields = line.split()
            if fields[9] in inodes and int(fields[1].split(":")[-1], 16) == port:
                found.append(fields[9])
    if len(found) != 1:
        raise RuntimeError("listener identity unavailable")
    return found[0]


def characterize(network, out, scenario, peer_binary):
    owned = []
    snapshots = []
    try:
        with tempfile.TemporaryDirectory(prefix="remora-recovery-") as temp:
            f = Path(temp) / "fixtures"
            subprocess.run(
                [str(ROOT / "scripts/validate/fixtures.sh"), "--output", str(f)],
                check=True,
                stdout=subprocess.DEVNULL,
            )
            ports = json.loads((f / "ports.json").read_text())
            proxy = f"10.203.0.2:{ports['proxy']}"
            server_file, client_file = f / "server.toml", f / "client.toml"
            server_file.write_text(
                server_file.read_text().replace(f"127.0.0.1:{ports['proxy']}", proxy)
            )
            client_file.write_text(
                client_file.read_text().replace(
                    f'connect_address = "127.0.0.1:{ports["proxy"]}"',
                    f'connect_address = "{proxy}"',
                )
            )
            binary = str(ROOT / "target/release/remora")
            probe = str(ROOT / "scripts/validate/recovery_probe.py")
            echo = Owned(
                ns(
                    network.server,
                    sys.executable,
                    probe,
                    "--role",
                    "echo",
                    "--port",
                    ports["target"],
                    "--report",
                    out / "target-report.json",
                ),
                out / "target",
            )
            owned.append(echo)
            wait_udp_bound(echo, f"127.0.0.1:{ports['target']}")

            def start_server(name):
                server = Owned(
                    ns(network.server, binary, "server", "--config", server_file),
                    out / name,
                )
                owned.append(server)
                wait_udp_bound(server, proxy)
                return server

            server = start_server("server-before")
            client = Owned(
                ns(network.client, binary, "client", "--config", client_file),
                out / "client",
            )
            owned.append(client)
            listen = f"127.0.0.1:{ports['client']}"
            wait_udp_bound(client, listen)
            listener = listener_identity(client, ports["client"])
            control = out / "probe-control.json"
            publish(control, dict(paused=False))
            generator = Owned(
                ns(
                    network.client,
                    sys.executable,
                    probe,
                    "--role",
                    "client",
                    "--port",
                    ports["client"],
                    "--control",
                    control,
                    "--report",
                    out / "client-report.json",
                ),
                out / "generator",
            )
            owned.append(generator)
            healthy = [echo, server, client, generator]
            wait_condition(
                lambda: len(read_report(out / "client-report.json")["received"]) >= 5,
                10,
                healthy,
            )
            wall_start, mono_start = time.time_ns(), time.monotonic_ns()
            fault = time.monotonic_ns()
            restored = None
            permanent = scenario in ["revoked", "policy", "trust", "capability"]
            if scenario == "idle":
                publish(control, dict(paused=True))
                time.sleep(9)
                if any(p.exited() for p in healthy):
                    raise RuntimeError("idle role exited")
                latest = records(out / "client.stdout.ndjson")[-1]
                if (
                    latest["recovery"]["connection_attempts"] != 1
                    or latest["recovery"]["keepalive_probes"] < 2
                    or latest["sessions_active"] != 1
                ):
                    raise RuntimeError(
                        "paced keepalive did not preserve idle active transport"
                    )
                restored = time.monotonic_ns()
                publish(control, dict(paused=False))
            elif scenario in ["blackhole", "long-outage"]:
                for pid, dev in [(network.client, "p5c"), (network.server, "p5s")]:
                    run(
                        *ns(
                            pid,
                            "tc",
                            "qdisc",
                            "replace",
                            "dev",
                            dev,
                            "root",
                            "netem",
                            "loss",
                            "100%",
                        )
                    )
                fault = time.monotonic_ns()
                snapshots.append(network.snapshot())
                time.sleep(30 if scenario == "long-outage" else 10)
                network.shape()
                restored = time.monotonic_ns()
            elif scenario == "address-change":
                # All route/address changes are to the owned veth namespace.
                run(
                    *ns(
                        network.client,
                        "ip",
                        "addr",
                        "add",
                        "10.203.0.5/30",
                        "dev",
                        "p5c",
                    )
                )
                run(
                    *ns(
                        network.client,
                        "ip",
                        "route",
                        "add",
                        "10.203.0.2/32",
                        "dev",
                        "p5c",
                    )
                )
                run(
                    *ns(
                        network.server,
                        "ip",
                        "route",
                        "add",
                        "10.203.0.5/32",
                        "dev",
                        "p5s",
                    )
                )
                run(
                    *ns(
                        network.client,
                        "ip",
                        "addr",
                        "del",
                        "10.203.0.1/30",
                        "dev",
                        "p5c",
                    )
                )
                fault = restored = time.monotonic_ns()
                snapshots.append(network.snapshot())
            else:
                fault = time.monotonic_ns()
                server.stop()
                if scenario == "revoked":
                    path = f / "credentials.toml"
                    path.write_text(
                        re.sub(
                            r'token_sha256 = "[0-9a-f]{64}"',
                            'token_sha256 = "' + "0" * 64 + '"',
                            path.read_text(),
                        )
                    )
                elif scenario == "policy":
                    path = f / "credentials.toml"
                    path.write_text(
                        path.read_text().replace(
                            f'port = {ports["target"]}',
                            f'port = {ports["target"] + 1}',
                        )
                    )
                elif scenario == "trust":
                    fresh = Path(temp) / "other-ca"
                    subprocess.run(
                        [
                            str(ROOT / "scripts/validate/fixtures.sh"),
                            "--output",
                            str(fresh),
                        ],
                        check=True,
                        stdout=subprocess.DEVNULL,
                    )
                    for name in ["server.pem", "server-key.pem"]:
                        shutil.copyfile(fresh / name, f / name)
                time.sleep(max(0, 10 - (time.monotonic_ns() - fault) / 1e9))
                if scenario == "capability":
                    if not peer_binary or not peer_binary.is_file():
                        raise RuntimeError(
                            "pinned independent peer required for capability recovery"
                        )
                    peer_config = f / "peer.json"
                    token = (f / "client.token").read_text().strip()
                    peer_config.write_text(
                        json.dumps(
                            dict(
                                role="server",
                                listen=proxy,
                                authority=f"remora.test:{ports['proxy']}",
                                target=f"127.0.0.1:{ports['target']}",
                                certificate_file=str(f / "server.pem"),
                                key_file=str(f / "server-key.pem"),
                                digest=hashlib.sha256(token.encode()).hexdigest(),
                                case="no-datagram",
                            )
                        )
                    )
                    server = Owned(
                        ns(network.server, str(peer_binary), "--config", peer_config),
                        out / "server-after",
                    )
                    owned.append(server)
                    wait_udp_bound(server, proxy)
                else:
                    server = start_server("server-after")
                restored = time.monotonic_ns()
            healthy = [echo, client, generator, server]
            if permanent:

                def rejected():
                    return next(
                        (
                            r
                            for r in reversed(records(out / "client.stdout.ndjson"))
                            if r.get("recovery", {}).get("client_state") == "rejected"
                        ),
                        None,
                    )

                first = wait_condition(rejected, 10, healthy)
                attempts = first["recovery"]["connection_attempts"]
                time.sleep(3)
                latest = wait_condition(rejected, 1, healthy)
                if latest["recovery"]["connection_attempts"] != attempts:
                    raise RuntimeError("permanent mapping retried")
                expected = {
                    "revoked": ("authentication", 401),
                    "policy": ("authorization", 403),
                    "trust": ("trust", 0),
                    "capability": ("capability", 0),
                }[scenario]
                observed = [
                    (x["class"], x["code"])
                    for row in records(out / "client.stdout.ndjson")
                    for x in row.get("failures", [])
                ]
                if expected not in observed:
                    raise RuntimeError("wrong permanent rejection cause")
                resumed = None
            else:
                resumed = wait_condition(
                    lambda: next(
                        (
                            r
                            for r in read_report(out / "client-report.json")["received"]
                            if r["sent_ns"] >= restored
                        ),
                        None,
                    ),
                    10,
                    healthy,
                )
                time.sleep(1)
            if listener_identity(client, ports["client"]) != listener:
                raise RuntimeError("listener replaced during recovery")
            final = records(out / "client.stdout.ndjson")
            # Preserve exact failure-class monotonic evidence plus conservative
            # observable detection from the first post-fault recovery status.
            events = records(out / "client.stderr.ndjson")
            detected = []
            for event in events:
                if event.get("event") == "recovering":
                    stamp = int(
                        datetime.fromisoformat(
                            event["timestamp"].replace("Z", "+00:00")
                        ).timestamp()
                        * 1e9
                    )
                    converted = mono_start + stamp - wall_start
                    if converted >= fault:
                        detected.append(converted)
            if scenario not in ["idle", "address-change"] and (
                not detected or (min(detected) - fault) / 1e9 > 15
            ):
                raise RuntimeError("SC-006 detection deadline")
            for proc in reversed(owned):
                proc.stop()
            target = read_report(out / "target-report.json")
            traffic = read_report(out / "client-report.json")
            if any(
                r["received_ns"] >= restored and fault <= r["sent_ns"] < restored
                for r in target["received"]
            ):
                raise RuntimeError("outage payload replay reached actual endpoint")
            if permanent and any(r["sent_ns"] >= restored for r in target["received"]):
                raise RuntimeError("permanent rejection forwarded to actual endpoint")
            if (
                target["corrupt"]
                or traffic["corrupt"]
                or target["duplicates"]
                or traffic["duplicates"]
            ):
                raise RuntimeError("corruption, duplication or cross-association")
            drift_ms = (
                abs((time.time_ns() - wall_start) - (time.monotonic_ns() - mono_start))
                / 1e6
            )
            if drift_ms >= 10:
                raise RuntimeError("timing clock evidence invalid")
            write(
                out / "recovery-summary.json",
                dict(
                    status="passed",
                    scenario=scenario,
                    binary_sha256=sha(ROOT / "target/release/remora"),
                    failure_onset_ns=fault,
                    restored_availability_ns=restored,
                    detection_seconds=(
                        (min(detected) - fault) / 1e9 if detected else None
                    ),
                    resume_seconds=(
                        (resumed["received_ns"] - restored) / 1e9 if resumed else None
                    ),
                    listener_inode_unchanged=True,
                    outage_replay=0,
                    permanent=permanent,
                    clock_drift_ms=drift_ms,
                    client_final=final[-1],
                    network_changes=snapshots,
                    release_acceptance_claim=False,
                ),
            )
            print(
                json.dumps(dict(event="recovery", scenario=scenario, status="passed")),
                flush=True,
            )
    finally:
        for proc in reversed(owned):
            proc.stop()


def main():
    install_termination_handler()
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--scenario", choices=["all", "shutdown", *SCENARIOS], default="all")
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--peer-binary", type=Path)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=False)
    if a.scenario == "shutdown":
        shutdown_matrix(a.output, a.peer_binary)
        return
    for scenario in SCENARIOS if a.scenario == "all" else [a.scenario]:
        command = [
            str(ROOT / "scripts/validate/network.sh"),
            "--scenario",
            "reference",
            "--recovery-scenario",
            scenario,
            "--output",
            str(a.output / scenario),
        ]
        if a.peer_binary:
            command += ["--peer-binary", str(a.peer_binary.resolve())]
        subprocess.run(command, cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
