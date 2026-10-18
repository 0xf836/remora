#!/usr/bin/env python3
"""Reconstruct and build a proposed candidate only after its policy is frozen.
Existing source/binary/input outputs are refused. Dependencies may be cached in
an isolated target directory; source and executable identities remain explicit.
"""
import argparse
import io
import re
from pathlib import Path
import shutil
import subprocess
import tarfile
import time

from benchmark import sha, write
from diagnostic_regression import read


def extract_sources(archive, source):
    """Extract regular source files/directories, including on Python 3.11.2.

    Source snapshots do not need links or devices. Validate paths explicitly
    instead of falling back to unfiltered extractall on older Python versions.
    """
    source = source.resolve()
    members = archive.getmembers()
    for member in members:
        path = Path(member.name)
        if (path.is_absolute() or ".." in path.parts
                or not (member.isdir() or member.isfile())
                or not (source / path).resolve().is_relative_to(source)):
            raise ValueError("unsafe source archive member")
    for member in members:
        destination = source / member.name
        if member.isdir():
            destination.mkdir(parents=True, exist_ok=True)
        else:
            destination.parent.mkdir(parents=True, exist_ok=True)
            with archive.extractfile(member) as incoming, destination.open("wb") as output:
                shutil.copyfileobj(incoming, output)
            destination.chmod(member.mode & 0o755)


def prepare_baseline(inputs, revision, source, target, traffic):
    from benchmark import ROOT
    if not re.fullmatch(r"[0-9a-f]{40}", revision) or inputs.exists() or source.exists():
        raise ValueError("explicit revision and fresh baseline inputs/source required")
    inputs.mkdir(parents=True); source.mkdir(parents=True)
    archive = subprocess.check_output(["git", "archive", revision], cwd=ROOT)
    (inputs / "phase6.tar").write_bytes(archive)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar: extract_sources(tar, source)
    files = sorted(p for p in (ROOT / "src").rglob("*") if p.is_file())
    files += [ROOT / name for name in ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"]]
    with tarfile.open(inputs / "phase7-proposed.tar", "w") as tar:
        for path in files: tar.add(path, arcname=str(path.relative_to(ROOT)))
    write(inputs / "proposal.json", {"recorded_utc_seconds": time.time(), "base_revision": revision,
        "phase6_archive_sha256": sha(inputs / "phase6.tar"), "phase7_archive_sha256": sha(inputs / "phase7-proposed.tar"),
        "proposed_sources": {str(p.relative_to(ROOT)): sha(p) for p in files}})
    command = ["cargo", "build", "--manifest-path", str(source / "Cargo.toml"), "--target-dir", str(target),
        "--locked", "--offline", "--release", "--features", "qlog"]
    subprocess.run(command, check=True)
    binary = inputs / "phase6-remora"; shutil.copy2(target / "release/remora", binary)
    shutil.copy2(traffic, inputs / "traffic")
    sources = {str(p.relative_to(source)): sha(p) for p in (source / "src").rglob("*") if p.is_file()}
    sources.update({name: sha(source / name) for name in ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"]})
    identity = {"revision": revision, "source_root": str(source), "sources": sources, "features": ["qlog"],
        "compiler": subprocess.check_output(["rustc", "--version"], text=True).strip(), "binary_sha256": sha(binary),
        "built_utc_seconds": time.time(), "archive_sha256": sha(inputs / "phase6.tar"),
        "build_command": command, "builder_sha256": sha(Path(__file__)),
        "version_output": subprocess.check_output([str(binary), "--version"], text=True).strip()}
    write(inputs / "phase6-input.json", identity)
    return identity


def build_candidate(inputs, policy_path, source, target):
    policy = read(policy_path)
    proposal_path = inputs / "proposal.json"
    proposal = read(proposal_path)
    if policy.get("frozen") is not True or policy["proposal_sha256"] != sha(proposal_path) or policy["proposal"] != proposal:
        raise ValueError("unmatched frozen proposal")
    if sha(inputs / "phase6.tar") != proposal["phase6_archive_sha256"] or sha(inputs / "phase7-proposed.tar") != proposal["phase7_archive_sha256"]:
        raise ValueError("unmatched source archives")
    if policy["created_utc_seconds"] >= time.time():
        raise ValueError("policy freeze must precede construction")
    if source.exists() or (inputs / "phase7-remora").exists() or (inputs / "phase7-input.json").exists():
        raise ValueError("fresh candidate source/output required")
    source.mkdir(parents=True)
    materialized = time.time()
    for name in ["phase6.tar", "phase7-proposed.tar"]:
        with tarfile.open(inputs / name) as archive:
            extract_sources(archive, source)
    sources = {str(p.relative_to(source)): sha(p) for p in (source / "src").rglob("*") if p.is_file()}
    sources.update({p: sha(source / p) for p in ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"]})
    if sources != proposal["proposed_sources"]:
        raise ValueError("reconstructed source differs from frozen proposal")
    command = ["cargo", "build", "--manifest-path", str(source / "Cargo.toml"), "--target-dir", str(target), "--locked", "--offline", "--release", "--features", "qlog"]
    write(inputs / "candidate-construction.json", {"policy_sha256": sha(policy_path), "materialized_utc_seconds": materialized,
        "proposal_sha256": sha(proposal_path), "source_root": str(source), "sources": sources, "build_command": command,
        "builder_sha256": sha(Path(__file__)), "original_implementation_existed_before_freeze": True})
    subprocess.run(command, check=True)
    binary = inputs / "phase7-remora"; shutil.copy2(target / "release/remora", binary)
    identity = {"revision": "Phase7-proposal-on-" + proposal["base_revision"], "source_root": str(source), "sources": sources,
        "features": ["qlog"], "compiler": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "binary_sha256": sha(binary), "materialized_utc_seconds": materialized, "built_utc_seconds": time.time(),
        "policy_sha256": sha(policy_path), "builder_sha256": sha(Path(__file__)),
        "version_output": subprocess.check_output([str(binary), "--version"], text=True).strip()}
    write(inputs / "phase7-input.json", identity)
    return identity


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["inputs", "source", "target"]: parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--policy", type=Path)
    parser.add_argument("--prepare-baseline", action="store_true")
    parser.add_argument("--revision")
    parser.add_argument("--traffic-binary", type=Path)
    args = parser.parse_args()
    if args.prepare_baseline:
        if not args.revision or not args.traffic_binary: parser.error("baseline requires --revision and --traffic-binary")
        prepare_baseline(args.inputs.resolve(), args.revision, args.source.resolve(), args.target.resolve(), args.traffic_binary.resolve())
    else:
        if not args.policy: parser.error("candidate requires --policy")
        build_candidate(args.inputs.resolve(), args.policy.resolve(), args.source.resolve(), args.target.resolve())
