#!/usr/bin/env python3
"""Unprivileged clean-install and bounded collected-failure walkthrough.
No host configuration changes, public listeners or benchmark claims.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
from benchmark import install_termination_handler
import platform
import shutil
import socket
import subprocess
import tempfile
import time
import tomllib
from benchmark import ROOT, Owned, sha, wait_udp_bound, machine_diagnostics


def snapshots(path):
    return [json.loads(line) for line in path.read_text().splitlines()]


def correlated(records, failure, code):
    if any(r["coverage_lost"] or r["status_write_failures"] or r["counter_overflow"] for r in records):
        raise RuntimeError("SC-010 collection failed: explicit coverage gap or invalid counters")
    return [f for r in records for f in r["failures"] if f["class"] == failure and f["code"] == code and f["session"] and f["connection"]]


def exercise(sender, target, listen, payload):
    deadline = time.monotonic() + 6
    while time.monotonic() < deadline:
        sender.sendto(payload, listen)
        try:
            data, relay = target.recvfrom(65536)
            if data != payload:
                raise RuntimeError("application datagram integrity failed")
            target.sendto(data, relay)
            reply, _ = sender.recvfrom(65536)
            if reply != payload:
                raise RuntimeError("return datagram integrity failed")
            return
        except socket.timeout:
            pass
    raise RuntimeError("authorized bidirectional readiness failed")


def case(binary, output, name):
    root = output / name
    root.mkdir(mode=0o700)
    owned = []
    with tempfile.TemporaryDirectory(prefix="remora-operations-") as temporary:
        fixture = Path(temporary) / "private"
        subprocess.run([str(ROOT / "scripts/validate/fixtures.sh"), "--output", str(fixture)], check=True, timeout=40)
        ports = json.loads((fixture / "ports.json").read_text())
        expected = None
        if name == "authentication":
            (fixture / "client.token").write_text("0" * 64 + "\n")
            expected = ("server", "authentication", 401)
        elif name == "authorization":
            p = fixture / "credentials.toml"
            p.write_text(p.read_text().replace('host = "127.0.0.1"', 'host = "denied.test"'))
            expected = ("server", "authorization", 403)
        elif name == "trust":
            p = fixture / "client.toml"
            p.write_text(p.read_text().replace("remora.test", "wrong.test"))
        if name == "resource":
            p = fixture / "server.toml"
            p.write_text(p.read_text() + "\n[limits]\nmax_sessions_per_identity = 1\n")
            expected = ("server", "resource", 503)
        if name == "size":
            expected = ("client", "size", 3)
        if name == "socket":
            expected = ("server", "socket", 111)
        for role in ["client", "server"]:
            p = fixture / f"{role}.toml"
            # All failures before cadence must survive through terminal cleanup.
            p.write_text(machine_diagnostics(p.read_text(), "error", 10000))
            checked = subprocess.run([str(binary), "check-config", "--config", str(p)], capture_output=True, timeout=5)
            if checked.returncode:
                raise RuntimeError("private candidate check-config failed")
        listen = ("127.0.0.1", ports["client"])
        target = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sender = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        target.bind(("127.0.0.1", ports["target"]))
        sender.bind(("127.0.0.1", 0))
        target.settimeout(0.15)
        sender.settimeout(0.15)
        extra = None
        try:
            for role in ["server", "client"]:
                proc = Owned([str(binary), role, "--config", str(fixture / f"{role}.toml")], root / role)
                owned.append(proc)
                address = tomllib.loads((fixture / f"{role}.toml").read_text())[role]["listen"]
                if role == "server" or name not in ["authentication", "authorization", "trust"]:
                    wait_udp_bound(proc, address[0] if isinstance(address, list) else address)
            sentinel = b"OPERATIONS_PRIVATE_PAYLOAD_SENTINEL"
            if name in ["authentication", "authorization", "trust"]:
                for _ in range(12):
                    sender.sendto(sentinel, listen)
                    try:
                        target.recvfrom(65536)
                        raise RuntimeError("unauthorized destination traffic")
                    except socket.timeout:
                        pass
            else:
                exercise(sender, target, listen, sentinel)
                if name == "clean-install":
                    for size in [0, 1, 128, 512, 1200]:
                        exercise(sender, target, listen, b"x" * size)
                elif name == "size":
                    sender.sendto(b"x" * 1401, listen)
                    try:
                        target.recvfrom(65536)
                        raise RuntimeError("oversize packet reached target")
                    except socket.timeout:
                        pass
                elif name == "resource":
                    extra = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                    extra.bind(("127.0.0.1", 0))
                    for _ in range(12):
                        extra.sendto(sentinel, listen)
                        try:
                            target.recvfrom(65536)
                            raise RuntimeError("capacity-rejected sender reached target")
                        except socket.timeout:
                            pass
                elif name == "socket":
                    target.close()
                    for _ in range(12):
                        sender.sendto(sentinel, listen)
                        time.sleep(0.04)
                elif name == "connection":
                    owned[0].stop()
                    # Old accepted Session receives the observed native close;
                    # it may be Transport/0 or native idle PeerTimeout/0.
                    for _ in range(30):
                        sender.sendto(sentinel, listen)
                        time.sleep(0.2)
        finally:
            for proc in reversed(owned):
                proc.stop()
            target.close()
            sender.close()
            if extra:
                extra.close()
        token = (fixture / "client.token").read_text().strip()
        all_records = {}
        for role, proc in zip(["server", "client"], owned):
            text = "\n".join((root / f"{role}.{stream}.ndjson").read_text() for stream in ["stdout", "stderr"])
            if token in text or "OPERATIONS_PRIVATE_PAYLOAD_SENTINEL" in text:
                raise RuntimeError("diagnostic privacy failure")
            records = snapshots(root / f"{role}.stdout.ndjson")
            if not records or any(r["coverage_lost"] or r["status_write_failures"] or r["counter_overflow"] for r in records):
                raise RuntimeError("SC-010 collection failed or incomplete")
            all_records[role] = records
            if role == "server" and proc.p.returncode != 0:
                raise RuntimeError("server did not stop gracefully")
        if expected:
            role, failure, code = expected
            matched = correlated(all_records[role], failure, code)
            if not matched:
                raise RuntimeError(f"missing correlated {failure}/{code}")
        elif name == "trust":
            # Trust can fail before a Session exists: connection correlation is
            # required, and absence of a Session is explicit rather than invented.
            matched = [f for r in all_records["client"] for f in r["failures"] if f["class"] == "trust" and f["code"] == 0 and f["connection"]]
            if not matched:
                raise RuntimeError("missing pre-session trust/0 connection correlation")
        elif name == "connection":
            matched = correlated(all_records["client"], "transport", 0) or correlated(all_records["client"], "peer_timeout", 0)
            if not matched:
                raise RuntimeError("missing affected Session connection-close correlation")
        else:
            matched = []
            if any(proc.p.returncode != 0 for proc in owned):
                raise RuntimeError("clean session runtime failed")
            if not any(r["lifecycle"]["accepted_total"] > 0 for r in all_records["server"]):
                raise RuntimeError("accepted session status missing")
        return {"case": name, "status": "pass", "correlated_entries": len(matched), "coverage_lost": 0,
                "status_write_failures": 0, "role_exit_codes": {role: proc.p.returncode for role, proc in zip(["server", "client"], owned)}}


def main():
    install_termination_handler()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/remora")
    args = parser.parse_args()
    if os.geteuid() == 0:
        parser.error("run as an ordinary user; elevated runtime is outside this walkthrough")
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error("build the locked release executable first")
    os.umask(0o077)
    output = args.output.absolute()
    output.mkdir(mode=0o700)  # Refuse existing output; preserve all failed runs.
    installed = output / "install/bin/remora"
    installed.parent.mkdir(parents=True, mode=0o700)
    shutil.copy2(binary, installed)
    installed.chmod(0o700)
    source_paths = [p for base in [ROOT / "src", ROOT / "tests", ROOT / "examples"] for p in base.rglob("*") if p.is_file() and "__pycache__" not in p.parts]
    source_paths += [ROOT / p for p in ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml", "scripts/validate/operations.py", "scripts/validate/operations.sh", "scripts/validate/fixtures.sh", "scripts/validate/fixture_ports.py"]]
    manifest = {"schema_version": 1, "timestamp": datetime.now(timezone.utc).isoformat(),
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True)),
        "binary_sha256": sha(installed), "source_sha256": {str(p.relative_to(ROOT)): sha(p) for p in sorted(source_paths)},
        "uid": os.geteuid(), "kernel": platform.release(), "machine": platform.machine(),
        "python": platform.python_version(), "openssl": subprocess.check_output(["openssl", "version"], text=True).strip(),
        "scope": "T064-T070 local operations; T071 excluded; no performance or release qualification"}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    results = []
    status = "fail"
    try:
        for name in ["clean-install", "authentication", "authorization", "trust", "resource", "size", "socket", "connection"]:
            result = case(installed, output, name)
            results.append(result)
            print(json.dumps(result), flush=True)
        # Bind process suites explicitly to the compiler-reported debug binary.
        build = subprocess.run(["cargo", "test", "--locked", "--all-features", "--test", "e2e", "--no-run", "--message-format=json"], cwd=ROOT, capture_output=True, text=True, check=True, timeout=120)
        artifacts = [r for line in build.stdout.splitlines() if (r := json.loads(line)).get("reason") == "compiler-artifact" and r.get("target", {}).get("name") == "remora" and "bin" in r.get("target", {}).get("kind", []) and r.get("executable")]
        if len(artifacts) != 1:
            raise RuntimeError("exact process-test binary unavailable")
        manifest["process_test_binary_sha256"] = sha(Path(artifacts[0]["executable"]))
        (output / "test-build.jsonl").write_text(build.stdout)
        (output / "test-build.log").write_text(build.stderr)
        for index, (target, selector) in enumerate([
            ("e2e", "operations::"), ("e2e", "diagnostics::"),
            ("security", "status_writer::"), ("security", "qlog::"),
            ("lib", "diagnostic_setup_fault_matrix"),
        ]):
            command = ["cargo", "test", "--locked", "--all-features"] + (["--lib"] if target == "lib" else ["--test", target]) + [selector, "--", "--test-threads=1"]
            with (output / f"checks-{index}.log").open("x") as log:
                subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
            results.append({"case": f"{target}/{selector}", "status": "pass", "command": command})
        (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        status = "pass"
    except Exception as error:
        status = "fail"
        results.append({"status": "fail", "error_class": type(error).__name__, "safe_reason": str(error) if isinstance(error, RuntimeError) else "prerequisite or command failed; inspect retained logs"})
        raise
    finally:
        (output / "summary.json").write_text(json.dumps({"schema_version": 1, "status": status, "results": results,
            "limitations": ["native ARM64 and final release not qualified", "T071 overhead not measured",
                "DNS/setup faults are deterministic completion injections, not a live DNS failure claim",
                "intentional blocked/overflow tests establish robustness, not zero-loss SC-010 coverage"]}, indent=2) + "\n")

if __name__ == "__main__":
    main()
