#!/usr/bin/env python3
"""Real LoadCredential ACL gate; run as a non-root user with system-manager authorization."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid


SECRETS = {"private_key_file": "server-key.pem", "credentials_file": "credentials.toml", "token_file": "client.token"}


def run(arguments, timeout=10):
    return subprocess.run([str(arg) for arg in arguments], capture_output=True, text=True,
                          stdin=subprocess.DEVNULL, timeout=timeout, check=False)


def required(arguments):
    result = run(arguments)
    if result.returncode:
        raise RuntimeError(f"{arguments[0]} failed (exit {result.returncode})")
    return result.stdout.strip()


def write_configs(root, overrides=None):
    paths = {field: root / name for field, name in SECRETS.items()}
    paths.update(overrides or {})
    (root / "server.toml").write_text(f'''schema_version = 1
[server]
listen = ["127.0.0.1:7443"]
proxy = "https://remora.test:7443"
certificate_file = "{root / 'server.pem'}"
private_key_file = "{paths['private_key_file']}"
credentials_file = "{paths['credentials_file']}"
''')
    (root / "client.toml").write_text(f'''schema_version = 1
[client]
listen = "127.0.0.1:51821"
proxy = "https://remora.test:7443"
connect_address = "127.0.0.1:7443"
target = "127.0.0.1:51820"
ca_file = "{root / 'server.pem'}"
token_file = "{paths['token_file']}"
''')


def provision(root):
    required(["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
              "-nodes", "-keyout", root / "server-key.pem", "-out", root / "server.pem",
              "-days", "2", "-subj", "/CN=remora.test", "-addext", "subjectAltName=DNS:remora.test"])
    token = "a" * 64  # Fixed disposable test data, never an operator token.
    (root / "client.token").write_text(token + "\n")
    (root / "credentials.toml").write_text(f'''[[identities]]
id = "systemd-acl-test"
token_sha256 = "{hashlib.sha256(token.encode()).hexdigest()}"
[[identities.targets]]
host = "127.0.0.1"
port = 51820
addresses = ["127.0.0.1/32"]
''')
    write_configs(root)
    for path in root.iterdir():
        path.chmod(0o600)


def worker(root, binary, expect):
    result = {"uid": os.geteuid(), "cases": [], "status": "passed"}
    for field, name in SECRETS.items():
        path = Path(os.environ["CREDENTIALS_DIRECTORY"]) / name
        with path.open("rb") as file:
            metadata = os.fstat(file.fileno())
            readable = bool(file.read(1))
        acl = required(["getfacl", "-ncp", path]).splitlines()
        shape = acl == ["user::r--", f"user:{os.geteuid()}:r--", "group::---", "mask::r--", "other::---"]
        actual_acl_case = metadata.st_uid == 0 and os.geteuid() != 0 and metadata.st_mode & 0o777 == 0o440 and readable and shape
        write_configs(root, {field: path})
        config = root / ("client.toml" if field == "token_file" else "server.toml")
        check = run([binary, "check-config", "--config", config])
        matches = (check.returncode == 0) if expect == "valid" else (
            check.returncode == 2 and check.stderr.strip() == f"invalid configuration: {field} (Permissions)")
        result["cases"].append({"field": field, "owner": metadata.st_uid, "mode": oct(metadata.st_mode & 0o777),
                                "acl": acl, "readable": readable, "root_acl_case": actual_acl_case,
                                "check_exit": check.returncode, "expectation_matched": matches})
        if not actual_acl_case or not matches:
            result["status"] = "failed"
    print(json.dumps(result))
    return 0 if result["status"] == "passed" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", type=Path, help="New private result directory; never overwrite existing evidence")
    parser.add_argument("--expect", choices=["valid", "rejected"], default="valid")
    parser.add_argument("--sudo", action="store_true", help="Use sudo -n only for systemd-run")
    parser.add_argument("--worker", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    os.umask(0o077)
    binary = args.binary.resolve(strict=True)
    if args.worker:
        return worker(args.worker, binary, args.expect)
    if args.output is None or os.geteuid() == 0:
        parser.error("supply --output and run as a non-root service user; --sudo elevates only the manager call")
    for tool in ["openssl", "getfacl", "systemctl", "systemd-run"] + (["sudo"] if args.sudo else []):
        if shutil.which(tool) is None:
            parser.error(f"required installed tool missing: {tool}")
    output = args.output.absolute()
    output.mkdir(mode=0o700)  # No overwrite, including a pre-existing symlink.
    evidence = {"status": "not-run", "binary": str(binary), "expect": args.expect,
                "version": required([binary, "--version"]), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "kernel": os.uname().release, "service_uid": os.geteuid()}
    try:
        evidence["systemd_version"] = required(["systemctl", "show", "--property=Version", "--value"])
        with tempfile.TemporaryDirectory(prefix="fixtures-", dir=output) as directory:
            root = Path(directory)
            provision(root)
            # Baselines must pass before any systemd representation is tested.
            for config in ["client.toml", "server.toml"]:
                required([binary, "check-config", "--config", root / config])
            arguments = ["systemd-run", "--system", "--no-ask-password", "--wait", "--pipe", "--collect",
                         "--unit=remora-credential-acl-" + uuid.uuid4().hex[:12], "-p", f"User={os.geteuid()}",
                         "-p", f"Group={os.getegid()}", "-p", "RuntimeMaxSec=30s"]
            for name in SECRETS.values():
                arguments.extend(["-p", f"LoadCredential={name}:{root / name}"])
            arguments.extend(["/usr/bin/python3", str(Path(__file__).resolve()), "--worker", str(root),
                              "--binary", str(binary), "--expect", args.expect])
            if args.sudo:
                arguments = ["sudo", "-n", "--", *arguments]
            process = run(arguments, timeout=45)
            evidence["manager_exit"] = process.returncode
            # Do not retain journal/stdout noise or any secret contents.
            if process.stdout.strip():
                evidence["worker"] = json.loads(process.stdout)
                evidence["status"] = "passed" if process.returncode == 0 and evidence["worker"]["status"] == "passed" else "failed"
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as failure:
        evidence["failure_class"] = type(failure).__name__
    (output / "result.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps(evidence, indent=2))
    return 0 if evidence["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
