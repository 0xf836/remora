#!/usr/bin/env python3
"""Owned loopback fixtures, two role processes, independent directional traffic.
No secrets, credentials or TLS keys are retained in result artifacts.
"""

import argparse, hashlib, json, os, platform, signal, subprocess, tempfile, time, tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class Owned:
    def __init__(self, cmd, path):
        self.out = open(path.with_suffix(".stdout.ndjson"), "wb")
        self.err = open(path.with_suffix(".stderr.ndjson"), "wb")
        self.p = subprocess.Popen(
            cmd,
            stdin=subprocess.DEVNULL,
            stdout=self.out,
            stderr=self.err,
            start_new_session=True,
        )
        self.reaped = False

    def exited(self):
        # Never reap while the group identity is still needed for cleanup.
        return (
            os.waitid(os.P_PID, self.p.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            is not None
        )

    def stop(self):
        if self.reaped:
            return
        try:
            os.killpg(self.p.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        deadline = time.monotonic() + 5
        while not self.exited() and time.monotonic() < deadline:
            time.sleep(0.01)
        try:
            os.killpg(self.p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        self.p.wait(timeout=2)
        self.reaped = True
        self.out.close()
        self.err.close()


def wait_udp_bound(process, address):
    """Read only this unreaped child's socket inode/port before client startup."""
    port = int(address.rsplit(":", 1)[1])
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if process.exited():
            raise RuntimeError("role exited before UDP readiness")
        inodes = set()
        for fd in Path(f"/proc/{process.p.pid}/fd").iterdir():
            try:
                link = os.readlink(fd)
            except FileNotFoundError:
                continue
            if link.startswith("socket:["):
                inodes.add(link[8:-1])
        for family in ["udp", "udp6"]:
            for line in (
                Path(f"/proc/{process.p.pid}/net/{family}").read_text().splitlines()[1:]
            ):
                fields = line.split()
                if (
                    len(fields) > 9
                    and fields[9] in inodes
                    and int(fields[1].rsplit(":", 1)[1], 16) == port
                ):
                    return
        time.sleep(0.005)
    raise TimeoutError("owned UDP readiness deadline")


def write(path, value):
    with path.open("x") as f:
        json.dump(value, f, indent=2, sort_keys=True)
        f.write("\n")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def resource(p):
    fields = Path(f"/proc/{p.pid}/stat").read_text().split(") ", 1)[1].split()
    hz = os.sysconf("SC_CLK_TCK")
    status = Path(f"/proc/{p.pid}/status").read_text().splitlines()
    rss = int(next((l.split()[1] for l in status if l.startswith("VmRSS:")), 0)) * 1024
    owner = (
        Path(f"/proc/{p.pid}/task/{p.pid}/stat").read_text().split(") ", 1)[1].split()
    )
    return {
        "cpu_seconds": (int(fields[11]) + int(fields[12])) / hz,
        "owner_cpu_seconds": (int(owner[11]) + int(owner[12])) / hz,
        "rss_bytes": rss,
    }


def main():
    a = argparse.ArgumentParser()
    a.add_argument(
        "--profile", "--manifest", dest="profile", default="benchmarks/mvp-x86_64.toml"
    )
    a.add_argument("--output", required=True)
    args = a.parse_args()
    profile = tomllib.loads(Path(args.profile).read_text())
    if (
        profile["topology"] != "two-leg"
        or profile["sessions"] != 1
        or profile["architecture"] != platform.machine()
        or profile["io"] not in ["single", "mmsg"]
        or profile["cc"] != "cubic"
        or profile["diagnostics"] != "info"
        or profile["gso_requested"] != "off"
        or profile["gro_requested"] != "off"
    ):
        raise ValueError("unsupported early coverage/topology/platform")
    out = Path(args.output)
    out.mkdir(parents=True, exist_ok=False)
    manifest = {
        "schema_version": 1,
        "profile": profile,
        "release_acceptance_claim": False,
        "regression_claim": False,
        "revision": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
        "dirty": bool(
            subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)
        ),
        "platform": {
            "arch": platform.machine(),
            "kernel": platform.release(),
            "cpu_count": os.cpu_count(),
            "cpu_model": next(
                (
                    l.split(":", 1)[1].strip()
                    for l in Path("/proc/cpuinfo").read_text().splitlines()
                    if l.startswith("model name")
                ),
                "unreported",
            ),
            "affinity": sorted(os.sched_getaffinity(0)),
        },
        "offloads": {
            "gso": {
                "requested": profile["gso_requested"],
                "effective": "off",
                "reason": "configured_off",
            },
            "gro": {
                "requested": profile["gro_requested"],
                "effective": "off",
                "reason": "configured_off",
            },
        },
        "source_sha256": {
            str(p.relative_to(ROOT)): sha(p)
            for base in ["src", "tools/traffic/src", "scripts/validate"]
            for p in sorted((ROOT / base).rglob("*"))
            if p.is_file() and "__pycache__" not in p.parts and p.suffix != ".pyc"
        },
        "locks": {p: sha(ROOT / p) for p in ["Cargo.lock", "tools/traffic/Cargo.lock"]},
        "binaries": {
            p: sha(ROOT / p)
            for p in [
                "target/release/remora",
                "tools/traffic/target/release/remora-traffic",
            ]
        },
        "role_locations": {
            "generator": "loopback client endpoint",
            "client": "same-host client role",
            "server": "same-host server role",
            "receiver": "loopback target endpoint",
        },
        "clock": "common wall-time start mapped once onto per-process monotonic clocks; RTT uses same sender clock",
        "baseline": "two role Linux UDP relay using same single/mmsg module, 100us bounded polling; no sustainable-capacity claim",
        "kernel_socket_buffers_requested": {"outer": 262144, "relay": 65536},
        "coverage_limits": [
            "one session",
            "3-second default measurement",
            "no controlled bottleneck/RTT",
            "two early pairs",
            "no 500 Mbit resource load",
            "not release qualification",
        ],
    }
    write(out / "manifest.json", manifest)
    all_summaries = []
    tool = str(ROOT / "tools/traffic/target/release/remora-traffic")
    binary = str(ROOT / "target/release/remora")
    for pair in range(profile["paired_repetitions"]):
        for direction in ["forward", "reverse", "simultaneous"]:
            for mode in (
                ["baseline", "tunnel"] if pair % 2 == 0 else ["tunnel", "baseline"]
            ):
                run = out / f"{pair}-{direction}-{mode}"
                run.mkdir()
                owned = []
                samples = []
                try:
                    with tempfile.TemporaryDirectory(prefix="remora-bench-") as tmp:
                        f = Path(tmp) / "fixtures"
                        subprocess.run(
                            [
                                str(ROOT / "scripts/validate/fixtures.sh"),
                                "--output",
                                str(f),
                            ],
                            check=True,
                            stdout=subprocess.DEVNULL,
                        )
                        for role in ["client", "server"]:
                            with (f / f"{role}.toml").open("a") as config:
                                config.write(
                                    f'\n[io]\nmode = "{profile["io"]}"\nbatch_max = {profile["batch_max"]}\n'
                                )
                        ports = json.loads((f / "ports.json").read_text())
                        address = lambda key: f"127.0.0.1:{ports[key]}"
                        if mode == "tunnel":
                            for role in ["server", "client"]:
                                if role == "client":
                                    wait_udp_bound(owned[0], address("proxy"))
                                owned.append(
                                    Owned(
                                        [
                                            binary,
                                            role,
                                            "--config",
                                            str(f / f"{role}.toml"),
                                        ],
                                        run / role,
                                    )
                                )
                        else:
                            owned.append(
                                Owned(
                                    [
                                        tool,
                                        "baseline-server",
                                        "--listen",
                                        address("proxy"),
                                        "--to",
                                        address("target"),
                                        "--io",
                                        profile["io"],
                                    ],
                                    run / "server",
                                )
                            )
                            owned.append(
                                Owned(
                                    [
                                        tool,
                                        "baseline-client",
                                        "--listen",
                                        address("client"),
                                        "--to",
                                        address("proxy"),
                                        "--io",
                                        profile["io"],
                                    ],
                                    run / "client",
                                )
                            )
                        start = (
                            int(time.time() * 1000) + profile["warmup_seconds"] * 1000
                        )
                        common = [
                            "--seconds",
                            str(profile["measurement_seconds"]),
                            "--size",
                            str(profile["payload_bytes"]),
                            "--start-ms",
                            str(start),
                            "--drain-ms",
                            str(profile["drain_ms"]),
                        ]
                        target = Owned(
                            [
                                tool,
                                "responder",
                                "--listen",
                                address("target"),
                                "--reverse-pps",
                                str(profile["pps"] if direction != "forward" else 0),
                                *common,
                            ],
                            run / "receiver",
                        )
                        owned.append(target)
                        sender = Owned(
                            [
                                tool,
                                "sender",
                                "--to",
                                address("client"),
                                "--pps",
                                str(profile["pps"] if direction != "reverse" else 0),
                                *common,
                            ],
                            run / "generator",
                        )
                        owned.append(sender)
                        initial = None
                        final = None
                        wall0 = time.time()
                        mono0 = time.monotonic()
                        deadline = (
                            time.monotonic()
                            + profile["warmup_seconds"]
                            + profile["measurement_seconds"]
                            + profile["drain_ms"] / 1000
                            + 5
                        )
                        while not (target.exited() and sender.exited()):
                            if time.monotonic() > deadline:
                                raise TimeoutError("traffic deadline")
                            current = {
                                "time_ms": int(time.time() * 1000),
                                "roles": {
                                    role: resource(proc.p)
                                    for role, proc in zip(
                                        ["server", "client", "receiver", "generator"],
                                        owned,
                                    )
                                },
                            }
                            samples.append(current)
                            if current["time_ms"] >= start and initial is None:
                                initial = current
                            if (
                                current["time_ms"]
                                <= start + profile["measurement_seconds"] * 1000
                            ):
                                final = current
                            time.sleep(0.05)
                        for proc in owned:
                            proc.stop()
                        tx = json.loads((run / "generator.stdout.ndjson").read_text())
                        rx = json.loads((run / "receiver.stdout.ndjson").read_text())
                        forward = sum(
                            c["unique"] for c in rx["forward_received"].values()
                        )
                        reverse = tx["reverse_received"]["unique"]
                        seconds = profile["measurement_seconds"]
                        size = profile["payload_bytes"]
                        txf = tx["offered"]
                        txr = rx["offered"]
                        stats = {}
                        for role in ["server", "client", "receiver", "generator"]:
                            elapsed = (final["time_ms"] - initial["time_ms"]) / 1000
                            cpu = (
                                final["roles"][role]["cpu_seconds"]
                                - initial["roles"][role]["cpu_seconds"]
                            ) / elapsed
                            stats[role] = {
                                "average_cpu_cores": cpu,
                                "peak_rss_bytes": max(
                                    s["roles"][role]["rss_bytes"] for s in samples
                                ),
                                "owner_average_cpu_cores": (
                                    (
                                        final["roles"][role]["owner_cpu_seconds"]
                                        - initial["roles"][role]["owner_cpu_seconds"]
                                    )
                                    / elapsed
                                    if role in ["server", "client"]
                                    else None
                                ),
                                "owner_headroom_cores": (
                                    max(
                                        0,
                                        1
                                        - (
                                            final["roles"][role]["owner_cpu_seconds"]
                                            - initial["roles"][role][
                                                "owner_cpu_seconds"
                                            ]
                                        )
                                        / elapsed,
                                    )
                                    if role in ["server", "client"]
                                    else None
                                ),
                            }
                        errors = (
                            tx["enqueue_failures"]
                            + rx["enqueue_failures"]
                            + tx["generator_unscheduled"]
                            + rx["generator_unscheduled"]
                        )
                        integrity = sum(
                            c["corrupt"] + c["wrong_direction"] + c["overflow"]
                            for c in rx["forward_received"].values()
                        ) + sum(
                            tx["reverse_received"][k]
                            for k in ["corrupt", "wrong_direction", "overflow"]
                        )
                        clock_drift_ms = (
                            abs((time.time() - wall0) - (time.monotonic() - mono0))
                            * 1000
                        )
                        diagnostic_last = {
                            role: json.loads(
                                (run / f"{role}.stdout.ndjson")
                                .read_text()
                                .strip()
                                .splitlines()[-1]
                            )
                            for role in ["server", "client"]
                        }
                        summary = {
                            "mode": mode,
                            "direction": direction,
                            "pair": pair,
                            "status": (
                                "valid-low-rate"
                                if errors == 0
                                and integrity == 0
                                and tx["rtt_p95_us"] is not None
                                and clock_drift_ms < 10
                                else "invalid"
                            ),
                            "errors": errors,
                            "integrity_errors": integrity,
                            "clock_drift_ms": clock_drift_ms,
                            "forward": {
                                "offered": txf,
                                "delivered_unique": forward,
                                "loss_percent": (
                                    100 * (txf - forward) / txf if txf else None
                                ),
                                "delivered_mbit_s": forward * size * 8 / seconds / 1e6,
                            },
                            "reverse": {
                                "offered": txr,
                                "delivered_unique": reverse,
                                "loss_percent": (
                                    100 * (txr - reverse) / txr if txr else None
                                ),
                                "delivered_mbit_s": reverse * size * 8 / seconds / 1e6,
                            },
                            "rtt_p95_ms": (
                                tx["rtt_p95_us"] / 1000
                                if tx["rtt_p95_us"] is not None
                                else None
                            ),
                            "resources": stats,
                            "release_acceptance_claim": False,
                            "regression_claim": False,
                            "diagnostic_last": diagnostic_last,
                        }
                        write(run / "summary.json", summary)
                        write(run / "samples.json", samples)
                        write(
                            run / "rtt-histogram.json",
                            {
                                "raw_us": tx["rtt_samples_us"],
                                "sample_count": len(tx["rtt_samples_us"]),
                                "p95_us": tx["rtt_p95_us"],
                            },
                        )
                        all_summaries.append(summary)
                except Exception as e:
                    write(
                        run / "failure.json",
                        {"status": "invalid", "reason": type(e).__name__},
                    )
                    raise
                finally:
                    for proc in owned:
                        if not proc.reaped:
                            proc.stop()
    write(out / "summary.json", all_summaries)


if __name__ == "__main__":
    main()
