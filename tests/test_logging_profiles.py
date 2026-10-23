import json
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts/validate"))
from benchmark import machine_diagnostics, last_snapshot, runtime_failures


class LoggingProfiles(unittest.TestCase):
    def test_runtime_failure_identity_is_not_inferred_from_human_prose(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "stderr.ndjson"
            path.write_text('ERROR Diagnostic output incomplete\n'
                            '{"event":"failure"}\n'
                            'runtime failed: DiagnosticCollectionIncomplete\n')
            self.assertEqual(runtime_failures(path), ["DiagnosticCollectionIncomplete"])
    def test_machine_config_is_explicit_and_does_not_duplicate_fixture_table(self):
        original = ('schema_version=1\n[diagnostics]\nformat = "json"\n'
                    'snapshots = true\n[client]\nlisten="127.0.0.1:1234"\n')
        config = tomllib.loads(machine_diagnostics(original, "error", 10000))
        self.assertEqual(config["diagnostics"], dict(format="json", snapshots=True,
                         level="error", snapshot_interval_ms=10000))
        self.assertEqual(config["client"]["listen"], "127.0.0.1:1234")

    def test_collector_requires_real_snapshots_instead_of_silent_empty_metrics(self):
        snapshot = dict(schema_version=1, event="snapshot", counters={}, failures=[],
                        coverage_lost=0, status_write_failures=0,
                        diagnostics_dropped=0, counter_overflow=False)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "stdout.ndjson"
            for invalid in ["", "INFO UDP tunnel accepted\n", "{}\n", "[]\n",
                            json.dumps(dict(snapshot, counter_overflow=0))]:
                path.write_text(invalid)
                with self.assertRaises(RuntimeError):
                    last_snapshot(path)
            path.write_text(json.dumps(snapshot) + "\n")
            self.assertEqual(last_snapshot(path), snapshot)
            path.write_text('{"truncated":')
            self.assertEqual(last_snapshot(path, allow_incomplete=True), {})


if __name__ == "__main__":
    unittest.main()
