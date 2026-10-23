#!/usr/bin/env python3
"""Owned loopback fixtures, two role processes, independent directional traffic.
No secrets, credentials or TLS keys are retained in result artifacts.
"""

import argparse, hashlib, json, os, platform, signal, subprocess, tempfile, time, tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def machine_diagnostics(text, level="info", interval=1000, extra=""):
    """Pin the collector schema, never inherit operator logging defaults."""
    base = text.replace('[diagnostics]\nformat = "json"\nsnapshots = true\n', '')
    return base + (f'\n[diagnostics]\nformat = "json"\nsnapshots = true\n'
                   f'level = "{level}"\nsnapshot_interval_ms = {interval}\n' + extra)


def last_snapshot(path, allow_incomplete=False):
    """Missing/wrong-format status is invalid evidence, never zero metrics."""
    records = []
    for line in path.read_text().splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            if allow_incomplete:
                continue
            raise RuntimeError("collector expected schema-1 JSON snapshots") from None
        if (not isinstance(record, dict) or record.get("schema_version") != 1
                or record.get("event") != "snapshot"
                or not isinstance(record.get("counters"), dict)
                or not isinstance(record.get("failures"), list)
                or any(type(record.get(key)) is not int for key in
                       ["coverage_lost", "status_write_failures", "diagnostics_dropped"])
                or type(record.get("counter_overflow")) is not bool):
            raise RuntimeError("collector received an invalid or incomplete snapshot schema")
        records.append(record)
    if not records and not allow_incomplete:
        raise RuntimeError("collector received no diagnostic snapshots; check the JSON profile")
    return records[-1] if records else {}


def runtime_failures(path):
    """Legacy executable markers, not English operator-message parsing."""
    return [line.removeprefix("runtime failed: ") for line in path.read_text().splitlines()
            if line.startswith("runtime failed: ")]


def install_termination_handler():
    """Let existing finally blocks reap owned groups on timed/caller stops."""
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"runner received signal {signum}")
    for signum in [signal.SIGTERM, signal.SIGHUP]:
        signal.signal(signum, interrupted)


class Owned:
    def __init__(self, cmd, path, blocked_stdout=False, cpu=None):
        self.out = open(path.with_suffix(".stdout.ndjson"), "wb")
        self.err = open(path.with_suffix(".stderr.ndjson"), "wb")
        self.blocked_stdout = blocked_stdout
        self.stop_seconds = None
        self.p = subprocess.Popen(
            ["taskset", "-c", str(cpu), *cmd] if cpu is not None else cmd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE if blocked_stdout else self.out,
            stderr=self.err,
            start_new_session=True,
        )
        if blocked_stdout:
            import fcntl
            fcntl.fcntl(self.p.stdout.fileno(), fcntl.F_SETPIPE_SZ, 4096)
        self.reaped = False
        self._wait_for_cpu(cpu)

    def _wait_for_cpu(self, cpu):
        if cpu is None:
            return
        deadline = time.monotonic() + 3
        # Popen can return while taskset still has the inherited parent mask.
        # Sampling starts only after the assigned mask is installed; all later
        # Rust/traffic worker threads inherit that mask across exec/spawn.
        while os.sched_getaffinity(self.p.pid) != {cpu}:
            if self.exited() or time.monotonic() >= deadline:
                self.stop()
                raise RuntimeError("role CPU assignment readiness failed")
            time.sleep(0.001)

    def exited(self):
        # Never reap while the group identity is still needed for cleanup.
        return (
            os.waitid(os.P_PID, self.p.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            is not None
        )

    def stop(self):
        if self.reaped:
            return
        stopping = time.monotonic()
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
        self.stop_seconds = time.monotonic() - stopping
        if self.blocked_stdout:
            self.out.write(self.p.stdout.read())
            self.p.stdout.close()
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
        "voluntary_context_switches": int(
            next(
                (
                    l.split()[1]
                    for l in status
                    if l.startswith("voluntary_ctxt_switches:")
                ),
                0,
            )
        ),
        "involuntary_context_switches": int(
            next(
                (
                    l.split()[1]
                    for l in status
                    if l.startswith("nonvoluntary_ctxt_switches:")
                ),
                0,
            )
        ),
    }


def main():
    install_termination_handler()
    a = argparse.ArgumentParser()
    a.add_argument(
        "--profile", "--manifest", dest="profile", default="benchmarks/mvp-x86_64.toml"
    )
    a.add_argument("--output", required=True)
    a.add_argument("--traffic-binary", type=Path)
    a.add_argument("--remora-binary", type=Path)
    a.add_argument("--remora-input-manifest", type=Path)
    a.add_argument("--diagnostics-kind", choices=["repeatability", "regression"], default="repeatability")
    a.add_argument("--baseline-binary", type=Path)
    a.add_argument("--baseline-input-manifest", type=Path)
    a.add_argument("--regression-plan", type=Path)
    a.add_argument("--proposal", type=Path)
    a.add_argument("--diagnostics-stage", choices=["reference", "freeze", "candidate"])
    a.add_argument("--reference", type=Path)
    a.add_argument("--policy", type=Path)
    a.add_argument("--cpu-profile", action="store_true")
    a.add_argument("--plan-only", action="store_true")
    a.add_argument("--development-smoke", action="store_true")
    a.add_argument("--smoke-selected", action="store_true")
    args = a.parse_args()
    profile = tomllib.loads(Path(args.profile).read_text())
    if profile.get("coverage") == "diagnostics-operator-overhead":
        if args.diagnostics_kind == "regression":
            from diagnostic_regression import dispatch
        else:
            from diagnostic_benchmark import dispatch
        return dispatch(args, profile)
    if profile.get("coverage") == "full-reference-methodology":
        from reference import plan

        method = plan(profile)
        if args.plan_only:
            out = Path(args.output)
            out.mkdir(parents=True, exist_ok=False)
            write(out / "plan.json", method)
        else:
            cmd = [
                str(ROOT / "scripts/validate/network.sh"),
                "--scenario",
                "reference",
                "--output",
                args.output,
                "--benchmark-profile",
                str(Path(args.profile).resolve()),
            ]
            if args.development_smoke:
                cmd.append("--development-smoke")
            if args.smoke_selected:
                cmd.append("--smoke-selected")
            subprocess.run(cmd, check=True)
        return
    if args.plan_only or args.development_smoke or args.smoke_selected:
        raise ValueError("reference-only option")
    if (
        profile["topology"] != "two-leg"
        or profile["sessions"] != 1
        or profile["architecture"] != platform.machine()
        or profile["io"] not in ["single", "mmsg"]
        or profile["cc"] not in ["cubic", "bbr2_gcongestion", "reno"]
        or profile["diagnostics"] not in ["info", "error", "trace", "filtered-qlog"]
        or profile["gso_requested"] != "off"
        or profile["gro_requested"] != "off"
    ):
        raise ValueError("unsupported early coverage/topology/platform")
    tool_path = (
        args.traffic_binary or ROOT / "tools/traffic/target/release/remora-traffic"
    ).resolve()
    out = Path(args.output)
    out.mkdir(parents=True, exist_ok=False)
    allowed = sorted(os.sched_getaffinity(0))
    if profile.get("role_cpu_pinning") and len(allowed) != 4:
        raise ValueError("pinned diagnostics require four declared role CPUs")
    role_cpus = dict(zip(["client", "server", "generator", "receiver"], allowed)) if profile.get("role_cpu_pinning") else {}
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
            "cpu_frequency_policy": {
                str(cpu): {name: (Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq") / name).read_text().strip()
                    if (Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq") / name).exists() else None
                    for name in ["scaling_governor", "scaling_driver", "scaling_min_freq", "scaling_max_freq"]}
                for cpu in sorted(os.sched_getaffinity(0))
            },
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
            for base in ["src", "tools/traffic/src", "scripts/validate", "vendor"]
            for p in sorted((ROOT / base).rglob("*"))
            if p.is_file() and "__pycache__" not in p.parts and "target" not in p.parts and p.suffix != ".pyc"
        },
        "locks": {p: sha(ROOT / p) for p in ["Cargo.lock", "tools/traffic/Cargo.lock"]},
        "binaries": {
            p: sha(tool_path if p.startswith("tools/") else (args.remora_binary or ROOT / p))
            for p in [
                "target/release/remora",
                "tools/traffic/target/release/remora-traffic",
            ]
        },
        "role_cpu_affinity": role_cpus,
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
    if args.remora_input_manifest:
        from diagnostic_regression import checked_input
        identity = checked_input(args.remora_input_manifest, args.remora_binary)
        manifest["remora_input"] = identity
        manifest["source_sha256"] = {**{k: v for k, v in manifest["source_sha256"].items() if not k.startswith("src/")}, **identity["sources"]}
        manifest["locks"]["Cargo.lock"] = identity["sources"]["Cargo.lock"]
    write(out / "manifest.json", manifest)
    all_summaries = []
    tool = str(tool_path)
    binary = str(args.remora_binary or ROOT / "target/release/remora")
    for pair in range(profile["paired_repetitions"]):
        for direction in profile.get("directions", ["forward", "reverse", "simultaneous"]):
            for mode in (["tunnel"] if profile.get("tunnel_only") else
                (["baseline", "tunnel"] if pair % 2 == 0 else ["tunnel", "baseline"])):
                run = out / f"{pair}-{direction}-{mode}"
                run.mkdir()
                owned = []
                samples = []
                profilers = []
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
                                    f'\n[transport]\ncc = "{profile["cc"]}"\n'
                                    + (
                                        "\n[limits]\n"
                                        + "".join(
                                            f"{k} = {v}\n"
                                            for k, v in profile["limits"].items()
                                        )
                                        if profile.get("limits")
                                        else ""
                                    )
                                )
                        if profile.get("tunnel_only"):
                            for role in ["client", "server"]:
                                qlog = run / f"{role}-qlog"
                                extra = ""
                                if profile["diagnostics"] == "filtered-qlog":
                                    qlog.mkdir(mode=0o700)
                                    extra = f'qlog_directory = "{qlog.resolve()}"\n'
                                level = "info" if profile["diagnostics"] == "filtered-qlog" else profile["diagnostics"]
                                config = f / f"{role}.toml"
                                config.write_text(machine_diagnostics(config.read_text(), level, extra=extra))
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
                                        blocked_stdout=profile.get("blocked_sink", False),
                                        cpu=role_cpus.get(role),
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
                                        "--batch-max",
                                        str(profile["batch_max"]),
                                        "--application-pool-bytes",
                                        str(
                                            profile.get("limits", {}).get(
                                                "application_pool_bytes", 8388608
                                            )
                                        ),
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
                                        "--batch-max",
                                        str(profile["batch_max"]),
                                        "--application-pool-bytes",
                                        str(
                                            profile.get("limits", {}).get(
                                                "application_pool_bytes", 8388608
                                            )
                                        ),
                                    ],
                                    run / "client",
                                )
                            )
                        wait_udp_bound(owned[0], address("proxy"))
                        wait_udp_bound(owned[1], address("client"))
                        if (
                            args.cpu_profile
                            and mode == "tunnel"
                            and pair == 0
                            and direction == "simultaneous"
                        ):
                            for role, proc in zip(["server", "client"], owned):
                                profilers.append(
                                    Owned(
                                        [
                                            "perf",
                                            "record",
                                            "-e",
                                            "cpu-clock:u",
                                            "-F",
                                            "99",
                                            "-p",
                                            str(proc.p.pid),
                                            "-o",
                                            str(run / f"{role}.cpu.perf.data"),
                                            "--",
                                            "sleep",
                                            "2",
                                        ],
                                        run / f"{role}-profiler",
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
                            cpu=role_cpus.get("receiver"),
                        )
                        owned.append(target)
                        wait_udp_bound(target, address("target"))
                        sender = Owned(
                            [
                                tool,
                                "sender",
                                "--to",
                                address("client"),
                                "--pps",
                                str(profile["pps"] if direction != "reverse" else 0),
                                "--reverse-pps",
                                str(profile["pps"] if direction != "forward" else 0),
                                *common,
                            ],
                            run / "generator",
                            cpu=role_cpus.get("generator"),
                        )
                        owned.append(sender)
                        initial = None
                        final = None
                        blocked_observed = {role: False for role in ["client", "server"]}
                        affinity_ok = True
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
                            if profile.get("blocked_sink"):
                                for role, proc in zip(["server", "client"], owned):
                                    for task in Path(f"/proc/{proc.p.pid}/task").glob("*/wchan"):
                                        try:
                                            blocked_observed[role] |= "pipe_write" in task.read_text()
                                        except FileNotFoundError:
                                            pass
                            if role_cpus:
                                for role, proc in zip(["server", "client", "receiver", "generator"], owned):
                                    for task in Path(f"/proc/{proc.p.pid}/task").iterdir():
                                        try:
                                            affinity_ok &= os.sched_getaffinity(int(task.name)) == {role_cpus[role]}
                                        except ProcessLookupError:
                                            pass
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
                                "owner_voluntary_context_switches": final["roles"][
                                    role
                                ]["voluntary_context_switches"]
                                - initial["roles"][role]["voluntary_context_switches"],
                                "owner_involuntary_context_switches": final["roles"][
                                    role
                                ]["involuntary_context_switches"]
                                - initial["roles"][role][
                                    "involuntary_context_switches"
                                ],
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
                        diagnostic_last = {}
                        for role, proc in zip(["server", "client"], owned):
                            lines = (run / f"{role}.stdout.ndjson").read_text().splitlines()
                            records = []
                            for line in lines:
                                try: records.append(json.loads(line))
                                except json.JSONDecodeError:
                                    if not profile.get("blocked_sink"): raise
                            diagnostic_last[role] = (
                                last_snapshot(run / f"{role}.stdout.ndjson", profile.get("blocked_sink", False))
                                if mode == "tunnel" else (records[-1] if records else {})
                            )
                            if profile.get("blocked_sink"):
                                diagnostic_last[role]["collection_incomplete"] = True
                        qlog_files = {role: [p.stat().st_size for p in (run / f"{role}-qlog").glob("*.ndjson")] for role in ["client", "server"]}
                        exits = {role: proc.p.returncode for role, proc in zip(["server", "client"], owned)}
                        stops = {role: proc.stop_seconds for role, proc in zip(["server", "client"], owned)}
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
                            "role_exit_codes": exits,
                            "role_runtime_failures": {
                                role: runtime_failures(run / f"{role}.stderr.ndjson")
                                for role in ["client", "server"]
                            },
                            "stop_seconds": stops,
                            "writer_blocked_observed": blocked_observed,
                            "role_cpu_affinity_ok": affinity_ok if role_cpus else None,
                            "qlog_file_bytes": qlog_files,
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
                        if profile.get("tunnel_only"):
                            print(json.dumps({"diagnostics": profile["diagnostics"], "pair": pair, "direction": direction, "blocked": profile.get("blocked_sink", False), "status": summary["status"]}), flush=True)
                except Exception as e:
                    write(
                        run / "failure.json",
                        {"status": "invalid", "reason": type(e).__name__},
                    )
                    raise
                finally:
                    for proc in profilers + owned:
                        if not proc.reaped:
                            proc.stop()
    write(out / "summary.json", all_summaries)


if __name__ == "__main__":
    main()
