import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/validate"))
spec = importlib.util.spec_from_file_location("release_validation", ROOT / "scripts/validate/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
class ReleaseGate(unittest.TestCase):
    def test_full_plan_retains_duration_churn_and_all_external_gates(self):
        stages = {s["name"]: s["command"] for s in release.stages(Path("/tmp/run"), Path("/tmp/peer"), Path("/tmp/wg"), "full")}
        for name, value in [("generic-isolation-30min", "1800"), ("overload-10min", "600"), ("churn-1000", "1000"), ("wireguard-30min", "1800")]:
            self.assertIn(value, stages[name])
        self.assertTrue({"interop", "network", "recovery", "shutdown", "operations", "external-process"} <= stages.keys())
    def test_checks_plan_cannot_include_long_qualification(self):
        names = {s["name"] for s in release.stages(Path("/tmp/run"), Path("/tmp/peer"), None, "checks")}
        self.assertNotIn("wireguard-30min", names)
        self.assertNotIn("generic-isolation-30min", names)
    def test_gate_rejects_failed_missing_and_wrong_revision_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory) / "evidence.json"
            source = {"src/reactor.rs": "hash"}
            self.assertFalse(release.evidence_valid(p, source))
            for status, inputs in [("failed", source), ("passed", {}), ("checks-passed", source)]:
                p.write_text(json.dumps(dict(status=status, production_inputs=inputs)))
                self.assertFalse(release.evidence_valid(p, source))
            artifact = Path(directory) / "passed.log"
            artifact.write_text("passed")
            p.write_text(json.dumps(dict(status="passed", production_inputs=source, tasks={name: "passed" for name in ["T073", "T074", "T075"]}, artifacts={str(artifact): release.sha(artifact)})))
            self.assertTrue(release.evidence_valid(p, source))
            artifact.write_text("changed")
            self.assertFalse(release.evidence_valid(p, source))
    def test_resume_rejects_source_binary_and_shortened_duration_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); old = root / "old"; out = root / "new"
            (old / "operations").mkdir(parents=True); (old / "interop").mkdir()
            binary = root / "binary"; binary.write_text("binary")
            peer = root / "peer"; peer.write_text("peer")
            source = {"src/reactor.rs": "hash"}
            plan = [dict(name="soak", command=["run", str(out / "soak"), "1800"]), dict(name="network", command=["net"])]
            d = dict(status="failed", scope="full", architecture=release.platform.machine(), production_inputs=source,
                checks=[dict(name="soak", command=["run", str(old / "soak"), "1800"], status="passed", exit_code=0), dict(name="network", command=["net"], status="failed", exit_code=1)])
            summary = old / "summary.json"; summary.write_text(json.dumps(d))
            (old / "soak.log").write_text("passed")
            (old / "operations/manifest.json").write_text(json.dumps(dict(binary_sha256=release.sha(binary))))
            (old / "interop/manifest.json").write_text(json.dumps(dict(remora_binary_sha256=release.sha(binary), peer_binary_sha256=release.sha(peer))))
            self.assertEqual(len(release.resume_prefix(summary, out, plan, source, binary, peer)), 1)
            for changed in [{}, {"src/reactor.rs": "changed"}]:
                with self.assertRaises(ValueError): release.resume_prefix(summary, out, plan, changed, binary, peer)
            d["checks"][0]["command"][-1] = "10"; summary.write_text(json.dumps(d))
            with self.assertRaises(ValueError): release.resume_prefix(summary, out, plan, source, binary, peer)
            d["checks"][0]["command"][-1] = "1800"; summary.write_text(json.dumps(d)); binary.write_text("changed")
            with self.assertRaises(ValueError): release.resume_prefix(summary, out, plan, source, binary, peer)

    def test_inputs_include_build_and_lock_contract(self):
        files = release.inputs()
        self.assertTrue({"Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml", "src/reactor.rs", "tools/interop/go.sum", "git:quiche/quiche/src/lib.rs", "dependencies/quiche.json", "dependencies/quiche-tests/Cargo.lock"} <= files.keys())
if __name__ == "__main__": unittest.main()
