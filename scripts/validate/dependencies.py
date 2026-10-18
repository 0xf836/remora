#!/usr/bin/env python3
"""Resolved graphs, live advisory snapshots and native locked build evidence."""
import argparse
import datetime
import hashlib
import json
import platform
import subprocess
from pathlib import Path
from quiche_inputs import input_hashes, verify
from quiche_tests import prepare
ROOT = Path(__file__).resolve().parents[2]

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def go_applicability(stream, imported):
    decoder = json.JSONDecoder()
    records = []
    while stream.strip():
        record, length = decoder.raw_decode(stream.lstrip())
        stream = stream.lstrip()[length:]
        records.append(record)
    advisories = {r["osv"]["id"]: r["osv"] for r in records if "osv" in r}
    findings = {r["finding"]["osv"] for r in records if "finding" in r}
    rows = []
    for name in sorted(findings):
        advisory = advisories.get(name)
        if advisory is None:
            raise ValueError("finding without preserved advisory")
        packages = {i["path"] for a in advisory["affected"]
                    if a["package"]["name"] != "stdlib"
                    for i in a.get("ecosystem_specific", {}).get("imports", [])}
        relevant = sorted(packages & imported)
        # Require explicit review if an affected package is present or metadata
        # is insufficient. Never infer a clean graph from scanner exit alone.
        rows.append(dict(advisory=name, affected_packages=sorted(packages),
            imported_affected_packages=relevant,
            disposition="not-imported" if packages and not relevant else "review-required"))
    return rows

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--output", required=True, type=Path)
    a = p.parse_args()
    a.output.mkdir(parents=True, mode=0o700, exist_ok=False)
    out = a.output.resolve()
    provenance = verify()
    quiche_hashes = input_hashes()
    test_manifest = prepare(out / "quiche-source")
    results = []
    commands = [
        ("rust-version", ["rustc", "--version", "--verbose"], ROOT),
        ("go-version", ["go", "version"], ROOT),
        ("rust-graph", ["cargo", "metadata", "--locked", "--format-version=1", "--all-features"], ROOT),
        ("traffic-graph", ["cargo", "metadata", "--locked", "--format-version=1", "--manifest-path", "tools/traffic/Cargo.toml"], ROOT),
        ("fuzz-graph", ["cargo", "metadata", "--locked", "--format-version=1", "--manifest-path", "fuzz/Cargo.toml"], ROOT),
        ("safety-graph", ["cargo", "metadata", "--locked", "--format-version=1", "--manifest-path", "tools/safety/Cargo.toml"], ROOT),
        ("quiche-tests-graph", ["cargo", "metadata", "--locked", "--format-version=1", "--manifest-path", str(test_manifest)], ROOT),
        ("audit-version", ["cargo", "audit", "--version"], ROOT),
    ]
    for name, lock in [("production", "Cargo.lock"), ("traffic", "tools/traffic/Cargo.lock"), ("fuzz", "fuzz/Cargo.lock"), ("safety", "tools/safety/Cargo.lock"), ("quiche-tests", "dependencies/quiche-tests/Cargo.lock")]:
        commands.append((name+"-audit", ["cargo", "audit", "--json", "--file", lock, "--db", str(out / "advisory-db")], ROOT))
    commands += [
        ("go-graph", ["go", "list", "-m", "-json", "all"], ROOT / "tools/interop"),
        ("go-packages", ["go", "list", "-mod=readonly", "-deps", "./..."], ROOT / "tools/interop"),
        ("go-verify", ["go", "mod", "verify"], ROOT / "tools/interop"),
        ("govulncheck-version", ["govulncheck", "-version"], ROOT / "tools/interop"),
        ("go-audit", ["govulncheck", "-json", "./..."], ROOT / "tools/interop"),
        ("native-release", ["cargo", "build", "--locked", "--release"], ROOT),
        ("native-qlog", ["cargo", "build", "--locked", "--release", "--features", "qlog"], ROOT),
        ("native-traffic", ["cargo", "build", "--locked", "--release", "--manifest-path", "tools/traffic/Cargo.toml"], ROOT),
        ("native-peer", ["go", "build", "-mod=readonly", "-o", str(out / "interop-peer"), "."], ROOT / "tools/interop"),
    ]
    for name, command, cwd in commands:
        with (out / (name+".stdout")).open("x") as stdout, (out / (name+".stderr")).open("x") as stderr:
            try:
                code = subprocess.run(command, cwd=cwd, stdout=stdout, stderr=stderr, timeout=1800).returncode
            except (OSError, subprocess.TimeoutExpired) as error:
                stderr.write(str(error)); code = 125
        results.append(dict(name=name, command=command, exit_code=code, status="passed" if code == 0 else "failed"))
    try:
        applicability = go_applicability((out / "go-audit.stdout").read_text(), set((out / "go-packages.stdout").read_text().splitlines()))
        reviewed = all(row["disposition"] == "not-imported" for row in applicability)
    except (ValueError, KeyError) as error:
        applicability = [dict(error=str(error))]; reviewed = False
    (out / "go-applicability.json").write_text(json.dumps(applicability, indent=2)+"\n")
    pinned = (out / "rust-version.stdout").read_text().startswith("rustc 1.99.0 ")
    locks = {s: sha(ROOT/s) for s in ["Cargo.lock", "tools/traffic/Cargo.lock", "fuzz/Cargo.lock", "tools/safety/Cargo.lock", "dependencies/quiche-tests/Cargo.lock", "tools/interop/go.mod", "tools/interop/go.sum", "tools/build/Dockerfile"]}
    stable_quiche = input_hashes() == quiche_hashes
    status = "passed" if pinned and reviewed and stable_quiche and all(r["exit_code"] == 0 for r in results) else "failed"
    summary = dict(schema_version=1, status=status, architecture=platform.machine(), kernel=platform.release(),
        collected_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), locks=locks, checks=results,
        pinned_production_toolchain=pinned, go_advisory_applicability_reviewed=reviewed, other_native_targets_qualified=False,
        quiche_source_sha256=quiche_hashes, quiche_provenance=provenance, quiche_inputs_unchanged=stable_quiche)
    (out / "summary.json").write_text(json.dumps(summary, indent=2)+"\n")
    print(json.dumps(dict(status=status, output=str(out))))
    return 0 if status == "passed" else 1
if __name__ == "__main__":
    raise SystemExit(main())
