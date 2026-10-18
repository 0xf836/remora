"""Collector correctness: ordinary loss is separate from missing status coverage."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/validate"))
spec = importlib.util.spec_from_file_location("operations_runner", ROOT / "scripts/validate/operations.py")
operations = importlib.util.module_from_spec(spec)
spec.loader.exec_module(operations)


class Correlation(unittest.TestCase):
    def record(self):
        return {"coverage_lost": 0, "status_write_failures": 0, "counter_overflow": False,
                "diagnostics_dropped": 9000,
                "failures": [{"class": "authentication", "code": 401, "session": "opaque-session", "connection": "opaque-connection"}]}

    def test_suppressed_events_preserve_incapacity_class_session_correlation(self):
        self.assertEqual(len(operations.correlated([self.record()], "authentication", 401)), 1)

    def test_ring_write_and_arithmetic_gaps_invalidate_coverage(self):
        for field in ["coverage_lost", "status_write_failures", "counter_overflow"]:
            r = self.record()
            r[field] = 1
            with self.assertRaisesRegex(RuntimeError, "SC-010 collection failed"):
                operations.correlated([r], "authentication", 401)

    def test_wrong_code_or_missing_session_cannot_satisfy_session_evidence(self):
        self.assertFalse(operations.correlated([self.record()], "authentication", 403))
        r = self.record()
        r["failures"][0]["session"] = None
        self.assertFalse(operations.correlated([r], "authentication", 401))

    def test_all_snapshots_participate_in_coverage_check(self):
        bad = self.record()
        bad["coverage_lost"] = 1
        with self.assertRaises(RuntimeError):
            operations.correlated([bad, self.record()], "authentication", 401)

if __name__ == "__main__":
    unittest.main()
