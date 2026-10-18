import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("release_packaging", ROOT / "scripts/release/package.py")
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class ReleasePackaging(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.revision = "a" * 40
        for name in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "build.rs",
                     "tools/build/Dockerfile", "dependencies/quiche.json", "LICENSE"]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture\n")
        (self.root / "Cargo.toml").write_text('[package]\nversion = "0.1.0"\n')
        self.binary = self.root / "remora"
        elf = bytearray(64)
        elf[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", elf, 18, 62)
        self.binary.write_bytes(elf)
        self.packages = self.root / "packages.txt"
        self.packages.write_text("libc6 2.36\n")
        self.output = self.root / "release"
        self.binary_version = f"remora 0.1.0 commit={self.revision} Rust=fixture"
        self.addCleanup(patch.stopall)
        patch.object(packaging, "ROOT", self.root).start()
        patch.object(packaging, "command", side_effect=self.command).start()
        self.git_diff = patch.object(packaging.subprocess, "run").start()

    def command(self, *args):
        commands = {
            ("git", "rev-parse", "HEAD"): self.revision,
            ("git", "show", "-s", "--format=%ct", "HEAD"): "1700000000",
            (str(self.binary), "--version"): self.binary_version,
            ("rustc", "--version", "--verbose"): "rustc fixture",
            ("readelf", "--dynamic", str(self.binary)): "(NEEDED) Shared library: [libstdc++.so.6]",
        }
        return commands[args]

    def package(self, tag="v0.1.0"):
        return packaging.package(self.binary, self.packages, self.output, tag)

    def test_archive_has_only_public_allowlisted_files_and_verifiable_hashes(self):
        (self.root / "credentials.toml").write_text("private fixture")
        (self.root / "server-key.pem").write_text("private key")
        archive = self.package()
        digest, filename = (self.output / "SHA256SUMS").read_text().split()
        self.assertEqual(filename, archive.name)
        self.assertEqual(digest, hashlib.sha256(archive.read_bytes()).hexdigest())
        with tarfile.open(archive) as tar:
            self.assertEqual(set(tar.getnames()), {
                "remora", "LICENSE", "README.txt", "BUILD-INFO.json", "build-packages.txt"})
            metadata = json.load(tar.extractfile("BUILD-INFO.json"))
            self.assertEqual(metadata["source_revision"], self.revision)
            self.assertEqual(metadata["runtime_libraries"], ["libstdc++.so.6"])
            self.assertEqual(metadata["binary_sha256"], hashlib.sha256(tar.extractfile("remora").read()).hexdigest())
            self.assertEqual(metadata["build_packages_sha256"], hashlib.sha256(self.packages.read_bytes()).hexdigest())
            self.assertEqual(tar.getmember("remora").mode, 0o755)
            self.assertTrue(all(member.uid == member.gid == 0 for member in tar))
        again = packaging.package(self.binary, self.packages, self.root / "second", "v0.1.0")
        self.assertEqual(archive.read_bytes(), again.read_bytes())

    def test_tag_mismatch_or_malformed_tag_never_creates_output(self):
        for tag in ["v0.2.0", "0.1.0", "v0.1.0/../../escape", "v01.1.0", "v0.1.0-rc.1"]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                self.package(tag)
            self.assertFalse(self.output.exists())

    def test_wrong_architecture_is_rejected_before_execution(self):
        data = bytearray(self.binary.read_bytes())
        struct.pack_into("<H", data, 18, 183)  # EM_AARCH64
        self.binary.write_bytes(data)
        with self.assertRaisesRegex(ValueError, "x86_64"):
            self.package()
        self.assertFalse(self.output.exists())

    def test_wrong_binary_version_or_revision_is_rejected(self):
        for value in [f"remora 0.2.0 commit={self.revision} Rust=fixture",
                      "remora 0.1.0 commit=source-archive Rust=fixture"]:
            self.binary_version = value
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "embedded source revision"):
                self.package()
            self.assertFalse(self.output.exists())

    def test_changed_tracked_inputs_are_rejected(self):
        self.git_diff.side_effect = subprocess.CalledProcessError(1, "git diff")
        with self.assertRaises(subprocess.CalledProcessError):
            self.package()
        self.assertFalse(self.output.exists())

    def test_existing_output_is_preserved(self):
        self.output.mkdir()
        sentinel = self.output / "do-not-overwrite"
        sentinel.write_text("keep")
        with self.assertRaises(FileExistsError):
            self.package()
        self.assertEqual(sentinel.read_text(), "keep")


if __name__ == "__main__":
    unittest.main()
