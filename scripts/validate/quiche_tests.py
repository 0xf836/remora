#!/usr/bin/env python3
"""Test reviewed Git sources using the retained published-crate test lock."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

from quiche_inputs import ROOT, TEST_INPUTS, input_hashes, resolve


def prepare(output):
    policy, _, checkout = resolve()
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    for path in (checkout / "quiche").iterdir():
        if path.name not in {"Cargo.toml", "Cargo.lock", "target"}:
            (output / path.name).symlink_to(path, target_is_directory=path.is_dir())
    for name in ["Cargo.toml", "Cargo.lock"]:
        shutil.copy2(TEST_INPUTS / name, output / name)
    (output / "source.json").write_text(json.dumps({"git": policy["git"], "rev": policy["rev"],
        "test_profile": "published 0.30.0 manifest/lock with committed Git source tree"}, indent=2) + "\n")
    return output / "Cargo.toml"


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--prepare-only", action="store_true")
    a = p.parse_args()
    before = input_hashes()
    manifest = prepare(a.output.resolve())
    if a.prepare_only:
        print(manifest)
        return 0
    code = subprocess.run(["cargo", "test", "--locked", "--manifest-path", str(manifest),
        "--target-dir", str(ROOT / "target/quiche-tests"), "--lib"], cwd=ROOT).returncode
    if input_hashes() != before:
        raise RuntimeError("quiche inputs changed during tests")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
