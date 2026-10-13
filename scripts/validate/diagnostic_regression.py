"""Prospective Phase-6/Phase-7 paired regression acceptance, with exact inputs.
A new source reconstruction after policy freeze does not rewrite the original
missed freeze. Release qualification and SC-010 blocked-collector claims stay off.
"""
import json
import hashlib
import math
import os
from pathlib import Path
import subprocess
import time

import diagnostic_benchmark as d
from benchmark import ROOT, sha, write

BASE_MODES = ["off", "default", "verbose"]
ENV_FIELDS = ["platform", "offloads", "locks", "role_locations", "kernel_socket_buffers_requested", "clock", "role_cpu_affinity"]


def read(path):
    return json.loads(Path(path).read_text())


def checked_input(path, binary):
    identity = read(path)
    if identity["binary_sha256"] != sha(binary) or identity["features"] != ["qlog"]:
        raise ValueError("unmatched executable/build features")
    source = Path(identity["source_root"])
    actual = {str(p.relative_to(source)): sha(p) for p in source.joinpath("src").rglob("*") if p.is_file()}
    actual.update({name: sha(source / name) for name in ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"]})
    if actual != identity["sources"]:
        raise ValueError("source input changed or incomplete")
    return identity


def validate_plan(plan):
    if plan["schema_version"] != 1 or plan["reference_modes"] != BASE_MODES or plan["candidate_modes"] != list(d.MODES) or plan["qlog_reference_mode"] != "default":
        raise ValueError("unsupported regression plan")
    for values in [plan["floors"], *plan["increments"].values()]:
        for value in values.values():
            if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
                raise ValueError("invalid prospective increment/floor")
    if set(plan["increments"]) != set(d.MODES):
        raise ValueError("missing mode trade-off")
    for values in plan["increments"].values():
        if set(values) != {"process_cpu_cores", "owner_cpu_cores", "rss_bytes", "rtt_ms"}:
            raise ValueError("missing metric trade-off")


def keys(profile, reference=False):
    modes = BASE_MODES if reference else list(d.MODES)
    result = [(m, direction, pair, False) for m in modes for direction in d.DIRECTIONS for pair in range(profile["paired_repetitions"])]
    result += [(m, "simultaneous", pair, True) for m in (["default"] if reference else profile["blocked_sink_modes"]) for pair in range(profile["paired_repetitions"])]
    if reference and profile.get("reference_order") == "interleaved":
        result.sort(key=lambda k: (k[2], k[3], d.DIRECTIONS.index(k[1]), list(d.MODES).index(k[0])))
    return result


def reference_mode(mode):
    return "default" if mode == "filtered-qlog" else mode


def single_profile(profile, mode, direction, blocked):
    p = {k: v for k, v in profile.items() if k not in ["modes", "directions", "blocked_sink_modes"]}
    p.update(coverage="diagnostics-single-mode-loopback", diagnostics=d.MODES[mode], tunnel_only=True,
             blocked_sink=blocked, directions=[direction], paired_repetitions=1)
    return p


def valid(row, profile, mode, blocked):
    d.valid_run(row, mode, blocked)
    if profile.get("role_cpu_pinning") and row.get("role_cpu_affinity_ok") is not True:
        raise ValueError("role/thread CPU affinity changed")
    expected = profile["pps"] * profile["measurement_seconds"]
    for direction in ["forward", "reverse"]:
        flow = row[direction]
        if row["direction"] in [direction, "simultaneous"]:
            if flow["offered"] != expected or flow["delivered_unique"] != expected or flow["loss_percent"] != 0:
                raise ValueError("missing offered traffic or nonzero delivery loss")
        elif flow["offered"] != 0 or flow["delivered_unique"] != 0:
            raise ValueError("unexpected inactive direction traffic")
    for role in ["client", "server"]:
        if blocked and row["role_runtime_failures"][role] != ["DiagnosticCollectionIncomplete"]:
            raise ValueError("blocked exit is not exact collection failure")
        if not blocked and row["diagnostic_last"][role].get("diagnostics_dropped", 0):
            raise ValueError("normal diagnostic event loss")


def coverage(rows, profile, reference=False):
    expected = set(keys(profile, reference))
    seen = set()
    for row in rows:
        key = (row["diagnostic_mode"], row["direction"], row["pair"], row["blocked_sink"])
        if type(row["pair"]) is not int or type(row["blocked_sink"]) is not bool or key not in expected or key in seen:
            raise ValueError("duplicate/unexpected comparison run")
        valid(row, profile, key[0], key[3])
        seen.add(key)
    if seen != expected:
        raise ValueError("incomplete regression matrix")


def environment(directory, rows, profile, identity, baseline=False):
    env = None
    for row in rows:
        child = read(Path(directory) / row["artifact"] / "manifest.json")
        mode = reference_mode(row["diagnostic_mode"]) if baseline else row["diagnostic_mode"]
        if profile.get("role_cpu_pinning"):
            parent = read(Path(directory) / "manifest.json")
            expected_cpus = dict(zip(["client", "server", "generator", "receiver"], parent["affinity"]))
            if len(parent["affinity"]) != 4 or child["role_cpu_affinity"] != expected_cpus:
                raise ValueError("unmatched role CPU assignment")
        if child["profile"] != single_profile(profile, mode, row["direction"], row["blocked_sink"]):
            raise ValueError("unmatched child workload/mode")
        if child["remora_input"] != identity or child["binaries"]["target/release/remora"] != identity["binary_sha256"]:
            raise ValueError("unmatched child source/executable identity")
        if child["binaries"]["tools/traffic/target/release/remora-traffic"] != row["traffic_sha256"]:
            raise ValueError("unmatched child traffic executable")
        child_rows = read(Path(directory) / row["artifact"] / "summary.json")
        if len(child_rows) != 1 or row["artifact_summary_sha256"] != sha(Path(directory) / row["artifact"] / "summary.json"):
            raise ValueError("unmatched child summary")
        extra = {k: row[k] for k in ["diagnostic_mode", "pair", "blocked_sink", "artifact", "traffic_sha256", "measurement_started_utc_seconds", "measurement_finished_utc_seconds", "artifact_summary_sha256"]}
        if {**child_rows[0], **extra} != row:
            raise ValueError("rewritten child measurements")
        current = {field: child[field] for field in ENV_FIELDS}
        if env is not None and env != current:
            raise ValueError("unmatched child platform/clock/locks")
        env = current
    return env


def group_key(mode, direction, blocked):
    return f"{mode}/{direction}/{blocked}"


def bounds_from_reference(rows, profile, plan):
    bounds = {}
    for mode in d.MODES:
        for blocked in ([False, True] if mode in profile["blocked_sink_modes"] else [False]):
            for direction in (["simultaneous"] if blocked else d.DIRECTIONS):
                group = [r for r in rows if r["diagnostic_mode"] == reference_mode(mode) and r["direction"] == direction and r["blocked_sink"] == blocked]
                if len(group) != profile["paired_repetitions"]:
                    raise ValueError("insufficient pre-change samples")
                limits = {}
                for metric in d.metrics(group[0]):
                    values = [d.metrics(r)[metric] for r in group]
                    if metric.endswith("cpu_cores"):
                        unit = "cores"
                        floor = plan["floors"]["cpu_ticks"] / os.sysconf("SC_CLK_TCK") / (profile["measurement_seconds"] - .1)
                        budget = plan["increments"][mode]["owner_cpu_cores" if ".owner_" in metric else "process_cpu_cores"]
                    elif metric.endswith("bytes"):
                        unit, floor, budget = "bytes", plan["floors"]["rss_bytes"], plan["increments"][mode]["rss_bytes"]
                    elif metric.endswith("mbit_s"):
                        unit, floor, budget = "Mbit/s", 0, 0
                    elif metric.endswith("percent"):
                        unit, floor, budget = "percentage points", 0, 0
                    else:
                        unit, floor, budget = "ms", plan["floors"]["rtt_ms"], plan["increments"][mode]["rtt_ms"]
                    noise = max(2 * (max(values) - min(values)), floor)
                    if metric.endswith(("mbit_s", "percent")):
                        noise = 0
                    minimum = metric.endswith("mbit_s")
                    baseline_limit = min(values) - noise if minimum else max(values) + noise
                    candidate_limit = baseline_limit - budget if minimum else baseline_limit + budget
                    limits[metric] = {"unit": unit, "reference_min": min(values), "reference_max": max(values),
                        "noise": noise, "increment": budget, "baseline_limit": max(0, baseline_limit),
                        "candidate_limit": max(0, candidate_limit), "side": "minimum" if minimum else "maximum"}
                bounds[group_key(mode, direction, blocked)] = limits
    return bounds


def freeze(reference, output, plan_path, proposal_path):
    manifest = read(reference / "manifest.json")
    rows = read(reference / "summary.json")
    plan, proposal = read(plan_path), read(proposal_path)
    validate_plan(plan)
    coverage(rows, manifest["profile"], reference=True)
    if manifest["plan_sha256"] != sha(plan_path) or manifest["proposal_sha256"] != sha(proposal_path):
        raise ValueError("plan/proposal changed after reference began")
    if manifest["baseline_input"]["revision"] != plan["baseline_revision"] or proposal["base_revision"] != plan["baseline_revision"]:
        raise ValueError("unmatched pre-Phase-7 revision")
    current_harness = {name: sha(ROOT / name) for name in manifest["harness_sources"]}
    if manifest["harness_sources"] != current_harness:
        raise ValueError("reference harness changed; fresh reference required")
    policy = {"schema_version": 1, "frozen": True, "created_utc_seconds": time.time(),
        "reference_manifest_sha256": sha(reference / "manifest.json"), "reference_summary_sha256": sha(reference / "summary.json"),
        "plan_sha256": sha(plan_path), "proposal_sha256": sha(proposal_path), "plan": plan, "proposal": proposal,
        "profile": manifest["profile"], "affinity": manifest["affinity"], "baseline_input": manifest["baseline_input"],
        "traffic_sha256": manifest["traffic_sha256"], "harness_sources": manifest["harness_sources"],
        "environment": environment(reference, rows, manifest["profile"], manifest["baseline_input"], baseline=True),
        "bounds": bounds_from_reference(rows, manifest["profile"], plan),
        "scope": "new prospective reconstruction cycle; original missed freeze not rewritten; no release qualification"}
    write(output, policy)
    return policy


def compare(reference, candidate, policy):
    ref, new = read(reference / "manifest.json"), read(candidate / "manifest.json")
    refrows, rows, paired = read(reference / "summary.json"), read(candidate / "summary.json"), read(candidate / "paired-baseline.json")
    policy_sha = hashlib.sha256((json.dumps(policy, indent=2, sort_keys=True) + "\n").encode()).hexdigest()
    if new["policy_sha256"] != policy_sha or new["candidate_input"]["policy_sha256"] != policy_sha:
        raise ValueError("policy differs from candidate construction/measurement seal")
    if policy.get("frozen") is not True or policy.get("schema_version") != 1 or policy["reference_manifest_sha256"] != sha(reference / "manifest.json") or policy["reference_summary_sha256"] != sha(reference / "summary.json"):
        raise ValueError("unmatched frozen reference")
    validate_plan(policy["plan"])
    coverage(refrows, ref["profile"], reference=True)
    coverage(rows, new["profile"])
    # Baseline qlog-labelled pairs deliberately run default, never pretend Phase 6 traced.
    seen = set()
    for row in paired:
        key = (row["diagnostic_mode"], row["direction"], row["pair"], row["blocked_sink"])
        if type(row["pair"]) is not int or key not in set(keys(new["profile"])) or key in seen:
            raise ValueError("missing/duplicate paired baseline")
        valid(row, new["profile"], reference_mode(key[0]), key[3]); seen.add(key)
    if seen != set(keys(new["profile"])):
        raise ValueError("incomplete paired baseline")
    for key in ["profile", "affinity", "baseline_input", "traffic_sha256", "harness_sources", "plan_sha256", "proposal_sha256"]:
        if ref[key] != new[key] or new[key] != policy[key]:
            raise ValueError(f"unmatched comparison {key}")
    if bounds_from_reference(refrows, ref["profile"], policy["plan"]) != policy["bounds"]:
        raise ValueError("rewritten numeric bounds")
    identity = new["candidate_input"]
    if identity["sources"] != policy["proposal"]["proposed_sources"] or identity["features"] != policy["baseline_input"]["features"] or identity["compiler"] != policy["baseline_input"]["compiler"]:
        raise ValueError("unmatched candidate source/features/compiler")
    if identity["binary_sha256"] == policy["baseline_input"]["binary_sha256"]:
        raise ValueError("same executable is not instrumentation comparison")
    if not policy["created_utc_seconds"] < identity["materialized_utc_seconds"] <= identity["built_utc_seconds"] < new["started_utc_seconds"]:
        raise ValueError("candidate construction/build/measurement predates freeze")
    for directory, data, input_id, baseline in [(reference, refrows, ref["baseline_input"], True), (candidate, paired, new["baseline_input"], True), (candidate, rows, identity, False)]:
        if environment(directory, data, new["profile"], input_id, baseline) != policy["environment"]:
            raise ValueError("unmatched comparison environment")
    pairs = {(r["diagnostic_mode"], r["direction"], r["pair"], r["blocked_sink"]): r for r in paired}
    outcomes = []
    for row in rows:
        key = (row["diagnostic_mode"], row["direction"], row["pair"], row["blocked_sink"])
        baseline = pairs[key]
        expected = ["baseline", "candidate"] if row["pair"] % 2 == 0 else ["candidate", "baseline"]
        first, second = (baseline, row) if expected[0] == "baseline" else (row, baseline)
        if first["measurement_finished_utc_seconds"] > second["measurement_started_utc_seconds"]:
            raise ValueError("incorrect/overlapping pair order")
        failures, deltas = [], {}
        for metric, bound in policy["bounds"][group_key(key[0], key[1], key[3])].items():
            b, c = d.metrics(baseline)[metric], d.metrics(row)[metric]
            deltas[metric] = c - b
            if bound["side"] == "maximum":
                if b > bound["baseline_limit"]: failures.append(f"baseline:{metric}")
                if c > bound["candidate_limit"] or c - b > bound["increment"] + bound["noise"]: failures.append(f"candidate:{metric}")
            else:
                if b < bound["baseline_limit"]: failures.append(f"baseline:{metric}")
                if c < bound["candidate_limit"] or b - c > bound["increment"] + bound["noise"]: failures.append(f"candidate:{metric}")
        outcomes.append({"mode": key[0], "direction": key[1], "pair": key[2], "blocked": key[3], "status": "fail" if failures else "pass", "failures": failures, "deltas": deltas})
    return {"status": "pass" if all(r["status"] == "pass" for r in outcomes) else "fail", "runs": outcomes,
        "measurement_kind": "pre-phase7-paired-regression", "regression_acceptance": all(r["status"] == "pass" for r in outcomes),
        "release_claim": False, "sc010_claim": False, "original_freeze_sequence_repaired": False}


def collect(args, profile, out, key, kind, identity):
    mode, direction, pair, blocked = key
    actual_mode = reference_mode(mode) if kind == "baseline" else mode
    tag = f'{mode}-{direction}-{pair}-{"blocked" if blocked else "normal"}-{kind}'
    config = out / f"{tag}.toml"
    p = single_profile(profile, actual_mode, direction, blocked)
    config.write_text("\n".join(f"{k} = {json.dumps(v)}" for k, v in p.items()) + "\n")
    binary = args.baseline_binary if kind == "baseline" else args.remora_binary
    input_path = args.baseline_input_manifest if kind == "baseline" else args.remora_input_manifest
    started = time.time()
    subprocess.run([str(ROOT / "scripts/validate/bench.sh"), "--profile", str(config), "--output", str(out / tag),
        "--remora-binary", str(binary.resolve()), "--remora-input-manifest", str(input_path.resolve()),
        "--traffic-binary", str(args.traffic_binary.resolve())], check=True)
    group = read(out / tag / "summary.json")
    if len(group) != 1:
        raise ValueError("unexpected single-run result")
    row = group[0]
    valid(row, profile, actual_mode, blocked)
    row.update(diagnostic_mode=mode, pair=pair, blocked_sink=blocked, artifact=tag,
               traffic_sha256=sha(args.traffic_binary), measurement_started_utc_seconds=started,
               measurement_finished_utc_seconds=time.time(), artifact_summary_sha256=sha(out / tag / "summary.json"))
    checked_input(input_path, binary)
    return row


def dispatch(args, profile):
    d.validate_profile(profile)
    if args.diagnostics_stage == "freeze":
        freeze(args.reference.resolve(), Path(args.output), args.regression_plan, args.proposal)
        return
    stage = args.diagnostics_stage
    if stage not in ["reference", "candidate"] or not args.baseline_binary or not args.baseline_input_manifest or not args.traffic_binary or not args.regression_plan or not args.proposal:
        raise ValueError("explicit regression inputs/plan/proposal/stage required")
    plan = read(args.regression_plan); validate_plan(plan)
    baseline = checked_input(args.baseline_input_manifest, args.baseline_binary)
    candidate = checked_input(args.remora_input_manifest, args.remora_binary) if stage == "candidate" else None
    if stage == "candidate" and (not args.policy or not args.reference):
        raise ValueError("candidate requires frozen policy/reference")
    if stage == "candidate":
        policy = read(args.policy)
        if (candidate["policy_sha256"] != sha(args.policy) or policy["plan"] != plan
                or policy["plan_sha256"] != sha(args.regression_plan) or policy["proposal_sha256"] != sha(args.proposal)
                or policy["baseline_input"] != baseline or candidate["sources"] != policy["proposal"]["proposed_sources"]
                or not policy["created_utc_seconds"] < candidate["materialized_utc_seconds"] <= candidate["built_utc_seconds"] < time.time()):
            raise ValueError("candidate does not match prospective reconstruction policy")
    allowed = sorted(os.sched_getaffinity(0))
    if profile.get("role_cpu_pinning"):
        affinity, seen = [], set()
        for cpu in allowed:
            topology = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology")
            key = ((topology / "physical_package_id").read_text(), (topology / "core_id").read_text())
            if key not in seen: affinity.append(cpu); seen.add(key)
            if len(affinity) == 4: break
        if len(affinity) != 4: raise ValueError("four distinct physical cores required")
    else: affinity = allowed[:4]
    os.sched_setaffinity(0, affinity)
    out = Path(args.output).resolve(); out.mkdir(parents=True, exist_ok=False)
    sources = {str(p.relative_to(ROOT)): sha(p) for p in [ROOT / "scripts/validate/benchmark.py", ROOT / "scripts/validate/diagnostic_benchmark.py", ROOT / "scripts/validate/diagnostic_regression.py"]}
    manifest = {"schema_version": 1, "stage": stage, "started_utc_seconds": time.time(), "profile": profile, "affinity": affinity,
        "baseline_input": baseline, "candidate_input": candidate, "traffic_sha256": sha(args.traffic_binary), "harness_sources": sources,
        "plan_sha256": sha(args.regression_plan), "proposal_sha256": sha(args.proposal), "policy_sha256": sha(args.policy) if stage == "candidate" else None,
        "release_claim": False, "sc010_claim": False}
    write(out / "manifest.json", manifest)
    rows, paired = [], []
    try:
        for key in keys(profile, reference=stage == "reference"):
            order = ["baseline"] if stage == "reference" else (["baseline", "candidate"] if key[2] % 2 == 0 else ["candidate", "baseline"])
            for kind in order:
                row = collect(args, profile, out, key, kind, baseline if kind == "baseline" else candidate)
                (paired if stage == "candidate" and kind == "baseline" else rows).append(row)
                write(out / f"progress-{len(rows) + len(paired):03}.json", row)
                print(json.dumps({"stage": stage, "key": key, "kind": kind, "completed": len(rows) + len(paired), "status": "valid"}), flush=True)
        if sources != {name: sha(ROOT / name) for name in sources}:
            raise ValueError("harness changed during collection")
        write(out / "summary.json", rows)
        if stage == "candidate": write(out / "paired-baseline.json", paired)
        if stage == "candidate":
            if sha(args.policy) != manifest["policy_sha256"] or sha(args.regression_plan) != manifest["plan_sha256"] or sha(args.proposal) != manifest["proposal_sha256"]:
                raise ValueError("policy/plan/proposal changed during collection")
            result = compare(args.reference.resolve(), out, read(args.policy))
            write(out / "comparison.json", result)
            if result["status"] != "pass": raise ValueError("regression failed; retain bounds and failed runs")
        else: coverage(rows, profile, reference=True)
    except Exception as error:
        if not (out / "summary.json").exists(): write(out / "summary.json", rows)
        if stage == "candidate" and not (out / "paired-baseline.json").exists(): write(out / "paired-baseline.json", paired)
        write(out / "failure.json", {"status": "fail", "error": type(error).__name__, "reason": str(error) if isinstance(error, ValueError) else "command failed; inspect artifacts"})
        raise
