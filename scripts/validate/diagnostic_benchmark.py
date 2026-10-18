"""Diagnostic-mode repeatability with immutable already instrumented inputs.
This same-binary comparator cannot establish pre-Phase-7 instrumentation
regression acceptance for T071. No release capacity or SC-010 claim.
"""
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import time
import tomllib

from benchmark import ROOT, sha, write

MODES = {"off": "error", "default": "info", "verbose": "trace", "filtered-qlog": "filtered-qlog"}
DIRECTIONS = ["forward", "reverse", "simultaneous"]


def validate_profile(p):
    if p.get("schema_version") != 1 or p.get("sessions") != 1 or p.get("modes") != list(MODES) or p.get("directions") != DIRECTIONS:
        raise ValueError("unsupported diagnostics matrix")
    if p.get("paired_repetitions", 0) < 5 or not 1 <= p.get("pps", 0) <= 100 or p.get("measurement_seconds", 0) < 5:
        raise ValueError("insufficient repetitions/interval or qlog rate coverage")
    if p.get("blocked_sink_modes") != ["default", "filtered-qlog"]:
        raise ValueError("incomplete blocked collector matrix")


def valid_run(row, mode, blocked=False):
    if row["status"] != "valid-low-rate" or row["errors"] or row["integrity_errors"]:
        raise ValueError("invalid traffic/integrity/clock evidence")
    for role in ["client", "server"]:
        d = row["diagnostic_last"][role]
        if blocked:
            if not row["writer_blocked_observed"][role] or row["role_exit_codes"][role] != 1 or row["stop_seconds"][role] > 5:
                raise ValueError("blocked collector/stop evidence missing")
        elif row["role_exit_codes"][role] != 0 or any(d.get(k, 0) for k in ["coverage_lost", "status_write_failures", "counter_overflow"]):
            raise ValueError("diagnostic collection/overflow gap")
        if not blocked and not d:
            raise ValueError("missing mandatory status")
        files = row["qlog_file_bytes"][role]
        if sum(files) > 128 * 1024**2 or any(size > 16 * 1024**2 for size in files):
            raise ValueError("qlog retention exceeded")
        if mode == "filtered-qlog" and not blocked:
            q = d.get("qlog", {})
            if not files or q.get("accepted_events", 0) <= 0 or q.get("disabled_filters") or q.get("output_failures") or q.get("scratch_bytes", 0) > 512 * 1024:
                raise ValueError("inactive/stopped/unbounded filtered qlog")
    for v in metrics(row).values():
        if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v) or v < 0:
            raise ValueError("invalid numeric metric")


def metrics(row):
    result = {"rtt_p95_ms": row["rtt_p95_ms"]}
    for role in ["client", "server"]:
        for key in ["average_cpu_cores", "owner_average_cpu_cores", "peak_rss_bytes"]:
            result[f"{role}.{key}"] = row["resources"][role][key]
    for direction in ["forward", "reverse"]:
        if row["direction"] in [direction, "simultaneous"]:
            for key in ["delivered_mbit_s", "loss_percent"]:
                result[f"{direction}.{key}"] = row[direction][key]
    return result


def coverage(rows, profile):
    expected = {(m, d, p, False) for m in MODES for d in DIRECTIONS for p in range(profile["paired_repetitions"])}
    expected |= {(m, "simultaneous", p, True) for m in profile["blocked_sink_modes"] for p in range(profile["paired_repetitions"])}
    seen = set()
    for row in rows:
        key = (row["diagnostic_mode"], row["direction"], row["pair"], row["blocked_sink"])
        if type(row["pair"]) is not int or key not in expected or key in seen:
            raise ValueError("duplicate/unexpected run")
        valid_run(row, key[0], key[3])
        seen.add(key)
    if seen != expected:
        raise ValueError("incomplete matrix")


def measurement_identity(directory):
    result = {}
    parent = json.loads((directory / "manifest.json").read_text())
    for mode in MODES:
        for blocked in ([False, True] if mode in parent["profile"]["blocked_sink_modes"] else [False]):
            tag = f"{mode}-{'blocked' if blocked else 'normal'}"
            child = json.loads((directory / tag / "manifest.json").read_text())
            if child["profile"] != tomllib.loads(toml_profile(parent["profile"], mode, blocked)):
                raise ValueError("unmatched child mode/settings")
            if child["binaries"]["target/release/remora"] != parent["binaries"]["remora"] or child["binaries"]["tools/traffic/target/release/remora-traffic"] != parent["binaries"]["traffic"]:
                raise ValueError("unmatched child executable")
            fields = ["platform", "offloads", "locks", "role_locations", "kernel_socket_buffers_requested", "clock"]
            result[tag] = {field: child[field] for field in fields}
    return {"children": result, "production_sources": parent["production_sources"]}


def validate_bound(metric, bound):
    side = "minimum" if metric.endswith("mbit_s") else "maximum"
    unit = "cores" if metric.endswith("cores") else "bytes" if metric.endswith("bytes") else "Mbit/s" if metric.endswith("mbit_s") else "percentage points" if metric.endswith("percent") else "ms"
    if set(bound) != {side, "unit", "reference_min", "reference_max", "tolerance"} or bound["unit"] != unit:
        raise ValueError("unmatched metric unit/interpretation")
    for key in [side, "reference_min", "reference_max", "tolerance"]:
        value = bound[key]
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
            raise ValueError("invalid metric limit")
    if bound["reference_min"] > bound["reference_max"]:
        raise ValueError("invalid reference extrema")


def freeze(reference, output):
    manifest = json.loads((reference / "manifest.json").read_text())
    rows = json.loads((reference / "summary.json").read_text())
    coverage(rows, manifest["profile"])
    profile = manifest["profile"]
    policy = {"schema_version": 1, "frozen": True, "created_utc_seconds": time.time(),
        "reference_manifest_sha256": sha(reference / "manifest.json"), "reference_summary_sha256": sha(reference / "summary.json"),
        "profile": profile, "binaries": manifest["binaries"], "affinity": manifest["affinity"],
        "method": "every run; reference extrema plus twice repeat spread or predeclared measurement-resolution floor; no averaging failures",
        "scope": "unchanged pre-T071 Phase7 executable; no claim about pre-T064 instrumentation regression",
        "measurement_identity": measurement_identity(reference), "bounds": {}}
    for mode in MODES:
        for blocked in ([False, True] if mode in profile["blocked_sink_modes"] else [False]):
            for direction in (DIRECTIONS if not blocked else ["simultaneous"]):
                group = [r for r in rows if r["diagnostic_mode"] == mode and r["blocked_sink"] == blocked and r["direction"] == direction]
                bounds = {}
                for metric in metrics(group[0]):
                    values = [metrics(r)[metric] for r in group]
                    if metric.endswith("cores"):
                        floor = 2 / os.sysconf("SC_CLK_TCK") / (profile["measurement_seconds"] - .1)
                        unit = "cores"
                    elif metric.endswith("bytes"):
                        floor, unit = 256 * 1024, "bytes"
                    elif metric.endswith("mbit_s"):
                        floor, unit = 2 * profile["payload_bytes"] * 8 / profile["measurement_seconds"] / 1e6, "Mbit/s"
                    elif metric.endswith("percent"):
                        floor, unit = 200 / (profile["pps"] * profile["measurement_seconds"]), "percentage points"
                    else:
                        floor, unit = .25, "ms"
                    tolerance = max(2 * (max(values) - min(values)), floor)
                    key = "minimum" if metric.endswith("mbit_s") else "maximum"
                    bound = max(0, min(values) - tolerance) if key == "minimum" else max(values) + tolerance
                    bounds[metric] = {key: bound, "unit": unit, "reference_min": min(values), "reference_max": max(values), "tolerance": tolerance}
                policy["bounds"][f"{mode}/{direction}/{blocked}"] = bounds
    write(output, policy)
    return policy


def compare(reference, candidate, policy):
    ref = json.loads((reference / "manifest.json").read_text())
    new = json.loads((candidate / "manifest.json").read_text())
    if policy.get("frozen") is not True or policy.get("schema_version") != 1 or policy["reference_manifest_sha256"] != sha(reference / "manifest.json") or policy["reference_summary_sha256"] != sha(reference / "summary.json"):
        raise ValueError("unfrozen or unmatched reference")
    for field in ["profile", "binaries", "affinity"]:
        if ref[field] != new[field] or policy[field] != ref[field]:
            raise ValueError(f"unmatched {field}")
    if new["started_utc_seconds"] <= policy["created_utc_seconds"]:
        raise ValueError("candidate predates frozen policy")
    if measurement_identity(candidate) != policy["measurement_identity"] or measurement_identity(reference) != policy["measurement_identity"]:
        raise ValueError("unmatched platform/locks/source/clock metadata")
    rows = json.loads((candidate / "summary.json").read_text())
    coverage(rows, new["profile"])
    outcomes = []
    for row in rows:
        key = f'{row["diagnostic_mode"]}/{row["direction"]}/{row["blocked_sink"]}'
        bounds = policy["bounds"][key]
        if set(bounds) != set(metrics(row)):
            raise ValueError("incomplete metric bounds")
        failures = []
        for metric, value in metrics(row).items():
            bound = bounds[metric]
            validate_bound(metric, bound)
            limit = bound.get("maximum", bound.get("minimum"))
            if ("maximum" in bound and value > limit) or ("minimum" in bound and value < limit):
                failures.append(metric)
        outcomes.append({"mode": row["diagnostic_mode"], "direction": row["direction"], "pair": row["pair"], "blocked_sink": row["blocked_sink"], "status": "fail" if failures else "pass", "failed_metrics": failures})
    return {"status": "pass" if all(r["status"] == "pass" for r in outcomes) else "fail", "runs": outcomes, "sc010_claim": False, "release_claim": False, "measurement_kind": "same-binary-repeatability", "regression_acceptance": False}


def toml_profile(p, mode, blocked):
    settings = {k: v for k, v in p.items() if k not in ["modes", "directions", "blocked_sink_modes"]}
    settings.update(coverage="diagnostics-single-mode-loopback", diagnostics=MODES[mode], tunnel_only=True, blocked_sink=blocked,
                    directions=["simultaneous"] if blocked else DIRECTIONS)
    return "\n".join(f"{key} = {json.dumps(value)}" for key, value in settings.items()) + "\n"


def dispatch(args, profile):
    validate_profile(profile)
    stage = args.diagnostics_stage
    if stage not in ["reference", "freeze", "candidate"]:
        raise ValueError("diagnostics requires --diagnostics-stage reference|freeze|candidate")
    if stage == "freeze":
        if not args.reference:
            raise ValueError("freeze requires --reference")
        freeze(args.reference.resolve(), Path(args.output))
        return
    if not args.remora_binary or not args.traffic_binary:
        raise ValueError("explicit immutable binaries required")
    if stage == "candidate" and (not args.reference or not args.policy):
        raise ValueError("candidate requires frozen reference and policy")
    affinity = sorted(os.sched_getaffinity(0))[:4]
    os.sched_setaffinity(0, affinity)
    out = Path(args.output).resolve(); out.mkdir(parents=True, exist_ok=False)
    manifest = {"schema_version": 1, "started_utc_seconds": time.time(), "profile": profile, "stage": stage,
        "affinity": affinity, "binaries": {"remora": sha(args.remora_binary), "traffic": sha(args.traffic_binary)},
        "harness_sources": {str(p.relative_to(ROOT)): sha(p) for p in [ROOT / "scripts/validate/benchmark.py", ROOT / "scripts/validate/diagnostic_benchmark.py"]},
        "production_sources": {str(p.relative_to(ROOT)): sha(p) for p in (ROOT / "src").rglob("*") if p.is_file()},
        "off_semantics": "error threshold suppresses ordinary detail; mandatory status/counters stay on", "release_claim": False, "sc010_claim": False, "measurement_kind": "same-binary-repeatability", "regression_acceptance": False}
    policy_sha = sha(args.policy) if stage == "candidate" else None
    manifest["policy_sha256"] = policy_sha
    write(out / "manifest.json", manifest)
    rows = []
    order = list(MODES) if stage == "reference" else list(reversed(MODES))
    try:
        for blocked in [False, True]:
            for mode in order:
                if blocked and mode not in profile["blocked_sink_modes"]:
                    continue
                tag = f"{mode}-{'blocked' if blocked else 'normal'}"
                config = out / f"{tag}.toml"; config.write_text(toml_profile(profile, mode, blocked))
                command = [str(ROOT / "scripts/validate/bench.sh"), "--profile", str(config), "--output", str(out / tag), "--remora-binary", str(args.remora_binary.resolve()), "--traffic-binary", str(args.traffic_binary.resolve())]
                subprocess.run(command, check=True)
                group = json.loads((out / tag / "summary.json").read_text())
                for row in group:
                    row.update(diagnostic_mode=mode, blocked_sink=blocked)
                    valid_run(row, mode, blocked)
                rows.extend(group)
                print(json.dumps({"stage": stage, "mode": mode, "blocked": blocked, "runs": len(group), "status": "valid"}), flush=True)
        coverage(rows, profile)
        write(out / "summary.json", rows)
        if stage == "candidate":
            if sha(args.policy) != policy_sha:
                raise ValueError("policy changed during candidate collection")
            result = compare(args.reference.resolve(), out, json.loads(args.policy.read_text()))
            write(out / "comparison.json", result)
            if result["status"] != "pass":
                raise ValueError("diagnostic comparison failed; preserve original policy/results")
    except Exception as error:
        write(out / "failure.json", {"status": "fail", "error": type(error).__name__, "reason": str(error) if isinstance(error, ValueError) else "run command failed; see retained artifacts"})
        raise
