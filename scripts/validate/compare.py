#!/usr/bin/env python3
"""Apply the already-frozen MVP comparison policy; never derives new bounds."""

import argparse
import hashlib
import json
import math
from pathlib import Path


def read(path):
    return json.loads(path.read_text())


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


MODES = ("baseline", "tunnel")
DIRECTIONS = ("forward", "reverse", "simultaneous")


def finite_nonnegative(value):
    return (not isinstance(value, bool) and isinstance(value, (int, float))
            and math.isfinite(value) and value >= 0)


def validate_coverage(runs, repetitions):
    if type(repetitions) is not int or not 1 <= repetitions <= 100:
        raise ValueError("invalid repetition count")
    expected = {(p, m, d) for p in range(repetitions) for m in MODES for d in DIRECTIONS}
    if len(runs) != len(expected):
        raise ValueError("incomplete run coverage")
    seen = set()
    for r in runs:
        if type(r["pair"]) is not int:
            raise ValueError("invalid pair index")
        key = (r["pair"], r["mode"], r["direction"])
        if key not in expected or key in seen or r["status"] != "valid-low-rate":
            raise ValueError("duplicate, unexpected or invalid run")
        seen.add(key)
    if seen != expected:
        raise ValueError("incomplete run coverage")


def required_metrics(direction):
    metrics = {"rtt_p95_ms": ("ms", "candidate_max")}
    for role in ("client", "server"):
        for metric in ("average_cpu_cores", "owner_average_cpu_cores"):
            metrics[role + "_" + metric] = ("cores", "candidate_max")
        metrics[role + "_peak_rss_bytes"] = ("bytes", "candidate_max")
    for flow in ("forward", "reverse"):
        if direction in (flow, "simultaneous"):
            metrics[flow + "_delivered_mbit_s"] = ("Mbit/s", "candidate_min")
            metrics[flow + "_loss_percent"] = ("percentage points", "candidate_max")
    return metrics


def validate_policy(policy):
    if policy.get("schema_version") != 1 or policy.get("frozen") is not True:
        raise ValueError("unknown or unfrozen policy")
    expected = {(m, d) for m in MODES for d in DIRECTIONS}
    seen = set()
    for workload in policy["workloads"]:
        key = (workload["mode"], workload["direction"])
        if key not in expected or key in seen:
            raise ValueError("duplicate or unexpected policy workload")
        seen.add(key)
        required = required_metrics(workload["direction"])
        if set(workload["metrics"]) != set(required):
            raise ValueError("incomplete policy metrics")
        for metric, (unit, limit) in required.items():
            bound = workload["metrics"][metric]
            if bound.get("unit") != unit or not finite_nonnegative(bound.get(limit)):
                raise ValueError("missing/non-finite policy bound or unmatched unit")
            for optional in ("candidate_min", "candidate_max"):
                if optional in bound and not finite_nonnegative(bound[optional]):
                    raise ValueError("invalid policy bound")
            if ("candidate_min" in bound and "candidate_max" in bound
                    and bound["candidate_min"] > bound["candidate_max"]):
                raise ValueError("contradictory policy bounds")
    if seen != expected:
        raise ValueError("incomplete policy coverage")


def compare(reference, candidate, policy):
    validate_policy(policy)
    if (
        digest(reference / "manifest.json") != policy["reference_manifest_sha256"]
        or digest(reference / "summary.json") != policy["reference_summary_sha256"]
    ):
        raise ValueError("unmatched reference")
    old = read(reference / "manifest.json")
    new = read(candidate / "manifest.json")
    for key in [
        "profile",
        "platform",
        "offloads",
        "locks",
        "role_locations",
        "kernel_socket_buffers_requested",
    ]:
        if old[key] != new[key]:
            raise ValueError("unmatched workload or environment")
    runs = read(candidate / "summary.json")
    validate_coverage(read(reference / "summary.json"), old["profile"]["paired_repetitions"])
    validate_coverage(runs, old["profile"]["paired_repetitions"])
    results = []
    for run in runs:
        if (
            run["errors"] != 0
            or run["integrity_errors"] != 0
            or not (0 <= run["clock_drift_ms"] < 10)
        ):
            raise ValueError("invalid generator/integrity/clock evidence")
        for direction in ["forward", "reverse"]:
            d = run[direction]
            active = run["direction"] in [direction, "simultaneous"]
            offered = (
                old["profile"]["pps"] * old["profile"]["measurement_seconds"]
                if active
                else 0
            )
            if d["offered"] != offered or not (0 <= d["delivered_unique"] <= offered):
                raise ValueError("invalid delivery accounting")
            rate = (
                d["delivered_unique"]
                * old["profile"]["payload_bytes"]
                * 8
                / old["profile"]["measurement_seconds"]
                / 1e6
            )
            loss = (
                100 * (offered - d["delivered_unique"]) / offered if offered else None
            )
            if d["delivered_mbit_s"] != rate or d["loss_percent"] != loss:
                raise ValueError("inconsistent delivery metrics")
        workload = next(
            w
            for w in policy["workloads"]
            if w["mode"] == run["mode"] and w["direction"] == run["direction"]
        )
        values = {"rtt_p95_ms": run["rtt_p95_ms"]}
        for direction in ["forward", "reverse"]:
            values[direction + "_delivered_mbit_s"] = run[direction]["delivered_mbit_s"]
            values[direction + "_loss_percent"] = run[direction]["loss_percent"]
        for role in ["client", "server"]:
            for metric in [
                "average_cpu_cores",
                "owner_average_cpu_cores",
                "peak_rss_bytes",
            ]:
                values[role + "_" + metric] = run["resources"][role][metric]
        failures = []
        for metric, bound in workload["metrics"].items():
            value = values[metric]
            if (
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not math.isfinite(value)
                or value < 0
            ):
                raise ValueError("missing or non-finite metric")
            if (
                value is None
                or ("candidate_min" in bound and value < bound["candidate_min"])
                or ("candidate_max" in bound and value > bound["candidate_max"])
            ):
                failures.append({"metric": metric, "value": value, "bound": bound})
        results.append(
            {
                "pair": run["pair"],
                "mode": run["mode"],
                "direction": run["direction"],
                "status": "fail" if failures else "pass",
                "failures": failures,
            }
        )
    return {
        "schema_version": 1,
        "status": "pass" if all(r["status"] == "pass" for r in results) else "fail",
        "policy_sha256": None,
        "scope": policy["scope"],
        "release_acceptance_claim": False,
        "runs": results,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    a = parser.parse_args()
    try:
        result = compare(a.reference, a.candidate, read(a.policy))
        result["policy_sha256"] = digest(a.policy)
    except (
        ValueError,
        KeyError,
        StopIteration,
        TypeError,
        OSError,
        OverflowError,
    ) as e:
        result = {
            "schema_version": 1,
            "status": "invalid",
            "reason": type(e).__name__,
            "release_acceptance_claim": False,
        }
    with a.output.open("x") as f:
        json.dump(result, f, indent=2, sort_keys=True)
        f.write("\n")
    print(
        json.dumps(
            {
                "event": "mvp_comparison",
                "status": result["status"],
                "release_acceptance_claim": False,
            }
        )
    )
    raise SystemExit(0 if result["status"] == "pass" else 1)


if __name__ == "__main__":
    main()
