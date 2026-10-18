from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class FuzzOutputDirectory(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        scripts = self.root / "scripts/validate"
        scripts.mkdir(parents=True)
        self.runner = scripts / "fuzz.sh"
        shutil.copy2(ROOT / "scripts/validate/fuzz.sh", self.runner)
        # Stop at fixture provisioning, before toolchain/download/fuzz work.
        fixtures = scripts / "fixtures.sh"
        fixtures.write_text("#!/usr/bin/env bash\nprintf 'fixture provisioning reached\\n'\nexit 73\n")
        fixtures.chmod(0o755)

    def run_wrapper(self, output):
        return subprocess.run(["bash", str(self.runner), "1", str(output)],
                              cwd=self.root, text=True, capture_output=True, timeout=10)

    def test_fresh_checkout_creates_missing_parents_and_private_output(self):
        for relative in ["benchmarks/results/ci-fuzz", "parent with spaces/results/ci-fuzz"]:
            with self.subTest(output=relative):
                output = self.root / relative
                result = self.run_wrapper(output)
                self.assertEqual(result.returncode, 73, result.stderr)
                self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o700)
                self.assertEqual((output / "fixtures.log").read_text(), "fixture provisioning reached\n")

    def test_existing_output_is_rejected_without_changing_retained_evidence(self):
        output = self.root / "benchmarks/results/existing"
        output.mkdir(parents=True)
        evidence = output / "fixtures.log"
        evidence.write_text("retain earlier evidence\n")
        result = self.run_wrapper(output)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotEqual(result.returncode, 73)
        self.assertEqual(evidence.read_text(), "retain earlier evidence\n")
        self.assertEqual(list(output.iterdir()), [evidence])


if __name__ == "__main__":
    unittest.main()
