"""Test-only port leases: keep configured endpoints out of UDP ephemeral reuse.

Sockets cannot remain bound while Remora binds them. Keep a cross-process lease
until the provisioning caller exits, including intentional server restarts.
No system networking settings are changed.
"""
import fcntl
import json
import os
from pathlib import Path
import secrets
import socket
import stat
import tempfile


def process_identity(pid):
    try:
        fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        boot = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
        return f'{boot}:{pid}:{fields[19]}'
    except (OSError, IndexError):
        return None


def candidates():
    first, last = map(int, Path('/proc/sys/net/ipv4/ip_local_port_range').read_text().split())
    return list(range(1024, first)) + list(range(last + 1, 65536))


def registry_directory():
    namespace = os.stat('/proc/self/ns/net').st_ino
    return Path(tempfile.gettempdir()) / f'remora-fixture-ports-{os.getuid()}-{namespace}'


def reserve_ports(family, registry=None):
    directory = Path(registry) if registry is not None else registry_directory()
    directory.mkdir(mode=0o700, exist_ok=True)
    info = directory.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise RuntimeError('unsafe fixture port registry')
    owner = os.getppid()
    identity = process_identity(owner)
    if identity is None:
        raise RuntimeError('fixture caller identity unavailable')
    sockets = []
    selected = []
    fd = os.open(directory / 'lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        state = directory / 'leases.json'
        if state.exists():
            with state.open() as source:
                raw = source.read(8 * 1024 * 1024 + 1)
            if len(raw) > 8 * 1024 * 1024:
                raise RuntimeError('fixture port registry exceeds bound')
            leases = json.loads(raw)
        else:
            leases = {}
        live = {}
        for port, record in leases.items():
            pid = record['pid']
            if pid not in live:
                live[pid] = process_identity(pid)
        leases = {p: r for p, r in leases.items() if live[r['pid']] == r['identity']}
        available = candidates()
        for _ in range(512):
            port = secrets.choice(available)
            if str(port) in leases:
                continue
            guards = []
            try:
                # Reserve both loopback families: Rust IPv6 cases rewrite an
                # IPv4 fixture after provisioning. No wildcard/ephemeral alias.
                for af, host in [(socket.AF_INET, '127.0.0.1'), (socket.AF_INET6, '::1')]:
                    if af == socket.AF_INET6 and not socket.has_ipv6:
                        if family == 'ipv6':
                            raise RuntimeError('IPv6 unavailable')
                        continue
                    sock = socket.socket(af, socket.SOCK_DGRAM)
                    guards.append(sock)
                    if af == socket.AF_INET6:
                        sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                    sock.bind((host, port))
            except OSError:
                for sock in guards:
                    sock.close()
                continue
            sockets.extend(guards)
            selected.append(port)
            leases[str(port)] = dict(pid=owner, identity=identity)
            if len(selected) == 3:
                break
        if len(selected) != 3:
            raise RuntimeError('fixture port selection exhausted')
        with tempfile.NamedTemporaryFile(mode='w', dir=directory, delete=False) as output:
            temporary = Path(output.name)
            try:
                json.dump(leases, output)
                output.flush()
                os.fsync(output.fileno())
                os.replace(temporary, state)
            finally:
                temporary.unlink(missing_ok=True)
        return selected, sockets
    except BaseException:
        for sock in sockets:
            sock.close()
        raise
    finally:
        os.close(fd)
