"""Exercise actual network.main dispatch without namespace privileges."""
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/validate"))
import network
import recovery
import reference
class Dispatch(unittest.TestCase):
    def invoke(self, extra):
        with tempfile.TemporaryDirectory() as directory:
            fd = os.open("/proc/self/ns/net", os.O_RDONLY)
            lab = MagicMock()
            lab.snapshot.return_value = {}
            try:
                argv = ["network.py", "--scenario", "reference", "--output", str(Path(directory) / "run"),
                    "--worker-host-net-fd", str(fd), "--worker-host-user-fd", str(fd)] + extra
                with patch.object(sys, "argv", argv), patch.object(network, "Network", return_value=lab), patch.object(network, "sha", return_value="hash"):
                    network.main()
                lab.close.assert_called_once()
            finally:
                os.close(fd)
    def test_default_characterization_does_not_resolve_recovery_local_name(self):
        with patch.object(network, "characterize") as ordinary, patch.object(recovery, "characterize") as special:
            self.invoke([])
            ordinary.assert_called_once()
            special.assert_not_called()
    def test_recovery_still_dispatches_only_recovery_characterization(self):
        with patch.object(network, "characterize") as ordinary, patch.object(recovery, "characterize") as special:
            self.invoke(["--recovery-scenario", "restart"])
            ordinary.assert_not_called()
            special.assert_called_once()
    def test_reference_benchmark_does_not_characterize(self):
        with patch.object(network, "characterize") as ordinary, patch.object(reference, "execute") as benchmark:
            self.invoke(["--benchmark-profile", str(ROOT / "benchmarks/reference-x86_64.toml")])
            ordinary.assert_not_called()
            benchmark.assert_called_once()
    def test_benchmark_failure_updates_terminal_progress_and_retains_detail(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            fd = os.open("/proc/self/ns/net", os.O_RDONLY)
            out = Path(directory) / "run"
            lab = MagicMock()
            lab.snapshot.return_value = {}
            def fail(_network, bench_out, *_args):
                reference.write_progress(bench_out, {"status": "running", "stage": "calibration"})
                raise RuntimeError("invalid capacity calibration: generator_errors=174")
            argv = ["network.py", "--scenario", "reference", "--output", str(out),
                    "--worker-host-net-fd", str(fd), "--worker-host-user-fd", str(fd),
                    "--benchmark-profile", str(ROOT / "benchmarks/reference-x86_64.toml")]
            try:
                with patch.object(sys, "argv", argv), patch.object(network, "Network", return_value=lab), patch.object(network, "sha", return_value="hash"), patch.object(reference, "execute", side_effect=fail):
                    with self.assertRaisesRegex(RuntimeError, "generator_errors=174"):
                        network.main()
                progress = json.loads((out / "reference/benchmark/progress.json").read_text())
                failure = json.loads((out / "reference/failure.json").read_text())
                self.assertEqual(progress["status"], "invalid")
                self.assertEqual(progress["detail"], failure["detail"])
                lab.close.assert_called_once()
            finally:
                os.close(fd)
if __name__ == "__main__": unittest.main()
