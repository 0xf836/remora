"""Cross-revision acceptance and rejection tests, without runtime timing noise."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/validate"))
import diagnostic_regression as r
from test_diagnostic_benchmark import profile, row


def complete_row(mode, direction, pair, blocked):
    value = row(mode, direction, pair, blocked)
    for flow in ["forward", "reverse"]:
        active = direction in [flow, "simultaneous"]
        value[flow].update(offered=500 if active else 0, delivered_unique=500 if active else 0)
    value["role_runtime_failures"] = {role: ["DiagnosticCollectionIncomplete"] if blocked else [] for role in ["client", "server"]}
    return value


def overwrite(path, value):
    Path(path).write_text(json.dumps(value) + "\n")


class RegressionEvidence(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.ref = self.root / "reference"; self.ref.mkdir()
        self.new = self.root / "candidate"; self.new.mkdir()
        self.p = profile()
        self.plan = r.read(ROOT / "benchmarks/policies/phase7-regression-plan.json")
        self.plan_path = self.root / "plan.json"; r.write(self.plan_path, self.plan)
        self.proposal_path = self.root / "proposal.json"
        self.sources = {"src/diagnostics.rs": "new", "Cargo.lock": "lock"}
        r.write(self.proposal_path, {"base_revision": self.plan["baseline_revision"], "proposed_sources": self.sources})
        self.baseline = {"revision": self.plan["baseline_revision"], "features": ["qlog"], "compiler": "pinned", "binary_sha256": "a" * 64, "sources": {"src/diagnostics.rs": "old", "Cargo.lock": "lock"}}
        self.manifest = {"profile": self.p, "affinity": [0, 1], "baseline_input": self.baseline, "traffic_sha256": "b" * 64,
            "harness_sources": {}, "plan_sha256": r.sha(self.plan_path), "proposal_sha256": r.sha(self.proposal_path)}
        r.write(self.ref / "manifest.json", self.manifest)
        self.refrows = [self.artifact(self.ref, k, "baseline", self.baseline, 0) for k in r.keys(self.p, True)]
        r.write(self.ref / "summary.json", self.refrows)
        self.policy = r.freeze(self.ref, self.root / "policy.json", self.plan_path, self.proposal_path)
        t = self.policy["created_utc_seconds"]
        self.identity = {**self.baseline, "binary_sha256": "c" * 64, "sources": self.sources, "materialized_utc_seconds": t + 1, "built_utc_seconds": t + 2, "policy_sha256": r.sha(self.root / "policy.json")}
        r.write(self.new / "manifest.json", {**self.manifest, "candidate_input": self.identity, "started_utc_seconds": t + 3, "policy_sha256": r.sha(self.root / "policy.json")})
        self.rows, self.paired = [], []
        for k in r.keys(self.p):
            first = t + 4
            self.rows.append(self.artifact(self.new, k, "candidate", self.identity, first if k[2] % 2 else first + 2))
            self.paired.append(self.artifact(self.new, k, "baseline", self.baseline, first + 2 if k[2] % 2 else first))
        r.write(self.new / "summary.json", self.rows)
        r.write(self.new / "paired-baseline.json", self.paired)

    def artifact(self, root, key, kind, identity, start):
        mode, direction, pair, blocked = key
        actual = r.reference_mode(mode) if kind == "baseline" else mode
        childrow = complete_row(actual, direction, 0, blocked)
        tag = f"{mode}-{direction}-{pair}-{blocked}-{kind}"
        child = root / tag; child.mkdir()
        metadata = {field: {} for field in r.ENV_FIELDS}
        metadata.update(profile=r.single_profile(self.p, actual, direction, blocked), remora_input=identity,
            binaries={"target/release/remora": identity["binary_sha256"], "tools/traffic/target/release/remora-traffic": "b" * 64})
        r.write(child / "manifest.json", metadata); r.write(child / "summary.json", [childrow])
        return {**childrow, "diagnostic_mode": mode, "pair": pair, "blocked_sink": blocked, "artifact": tag,
            "traffic_sha256": "b" * 64, "measurement_started_utc_seconds": start,
            "measurement_finished_utc_seconds": start + 1, "artifact_summary_sha256": r.sha(child / "summary.json")}

    def test_different_production_sources_and_binaries_pass_matching_pairs(self):
        result = r.compare(self.ref, self.new, self.policy)
        self.assertEqual(result["status"], "pass"); self.assertEqual(len(result["runs"]), 70)
        self.assertTrue(result["regression_acceptance"])
        self.assertFalse(result["release_claim"]); self.assertFalse(result["sc010_claim"])
        self.assertFalse(result["original_freeze_sequence_repaired"])

    def test_construction_before_freeze_and_same_binary_are_rejected(self):
        original = r.read(self.new / "manifest.json")
        for field, value in [("materialized_utc_seconds", 0), ("binary_sha256", self.baseline["binary_sha256"])]:
            changed = copy.deepcopy(original); changed["candidate_input"][field] = value
            overwrite(self.new / "manifest.json", changed)
            with self.assertRaises(ValueError): r.compare(self.ref, self.new, self.policy)

    def test_missing_pair_or_wrong_order_cannot_pass(self):
        overwrite(self.new / "paired-baseline.json", self.paired[:-1])
        with self.assertRaises(ValueError): r.compare(self.ref, self.new, self.policy)
        changed = copy.deepcopy(self.paired); changed[0]["measurement_finished_utc_seconds"] += 10
        overwrite(self.new / "paired-baseline.json", changed)
        with self.assertRaises(ValueError): r.compare(self.ref, self.new, self.policy)

    def test_plan_and_numeric_bounds_cannot_be_relaxed_after_reference(self):
        bad = copy.deepcopy(self.policy)
        next(iter(bad["bounds"].values()))["rtt_p95_ms"]["candidate_limit"] += 100
        with self.assertRaises(ValueError): r.compare(self.ref, self.new, bad)
        overwrite(self.plan_path, {**self.plan, "scope": "changed"})
        with self.assertRaises(ValueError): r.freeze(self.ref, self.root / "other-policy.json", self.plan_path, self.proposal_path)

    def test_embedded_plan_and_bounds_cannot_be_rewritten_together(self):
        bad = copy.deepcopy(self.policy)
        bad["plan"]["increments"]["off"]["process_cpu_cores"] = 10
        bad["bounds"] = r.bounds_from_reference(self.refrows, self.p, bad["plan"])
        with self.assertRaises(ValueError): r.compare(self.ref, self.new, bad)

    def test_child_metadata_and_rewritten_parent_metrics_are_rejected(self):
        changed = copy.deepcopy(self.rows); changed[0]["rtt_p95_ms"] += 0.1
        overwrite(self.new / "summary.json", changed)
        with self.assertRaises(ValueError): r.compare(self.ref, self.new, self.policy)
        overwrite(self.new / "summary.json", self.rows)
        child = self.new / self.rows[0]["artifact"] / "manifest.json"
        meta = r.read(child); meta["platform"] = {"kernel": "different"}; overwrite(child, meta)
        with self.assertRaises(ValueError): r.compare(self.ref, self.new, self.policy)

    def test_one_failed_pair_is_not_averaged_into_success(self):
        changed = copy.deepcopy(self.rows)
        changed[0]["resources"]["client"]["average_cpu_cores"] = 0.5
        child = self.new / changed[0]["artifact"] / "summary.json"
        local = r.read(child); local[0]["resources"] = changed[0]["resources"]; overwrite(child, local)
        changed[0]["artifact_summary_sha256"] = r.sha(child)
        overwrite(self.new / "summary.json", changed)
        result = r.compare(self.ref, self.new, self.policy)
        self.assertEqual(result["status"], "fail"); self.assertFalse(result["regression_acceptance"])
        self.assertEqual(sum(x["status"] == "fail" for x in result["runs"]), 1)

    def test_exact_blocked_failure_full_delivery_and_finite_budgets_required(self):
        value = complete_row("default", "simultaneous", 0, True)
        value["role_runtime_failures"]["server"] = ["Socket"]
        with self.assertRaises(ValueError): r.valid(value, self.p, "default", True)
        value = complete_row("default", "forward", 0, False); value["forward"]["offered"] = 499
        with self.assertRaises(ValueError): r.valid(value, self.p, "default", False)
        for bad in [True, float("nan"), -1]:
            plan = copy.deepcopy(self.plan); plan["increments"]["off"]["rtt_ms"] = bad
            with self.assertRaises(ValueError): r.validate_plan(plan)

    def test_interleaved_reference_retains_coverage_and_pinning_is_required(self):
        p = self.p | {"reference_order": "interleaved", "role_cpu_pinning": True}
        ordered = r.keys(p, True)
        self.assertEqual(set(ordered), set(r.keys(self.p, True)))
        self.assertEqual(len(ordered), 50)
        self.assertEqual([k[0] for k in ordered[:3]], ["off", "default", "verbose"])
        self.assertEqual([k[2] for k in ordered[:10]], [0] * 10)
        value = complete_row("default", "forward", 0, False)
        with self.assertRaises(ValueError): r.valid(value, p, "default", False)
        value["role_cpu_affinity_ok"] = True
        r.valid(value, p, "default", False)
        value["role_cpu_affinity_ok"] = False
        with self.assertRaises(ValueError): r.valid(value, p, "default", False)

    def test_changed_or_incomplete_source_input_is_rejected(self):
        source = self.root / "source"; (source / "src").mkdir(parents=True)
        for name in ["src/lib.rs", "Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"]:
            (source / name).write_text(name)
        binary = self.root / "binary"; binary.write_bytes(b"binary")
        identity = {"source_root": str(source), "features": ["qlog"], "binary_sha256": r.sha(binary),
            "sources": {str(p.relative_to(source)): r.sha(p) for p in source.rglob("*") if p.is_file()}}
        path = self.root / "input.json"; overwrite(path, identity)
        r.checked_input(path, binary)
        (source / "src/lib.rs").write_text("changed")
        with self.assertRaises(ValueError): r.checked_input(path, binary)


if __name__ == "__main__": unittest.main()
