#!/usr/bin/env bash
# Test-only provisioning; never edits networking or an existing directory.
set -euo pipefail
exec python3 - "$@" <<'PY'
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys

parser = argparse.ArgumentParser(description="Generate private, isolated Remora fixtures")
parser.add_argument("--output", required=True)
parser.add_argument("--family", choices=["ipv4", "ipv6"], default="ipv4")
args = parser.parse_args()
root = Path(args.output).absolute()
os.umask(0o077)
owned = False
sockets = []
try:
    # Atomic mkdir: neither an existing directory nor a symlink is overwritten.
    root.mkdir(mode=0o700)
    owned = True
    host = "::1" if args.family == "ipv6" else "127.0.0.1"
    bracketed = f"[{host}]" if args.family == "ipv6" else host
    prefix = 128 if args.family == "ipv6" else 32
    for _ in range(3):
        sock = socket.socket(socket.AF_INET6 if args.family == "ipv6" else socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind((host, 0))
        if sock.getsockname()[1] < 1024:
            raise RuntimeError("invalid ephemeral port")
        sockets.append(sock)
    client_port, proxy_port, target_port = [s.getsockname()[1] for s in sockets]
    # Hold all sockets until provisioning completes, keeping selected ports distinct.
    def openssl(*options):
        subprocess.run(["openssl", *options], cwd=root, check=True, timeout=10,
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    openssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
            "-nodes", "-keyout", "ca-key.pem", "-out", "ca.pem", "-days", "2",
            "-subj", "/CN=Remora Test CA", "-addext", "basicConstraints=critical,CA:TRUE")
    openssl("req", "-new", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
            "-nodes", "-keyout", "server-key.pem", "-out", "server.csr", "-subj", "/CN=remora.test")
    (root / "server.ext").write_text("subjectAltName=DNS:remora.test\nbasicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\n")
    openssl("x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca-key.pem",
            "-set_serial", str(secrets.randbits(128) or 1), "-out", "server.pem", "-days", "2", "-extfile", "server.ext")
    token = secrets.token_hex(32)
    (root / "client.token").write_text(token + "\n", encoding="ascii")
    digest = hashlib.sha256(token.encode("ascii")).hexdigest()
    (root / "credentials.toml").write_text(f'''[[identities]]
id = "fixture"
token_sha256 = "{digest}"
[[identities.targets]]
host = "{host}"
port = {target_port}
addresses = ["{host}/{prefix}"]
''')
    (root / "client.toml").write_text(f'''schema_version = 1
[client]
listen = "{bracketed}:{client_port}"
allow_non_loopback = false
server_address = "{bracketed}:{proxy_port}"
server_name = "remora.test"
proxy_authority = "remora.test:{proxy_port}"
uri_template = "https://remora.test:{proxy_port}/.well-known/masque/udp/{{target_host}}/{{target_port}}/"
ca_file = "ca.pem"
token_file = "client.token"
target_host = "{host}"
target_port = {target_port}
''')
    (root / "server.toml").write_text(f'''schema_version = 1
[server]
listen = ["{bracketed}:{proxy_port}"]
proxy_authority = "remora.test:{proxy_port}"
path_prefix = "/.well-known/masque/udp/"
certificate_file = "server.pem"
private_key_file = "server-key.pem"
credentials_file = "credentials.toml"
''')
    (root / "ports.json").write_text(json.dumps({"client": client_port, "proxy": proxy_port, "target": target_port}) + "\n")
    for path in root.iterdir():
        path.chmod(0o600)
    # Ephemeral ports are selections, not reservations after this script exits.
    # Future process runners bind immediately and retry only with fresh fixtures.
except FileExistsError:
    print("fixture output already exists; refusing overwrite", file=sys.stderr)
    sys.exit(2)
except Exception as failure:
    if owned:
        shutil.rmtree(root)
    print(f"fixture generation failed ({type(failure).__name__}; check Python 3/OpenSSL and local UDP bind permission)", file=sys.stderr)
    sys.exit(1)
finally:
    for sock in sockets:
        sock.close()
PY
