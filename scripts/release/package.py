#!/usr/bin/env python3
"""Package a native release binary without including the working tree or fixtures."""

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import struct
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
TARGET = "x86_64-unknown-linux-gnu"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def package(binary, build_packages, output, tag):
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
        raise ValueError("expected a stable version tag such as v0.1.0")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    if tag != f"v{version}":
        raise ValueError("tag does not match Cargo.toml version")
    binary = binary.resolve(strict=True)
    data = binary.read_bytes()
    # ELF64, little endian, EM_X86_64. Reject cross-built or wrong-platform assets.
    if len(data) < 64 or data[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", data, 18)[0] != 62:
        raise ValueError("release binary must be a Linux x86_64 ELF executable")
    revision = command("git", "rev-parse", "HEAD")
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("release requires a Git source revision")
    binary_version = command(str(binary), "--version")
    if not binary_version.startswith(f"remora {version} commit={revision} "):
        raise ValueError("binary version or embedded source revision does not match checkout")
    # Changing tracked source after the build would make the metadata misleading.
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--"], cwd=ROOT, check=True,
                   stdout=subprocess.DEVNULL)
    timestamp = int(command("git", "show", "-s", "--format=%ct", "HEAD"))
    metadata = {
        "schema_version": 1,
        "version": version,
        "tag": tag,
        "source_revision": revision,
        "target": TARGET,
        "features": ["qlog"],
        "minimum_linux_version": "6.1",
        "minimum_glibc_version": "2.36",
        "runtime_libraries": re.findall(r"\(NEEDED\).*?\[(.*?)\]",
                                        command("readelf", "--dynamic", str(binary))),
        "binary_sha256": sha256(data),
        "binary_version": binary_version,
        "rustc_version": command("rustc", "--version", "--verbose"),
        "build_inputs_sha256": {
            name: sha256((ROOT / name).read_bytes())
            for name in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "build.rs",
                         "tools/build/Dockerfile", "dependencies/quiche.json"]
        },
        "build_packages_sha256": sha256(build_packages.read_bytes()),
    }
    readme = f"""Remora {version}

Linux GNU x86_64; Linux 6.1+ and glibc 2.36+.
Requires libstdc++.so.6 and libgcc_s.so.1 from Debian 12 or a compatible system.
Built with qlog support; tracing is disabled until configured at runtime.

Verify the downloaded archive with: sha256sum --check SHA256SUMS
Inspect this binary with: ./remora --version
Install it, for example, with: sudo install -m 0755 remora /usr/local/bin/remora

Configuration, TLS provisioning, usage and known limitations:
https://github.com/0xf836/remora/tree/{tag}

BUILD-INFO.json identifies this binary and its build inputs.
build-packages.txt records packages in the pinned build container.
Recorded performance/qualification results identify their own executables;
this archive does not transfer those results to a new binary hash.
"""
    members = {
        "remora": (data, 0o755),
        "LICENSE": ((ROOT / "LICENSE").read_bytes(), 0o644),
        "README.txt": (readme.encode(), 0o644),
        "BUILD-INFO.json": ((json.dumps(metadata, indent=2) + "\n").encode(), 0o644),
        "build-packages.txt": (build_packages.read_bytes(), 0o644),
    }
    # A fresh directory prevents packaging stale files left by an earlier build.
    output.mkdir(parents=True, exist_ok=False)
    archive = output / f"remora-{tag}-{TARGET}.tar.gz"
    with archive.open("xb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=timestamp) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as tar:
                for name, (content, mode) in members.items():
                    info = tarfile.TarInfo(name)
                    info.size, info.mode, info.mtime = len(content), mode, timestamp
                    info.uid = info.gid = 0
                    tar.addfile(info, io.BytesIO(content))
    (output / "SHA256SUMS").write_text(f"{sha256(archive.read_bytes())}  {archive.name}\n")
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--build-packages", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        print(package(args.binary, args.build_packages, args.output, args.tag))
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release packaging failed: {error}\n")


if __name__ == "__main__":
    main()
