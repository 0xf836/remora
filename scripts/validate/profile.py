#!/usr/bin/env python3
"""Sequential owned low-rate experiments; never a release capacity claim."""

import argparse
import json
import random
import shutil
import subprocess
import time
import tomllib
from pathlib import Path
from benchmark import ROOT, sha, write


def toml(profile):
    lines = []
    for k, v in profile.items():
        if k != "limits":
            lines.append(f"{k} = {json.dumps(v)}")
    if profile.get("limits"):
        lines += ["[limits]"] + [f"{k} = {v}" for k, v in profile["limits"].items()]
    return "\n".join(lines) + "\n"


def freeze(reference, output):
    """Prospective workload bounds from retained repetitions, never candidates."""
    rows = json.loads((reference / "summary.json").read_text())
    workloads = []
    for mode in ["baseline", "tunnel"]:
        for direction in ["forward", "reverse", "simultaneous"]:
            group = [
                r for r in rows if r["mode"] == mode and r["direction"] == direction
            ]
            if len(group) < 5 or any(r["status"] != "valid-low-rate" for r in group):
                raise ValueError("insufficient valid reference variability")
            metrics = {}

            def bound(key, values, unit, floor, lower=False):
                spread = max(values) - min(values)
                allowance = max(2 * spread, floor)
                metrics[key] = {
                    "unit": unit,
                    "candidate_min" if lower else "candidate_max": (
                        max(0, min(values) - allowance)
                        if lower
                        else max(values) + allowance
                    ),
                }

            for d in ["forward", "reverse"]:
                if direction == d or direction == "simultaneous":
                    # Two missing messages of 3000 is the declared resolution.
                    bound(
                        f"{d}_delivered_mbit_s",
                        [r[d]["delivered_mbit_s"] for r in group],
                        "Mbit/s",
                        2 * 128 * 8 / 3 / 1e6,
                        True,
                    )
                    bound(
                        f"{d}_loss_percent",
                        [r[d]["loss_percent"] for r in group],
                        "percentage points",
                        100 * 2 / 3000,
                    )
            bound("rtt_p95_ms", [r["rtt_p95_ms"] for r in group], "ms", 0.25)
            for role in ["client", "server"]:
                for metric in ["average_cpu_cores", "owner_average_cpu_cores"]:
                    # Actual /proc sampling duration is slightly shorter than 3s.
                    bound(
                        f"{role}_{metric}",
                        [r["resources"][role][metric] for r in group],
                        "cores",
                        0.02,
                    )
                bound(
                    f"{role}_peak_rss_bytes",
                    [r["resources"][role]["peak_rss_bytes"] for r in group],
                    "bytes",
                    262144,
                )
            workloads.append({"mode": mode, "direction": direction, "metrics": metrics})
    policy = {
        "schema_version": 1,
        "frozen": True,
        "scope": "incremental-five-pair-128-byte-1000pps-loopback",
        "method": "every matched candidate run; reference extrema plus two observed repeat spreads or measurement floors, no averaging failures",
        "limitations": [
            "five short pairs are a noise observation, not a capacity or statistical confidence claim",
            "new workloads require new prospective references; SC criteria unchanged",
        ],
        "tolerance_basis": {
            "delivery": "two messages per 3 seconds",
            "rtt": "2x reference spread or 0.25ms scheduling floor",
            "cpu": "2x reference spread or two 10ms ticks per approximately 3 seconds rounded up to 0.02 cores",
            "rss": "2x reference spread or 256KiB allocation floor",
        },
        "workloads": workloads,
    }
    template = output / "prospective-policy-template.json"
    write(template, policy)
    subprocess.run(
        [
            "python3",
            str(ROOT / "scripts/validate/freeze-policy.py"),
            "--reference",
            str(reference),
            "--policy",
            str(template),
            "--output",
            str(output / "comparison-policy.json"),
        ],
        check=True,
    )


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--experiments", type=Path, default=ROOT / "benchmarks/experiments.toml"
    )
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--resume", action="store_true")
    args = p.parse_args()
    config = tomllib.loads(args.experiments.read_text())
    if not args.resume:
        args.output.mkdir(parents=True, exist_ok=False)
    elif not args.output.is_dir():
        raise ValueError("missing owned experiment directory")
    cases = list(config["cases"])
    random.Random(config["seed"]).shuffle(cases)
    if args.resume:
        existing = json.loads((args.output / "manifest.json").read_text())
        if existing["experiments_sha256"] != sha(args.experiments) or existing[
            "order"
        ] != [c["name"] for c in cases]:
            raise ValueError("resume configuration/order mismatch")
    if not args.resume:
        write(
            args.output / "manifest.json",
            {
                "experiments_sha256": sha(args.experiments),
                "seed": config["seed"],
                "order": [c["name"] for c in cases],
                "release_acceptance_claim": False,
                "syscalls": "actual bounded adapter counters; not syscall latency",
                "wakeups": "owner voluntary/involuntary context switches; owner visits are not OS wakeups",
                "allocations": "payload slabs and quiche custom buffer factory only; excludes crypto/metadata allocations",
                "copies": "explicit packet queue/receive/factory boundaries only",
                "cpu_profile": "perf cpu-clock user IP samples only; no callgraphs, stack or payload dumps",
                "coarse_timer": "isolated Python 1ms sleep baseline, not a substitute datapath",
            },
        )
    for case in cases:
        name = case.pop("name")
        if args.resume and (args.output / name / "summary.json").is_file():
            rows = json.loads((args.output / name / "summary.json").read_text())
            if len(rows) != 12 or any(r["status"] != "valid-low-rate" for r in rows):
                raise ValueError("invalid retained experiment")
            continue
        if (args.output / name).exists():
            raise ValueError("incomplete experiment retained; use a fresh output")
        profile = dict(
            schema_version=1,
            coverage=config["coverage"],
            architecture="x86_64",
            topology="two-leg",
            sessions=1,
            payload_bytes=128,
            pps=config["pps"],
            warmup_seconds=config["warmup_seconds"],
            measurement_seconds=config["measurement_seconds"],
            drain_ms=200,
            paired_repetitions=config["paired_repetitions"],
            diagnostics="info",
            gso_requested="off",
            gro_requested="off",
            **case,
        )
        path = args.output / f"{name}.toml"
        path.write_text(toml(profile))
        print(f"Profiling {name}", flush=True)
        subprocess.run(
            [
                str(ROOT / "scripts/validate/bench.sh"),
                "--profile",
                str(path),
                "--output",
                str(args.output / name),
                "--cpu-profile",
            ],
            check=True,
        )
    # Independent low-rate fairness/control experiment; not full SC-009 duration.
    subprocess.run(
        [
            "python3",
            str(ROOT / "scripts/validate/concurrency.py"),
            "--scenario",
            "isolation",
            "--seconds",
            "60",
            "--output",
            str(args.output / "fairness32"),
        ],
        check=True,
    )
    lateness = []
    start = time.monotonic_ns()
    for i in range(1, 401):
        deadline = start + i * 50000
        while time.monotonic_ns() < deadline:
            time.sleep(0.001)
        lateness.append((time.monotonic_ns() - deadline) / 1000)
    write(
        args.output / "coarse-timer.json",
        {
            "clock": "monotonic",
            "deadline_step_us": 50,
            "sleep_quantum_us": 1000,
            "lateness_us": lateness,
        },
    )
    reference_profile = tomllib.loads((ROOT / "benchmarks/mvp-x86_64.toml").read_text())
    reference_profile["paired_repetitions"] = 5
    reference_path = args.output / "reference.toml"
    reference_path.write_text(toml(reference_profile))
    reference = args.output / "pre-refinement"
    print("Collecting prospective reference (five pairs)", flush=True)
    subprocess.run(
        [
            str(ROOT / "scripts/validate/bench.sh"),
            "--profile",
            str(reference_path),
            "--output",
            str(reference),
        ],
        check=True,
    )
    # Preserve the exact test-only generator/baseline for post-harness comparisons.
    shutil.copy2(
        ROOT / "tools/traffic/target/release/remora-traffic",
        args.output / "reference-traffic",
    )
    freeze(reference, args.output)
    print("Prospective policy frozen; no candidate datapath selected", flush=True)


if __name__ == "__main__":
    main()
