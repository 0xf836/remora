import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/validate"))
import quiche_inputs
import quiche_tests


class QuicheInputs(unittest.TestCase):
    def fixture(self, root):
        (root / "quiche/src").mkdir(parents=True)
        names = ["Cargo.toml", "quiche/src/lib.rs", "quiche/src/tests.rs"]
        for name in names:
            (root / name).write_text(name)
        (root / "COPYING").write_text("license")
        (root / "quiche/COPYING").symlink_to("../COPYING")
        names.append("quiche/COPYING")
        return {"git": "https://github.com/example/quiche", "version": "0.30.0", "rev": "a" * 40,
                "files_sha256": {n: quiche_inputs.sha(root / n) for n in names},
                "symlinks": {"quiche/COPYING": "../COPYING"}}

    def test_reviewed_git_sources_match_policy(self):
        self.assertEqual(quiche_inputs.verify()["rev"], "154ce1995f23efa434e72dac269f707145be2015")

    def test_changed_missing_extra_and_unreviewed_symlink_fail(self):
        for change in ["changed", "missing", "extra", "symlink", "symlink-target"]:
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                policy = self.fixture(root)
                quiche_inputs.verify_checkout(root, policy)
                source = root / "quiche/src/lib.rs"
                if change == "changed":
                    source.write_text("changed")
                elif change == "missing":
                    source.unlink()
                elif change == "extra":
                    (root / "quiche/src/extra.rs").write_text("extra")
                elif change == "symlink":
                    source.unlink()
                    source.symlink_to(root / "COPYING")
                else:
                    (root / "quiche/COPYING").unlink()
                    (root / "quiche/COPYING").symlink_to(root / "COPYING")
                with self.assertRaises(ValueError):
                    quiche_inputs.verify_checkout(root, policy)

    def test_only_exact_git_revision_and_registry_transitives_are_accepted(self):
        policy = {"git": "https://github.com/example/quiche", "rev": "a" * 40, "version": "0.30.0"}
        package = {"name": "quiche", "version": "0.30.0",
                   "source": f"git+{policy['git']}?rev={policy['rev']}#{policy['rev']}"}
        quiche_inputs.select_package({"packages": [package]}, policy)
        cases = [[dict(package, source=None)], [dict(package, source="registry+crates.io")],
                 [dict(package, source=package["source"][:-1] + "b")],
                 [dict(package, version="0.30.1")], [package, package],
                 [package, {"name": "octets", "source": package["source"]}]]
        for packages in cases:
            with self.subTest(packages=packages), self.assertRaises(ValueError):
                quiche_inputs.select_package({"packages": packages}, policy)

    def test_test_harness_uses_pinned_manifest_and_resolved_sources(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            checkout = root / "checkout"
            policy = self.fixture(checkout)
            test_inputs = root / "test-inputs"
            test_inputs.mkdir()
            for name in ["Cargo.toml", "Cargo.lock"]:
                (test_inputs / name).write_text("pinned " + name)
            out = root / "test-package"
            before = quiche_inputs.sha(checkout / "quiche/src/lib.rs")
            with patch.object(quiche_tests, "resolve", return_value=(policy, {}, checkout)), patch.object(quiche_tests, "TEST_INPUTS", test_inputs):
                self.assertEqual(quiche_tests.prepare(out), out / "Cargo.toml")
                with self.assertRaises(FileExistsError):
                    quiche_tests.prepare(out)
            self.assertTrue((out / "src").is_symlink())
            self.assertEqual((out / "Cargo.lock").read_text(), "pinned Cargo.lock")
            self.assertEqual(json.loads((out / "source.json").read_text())["rev"], policy["rev"])
            self.assertEqual(quiche_inputs.sha(checkout / "quiche/src/lib.rs"), before)


if __name__ == "__main__":
    unittest.main()
