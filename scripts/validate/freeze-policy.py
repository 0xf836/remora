#!/usr/bin/env python3
"""Bind versioned MVP bounds to a local reference without changing limits."""

import argparse
import json
from pathlib import Path

from compare import digest, read, validate_policy


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    policy = read(args.policy)
    validate_policy(policy)
    # A bound policy must not be silently rebound to a different reference.
    if any(key in policy for key in (
        "reference", "reference_manifest_sha256", "reference_summary_sha256"
    )):
        raise ValueError("expected an unbound policy template")
    reference = args.reference.resolve()
    for filename in ("manifest.json", "summary.json"):
        read(reference / filename)
    policy["reference"] = str(reference / "manifest.json")
    policy["reference_manifest_sha256"] = digest(reference / "manifest.json")
    policy["reference_summary_sha256"] = digest(reference / "summary.json")
    policy["policy_template_sha256"] = digest(args.policy)
    with args.output.open("x") as output:
        json.dump(policy, output, indent=2, sort_keys=True)
        output.write("\n")


if __name__ == "__main__":
    main()
