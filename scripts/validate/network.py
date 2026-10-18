#!/usr/bin/env python3
"""Capability use confined to fresh runner-owned user/network namespaces."""

import argparse
import json
import os
from pathlib import Path
from benchmark import install_termination_handler
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from benchmark import ROOT, Owned, sha, wait_udp_bound, write

SCENARIOS = {
    "reference": dict(rtt_ms=20, loss_percent=0, mtu=1500),
    "rtt50": dict(rtt_ms=50, loss_percent=0, mtu=1500),
    "rtt100": dict(rtt_ms=100, loss_percent=0, mtu=1500),
    "loss01": dict(rtt_ms=20, loss_percent=0.1, mtu=1500),
    "loss1": dict(rtt_ms=20, loss_percent=1, mtu=1500),
    "asymmetric": dict(rtt_ms=20, loss_percent=0, mtu=1500, reverse_delay_ms=40),
    "burst": dict(rtt_ms=20, loss_percent=0, mtu=1500, queue_packets=32),
    "mtu1280": dict(rtt_ms=20, loss_percent=0, mtu=1280),
    "mtu-change": dict(rtt_ms=20, loss_percent=0, mtu=1500, reduced_mtu=1240),
}


def ns(pid, *cmd):
    return ["nsenter", f"--net=/proc/{pid}/ns/net", "--", *map(str, cmd)]


def run(*cmd):
    return subprocess.check_output(
        list(map(str, cmd)), text=True, stderr=subprocess.PIPE
    )


def namespace_holder(path):
    # No wall-clock expiry: full reference matrices take several days. The
    # owned process group remains alive until Network.close() tears it down.
    return Owned(["unshare", "--net", "--", "sleep", "infinity"], path)


class Network:
    def __init__(self, root, scenario, host_net_fd, host_user_fd):
        self.root, self.scenario = root, scenario
        self.children = []
        # Inherited descriptors bind this proof to the bootstrap's actual host
        # namespaces, not to a caller-supplied path/name or an integer guess.
        for fd, kind in [(host_net_fd, "net"), (host_user_fd, "user")]:
            if (
                not os.readlink(f"/proc/self/fd/{fd}").startswith(kind + ":[")
                or os.fstat(fd).st_ino == os.stat(f"/proc/self/ns/{kind}").st_ino
            ):
                raise RuntimeError(
                    "refusing capability work outside a fresh private namespace"
                )
        links = json.loads(run("ip", "-j", "link", "show"))
        if len(links) != 1 or links[0]["ifname"] != "lo":
            raise RuntimeError("setup namespace is not fresh")
        try:
            # The proven private setup namespace also hosts the loopback churn
            # fixture. Its lo starts DOWN after unshare; never mutate host lo.
            run("ip", "link", "set", "lo", "up")
            for name in ["client", "server"]:
                child = namespace_holder(root / f"namespace-{name}")
                self.children.append(child)
                deadline = time.monotonic() + 3
                parent_ns = os.stat("/proc/self/ns/net").st_ino
                while os.stat(f"/proc/{child.p.pid}/ns/net").st_ino == parent_ns:
                    if child.exited() or time.monotonic() > deadline:
                        raise RuntimeError("owned child network namespace unavailable")
                    time.sleep(0.005)
            self.client, self.server = [c.p.pid for c in self.children]
            run("ip", "link", "add", "p5c", "type", "veth", "peer", "name", "p5s")
            for pid, dev, ip in [
                (self.client, "p5c", "10.203.0.1/30"),
                (self.server, "p5s", "10.203.0.2/30"),
            ]:
                run("ip", "link", "set", dev, "netns", pid)
                run(*ns(pid, "ip", "link", "set", "lo", "up"))
                run(*ns(pid, "ip", "addr", "add", ip, "dev", dev))
                run(
                    *ns(
                        pid,
                        "ip",
                        "link",
                        "set",
                        dev,
                        "mtu",
                        SCENARIOS[scenario]["mtu"],
                        "up",
                    )
                )
            self.shape()
        except BaseException:
            self.close()
            raise

    def shape(self):
        self.ensure_alive()
        s = SCENARIOS[self.scenario]
        for index, (pid, dev) in enumerate(
            [(self.client, "p5c"), (self.server, "p5s")]
        ):
            delay = (
                s.get("reverse_delay_ms", s["rtt_ms"] / 2) if index else s["rtt_ms"] / 2
            )
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
                    "limit",
                    s.get("queue_packets", 2048),
                    "delay",
                    f"{delay}ms",
                    "loss",
                    f'{s["loss_percent"]}%',
                    "rate",
                    "1gbit",
                    "seed",
                    5057 + index,
                )
            )

    def snapshot(self):
        self.ensure_alive()
        value = {}
        for name, pid, dev in [
            ("client", self.client, "p5c"),
            ("server", self.server, "p5s"),
        ]:
            value[name] = {
                "namespace_inode": os.stat(f"/proc/{pid}/ns/net").st_ino,
                "links": json.loads(
                    run(*ns(pid, "ip", "-j", "-details", "link", "show"))
                ),
                "addresses": json.loads(run(*ns(pid, "ip", "-j", "addr", "show"))),
                "qdisc": json.loads(
                    run(*ns(pid, "tc", "-j", "-s", "qdisc", "show", "dev", dev))
                ),
                "nic": "runner-owned veth; no physical NIC throughput qualification",
                "offloads": json.loads(
                    run(
                        *ns(
                            pid,
                            sys.executable,
                            str(ROOT / "scripts/validate/network_features.py"),
                            dev,
                        )
                    )
                ),
            }
        return value

    def ensure_alive(self):
        if any(child.exited() for child in self.children):
            raise RuntimeError("owned network namespace holder exited before teardown")

    def close(self):
        for child in reversed(self.children):
            child.stop()


def characterize(network, out):
    processes = []
    captures = []
    try:
        with tempfile.TemporaryDirectory(prefix="remora-network-") as temp:
            f = Path(temp) / "fixtures"
            subprocess.run(
                [str(ROOT / "scripts/validate/fixtures.sh"), "--output", str(f)],
                check=True,
                stdout=subprocess.DEVNULL,
            )
            ports = json.loads((f / "ports.json").read_text())
            server = f / "server.toml"
            server.write_text(
                server.read_text().replace(
                    f'127.0.0.1:{ports["proxy"]}', f'10.203.0.2:{ports["proxy"]}'
                )
            )
            client = f / "client.toml"
            client.write_text(
                client.read_text().replace(
                    f'connect_address = "127.0.0.1:{ports["proxy"]}"',
                    f'connect_address = "10.203.0.2:{ports["proxy"]}"',
                )
            )
            endpoint = Owned(
                ns(
                    network.server,
                    "python3",
                    str(ROOT / "scripts/validate/network_probe.py"),
                    "--role",
                    "endpoint",
                    "--port",
                    ports["target"],
                ),
                out / "endpoint",
            )
            processes.append(endpoint)
            wait_udp_bound(endpoint, f'127.0.0.1:{ports["target"]}')
            for role, pid in [("server", network.server), ("client", network.client)]:
                child = Owned(
                    ns(
                        pid,
                        str(ROOT / "target/release/remora"),
                        role,
                        "--config",
                        str(f / f"{role}.toml"),
                    ),
                    out / role,
                )
                processes.append(child)
                wait_udp_bound(
                    child,
                    f'{"10.203.0.2" if role == "server" else "127.0.0.1"}:{ports["proxy" if role == "server" else "client"]}',
                )
            if not shutil.which("tcpdump"):
                raise RuntimeError("fragment observation requires tcpdump")
            for role, pid, dev in [
                ("client", network.client, "p5c"),
                ("server", network.server, "p5s"),
            ]:
                capture = Owned(
                    ns(
                        pid,
                        "tcpdump",
                        "-nn",
                        "-q",
                        "-l",
                        "-s",
                        "80",
                        "-i",
                        dev,
                        "ip and (ip[6:2] & 0x3fff != 0)",
                    ),
                    out / f"{role}-fragments",
                )
                captures.append(capture)
                deadline = time.monotonic() + 3
                while (
                    "listening on"
                    not in (out / f"{role}-fragments.stderr.ndjson").read_text()
                ):
                    if capture.exited() or time.monotonic() > deadline:
                        raise RuntimeError("fragment observer unavailable")
                    time.sleep(0.01)

            def probes(name, max_size):
                child = Owned(
                    ns(
                        network.client,
                        "python3",
                        str(ROOT / "scripts/validate/network_probe.py"),
                        "--role",
                        "probe",
                        "--port",
                        ports["client"],
                        "--max-size",
                        max_size,
                    ),
                    out / name,
                )
                processes.append(child)
                deadline = time.monotonic() + 50
                while not child.exited():
                    if time.monotonic() > deadline:
                        raise TimeoutError("bounded network probe deadline")
                    time.sleep(0.02)
                child.stop()
                if child.p.returncode:
                    raise RuntimeError("network characterization failed")
                return json.loads((out / f"{name}.stdout.ndjson").read_text())

            result = probes("sizes", 1200)
            if network.scenario == "mtu-change":
                for pid, dev in [(network.client, "p5c"), (network.server, "p5s")]:
                    run(
                        *ns(
                            pid,
                            "ip",
                            "link",
                            "set",
                            dev,
                            "mtu",
                            SCENARIOS[network.scenario]["reduced_mtu"],
                        )
                    )
                result["after_reduction"] = probes("reduced", 1200)
            time.sleep(1.05)  # Allow the ordinary numeric snapshot cadence.
            for child in processes:
                child.stop()
            for capture in captures:
                capture.stop()
            for role in ["client", "server"]:
                if (
                    out / f"{role}-fragments.stdout.ndjson"
                ).read_text().strip() or "0 packets dropped by kernel" not in (
                    out / f"{role}-fragments.stderr.ndjson"
                ).read_text():
                    raise RuntimeError("fragmented traffic or incomplete capture")
            result["ipv4_fragment_observation"] = (
                "zero fragments, zero capture drops, both owned veths"
            )
            endpoint_counts = json.loads((out / "endpoint.stdout.ndjson").read_text())
            if endpoint_counts["received_by_size"]["1201"] != 0:
                raise RuntimeError("oversized application reached actual endpoint")
            result["endpoint"] = endpoint_counts
            counters = {}
            for role in ["client", "server"]:
                records = [
                    json.loads(l)
                    for l in (out / f"{role}.stdout.ndjson").read_text().splitlines()
                ]
                records = [
                    r
                    for r in records
                    if r.get("event") in ["snapshot", "terminal_snapshot"]
                ]
                if not records:
                    raise RuntimeError("missing numeric status evidence")
                counters[role] = records[-1]
            write(
                out / "summary.json",
                {
                    "status": "passed-functional-characterization",
                    "probe": result,
                    "diagnostics": counters,
                    "release_acceptance_claim": False,
                },
            )
    finally:
        for child in reversed(captures + processes):
            child.stop()


def main():
    install_termination_handler()
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--scenario", choices=["all", *SCENARIOS], default="all")
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--generator-link-mbit", type=int, default=1000)
    p.add_argument("--benchmark-profile", type=Path)
    p.add_argument("--development-smoke", action="store_true")
    p.add_argument("--smoke-selected", action="store_true")
    p.add_argument(
        "--recovery-scenario",
        choices=[
            "blackhole",
            "restart",
            "address-change",
            "long-outage",
            "revoked",
            "policy",
            "trust",
            "capability",
            "idle",
        ],
    )
    p.add_argument("--peer-binary", type=Path)
    p.add_argument("--worker-host-net-fd", type=int, help=argparse.SUPPRESS)
    p.add_argument("--worker-host-user-fd", type=int, help=argparse.SUPPRESS)
    a = p.parse_args()
    if a.smoke_selected and not a.development_smoke:
        raise ValueError("selected coverage is smoke only")
    if a.generator_link_mbit != 1000:
        raise ValueError("generator/shaped-link mismatch")
    if a.worker_host_net_fd is None:
        fds = [os.open(f"/proc/self/ns/{k}", os.O_RDONLY) for k in ["net", "user"]]
        try:
            extra = []
            if a.benchmark_profile:
                extra += ["--benchmark-profile", str(a.benchmark_profile.resolve())]
            if a.development_smoke:
                extra.append("--development-smoke")
            if a.smoke_selected:
                extra.append("--smoke-selected")
            if a.recovery_scenario:
                extra += ["--recovery-scenario", a.recovery_scenario]
            if a.peer_binary:
                extra += ["--peer-binary", str(a.peer_binary.resolve())]
            result = subprocess.run(
                [
                    "unshare",
                    "--user",
                    "--map-root-user",
                    "--net",
                    "--",
                    sys.executable,
                    str(Path(__file__).resolve()),
                    "--scenario",
                    a.scenario,
                    "--output",
                    str(a.output.resolve()),
                    "--worker-host-net-fd",
                    str(fds[0]),
                    "--worker-host-user-fd",
                    str(fds[1]),
                    *extra,
                ],
                pass_fds=fds,
            )
            if result.returncode:
                raise SystemExit(result.returncode)
        finally:
            for fd in fds:
                os.close(fd)
        return
    a.output.mkdir(parents=True, exist_ok=False)
    for scenario in SCENARIOS if a.scenario == "all" else [a.scenario]:
        out = a.output / scenario
        out.mkdir()
        network = None
        try:
            network = Network(
                out, scenario, a.worker_host_net_fd, a.worker_host_user_fd
            )
            write(
                out / "manifest.json",
                {
                    "scenario": SCENARIOS[scenario],
                    "link_mbit_s_each_direction": 1000,
                    "generator_link_mbit_s": a.generator_link_mbit,
                    "setup_namespace_inode": os.stat("/proc/self/ns/net").st_ino,
                    "host_namespace_inode": os.fstat(a.worker_host_net_fd).st_ino,
                    "binary_sha256": sha(ROOT / "target/release/remora"),
                    "settings": network.snapshot(),
                    "capacity_claim": False,
                },
            )
            print(f"Characterizing {scenario}", flush=True)
            if a.recovery_scenario:
                if a.scenario != "reference" or a.benchmark_profile:
                    raise ValueError("recovery requires its isolated reference path")
                from recovery import characterize as characterize_recovery

                characterize_recovery(network, out, a.recovery_scenario, a.peer_binary)
            elif a.benchmark_profile:
                if a.scenario != "reference":
                    raise ValueError("reference path required")
                from reference import execute

                bench_out = out / "benchmark"
                bench_out.mkdir()
                execute(
                    network,
                    bench_out,
                    tomllib.loads(a.benchmark_profile.read_text()),
                    a.development_smoke,
                    a.smoke_selected,
                )
            else:
                characterize(network, out)
        except Exception as e:
            write(
                out / "failure.json", {"status": "invalid", "reason": type(e).__name__, "detail": str(e)}
            )
            progress_file = out / "benchmark/progress.json"
            if a.benchmark_profile and progress_file.is_file():
                from reference import write_progress
                write_progress(progress_file.parent, {
                    **json.loads(progress_file.read_text()),
                    "status": "invalid", "reason": type(e).__name__, "detail": str(e),
                })
            raise
        finally:
            if network:
                try:
                    write(out / "final-network.json", network.snapshot())
                finally:
                    network.close()


if __name__ == "__main__":
    main()
