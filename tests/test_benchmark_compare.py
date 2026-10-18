"""Validate comparison coverage/policy with synthetic bounded fixtures."""

import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "comparison", ROOT / "scripts/validate/compare.py"
)
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


class FrozenComparison(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="remora-comparison-")
        self.reference = Path(self.temp.name) / "reference"
        self.candidate = Path(self.temp.name) / "candidate"
        self.reference.mkdir()
        self.candidate.mkdir()
        manifest = {k: {} for k in ("platform", "offloads", "locks", "role_locations",
                                   "kernel_socket_buffers_requested")}
        manifest["profile"] = {"paired_repetitions": 2, "pps": 1000,
                               "measurement_seconds": 3, "payload_bytes": 128}
        runs = []
        workloads = []
        for mode in ("baseline", "tunnel"):
            for direction in ("forward", "reverse", "simultaneous"):
                metrics = {"rtt_p95_ms": {"unit": "ms", "candidate_max": 2.0}}
                for role in ("client", "server"):
                    for metric in ("average_cpu_cores", "owner_average_cpu_cores"):
                        metrics[role + "_" + metric] = {"unit": "cores", "candidate_max": 0.2}
                    metrics[role + "_peak_rss_bytes"] = {"unit": "bytes", "candidate_max": 2**21}
                for flow in ("forward", "reverse"):
                    if direction in (flow, "simultaneous"):
                        metrics[flow + "_delivered_mbit_s"] = {"unit": "Mbit/s", "candidate_min": 1.0}
                        metrics[flow + "_loss_percent"] = {"unit": "percentage points", "candidate_max": 0.1}
                workloads.append({"mode": mode, "direction": direction, "metrics": metrics})
                for pair in range(2):
                    row = {"pair": pair, "mode": mode, "direction": direction,
                           "status": "valid-low-rate", "errors": 0, "integrity_errors": 0,
                           "clock_drift_ms": 0, "rtt_p95_ms": 1.0,
                           "resources": {role: {"average_cpu_cores": 0.1,
                                        "owner_average_cpu_cores": 0.1,
                                        "peak_rss_bytes": 2**20} for role in ("client", "server")}}
                    for flow in ("forward", "reverse"):
                        active = direction in (flow, "simultaneous")
                        row[flow] = {"offered": 3000 if active else 0,
                                     "delivered_unique": 3000 if active else 0,
                                     "delivered_mbit_s": 1.024 if active else 0.0,
                                     "loss_percent": 0.0 if active else None}
                    runs.append(row)
        for directory in (self.reference, self.candidate):
            (directory / "manifest.json").write_text(json.dumps(manifest))
            (directory / "summary.json").write_text(json.dumps(runs))
        self.policy = {"schema_version": 1, "frozen": True, "scope": "synthetic-low-rate",
                       "reference_manifest_sha256": comparison.digest(self.reference / "manifest.json"),
                       "reference_summary_sha256": comparison.digest(self.reference / "summary.json"),
                       "workloads": workloads}

    def tearDown(self):
        self.temp.cleanup()

    def mutate(self, file, change):
        path = self.candidate / file
        value = json.loads(path.read_text())
        change(value)
        path.write_text(json.dumps(value))

    def test_boolean_schema_version_is_not_a_policy_version(self):
        policy=copy.deepcopy(self.policy)
        policy["schema_version"]=True
        with self.assertRaises(ValueError):comparison.validate_policy(policy)

    def test_reference_identity_and_every_run(self):
        self.assertEqual(
            comparison.compare(self.reference, self.candidate, self.policy)["status"],
            "pass",
        )
        self.mutate(
            "summary.json",
            lambda rows: rows[0]["resources"]["client"].update(average_cpu_cores=3.0),
        )
        result = comparison.compare(self.reference, self.candidate, self.policy)
        self.assertEqual(result["status"], "fail")
        self.assertEqual(sum(r["status"] == "fail" for r in result["runs"]), 1)

    def test_unfrozen_unmatched_reference_and_workload_are_invalid(self):
        for field, value in [
            ("frozen", False),
            ("reference_manifest_sha256", "0" * 64),
        ]:
            policy = copy.deepcopy(self.policy)
            policy[field] = value
            with self.assertRaises(ValueError):
                comparison.compare(self.reference, self.candidate, policy)
        self.mutate(
            "manifest.json", lambda m: m["profile"].update(measurement_seconds=4)
        )
        with self.assertRaises(ValueError):
            comparison.compare(self.reference, self.candidate, self.policy)

    def test_invalid_metrics_and_false_valid_label_are_rejected(self):
        for mutation in [
            lambda rows: rows[0].update(rtt_p95_ms=float("nan")),
            lambda rows: rows[0].update(errors=1),
            lambda rows: rows[0]["forward"].update(delivered_unique=3001),
        ]:
            (self.candidate / "summary.json").write_bytes(
                (self.reference / "summary.json").read_bytes()
            )
            self.mutate("summary.json", mutation)
            with self.assertRaises(ValueError):
                comparison.compare(self.reference, self.candidate, self.policy)

    def test_exact_coverage_rejects_relabelled_pairs_modes_and_missing_runs(self):
        original = (self.candidate / "summary.json").read_text()
        def relabel(rows):
            sample = copy.deepcopy(next(r for r in rows if r["mode"] == "tunnel" and r["direction"] == "forward"))
            rows[:] = [dict(sample, pair=i) for i in range(12)]
        for mutation in (relabel, lambda rows: rows[0].update(pair=True),
                         lambda rows: rows[0].update(pair=2),
                         lambda rows: rows[0].update(mode="other"),
                         lambda rows: rows.pop()):
            with self.subTest(mutation=mutation):
                (self.candidate / "summary.json").write_text(original)
                self.mutate("summary.json", mutation)
                with self.assertRaises(ValueError):
                    comparison.compare(self.reference, self.candidate, self.policy)

    def test_policy_requires_all_metrics_units_finite_bounds_and_workloads(self):
        for change in (
            lambda p: p["workloads"][0].update(metrics={}),
            lambda p: p["workloads"][0]["metrics"].pop("rtt_p95_ms"),
            lambda p: p["workloads"][0]["metrics"]["rtt_p95_ms"].update(unit="seconds"),
            lambda p: p["workloads"][0]["metrics"]["rtt_p95_ms"].update(candidate_max=float("inf")),
            lambda p: p["workloads"][0]["metrics"]["rtt_p95_ms"].update(candidate_max=True),
            lambda p: p["workloads"][0]["metrics"]["rtt_p95_ms"].pop("candidate_max"),
            lambda p: p["workloads"].pop(),
            lambda p: p["workloads"].append(copy.deepcopy(p["workloads"][0])),
        ):
            policy = copy.deepcopy(self.policy)
            change(policy)
            with self.subTest(change=change), self.assertRaises(ValueError):
                comparison.compare(self.reference, self.candidate, policy)


if __name__ == "__main__":
    unittest.main()
