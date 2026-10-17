#!/usr/bin/env python3
"""Final input-bound qualification. Checks scope never claims release acceptance."""
import argparse
import hashlib
import json
import platform
import subprocess
import sys
from pathlib import Path
from benchmark import install_termination_handler
from quiche_inputs import input_hashes
ROOT = Path(__file__).resolve().parents[2]

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def inputs():
    paths = [ROOT / "Cargo.toml", ROOT / "rust-toolchain.toml", ROOT / "build.rs", ROOT / "tools/build/Dockerfile", ROOT / "Cargo.lock", ROOT / "tools/traffic/Cargo.lock", ROOT / "tools/interop/go.mod", ROOT / "tools/interop/go.sum"]
    paths += list((ROOT / "src").rglob("*.rs"))
    return {**{str(p.relative_to(ROOT)): sha(p) for p in sorted(paths)}, **input_hashes()}

def stages(output, peer, fixture, scope):
    def stage(name, command):
        return dict(name=name, command=command)
    result = [
        stage("quiche-integrity", [sys.executable, "scripts/validate/quiche_inputs.py"]),
        stage("build", ["cargo", "build", "--locked", "--release"]),
        stage("qlog-build", ["cargo", "build", "--locked", "--release", "--features", "qlog"]),
        stage("format", ["cargo", "fmt", "--all", "--", "--check"]),
        stage("clippy", ["cargo", "clippy", "--locked", "--all-targets", "--all-features", "--", "-D", "warnings"]),
        stage("rust", ["cargo", "test", "--locked", "--all-targets", "--all-features"]),
        stage("quiche", [sys.executable, "scripts/validate/quiche_tests.py", "--output", str(output / "quiche-source")]),
        stage("python", [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py"]),
    ]
    for command in ["build", "test", "clippy", "fmt"]:
        args = ["cargo", command, "--manifest-path", "tools/traffic/Cargo.toml"]
        if command == "build": args += ["--locked", "--release"]
        if command == "test": args += ["--locked"]
        if command == "clippy": args += ["--locked", "--all-targets", "--", "-D", "warnings"]
        if command == "fmt": args += ["--all", "--", "--check"]
        result.append(stage("traffic-"+command, args))
    result += [
        stage("operations", ["./scripts/validate/operations.sh", "--output", str(output / "operations")]),
        stage("fixtures", ["./scripts/validate/fixtures.sh", "--output", str(output / "fixtures")]),
        stage("interop", ["./scripts/validate/interop.sh", "--fixtures", str(output / "fixtures"), "--peer-binary", str(peer), "--output", str(output / "interop")]),
        stage("external-process", ["cargo", "test", "--locked", "--all-features", "--test", "e2e", "--", "--ignored", "--test-threads=1"]),
    ]
    if scope == "full":
        result += [
            stage("generic-isolation-30min", [sys.executable, "scripts/validate/concurrency.py", "--scenario", "isolation", "--seconds", "1800", "--output", str(output / "isolation")]),
            stage("overload-10min", [sys.executable, "scripts/validate/concurrency.py", "--scenario", "overload", "--seconds", "600", "--output", str(output / "overload")]),
            stage("churn-1000", [sys.executable, "scripts/validate/concurrency.py", "--scenario", "churn", "--cycles", "1000", "--output", str(output / "churn")]),
            stage("network", ["./scripts/validate/network.sh", "--scenario", "all", "--output", str(output / "network")]),
            stage("recovery", ["./scripts/validate/recovery.sh", "--scenario", "all", "--peer-binary", str(peer), "--output", str(output / "recovery")]),
            stage("shutdown", ["./scripts/validate/recovery.sh", "--scenario", "shutdown", "--peer-binary", str(peer), "--output", str(output / "shutdown")]),
            stage("wireguard-30min", ["./scripts/validate/wireguard.sh", "--fixture", str(fixture), "--seconds", "1800", "--start-remora", "--output", str(output / "wireguard")]),
        ]
    return result

def evidence_valid(path, source):
    try:
        d = json.loads(path.read_text())
        if not isinstance(d, dict): return False
        artifacts = d.get("artifacts")
        return (d.get("status") == "passed" and d.get("production_inputs") == source
                and d.get("tasks") == {name: "passed" for name in ["T073", "T074", "T075"]}
                and isinstance(artifacts, dict) and bool(artifacts)
                and all(Path(name).is_absolute() and sha(Path(name)) == digest for name, digest in artifacts.items()))
    except (OSError, ValueError, TypeError):
        return False

def resume_prefix(path, output, plan, source, binary, peer):
    """Reuse only an exact passed prefix from failed full-scope qualification."""
    old = path.parent.resolve()
    d = json.loads(path.read_text())
    checks = d.get("checks", [])
    if (d.get("status") != "failed" or d.get("scope") != "full"
            or d.get("architecture") != platform.machine()
            or d.get("production_inputs") != source or not checks
            or checks[-1].get("status") != "failed"):
        raise ValueError("unmatched failed qualification source/scope/prefix")
    operations = json.loads((old / "operations/manifest.json").read_text())
    interop = json.loads((old / "interop/manifest.json").read_text())
    if (operations.get("binary_sha256") != sha(binary)
            or interop.get("remora_binary_sha256") != sha(binary)
            or interop.get("peer_binary_sha256") != sha(peer)):
        raise ValueError("qualification executable changed; passed prefix cannot be reused")
    reused = []
    for index, check in enumerate(checks):
        normalized = [arg.replace(str(old), str(output)) for arg in check["command"]]
        if index >= len(plan) or check["name"] != plan[index]["name"] or normalized != plan[index]["command"]:
            raise ValueError("qualification command/profile changed")
        if index == len(checks)-1: break
        log = old / (check["name"]+".log")
        if check.get("status") != "passed" or check.get("exit_code") != 0 or not log.is_file():
            raise ValueError("incomplete passed qualification prefix")
        reused.append(dict(check, reused_from=str(old), retained_log_sha256=sha(log)))
    return reused

def main():
    install_termination_handler()
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--output", required=True, type=Path)
    p.add_argument("--peer-binary", required=True, type=Path)
    p.add_argument("--wireguard-fixture", type=Path)
    p.add_argument("--scope", choices=["checks", "full"], default="full")
    p.add_argument("--prerequisites", type=Path, help="T073-T075 passed input-bound evidence JSON")
    p.add_argument("--resume-from", type=Path, help="failed full summary; exact passed prefix with unchanged production/executables")
    p.add_argument("--plan-only", action="store_true")
    a = p.parse_args()
    out = a.output.resolve()
    source = inputs()
    plan = stages(out, a.peer_binary.resolve(), a.wireguard_fixture, a.scope)
    if a.plan_only:
        print(json.dumps(dict(scope=a.scope, stages=plan, release_acceptance_claim=False), indent=2)); return 0
    out.mkdir(parents=True, mode=0o700, exist_ok=False)
    missing = []
    if not a.peer_binary.is_file(): missing.append("pinned independent peer binary")
    if a.scope == "full":
        if not a.wireguard_fixture or not a.wireguard_fixture.is_file(): missing.append("prepared private WireGuard fixture")
        if not a.prerequisites or not evidence_valid(a.prerequisites, source): missing.append("T073-T075 passed evidence for these exact inputs")
    summary = dict(schema_version=1, scope=a.scope, architecture=platform.machine(), kernel=platform.release(),
        production_inputs=source, revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        missing=missing, status="blocked" if missing else "running", checks=[], release_acceptance_claim=False)
    def save(): (out / "summary.json").write_text(json.dumps(summary, indent=2)+"\n")
    save()
    if a.resume_from and not missing:
        if a.scope != "full": raise ValueError("resume requires full scope")
        try:
            reused = resume_prefix(a.resume_from.resolve(), out, plan, source, ROOT / "target/release/remora", a.peer_binary)
            summary["checks"] = reused
            summary["resumed_from"] = str(a.resume_from.resolve())
            summary["resumed_summary_sha256"] = sha(a.resume_from)
            plan = plan[len(reused):]
        except (OSError, ValueError, KeyError, TypeError) as error:
            summary["status"] = "invalid"; summary["missing"].append(str(error)); save(); return 2
        save()
    if missing: print(json.dumps(summary)); return 2
    import os
    env = dict(os.environ, REMORA_INTEROP_BIN=str(a.peer_binary.resolve()))
    for stage in plan:
        with (out / (stage["name"]+".log")).open("x") as log:
            try:
                code = subprocess.run(stage["command"], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=7200).returncode
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(str(error)); code = 125
        summary["checks"].append(dict(stage, exit_code=code, status="passed" if code == 0 else "failed")); save()
        print(stage["name"], code, flush=True)
        if code != 0:
            summary["status"] = "failed"; save(); return 1
        if inputs() != source:
            summary["status"] = "invalid"; summary["missing"].append("production inputs changed during qualification"); save(); return 1
    summary["status"] = "passed" if a.scope == "full" else "checks-passed"
    # T077 numeric acceptance and T078 explicit reviews remain separate gates.
    summary["release_binary_sha256"] = sha(ROOT / "target/release/remora")
    summary["peer_sha256"] = sha(a.peer_binary)
    save(); return 0
if __name__ == "__main__": raise SystemExit(main())
