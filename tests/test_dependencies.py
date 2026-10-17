import importlib.util
import json
import sys
from pathlib import Path
import unittest
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/validate"))
spec = importlib.util.spec_from_file_location("deps_validation", ROOT / "scripts/validate/dependencies.py")
deps = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deps)
class AdvisoryGate(unittest.TestCase):
    def stream(self, packages):
        advisory = dict(id="GO-test", affected=[dict(package=dict(name="module"), ecosystem_specific=dict(imports=[dict(path=p) for p in packages]))])
        return json.dumps(dict(osv=advisory))+"\n"+json.dumps(dict(finding=dict(osv="GO-test")))
    def test_absent_package_explicitly_non_applicable(self):
        rows = deps.go_applicability(self.stream(["http2"]), {"http3", "http2/hpack"})
        self.assertEqual(rows[0]["disposition"], "not-imported")
    def test_present_package_and_missing_metadata_require_review(self):
        for packages in [["http3"], []]:
            rows = deps.go_applicability(self.stream(packages), {"http3"})
            self.assertEqual(rows[0]["disposition"], "review-required")
    def test_incomplete_or_malformed_report_cannot_pass(self):
        for stream in ['{', json.dumps(dict(finding=dict(osv="missing")))]:
            with self.assertRaises(ValueError): deps.go_applicability(stream, set())
if __name__ == "__main__": unittest.main()
