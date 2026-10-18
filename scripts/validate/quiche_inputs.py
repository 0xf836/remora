#!/usr/bin/env python3
"""Bind Cargo's resolved quiche to the reviewed Git revision and source bytes."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
POLICY = ROOT / "dependencies/quiche.json"
TEST_INPUTS = ROOT / "dependencies/quiche-tests"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def select_package(metadata, policy):
    source = f"git+{policy['git']}?rev={policy['rev']}#{policy['rev']}"
    packages = [p for p in metadata["packages"] if p["name"] == "quiche"]
    if len(packages) != 1 or packages[0]["version"] != policy["version"] or packages[0]["source"] != source:
        raise ValueError("quiche is not resolved to the reviewed Git revision")
    if any(p.get("source", "") and p["source"].startswith("git+") and p["name"] != "quiche" for p in metadata["packages"]):
        raise ValueError("unexpected Git dependency; preserve registry transitive pins")
    return packages[0]


def verify_checkout(checkout, policy):
    expected = policy["files_sha256"]
    if not expected or "Cargo.toml" not in expected or "quiche/src/lib.rs" not in expected:
        raise ValueError("incomplete quiche source policy")
    actual = {"Cargo.toml"} | {
        str(p.relative_to(checkout)) for p in (checkout / "quiche").rglob("*")
        if p.is_file() and not {"target", "__pycache__"} & set(p.relative_to(checkout).parts)
    }
    if actual != set(expected):
        raise ValueError("quiche input file set changed")
    for name, digest in expected.items():
        relative = Path(name)
        path = checkout / relative
        if (relative.is_absolute() or ".." in relative.parts
                or any((checkout / Path(*relative.parts[:i])).is_symlink() for i in range(1, len(relative.parts)))
                or not path.resolve().is_relative_to(checkout.resolve())):
            raise ValueError("invalid quiche source path")
        target = policy.get("symlinks", {}).get(name)
        if path.is_symlink() != (target is not None) or target is not None and os.readlink(path) != target:
            raise ValueError(f"unreviewed quiche symlink: {name}")
        if sha(path) != digest:
            raise ValueError(f"quiche source changed: {name}")
    return {"git:quiche/" + name: checkout / name for name in expected}


def resolve():
    policy = json.loads(POLICY.read_text())
    for name in ["Cargo.toml", "tools/traffic/Cargo.toml", "fuzz/Cargo.toml"]:
        manifest = tomllib.loads((ROOT / name).read_text())
        if manifest["patch"]["crates-io"]["quiche"] != {"git": policy["git"], "rev": policy["rev"]}:
            raise ValueError(f"unmatched quiche override: {name}")
    try:
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--offline", "--all-features", "--format-version=1"],
            cwd=ROOT, text=True,
        ))
    except subprocess.CalledProcessError as error:
        raise ValueError("cannot resolve pinned quiche offline; run cargo fetch --locked first") from error
    package = select_package(metadata, policy)
    checkout = Path(package["manifest_path"]).parent.parent
    files = verify_checkout(checkout, policy)
    files["dependencies/quiche.json"] = POLICY
    files.update({"dependencies/quiche-tests/" + p.name: p for p in [TEST_INPUTS / "Cargo.toml", TEST_INPUTS / "Cargo.lock"]})
    return policy, files, checkout


def input_hashes():
    return {name: sha(path) for name, path in resolve()[1].items()}


def verify():
    policy, _, _ = resolve()
    return policy


if __name__ == "__main__":
    policy, files, _ = resolve()
    print(json.dumps({"status": "passed", "git": policy["git"], "rev": policy["rev"],
                      "upstream": policy["upstream"], "inputs_sha256": {n: sha(p) for n, p in files.items()}}))
