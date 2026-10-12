from pathlib import Path
import json
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts/validate"))
import recovery
import recovery_probe


class RecoveryEvidence(unittest.TestCase):
    def test_tag_integrity_bounds_and_atomic_control_publication(self):
        data = recovery_probe.packet(4095, 12345)
        self.assertEqual(len(data), 128)
        self.assertEqual(recovery_probe.decode(data), (4095, 12345))
        for bad in [
            data[:-1],
            b"x" + data[1:],
            data[:-1] + b"x",
            recovery_probe.packet(4096, 12345),
        ]:
            with self.assertRaises(ValueError):
                recovery_probe.decode(bad)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "control.json"
            recovery_probe.publish(path, dict(paused=True))
            recovery_probe.publish(path, dict(paused=False))
            self.assertEqual(json.loads(path.read_text()), dict(paused=False))
            self.assertFalse(path.with_suffix(".tmp").exists())

    def test_streamed_status_preserves_exact_cause_and_ignores_only_partial_tail(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "status.ndjson"
            row = dict(
                event="snapshot",
                failures=[dict(**{"class": "authentication"}, code=401)],
            )
            path.write_text(json.dumps(row) + '\n{"event":')
            self.assertEqual(recovery.records(path), [row])
            path.write_text("not-json\n" + json.dumps(row) + "\n")
            with self.assertRaises(json.JSONDecodeError):
                recovery.records(path)


if __name__ == "__main__":
    unittest.main()
