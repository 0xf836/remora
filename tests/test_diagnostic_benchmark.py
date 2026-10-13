"""T071 policy/collector rejection tests, independent of native timing noise."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import time
import tomllib
import unittest
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/validate"))
import diagnostic_benchmark as d


def profile():
    return {"schema_version": 1, "sessions": 1, "modes": list(d.MODES), "directions": d.DIRECTIONS,
            "blocked_sink_modes": ["default", "filtered-qlog"], "pps": 100,
            "payload_bytes": 128, "measurement_seconds": 5, "paired_repetitions": 5}


def row(mode="default", direction="simultaneous", pair=0, blocked=False):
    roles = ["client", "server"]
    return {"status": "valid-low-rate", "errors": 0, "integrity_errors": 0,
            "direction": direction, "pair": pair, "diagnostic_mode": mode, "blocked_sink": blocked,
            "rtt_p95_ms": 0.5, "forward": {"delivered_mbit_s": 0.1024, "loss_percent": 0},
            "reverse": {"delivered_mbit_s": 0.1024, "loss_percent": 0},
            "resources": {r: {"average_cpu_cores": .1, "owner_average_cpu_cores": .09, "peak_rss_bytes": 10 * 1024**2} for r in roles},
            "role_exit_codes": {r: 1 if blocked else 0 for r in roles},
            "stop_seconds": {r: 1.1 if blocked else .01 for r in roles},
            "writer_blocked_observed": {r: blocked for r in roles},
            "qlog_file_bytes": {r: [100] if mode == "filtered-qlog" else [] for r in roles},
            "diagnostic_last": {r: {"coverage_lost": 0, "status_write_failures": 0, "counter_overflow": False,
                                    "qlog": {"accepted_events": 10, "disabled_filters": 0, "output_failures": 0, "scratch_bytes": 65536}} for r in roles}}


def rows():
    return [row(m, direction, p) for m in d.MODES for direction in d.DIRECTIONS for p in range(5)] + [row(m, "simultaneous", p, True) for m in ["default", "filtered-qlog"] for p in range(5)]


class DiagnosticEvidence(unittest.TestCase):
    def test_profile_rejects_insufficient_sample_and_qlog_rate_coverage(self):
        d.validate_profile(profile())
        for key, bad in [("paired_repetitions", 4), ("pps", 1000), ("measurement_seconds", 1), ("modes", ["default"])]:
            p = profile(); p[key] = bad
            with self.assertRaises(ValueError): d.validate_profile(p)

    def test_normal_collection_gaps_and_disabled_qlog_are_invalid(self):
        for field in ["coverage_lost", "status_write_failures", "counter_overflow"]:
            r = row(); r["diagnostic_last"]["server"][field] = 1
            with self.assertRaises(ValueError): d.valid_run(r, "default")
        r = row("filtered-qlog"); r["diagnostic_last"]["client"]["qlog"]["disabled_filters"] = 1
        with self.assertRaises(ValueError): d.valid_run(r, "filtered-qlog")

    def test_blocked_sink_requires_actual_block_nonzero_exit_and_bounded_stop(self):
        d.valid_run(row(blocked=True), "default", True)
        for field, value in [("writer_blocked_observed", False), ("role_exit_codes", 0), ("stop_seconds", 5.1)]:
            r = row(blocked=True); r[field]["server"] = value
            with self.assertRaises(ValueError): d.valid_run(r, "default", True)

    def test_missing_and_duplicate_runs_cannot_pass(self):
        d.coverage(rows(), profile())
        for runs in [rows()[:-1], rows() + [rows()[0]]]:
            with self.assertRaises(ValueError): d.coverage(runs, profile())

    def test_retention_and_nonfinite_metrics_are_invalid(self):
        r = row("filtered-qlog"); r["qlog_file_bytes"]["server"] = [17 * 1024**2]
        with self.assertRaises(ValueError): d.valid_run(r, "filtered-qlog")
        for value in [float("nan"), False]:
            r = row(); r["rtt_p95_ms"] = value
            with self.assertRaises(ValueError): d.valid_run(r, "default")

    def test_frozen_before_candidate_and_each_run_enforces_all_bounds(self):
        with tempfile.TemporaryDirectory() as temp:
            ref = Path(temp) / "ref"; ref.mkdir()
            candidate = Path(temp) / "new"; candidate.mkdir()
            for directory in [ref, candidate]:
                for m in d.MODES:
                    for blocked in ([False, True] if m in ["default", "filtered-qlog"] else [False]):
                        child = directory / f"{m}-{'blocked' if blocked else 'normal'}"; child.mkdir()
                        meta = {key: {} for key in ["platform", "offloads", "locks", "role_locations", "kernel_socket_buffers_requested", "clock"]}
                        meta.update(profile=tomllib.loads(d.toml_profile(profile(), m, blocked)), binaries={"target/release/remora": "a" * 64, "tools/traffic/target/release/remora-traffic": "b" * 64})
                        (child / "manifest.json").write_text(json.dumps(meta))
            manifest = {"production_sources": {}, "profile": profile(), "binaries": {"remora": "a" * 64, "traffic": "b" * 64}, "affinity": [0, 1], "started_utc_seconds": 0}
            (ref / "manifest.json").write_text(json.dumps(manifest))
            (ref / "summary.json").write_text(json.dumps(rows()))
            policy = d.freeze(ref, Path(temp) / "policy.json")
            manifest["started_utc_seconds"] = time.time() + 1
            (candidate / "manifest.json").write_text(json.dumps(manifest))
            (candidate / "summary.json").write_text(json.dumps(rows()))
            comparison = d.compare(ref, candidate, policy)
            self.assertEqual(comparison["status"], "pass")
            self.assertEqual(comparison["measurement_kind"], "same-binary-repeatability")
            self.assertFalse(comparison["regression_acceptance"])
            child = candidate / "default-normal/manifest.json"
            original = child.read_text(); changed_child = json.loads(original)
            changed_child["locks"] = {"wrong": "identity"}; child.write_text(json.dumps(changed_child))
            with self.assertRaises(ValueError): d.compare(ref, candidate, policy)
            child.write_text(original)
            bad = rows(); bad[-1]["rtt_p95_ms"] = 100
            (candidate / "summary.json").write_text(json.dumps(bad))
            result = d.compare(ref, candidate, policy)
            self.assertEqual(result["status"], "fail")
            self.assertEqual(sum(r["status"] == "fail" for r in result["runs"]), 1)
            changed = copy.deepcopy(policy); changed["reference_summary_sha256"] = "0" * 64
            with self.assertRaises(ValueError): d.compare(ref, candidate, changed)

    def test_metric_units_and_limit_direction_cannot_be_reinterpreted(self):
        correct = {"maximum": .2, "unit": "cores", "reference_min": .1, "reference_max": .1, "tolerance": .1}
        d.validate_bound("client.average_cpu_cores", correct)
        for change in [{**correct, "unit": "bytes"}, {**{k: v for k, v in correct.items() if k != "maximum"}, "minimum": 0}]:
            with self.assertRaises(ValueError): d.validate_bound("client.average_cpu_cores", change)
