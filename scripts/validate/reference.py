#!/usr/bin/env python3
"""Complete native reference method. Numerical release qualification is T077."""

import itertools
import json
import math
import os
import platform
import resource as rlimits
from pathlib import Path
import random
import subprocess
import tempfile
import time
from benchmark import ROOT, Owned, resource, sha, wait_udp_bound, write


def baseline_first(seed, pair, phase, sessions, size, direction):
    value = (
        seed
        ^ (pair << 3)
        ^ ({"capacity": 1, "subcapacity": 2, "resource": 3}[phase] << 1)
        ^ (sessions << 11)
        ^ (size << 18)
        ^ ({"forward": 1, "reverse": 2, "simultaneous": 3}[direction] << 32)
    )
    mask = (1 << 64) - 1
    value = (value + 0x9E3779B97F4A7C15) & mask
    value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & mask
    value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & mask
    return bool((value ^ (value >> 31)) & 1)


def plan(profile):
    if (
        profile.get("schema_version") != 2
        or profile.get("coverage") != "full-reference-methodology"
        or profile["architecture"] != "x86_64"
        or profile["topology"] != "two-leg"
        or profile["link_mbit_s_each_direction"] != 1000
        or profile["reference_rtt_ms"] != 20
        or profile["sessions"] != [1, 32]
        or profile["payload_bytes"] != [128, 512, 1200]
        or profile["directions"] != ["forward", "reverse", "simultaneous"]
        or profile["paired_repetitions"] < 5
        or profile["warmup_seconds"] != 60
        or profile["measurement_seconds"] != 300
        or profile["diagnostics"] != "info"
        or profile["search_step_percent"] != 1
        or profile["search_max_steps"] != 100
        or profile["sustainable_loss_percent_max"] != 0.1
        or profile["subcapacity_fraction"] != 0.5
        or profile["resource_mbit_s_each_direction"] != 500
        or profile["resource_sessions"] != 32
        or profile["resource_payload_bytes"] != 1200
        or profile["churn_cycles"] != 1000
        or profile["churn_settle_seconds_max"] != 60
        or profile["generator_cpu_core_max_exclusive"] != 0.9
        or profile["role_cores"] != 4
        or profile["io"] not in ["single", "mmsg"]
        or not 1 <= profile["batch_max"] <= 64
        or profile["cc"] not in ["cubic", "bbr2_gcongestion", "reno"]
        or profile["gso_requested"] != "off"
        or profile["gro_requested"] != "off"
        or not 1 <= profile["drain_ms"] <= 5000
        or not 1 <= profile["search_warmup_seconds"] <= 60
        or not 1 <= profile["search_measurement_seconds"] <= 300
    ):
        raise ValueError("unmatched full reference contract")
    workloads = list(
        itertools.product(
            profile["sessions"], profile["payload_bytes"], profile["directions"]
        )
    )
    rng = random.Random(profile["pair_order_seed"])
    pairs = []
    for pair in range(profile["paired_repetitions"]):
        for phase in ["capacity", "subcapacity"]:
            for sessions, size, direction in workloads:
                pairs.append(
                    dict(
                        pair=pair,
                        phase=phase,
                        sessions=sessions,
                        payload_bytes=size,
                        direction=direction,
                        baseline_first=baseline_first(
                            profile["pair_order_seed"],
                            pair,
                            phase,
                            sessions,
                            size,
                            direction,
                        ),
                    )
                )
        pairs.append(
            dict(
                pair=pair,
                phase="resource",
                sessions=32,
                payload_bytes=1200,
                direction="simultaneous",
                baseline_first=baseline_first(
                    profile["pair_order_seed"],
                    pair,
                    "resource",
                    32,
                    1200,
                    "simultaneous",
                ),
            )
        )
    rng.shuffle(pairs)
    return {
        "searches": [
            {"sessions": s, "payload_bytes": b, "direction": d} for s, b, d in workloads
        ],
        "pairs": pairs,
        "pair_order_seed": profile["pair_order_seed"],
        "release_acceptance_claim": False,
    }


def active(direction, d):
    return direction == "simultaneous" or direction == ["forward", "reverse"][d]


def kernel_socket_sample(process):
    inodes = set()
    for fd in Path(f"/proc/{process.pid}/fd").iterdir():
        try:
            link = os.readlink(fd)
        except FileNotFoundError:
            continue
        if link.startswith("socket:["):
            inodes.add(link[8:-1])
    tx = rx = drops = sockets = 0
    for family in ["udp", "udp6"]:
        for line in (
            Path(f"/proc/{process.pid}/net/{family}").read_text().splitlines()[1:]
        ):
            fields = line.split()
            if len(fields) > 12 and fields[9] in inodes:
                queued_tx, queued_rx = fields[4].split(":")
                tx += int(queued_tx, 16)
                rx += int(queued_rx, 16)
                drops += int(fields[-1])
                sockets += 1
    return {
        "owned_udp_socket_count": sockets,
        "owned_udp_tx_queue_bytes": tx,
        "owned_udp_rx_queue_bytes": rx,
        "owned_udp_socket_drops": drops,
    }


def measured_resource(process):
    return {**resource(process), **kernel_socket_sample(process)}


def resource_sample(roles, owned, time_ms, finish_ms):
    rows, completed = {}, []
    for index, (name, proc) in enumerate(zip(roles, owned)):

        def completed_after_interval():
            if not proc.exited():
                return False
            if index < 2 or time_ms < finish_ms:
                raise RuntimeError("role/traffic exit during measurement")
            completed.append(name)
            return True

        if completed_after_interval():
            continue
        try:
            row = measured_resource(proc.p)
        except (FileNotFoundError, ProcessLookupError):
            # A worker can exit between waitid and any of the /proc reads.
            # Only an observed, expected completion explains a missing path.
            if completed_after_interval():
                continue
            raise
        # Also discard a torn sample (e.g. RSS disappears after reading stat).
        if not completed_after_interval():
            rows[name] = row
    return {"time_ms": time_ms, "roles": rows, "completed_traffic": completed}


def run_once(
    network,
    out,
    profile,
    mode,
    sessions,
    size,
    direction,
    total_pps,
    warmup,
    seconds,
    affinity,
):
    """Identical two-leg endpoint mapping, one or 32 independent source sockets."""
    from network import ns

    network.ensure_alive()
    for name, digest in getattr(network, "expected_binaries", {}).items():
        if sha(ROOT / name) != digest:
            raise RuntimeError("benchmark executable changed after manifest freeze")
    out.mkdir()
    owned = []
    samples = []
    try:
        with tempfile.TemporaryDirectory(prefix="remora-reference-") as tmp:
            f = Path(tmp) / "fixtures"
            subprocess.run(
                [str(ROOT / "scripts/validate/fixtures.sh"), "--output", str(f)],
                check=True,
                stdout=subprocess.DEVNULL,
            )
            ports = json.loads((f / "ports.json").read_text())
            proxy = f'10.203.0.2:{ports["proxy"]}'
            address = lambda key: f"127.0.0.1:{ports[key]}"
            per_flow = total_pps // sessions
            actual_pps = per_flow * sessions
            for role in ["client", "server"]:
                path = f / f"{role}.toml"
                text = path.read_text()
                if role == "server":
                    text = text.replace(address("proxy"), proxy)
                else:
                    text = text.replace(
                        f'server_address = "{address("proxy")}"',
                        f'server_address = "{proxy}"',
                    )
                text += f'\n[io]\nmode = "{profile["io"]}"\nbatch_max = {profile["batch_max"]}\n\n[transport]\ncc = "{profile["cc"]}"\n\n[limits]\nmax_sessions = 32\nmax_sessions_per_identity = 32\nmax_sessions_per_connection = 32\n'
                path.write_text(text)
            tool = str(ROOT / "tools/traffic/target/release/remora-traffic")
            binary = str(ROOT / "target/release/remora")
            for role, pid in [("server", network.server), ("client", network.client)]:
                cmd = (
                    [binary, role, "--config", str(f / f"{role}.toml")]
                    if mode == "tunnel"
                    else [
                        tool,
                        f"baseline-{role}",
                        "--listen",
                        proxy if role == "server" else address("client"),
                        "--to",
                        address("target") if role == "server" else proxy,
                        "--io",
                        profile["io"],
                        "--batch-max",
                        str(profile["batch_max"]),
                    ]
                )
                proc = Owned(
                    ns(pid, "taskset", "-c", ",".join(map(str, affinity[role])), *cmd),
                    out / role,
                )
                owned.append(proc)
                wait_udp_bound(proc, proxy if role == "server" else address("client"))
                expected = network.expected_binaries[
                    (
                        "target/release/remora"
                        if mode == "tunnel"
                        else "tools/traffic/target/release/remora-traffic"
                    )
                ]
                if sha(Path(f"/proc/{proc.p.pid}/exe")) != expected:
                    raise RuntimeError("owned role executable identity mismatch")
            # Every process has two seconds to bind before a common *loaded*
            # warmup. Warmup has disjoint sequence space and is not counted.
            start = int(time.time() * 1000) + (warmup + 2) * 1000
            common = [
                "--seconds",
                str(seconds),
                "--size",
                str(size),
                "--start-ms",
                str(start),
                "--warmup-seconds",
                str(warmup),
                "--drain-ms",
                str(profile["drain_ms"]),
                "--sessions",
                str(sessions),
            ]
            target = Owned(
                ns(
                    network.server,
                    "taskset",
                    "-c",
                    ",".join(map(str, affinity["tools"])),
                    tool,
                    "responder",
                    "--listen",
                    address("target"),
                    "--pps",
                    str(per_flow),
                    "--reverse-pps",
                    str(per_flow if direction != "forward" else 0),
                    *common,
                ),
                out / "receiver",
            )
            owned.append(target)
            wait_udp_bound(target, address("target"))
            for index in range(sessions):
                owned.append(
                    Owned(
                        ns(
                            network.client,
                            "taskset",
                            "-c",
                            str(affinity["tools"][index % len(affinity["tools"])]),
                            tool,
                            "sender",
                            "--to",
                            address("client"),
                            "--session",
                            str(index + 1),
                            "--pps",
                            str(per_flow if direction != "reverse" else 0),
                            "--reverse-pps",
                            str(per_flow),
                            *common,
                        ),
                        out / f"generator-{index}",
                    )
                )
            roles = ["server", "client", "receiver"] + [
                f"generator-{i}" for i in range(sessions)
            ]
            wall0 = time.time()
            mono0 = time.monotonic()
            deadline = mono0 + warmup + seconds + profile["drain_ms"] / 1000 + 10
            while not all(p.exited() for p in owned[2:]):
                if any(p.exited() for p in owned[:2]) or time.monotonic() > deadline:
                    raise RuntimeError("role exit/traffic deadline")
                sample = resource_sample(
                    roles, owned, int(time.time() * 1000), start + seconds * 1000
                )
                samples.append(sample)
                time.sleep(0.05)
            for proc in owned:
                proc.stop()
            tx = [
                json.loads((out / f"generator-{i}.stdout.ndjson").read_text())
                for i in range(sessions)
            ]
            rx = json.loads((out / "receiver.stdout.ndjson").read_text())
            measured = [
                s for s in samples if start <= s["time_ms"] < start + seconds * 1000
            ]
            if len(measured) < 2:
                raise RuntimeError("missing measured resource interval")
            initial, final = measured[0], measured[-1]
            elapsed = (final["time_ms"] - initial["time_ms"]) / 1000
            resources = {
                name: {
                    "average_cpu_cores": (
                        final["roles"][name]["cpu_seconds"]
                        - initial["roles"][name]["cpu_seconds"]
                    )
                    / elapsed,
                    "owner_average_cpu_cores": (
                        final["roles"][name]["owner_cpu_seconds"]
                        - initial["roles"][name]["owner_cpu_seconds"]
                    )
                    / elapsed,
                    "peak_rss_bytes": max(
                        s["roles"][name]["rss_bytes"]
                        for s in samples
                        if name in s["roles"]
                    ),
                }
                for name in roles
            }
            diagnostics = {
                role: json.loads(
                    (out / f"{role}.stdout.ndjson").read_text().strip().splitlines()[-1]
                )
                for role in ["server", "client"]
            }

            def integrity(c):
                return sum(
                    c[k]
                    for k in ["corrupt", "wrong_direction", "overflow", "duplicates"]
                )

            forward = sum(c["unique"] for c in rx["forward_received"].values())
            reverse = sum(t["reverse_received"]["unique"] for t in tx)
            latencies = sorted(v for t in tx for v in t["rtt_samples_us"])
            result = dict(
                offered=[sum(t["offered"] for t in tx), rx["offered"]],
                submitted=[sum(t["submitted"] for t in tx), rx["submitted"]],
                unique=[forward, reverse],
                unique_per_session=[
                    [
                        rx["forward_received"].get(str(i + 1), {}).get("unique", 0),
                        t["reverse_received"]["unique"],
                    ]
                    for i, t in enumerate(tx)
                ],
                generator_errors=sum(
                    v.get("enqueue_failures", 0)
                    + v.get("generator_unscheduled", 0)
                    + v.get("generator_excess", 0)
                    + v.get("warmup_failures", 0)
                    for v in [rx, *tx]
                ),
                integrity_errors=sum(
                    integrity(c) for c in rx["forward_received"].values()
                )
                + sum(integrity(t["reverse_received"]) for t in tx),
                generator_cpu_max=max(
                    resources[name]["average_cpu_cores"] for name in roles[2:]
                ),
                clock_drift_ms=abs((time.time() - wall0) - (time.monotonic() - mono0))
                * 1000,
                same_clock_rtt=True,
                diagnostics=profile["diagnostics"],
                coverage_lost=sum(
                    d.get("coverage_lost", 0)
                    + d.get("status_write_failures", 0)
                    + d.get("diagnostics_dropped", 0)
                    for d in diagnostics.values()
                ),
                counter_overflow=any(
                    d.get("counter_overflow", False)
                    or d.get("profile", {}).get("io", {}).get("overflowed", False)
                    or d.get("profile", {}).get("factory", {}).get("overflowed", False)
                    for d in diagnostics.values()
                ),
                sample_count=len(measured),
                rtt_samples_per_session=[len(t["rtt_samples_us"]) for t in tx],
                rtt_p95_ms=(
                    latencies[math.ceil(len(latencies) * 0.95) - 1] / 1000
                    if latencies
                    else 0
                ),
                resources=[resources[role] for role in ["client", "server"]],
            )
            write(out / "summary.json", result)
            write(out / "samples.json", samples)
            write(
                out / "rtt-histogram.json",
                {
                    "raw_us": latencies,
                    "per_session_count": result["rtt_samples_per_session"],
                },
            )
            write(out / "diagnostics.json", diagnostics)
            write(out / "resources.json", resources)
            return result, actual_pps
    except BaseException as e:
        write(out / "failure.json", {"status": "invalid", "reason": type(e).__name__})
        raise
    finally:
        for proc in reversed(owned):
            proc.stop()


def spread_report(pairs):
    """Describe repeat variability; never use a mean to decide acceptance."""
    groups = {}
    for pair in pairs:
        key = (
            pair["phase"],
            pair["sessions"],
            pair["payload_bytes"],
            pair["direction"],
        )
        for mode in ["baseline", "tunnel"]:
            row = pair[mode]
            group = groups.setdefault((*key, mode), {})
            values = {
                "rtt_p95_ms": row["rtt_p95_ms"],
                "sample_count": row["sample_count"],
            }
            for index, direction in enumerate(["forward", "reverse"]):
                if row["offered"][index]:
                    values[direction + "_delivered_mbit_s"] = (
                        row["unique"][index]
                        * pair["payload_bytes"]
                        * 8
                        / pair["measurement_seconds"]
                        / 1e6
                    )
                    values[direction + "_loss_percent"] = (
                        100
                        * (row["offered"][index] - row["unique"][index])
                        / row["offered"][index]
                    )
            for role, resource in zip(["client", "server"], row["resources"]):
                for metric, value in resource.items():
                    values[role + "_" + metric] = value
            for metric, value in values.items():
                group.setdefault(metric, []).append(value)
    return [
        {
            "phase": key[0],
            "sessions": key[1],
            "payload_bytes": key[2],
            "direction": key[3],
            "mode": key[4],
            "metrics": {
                name: {
                    "sample_count": len(values),
                    "values": values,
                    "min": min(values),
                    "max": max(values),
                    "spread": max(values) - min(values),
                }
                for name, values in metrics.items()
            },
        }
        for key, metrics in sorted(groups.items())
    ]


def execute(network, out, profile, smoke=False, selected_only=False):
    if platform.machine() != profile["architecture"]:
        raise RuntimeError("native architecture/profile mismatch")
    if smoke:
        profile = {**profile, "drain_ms": 200}
    method = plan(profile)
    affinity = sorted(os.sched_getaffinity(0))
    if len(affinity) < 10:
        raise RuntimeError(
            "four disjoint role cores plus generator/endpoint cores required"
        )
    placement = {"client": affinity[:4], "server": affinity[4:8], "tools": affinity[8:]}
    network.expected_binaries = {
        name: sha(ROOT / name)
        for name in [
            "target/release/remora",
            "tools/traffic/target/release/remora-traffic",
        ]
    }
    write(out / "method.json", method)
    write(
        out / "manifest.json",
        {
            "profile": profile,
            "placement": placement,
            "network": network.snapshot(),
            "remora_sha256": network.expected_binaries["target/release/remora"],
            "traffic_sha256": network.expected_binaries[
                "tools/traffic/target/release/remora-traffic"
            ],
            "revision": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
            ).strip(),
            "source_sha256": {
                str(p.relative_to(ROOT)): sha(p)
                for base in ["src", "tools/traffic/src", "scripts/validate"]
                for p in sorted((ROOT / base).rglob("*"))
                if p.is_file() and "__pycache__" not in p.parts
            },
            "locks": {
                p: sha(ROOT / p) for p in ["Cargo.lock", "tools/traffic/Cargo.lock"]
            },
            "cpu": Path("/proc/cpuinfo").read_text(),
            "kernel": os.uname().release,
            "architecture": platform.machine(),
            "compiler": subprocess.check_output(
                ["rustc", "--version"], text=True
            ).strip(),
            "build_profile": "locked release",
            "build_inputs": {
                p: sha(ROOT / p)
                for p in [
                    "Cargo.toml",
                    "tools/traffic/Cargo.toml",
                    "rust-toolchain.toml",
                ]
            },
            "process_limits": {
                "nofile": rlimits.getrlimit(rlimits.RLIMIT_NOFILE),
                "nproc": rlimits.getrlimit(rlimits.RLIMIT_NPROC),
            },
            "cpu_governors": {
                str(cpu): (
                    Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor")
                    .read_text()
                    .strip()
                    if Path(
                        f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor"
                    ).exists()
                    else "unreported"
                )
                for cpu in affinity
            },
            "requested_socket_buffers": {
                "outer_bytes": 262144,
                "application_bytes": 65536,
            },
            "kernel_socket_accounting": "owned /proc UDP queue bytes/drop counts in samples; includes skb queue charges, not total socket overhead or reserved buffer maxima; effective buffer maxima retained in per-role diagnostics",
            "namespace_veth_evidence": True,
            "clock": "common host clock; monotonic generator schedules and same-sender RTT; drift measured per run",
            "smoke": smoke,
            "release_acceptance_claim": False,
        },
    )
    searches = []
    rates = {}
    search_selection = method["searches"]
    if selected_only:
        search_selection = [
            k
            for k in search_selection
            if (k["sessions"], k["payload_bytes"], k["direction"])
            in [(1, 128, "forward"), (32, 1200, "simultaneous")]
        ]
    for key in search_selection:
        s, b, d = key["sessions"], key["payload_bytes"], key["direction"]
        steps = []
        selected = None
        chosen = 0
        for index in range(1, 2 if smoke else profile["search_max_steps"] + 1):
            offered = 1000 if smoke else index * 10 * 1_000_000 // (b * 8)
            row, actual = run_once(
                network,
                out / f"search-{s}-{b}-{d}-{index}",
                profile,
                "baseline",
                s,
                b,
                d,
                offered,
                1 if smoke else profile["search_warmup_seconds"],
                1 if smoke else profile["search_measurement_seconds"],
                placement,
            )
            loss = [
                (
                    100 * (row["offered"][i] - row["unique"][i]) / row["offered"][i]
                    if row["offered"][i]
                    else 0
                )
                for i in range(2)
            ]
            valid = (
                row["generator_errors"] == 0
                and row["integrity_errors"] == 0
                and row["generator_cpu_max"] < 0.9
                and row["clock_drift_ms"] < 10
                and row["coverage_lost"] == 0
                and not row["counter_overflow"]
            )
            steps.append(
                dict(
                    payload_mbit_s=index * 10.0,
                    loss_percent=loss,
                    generator_valid=valid,
                )
            )
            if not valid:
                write(
                    out / f"invalid-search-{s}-{b}-{d}.json",
                    {
                        "status": "invalid",
                        "reason": "generator/clock/diagnostics; cannot establish capacity",
                        "steps": steps,
                    },
                )
                raise RuntimeError("invalid capacity calibration")
            if all(loss[i] <= 0.1 for i in range(2) if active(d, i)):
                selected = index - 1
                chosen = actual
            else:
                break
        if selected is None:
            raise RuntimeError("no sustainable baseline bracket")
        searches.append(dict(**key, steps=steps, selected_step=selected))
        rates[(s, b, d)] = chosen
    write(out / "sustainable-searches.json", searches)
    pairs = []
    selection = (
        [r for r in method["pairs"] if r["pair"] == 0] if smoke else method["pairs"]
    )
    if selected_only:
        selection = [
            k
            for k in selection
            if (k["sessions"], k["payload_bytes"], k["direction"]) in rates
        ]
    for index, key in enumerate(selection):
        s, b, d = key["sessions"], key["payload_bytes"], key["direction"]
        sustainable = rates[(s, b, d)]
        offered = (
            # Permit the baseline's <=0.1% loss while still delivering >=500M.
            # This is offered-load headroom, not a relaxed delivery threshold.
            math.ceil(500e6 / (0.999 * b * 8 * s)) * s
            if key["phase"] == "resource"
            else (
                sustainable if key["phase"] == "capacity" else sustainable // 2 // s * s
            )
        )
        if smoke and key["phase"] == "resource":
            offered = 1000 // s * s
        measurements = {}
        actual = 0
        for mode in (
            ["baseline", "tunnel"] if key["baseline_first"] else ["tunnel", "baseline"]
        ):
            print(
                f"Reference {index+1}/{len(selection)}: {key['phase']} {s}x{b} {d} {mode}",
                flush=True,
            )
            measurements[mode], actual = run_once(
                network,
                out / f"pair-{index}-{mode}",
                profile,
                mode,
                s,
                b,
                d,
                offered,
                1 if smoke else 60,
                1 if smoke else 300,
                placement,
            )
        pairs.append(
            dict(
                **key,
                warmup_seconds=1 if smoke else 60,
                measurement_seconds=1 if smoke else 300,
                offered_pps_per_direction=[
                    actual if active(d, i) else 0 for i in range(2)
                ],
                sustainable_pps_per_direction=[
                    sustainable if active(d, i) else 0 for i in range(2)
                ],
                **measurements,
            )
        )
        write(out / f"paired-{index}.json", pairs[-1])
    if not smoke:
        subprocess.run(
            [
                "python3",
                str(ROOT / "scripts/validate/concurrency.py"),
                "--scenario",
                "churn",
                "--cycles",
                "1000",
                "--output",
                str(out / "churn"),
            ],
            check=True,
        )
        churn = json.loads((out / "churn/summary.json").read_text())
        churn = dict(
            cycles=churn["cycles"],
            settled_seconds=churn["settled_seconds"],
            rss_growth_bytes=churn["rss_growth_bytes"],
            valid=churn["status"] == "passed",
        )
    else:
        churn = dict(cycles=0, settled_seconds=0, rss_growth_bytes=[], valid=False)
    suite = dict(
        schema_version=2,
        topology="two-leg",
        link_mbit_s=1000,
        reference_rtt_ms=20,
        architecture="x86_64",
        role_available_cores=[4, 4],
        network_settings_recorded=True,
        pair_order_seed=profile["pair_order_seed"],
        repetitions=1 if smoke else profile["paired_repetitions"],
        searches=searches,
        runs=pairs,
        churn=churn,
    )
    write(out / "suite.json", suite)
    write(out / "spread.json", spread_report(pairs))
    # The same Rust validator that is covered by deterministic adversarial
    # tests decides every criterion. Smoke is deliberately incomplete/invalid.
    result = subprocess.run(
        [
            str(ROOT / "tools/traffic/target/release/remora-traffic"),
            "validate-results",
            "--input",
            str(out / "suite.json"),
            "--output",
            str(out / "criteria.json"),
        ]
    )
    if not (out / "criteria.json").is_file():
        raise RuntimeError(
            "validator/schema execution failed; not an expected incomplete smoke outcome"
        )
    if smoke and json.loads((out / "criteria.json").read_text())["status"] != "invalid":
        raise RuntimeError("smoke was incorrectly labelled release qualified")
    if result.returncode and not smoke:
        raise RuntimeError(
            "reference numerical criteria failed/invalid; artifacts retained"
        )
