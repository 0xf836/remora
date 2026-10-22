#!/usr/bin/env python3
"""Provision a private, persistent, two-guest x86_64 KVM qualification lab."""
import argparse
import fcntl
import hashlib
import io
import ipaddress
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import signal
import subprocess
import tarfile
import tempfile
import time
import uuid
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
IMAGE_URL = ("https://cloud.debian.org/images/cloud/trixie/20261001-2618/"
             "debian-13-genericcloud-amd64-20261001-2618.qcow2")
IMAGE_SHA512 = ("f46f0671a6e5bdec5291ab8972bae2f10e5408c2f64a74078f11efc2f06a436a9d"
                "0313ed50e0472542eeabf780e9f7c792ac0a314c6c20507fcd9fd81b468c3d")
GUEST_ROOT = "/opt/remora-t037"


class LabError(Exception):
    """Operator-facing errors contain no credentials or remote output."""


def run(*cmd, data=None, timeout=120, check=True):
    result = subprocess.run(cmd, input=data, capture_output=True, timeout=timeout,
                            env={**os.environ, "LC_ALL": "C"})
    if check and result.returncode:
        raise LabError(f"{Path(cmd[0]).name} failed (exit {result.returncode}); inspect the owned resource")
    return result


def digest(path, algorithm="sha256"):
    with Path(path).open("rb") as file:
        return hashlib.file_digest(file, algorithm).hexdigest()


def write(path, text):
    path.write_text(text)
    path.chmod(0o600)


def validate_state(path):
    # Neither credentials nor the manifest may enter the source tree. Reject
    # symlink traversal and other users' or publicly readable state directories.
    if path.resolve() != path or path == ROOT or ROOT in path.parents:
        raise LabError("state must be outside the repository and contain no symlink components")
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    st = path.stat()
    if st.st_uid != os.getuid() or st.st_mode & 0o077:
        raise LabError("state must be owned by the caller with mode 0700")
    for child in path.rglob("*"):
        if child.is_symlink():
            raise LabError("state contains a symlink; refusing access")


def check_identity(actual, expected, label):
    if actual.strip() != expected:
        raise LabError(f"{label} identity changed; refusing mutation")


def cached_image(directory, source=None):
    """Publish only verified complete images; a cache hit never uses the network."""
    validate_state(directory)
    image = directory / "debian-13-genericcloud-amd64-20261001-2618.qcow2"

    def verify(file):
        if digest(file, "sha512") != IMAGE_SHA512:
            raise LabError("Debian image SHA-512 mismatch; refusing use or silent replacement")
        info = json.loads(run("qemu-img", "info", "--output=json", str(file)).stdout)
        if info.get("format") != "qcow2" or info.get("backing-filename"):
            raise LabError("base image must be a standalone qcow2")

    with (directory / "lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if image.exists():
            verify(image)
            print("Using verified local image cache (no download)", flush=True)
            return image
        fd, name = tempfile.mkstemp(prefix="image-", suffix=".partial", dir=directory)
        os.close(fd)
        pending = Path(name)
        try:
            if source:
                print("Importing image into local cache", flush=True)
                shutil.copyfile(source, pending)
            else:
                print("Downloading pinned image once into local cache", flush=True)
                cmd = ["curl", "--fail", "--silent", "--show-error", "--location",
                       "--proto", "=https", "--tlsv1.2", "--retry", "2"]
                proxy = os.environ.get("HTTPS_PROXY") or os.environ.get("HTTP_PROXY")
                if proxy:
                    cmd += ["--proxy", proxy]
                run(*cmd, IMAGE_URL, "--output", str(pending), timeout=600)
            verify(pending)
            pending.replace(image)
        finally:
            pending.unlink(missing_ok=True)
    return image


def check_subnet(cidr, networks):
    subnet = ipaddress.ip_network(cidr, strict=True)
    if subnet.version != 4 or subnet.prefixlen != 24 or not subnet.is_private:
        raise LabError("lab subnet must be a private IPv4 /24")
    if any(subnet.overlaps(ipaddress.ip_network(n, strict=False)) for n in networks):
        raise LabError("lab subnet overlaps an existing network or host route")
    return subnet


class Lab:
    def __init__(self, args):
        self.a = args
        self.state = args.state
        self.private = self.state / "private"
        self.cache = self.state / "cache"
        self.manifest = self.state / "manifest.json"
        self.m = json.loads(self.manifest.read_text()) if self.manifest.exists() else None
        if self.m:
            if self.m.get("schema_version") != 1 or self.m["uri"] != args.uri:
                raise LabError("manifest schema or libvirt URI mismatch")
            if not re.fullmatch(r"remora-[a-z0-9-]{1,24}", self.m["name"]):
                raise LabError("invalid manifest lab name")

    def virsh(self, *args, **kw):
        try:
            return run("virsh", "-c", self.a.uri, *args, **kw).stdout.decode().strip()
        except LabError:
            raise LabError(f"virsh {args[0]} failed; inspect the owned resource") from None

    def volume_names(self):
        # vol-list has no --name option (unlike domain/network/pool lists).
        return [line.split()[0] for line in self.virsh("vol-list", self.m["pool"]).splitlines()[2:]
                if line.strip()]

    def save(self):
        pending = self.state / "manifest.pending"
        write(pending, json.dumps(self.m, indent=2) + "\n")
        pending.replace(self.manifest)

    def xml(self, kind, name):
        command = {"domain": "dumpxml", "network": "net-dumpxml", "pool": "pool-dumpxml"}[kind]
        options = ["--inactive"] if kind == "domain" else []
        return ET.fromstring(self.virsh(command, name, *options))

    def domain_xmls(self, name):
        # Current XML includes live-only attachments, also for transient guests.
        # A persistent guest can reference resources in its next-boot XML too.
        current = ET.fromstring(self.virsh("dumpxml", name))
        views = [current]
        if name in self.virsh("list", "--all", "--persistent", "--name").splitlines():
            views.append(self.xml("domain", name))
        return views

    def ensure_network_unshared(self):
        own = {d["name"] for d in self.m["domains"].values()}
        for name in self.virsh("list", "--all", "--name").splitlines():
            if name not in own and any(source.get("network") == self.m["name"]
                    for xml in self.domain_xmls(name)
                    for source in xml.findall("devices/interface/source")):
                raise LabError("another domain uses the lab network; refusing network shutdown/deletion")

    def ensure_owned(self, kind, name, identity):
        command = "domuuid" if kind == "domain" else "net-uuid"
        check_identity(self.virsh(command, name), identity, name)

    def exists(self, kind, name):
        command = "list" if kind == "domain" else "net-list"
        return name in self.virsh(command, "--all", "--name").splitlines()

    def prerequisites(self):
        for cmd in ("virsh", "virt-install", "qemu-img", "xorriso", "curl", "ssh",
                    "ssh-keygen", "wg", "openssl", "ip"):
            if not shutil.which(cmd):
                raise LabError(f"missing host command: {cmd}")
        if platform.machine() != "x86_64" or not os.access("/dev/kvm", os.R_OK | os.W_OK):
            raise LabError("this lab requires native x86_64 and access to /dev/kvm")
        pool = self.xml("pool", self.a.pool)
        if pool.get("type") != "dir" or self.a.pool not in self.virsh("pool-list", "--name").splitlines():
            raise LabError("an existing active directory storage pool is required")
        if int(pool.findtext("available", "0")) < 18 * 1024**3:
            raise LabError("storage pool needs at least 18 GiB available")
        memory = dict(line.split(":", 1) for line in Path("/proc/meminfo").read_text().splitlines())
        if int(memory["MemAvailable"].split()[0]) < 3 * 1024**2:
            raise LabError("host needs at least 3 GiB available RAM")
        if shutil.disk_usage(self.state).free < 3 * 1024**3:
            raise LabError("state filesystem needs at least 3 GiB free for staging")

    def initialize(self):
        if self.m or any(p.name != "lock" for p in self.state.iterdir()):
            raise LabError("create/adopt requires an empty new state directory")
        self.prerequisites()
        name = self.a.name
        if any(self.exists("domain", name + "-" + r) for r in "ab"):
            raise LabError("domain name exists; use explicit adopt for the old test lab")
        if self.exists("network", name):
            raise LabError("network name exists; refusing takeover")
        networks = []
        for n in self.virsh("net-list", "--all", "--name").splitlines():
            for ip in self.xml("network", n).findall("ip"):
                if ip.get("family", "ipv4") == "ipv4":
                    networks.append(ip.get("address") + "/" + (ip.get("prefix") or ip.get("netmask")))
        for route in json.loads(run("ip", "-j", "-4", "route", "show", "table", "all").stdout):
            if route.get("dst", "default") != "default":
                networks.append(route["dst"])
        subnet = check_subnet(self.a.subnet, networks)
        self.m = {"schema_version": 1, "uri": self.a.uri, "pool": self.a.pool, "name": name,
                  "subnet": str(subnet), "network_uuid": str(uuid.uuid4()), "domains": {},
                  "volumes": {}, "phase": "creating"}
        # Random local MACs avoid collisions with other instances of this lab.
        for role, offset in (("a", 10), ("b", 11)):
            suffix = os.urandom(3).hex()
            self.m["domains"][role] = {"name": name + "-" + role, "uuid": str(uuid.uuid4()),
                "mac": "52:54:00:" + ":".join(suffix[i:i+2] for i in (0, 2, 4)),
                "ip": str(subnet.network_address + offset), "auto_nvram": True}
        self.private.mkdir(mode=0o700)
        self.cache.mkdir(mode=0o700)
        self.save()

    def volume(self, label, file, fmt):
        name = self.m["name"] + "-" + label
        pool = self.m["pool"]
        if name in self.volume_names():
            raise LabError("volume name exists; refusing overwrite")
        size = (json.loads(run("qemu-img", "info", "--output=json", str(file)).stdout)["virtual-size"]
                if fmt == "qcow2" else file.stat().st_size)
        # Intent precedes the side effect. If create or identity capture is
        # interrupted/ambiguous, recovery must not delete a same-named object.
        intents = self.m.setdefault("volume_intents", {})
        intents[name] = {"format": fmt, "capacity": size}
        self.save()
        self.virsh("vol-create-as", pool, name, str(size), "--format", fmt)
        key = self.virsh("vol-key", name, "--pool", pool)
        path = self.virsh("vol-path", name, "--pool", pool)
        st = Path(path).stat()
        self.m["volumes"][name] = {"key": key, "path": path, "inode": st.st_ino, "device": st.st_dev}
        del intents[name]
        self.save()
        # Ownership is durable before upload, not after it.
        self.virsh("vol-upload", name, str(file), "--pool", pool, timeout=300)
        return path

    def ssh_config(self, first_use=False):
        text = ""
        for d in self.m["domains"].values():
            text += (f"Host {d['name']}\n  HostName {d['ip']}\n  User lab\n"
                     f"  IdentityFile {shlex.quote(str(self.private / 'ssh-key'))}\n"
                     f"  UserKnownHostsFile {shlex.quote(str(self.private / 'known_hosts'))}\n"
                     f"  StrictHostKeyChecking {'accept-new' if first_use else 'yes'}\n"
                     "  IdentitiesOnly yes\n  BatchMode yes\n  ConnectTimeout 5\n")
        write(self.private / "ssh-config", text)

    def ssh(self, role, command, **kw):
        return run("ssh", "-F", str(self.private / "ssh-config"),
                   self.m["domains"][role]["name"], command, **kw)

    def create(self):
        self.initialize()
        image = cached_image(self.a.cache_dir, self.a.image)
        run("ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(self.private / "ssh-key"))
        self.ssh_config(first_use=True)
        pub = (self.private / "ssh-key.pub").read_text().strip()
        net = ipaddress.ip_network(self.m["subnet"])
        hosts = "".join(f'<host mac="{d["mac"]}" name="{d["name"]}" ip="{d["ip"]}"/>'
                        for d in self.m["domains"].values())
        xml = (f'<network><name>{self.m["name"]}</name><uuid>{self.m["network_uuid"]}</uuid>'
               f'<forward mode="nat"/><ip address="{net.network_address + 1}" prefix="24">'
               f'<dhcp><range start="{net.network_address + 100}" end="{net.network_address + 150}"/>'
               f'{hosts}</dhcp></ip></network>')
        write(self.cache / "network.xml", xml)
        self.virsh("net-define", str(self.cache / "network.xml"))
        self.virsh("net-start", self.m["name"])
        for role, d in self.m["domains"].items():
            print(f"Creating {d['name']} with UEFI and persistent storage", flush=True)
            disk = self.cache / f"{role}.qcow2"
            run("qemu-img", "convert", "-O", "qcow2", str(image), str(disk))
            run("qemu-img", "resize", str(disk), "8G")
            disk_path = self.volume(role + ".qcow2", disk, "qcow2")
            disk.unlink()
            seed_dir = self.cache / role
            seed_dir.mkdir(mode=0o700)
            write(seed_dir / "meta-data", f"instance-id: {d['uuid']}\nlocal-hostname: {d['name']}\n")
            write(seed_dir / "user-data", "#cloud-config\nusers:\n  - name: lab\n"
                  "    groups: [sudo]\n    shell: /bin/bash\n    lock_passwd: true\n"
                  "    sudo: ['ALL=(ALL) NOPASSWD:ALL']\n    ssh_authorized_keys:\n"
                  f"      - {pub}\nssh_pwauth: false\ndisable_root: true\npackage_update: true\n"
                  "packages: [wireguard-tools, python3, iproute2, openssl, nftables]\n")
            seed = self.cache / f"{role}-seed.iso"
            run("xorriso", "-as", "mkisofs", "-quiet", "-V", "cidata", "-J", "-r", "-o", str(seed),
                str(seed_dir / "user-data"), str(seed_dir / "meta-data"))
            seed_path = self.volume(role + "-seed.iso", seed, "raw")
            # libvirt selects the installed UEFI firmware and persistent NVRAM;
            # no firmware or disk path depends on /tmp or the private state dir.
            run("virt-install", "--connect", self.a.uri, "--name", d["name"], "--uuid", d["uuid"],
                "--memory", "1024", "--vcpus", "2", "--cpu", "qemu64,-svm", "--machine", "q35",
                "--os-variant", "debian13", "--import", "--boot", "uefi", "--graphics", "none",
                "--noautoconsole", "--disk", f"path={disk_path},format=qcow2,bus=virtio",
                "--disk", f"path={seed_path},device=cdrom", "--network",
                f"network={self.m['name']},model=virtio,mac={d['mac']}", timeout=120)
            d["auto_nvram"] = True
            self.save()
        self.m["phase"] = "booting"
        self.save()
        self.ready(first_use=True)
        self.configure()

    def ready(self, first_use=False):
        self.ensure_owned("network", self.m["name"], self.m["network_uuid"])
        for role, d in self.m["domains"].items():
            self.ensure_owned("domain", d["name"], d["uuid"])
            print(f"Waiting for SSH and cloud-init on {d['name']}", flush=True)
            deadline = time.monotonic() + 420
            while True:
                leases = self.virsh("net-dhcp-leases", self.m["name"], "--mac", d["mac"])
                if d["ip"] + "/24" in leases:
                    try:
                        r = self.ssh(role, "cloud-init status --wait", check=False, timeout=30)
                    except subprocess.TimeoutExpired:
                        r = None  # Package installation may outlast one SSH observation.
                    if r is not None and r.returncode == 0:
                        break
                    if r is not None and r.returncode not in (255,):
                        raise LabError("cloud-init failed; inspect the guest console")
                if time.monotonic() > deadline:
                    raise LabError("guest readiness deadline exceeded")
                time.sleep(2)
        if first_use:
            self.ssh_config()  # Pin first-use host keys; never auto-replace a changed key.

    def idle(self):
        for role in "ab":
            if self.virsh("domstate", self.m["domains"][role]["name"]) == "shut off":
                continue
            result = self.ssh(role, "systemctl list-units --all --no-legend 'remora-t037-*'", timeout=15)
            lines = result.stdout.decode().splitlines()
            if any(".service" in line and "remora-t037-wg.service" not in line for line in lines):
                raise LabError("test units exist; stop the owned test run before configure/stop")

    def configure(self):
        self.ready()
        self.idle()
        # Reuse independent WireGuard keys across reconfiguration. Fresh test
        # TLS/token fixtures have a two-day lifetime and are sent only over SSH.
        keys = {}
        for role in "ab":
            keyfile = self.private / f"{role}-wg.key"
            if not keyfile.exists():
                write(keyfile, run("wg", "genkey").stdout.decode())
            keys[role] = run("wg", "pubkey", data=keyfile.read_bytes()).stdout.decode().strip()
        fixture = {"schema_version": 1, "ssh_config": str(self.private / "ssh-config"), "roles": {}}
        with tempfile.TemporaryDirectory(dir=self.state) as staging:
            staging = Path(staging)
            for channel, target, listener, outer in (("wg", 51820, 41000, 4433), ("generic", 41020, 41010, 4434)):
                f = staging / channel
                run(str(ROOT / "scripts/validate/fixtures.sh"), "--output", str(f))
                token = (f / "client.token").read_text().strip()
                write(f / "credentials.toml", f'[[identities]]\nid = "lab-{channel}"\n'
                      f'token_sha256 = "{hashlib.sha256(token.encode()).hexdigest()}"\n'
                      f'[[identities.targets]]\nhost = "127.0.0.1"\nport = {target}\naddresses = ["127.0.0.1/32"]\n')
                text = (f / "client.toml").read_text()
                # Replace the ephemeral-port template fields explicitly.
                text = re.sub(r'listen = ".*"', f'listen = "127.0.0.1:{listener}"', text)
                text = re.sub(r'connect_address = ".*"', f'connect_address = "{self.m["domains"]["b"]["ip"]}:{outer}"', text)
                text = re.sub(r'proxy = ".*"', f'proxy = "https://remora.test:{outer}"', text)
                text = re.sub(r'target = ".*"', f'target = "127.0.0.1:{target}"', text)
                idle = 15000 if channel == "wg" else 5000
                write(f / "client.toml", text + f"\n[transport]\nidle_timeout_ms = {idle}\nouter_udp_max_bytes = 1350\napplication_max_bytes = 1200\n")
                text = (f / "server.toml").read_text()
                text = re.sub(r'listen = \[.*\]', f'listen = ["{self.m["domains"]["b"]["ip"]}:{outer}"]', text)
                text = re.sub(r'proxy = ".*"', f'proxy = "https://remora.test:{outer}"', text)
                write(f / "server.toml", text + f"\n[transport]\nidle_timeout_ms = {idle}\nouter_udp_max_bytes = 1350\napplication_max_bytes = 1200\n")
            for role in "ab":
                mode = "client" if role == "a" else "server"
                archive = io.BytesIO()
                with tarfile.open(fileobj=archive, mode="w:gz") as tar:
                    tar.add(self.a.binary, arcname="remora")
                    tar.add(ROOT / "scripts/validate/wireguard_traffic.py", arcname="wireguard_traffic.py")
                    tar.add(self.private / f"{role}-wg.key", arcname="wg.key")
                    for channel in ("wg", "generic"):
                        files = (["client.toml", "ca.pem", "client.token"] if role == "a" else
                                 ["server.toml", "server.pem", "server-key.pem", "credentials.toml"])
                        for file in files:
                            tar.add(staging / channel / file, arcname=channel + "/" + file)
                self.ssh(role, f"sudo -n install -d -m 700 -o lab -g lab {GUEST_ROOT} && "
                         f"tar --no-same-owner -xzf - -C {GUEST_ROOT}", data=archive.getvalue())
                script = self.guest_wg(role, keys)
                self.ssh(role, "sudo -n tee /usr/local/sbin/remora-t037-wg >/dev/null", data=script.encode())
                unit = ("[Unit]\nDescription=Dedicated Remora lab WireGuard fixture\nAfter=network.target\n"
                        "[Service]\nType=oneshot\nRemainAfterExit=yes\n"
                        "ExecStart=/usr/local/sbin/remora-t037-wg up\nExecStop=/usr/local/sbin/remora-t037-wg down\n"
                        "[Install]\nWantedBy=multi-user.target\n")
                self.ssh(role, "sudo -n tee /etc/systemd/system/remora-t037-wg.service >/dev/null", data=unit.encode())
                self.ssh(role, "sudo -n chmod 700 /usr/local/sbin/remora-t037-wg && "
                         "sudo -n systemctl daemon-reload && sudo -n systemctl enable remora-t037-wg && "
                         "sudo -n systemctl restart remora-t037-wg")
                for channel in ("wg", "generic"):
                    if self.ssh(role, f"{GUEST_ROOT}/remora check-config --config "
                                f"{GUEST_ROOT}/{channel}/{mode}.toml", check=False).returncode:
                        raise LabError(f"guest {role} {channel} configuration validation failed")
                peer = "10.77.37.2" if role == "a" else "10.77.37.1"
                r = {"ssh_host": self.m["domains"][role]["name"], "wg_interface": "wg-remora",
                     "wg_bind": ("10.77.37.1" if role == "a" else "10.77.37.2") + ":42000",
                     "wg_peer": peer + ":42000", "generic_bind": "127.0.0.1:" + ("41011" if role == "a" else "41020"),
                     "units": [f"remora-t037-{mode}-wg", f"remora-t037-{mode}-generic"]}
                if role == "a":
                    r["generic_peer"] = "127.0.0.1:41010"
                fixture["roles"][role] = r
            env = {"topology": "two dedicated KVM guests; separate generic UDP Remora pair",
                   "guest_os": "Debian 13", "kernel": self.ssh("a", "uname -r").stdout.decode().strip(),
                   "cpu_model": "qemu64", "vcpus_per_guest": 2, "ram_mib_per_guest": 1024,
                   "wireguard_inner_family": "IPv4", "wireguard_mtu": 1120, "remora_payload_ceiling": 1200,
                   "outer_network_mtu": 1500, "offloads": "off", "image_url": IMAGE_URL,
                   "image_sha512": IMAGE_SHA512, "remora_binary_sha256": digest(self.a.binary),
                   "root_lock_sha256": digest(ROOT / "Cargo.lock"), "wireguard_quic_idle_timeout_ms": 15000,
                   "generic_quic_idle_timeout_ms": 5000, "setup_timeout_ms": 5000,
                   "direct_target_access": "guest B nftables drops non-loopback UDP ports 51820 and 41020"}
            fixture["public_environment"] = env
            write(self.private / "fixture.pending", json.dumps(fixture, indent=2) + "\n")
            (self.private / "fixture.pending").replace(self.private / "fixture.json")
        self.m["phase"] = "configured"
        self.save()
        self.status()

    @staticmethod
    def guest_wg(role, keys):
        peer_role = "b" if role == "a" else "a"
        ip, peer = (("10.77.37.1", "10.77.37.2") if role == "a" else ("10.77.37.2", "10.77.37.1"))
        endpoint = " endpoint 127.0.0.1:41000 persistent-keepalive 25" if role == "a" else ""
        firewall = ("nft list table inet remora_t037 >/dev/null 2>&1 && nft delete table inet remora_t037 || :\n"
                    "nft -f - <<'NFT'\ntable inet remora_t037 { chain input { type filter hook input priority 0; "
                    'policy accept; iifname != "lo" udp dport { 51820, 41020 } counter drop\n}\n}\nNFT\n') if role == "b" else ""
        return f'''#!/bin/sh
set -eu
# Dedicated guest only. Refuse an interface belonging to a different key.
if ip link show wg-remora >/dev/null 2>&1; then
    test "$(wg show wg-remora public-key)" = "{keys[role]}"
    ip link delete wg-remora
fi
if [ "$1" = down ]; then
    {'nft delete table inet remora_t037 2>/dev/null || :' if role == 'b' else ':'}
    exit 0
fi
test "$1" = up
{firewall}ip link add wg-remora type wireguard
wg set wg-remora private-key {GUEST_ROOT}/wg.key listen-port 51820 peer {keys[peer_role]} allowed-ips {peer}/32{endpoint}
ip address add {ip}/32 dev wg-remora
ip link set wg-remora mtu 1120 up
ip route replace {peer}/32 dev wg-remora
'''

    def start(self):
        self.ensure_owned("network", self.m["name"], self.m["network_uuid"])
        if self.m["name"] not in self.virsh("net-list", "--name").splitlines():
            self.virsh("net-start", self.m["name"])
        for d in self.m["domains"].values():
            self.ensure_owned("domain", d["name"], d["uuid"])
            if self.virsh("domstate", d["name"]) == "shut off":
                self.virsh("start", d["name"])
        self.ready()
        self.status()

    def stop(self, force=False):
        if self.exists("network", self.m["name"]):
            self.ensure_owned("network", self.m["name"], self.m["network_uuid"])
            self.ensure_network_unshared()
        running = []
        for d in self.m["domains"].values():
            if not self.exists("domain", d["name"]):
                continue
            self.ensure_owned("domain", d["name"], d["uuid"])
            state = self.virsh("domstate", d["name"])
            if state == "running":
                running.append(d["name"])
            elif state != "shut off":
                raise LabError("guest is not running or shut off; inspect before stopping")
        if running:
            # Use SSH when available; incomplete creation must still be stoppable.
            if self.m["phase"] == "configured" and not force:
                self.idle()
            for name in running:
                self.virsh("shutdown", name)
            deadline = time.monotonic() + 90
            while any(self.virsh("domstate", name) != "shut off" for name in running):
                if time.monotonic() > deadline:
                    if force:
                        for name in running:
                            d = next(d for d in self.m["domains"].values() if d["name"] == name)
                            self.ensure_owned("domain", name, d["uuid"])
                            if self.virsh("domstate", name) != "shut off":
                                self.virsh("destroy", name)
                        break
                    raise LabError("graceful shutdown timed out; no forced power-off performed")
                time.sleep(1)
        if self.exists("network", self.m["name"]):
            self.ensure_owned("network", self.m["name"], self.m["network_uuid"])
            self.ensure_network_unshared()
            if self.m["name"] in self.virsh("net-list", "--name").splitlines():
                self.virsh("net-destroy", self.m["name"])

    def status(self):
        if self.m["phase"] == "destroyed":
            print(json.dumps({"phase": "destroyed", "state": str(self.state)}), flush=True)
            return
        self.ensure_owned("network", self.m["name"], self.m["network_uuid"])
        values = []
        for role, d in self.m["domains"].items():
            self.ensure_owned("domain", d["name"], d["uuid"])
            state = self.virsh("domstate", d["name"])
            ready = False
            if state == "running":
                ready = self.ssh(role, "systemctl is-active remora-t037-wg && "
                                 "ip -j link show wg-remora && "
                                 "sudo -n wg show wg-remora listen-port", check=False, timeout=15).returncode == 0
            values.append({"name": d["name"], "ip": d["ip"], "state": state, "wireguard_ready": ready})
        print(json.dumps({"phase": self.m["phase"], "guests": values,
                          "fixture": str(self.private / "fixture.json")}), flush=True)
        if self.m["phase"] == "configured" and any(v["state"] == "running" and not v["wireguard_ready"] for v in values):
            raise LabError("running guest WireGuard service is not ready")

    def destroy(self, force=False):
        if self.m.get("phase") == "destroyed":
            # A later run may legitimately reuse these names/volume paths.
            # A completed old manifest must never reclaim them.
            print("Recorded lab already destroyed; no resources touched", flush=True)
            return
        intents = self.m.get("volume_intents", {})
        if intents:
            present = set(self.volume_names())
            if present & set(intents):
                raise LabError("unresolved volume creation intent; inspect the manifest and pool before recovery; no deletion by name")
            # Missing intended objects need no deletion. Retain unresolved intent
            # on query failure; never claim completion while an object may exist.
            intents.clear()
            self.save()
        # Preflight all identities before the first destructive command.
        for d in self.m["domains"].values():
            if self.exists("domain", d["name"]):
                self.ensure_owned("domain", d["name"], d["uuid"])
        if self.exists("network", self.m["name"]):
            self.ensure_owned("network", self.m["name"], self.m["network_uuid"])
        for name, key in self.m["volumes"].items():
            if name in self.volume_names():
                check_identity(self.virsh("vol-key", name, "--pool", self.m["pool"]),
                               key["key"] if isinstance(key, dict) else key, name)
                if isinstance(key, dict):
                    st = Path(key["path"]).stat()
                    if (st.st_dev, st.st_ino) != (key["device"], key["inode"]):
                        raise LabError("volume file was replaced; refusing deletion")
        owned_paths = {v["path"] if isinstance(v, dict) else v for v in self.m["volumes"].values()}
        owned_domains = {d["name"] for d in self.m["domains"].values()}
        for name in self.virsh("list", "--all", "--name").splitlines():
            if name in owned_domains:
                continue
            for xml in self.domain_xmls(name):
                refs = {source.get("file") for source in xml.findall("devices/disk/source")}
                refs.add(xml.findtext("os/nvram"))
                if refs & owned_paths:
                    raise LabError("a lab volume is attached to another domain; refusing deletion")
        if self.exists("network", self.m["name"]):
            self.ensure_network_unshared()
        self.stop(force=force)
        for d in self.m["domains"].values():
            if self.exists("domain", d["name"]):
                # NVRAM is removed by libvirt for newly created guests. Adopted
                # NVRAM is an owned pool volume, removed in the volume loop.
                self.virsh("undefine", d["name"], "--nvram" if d.get("auto_nvram") else "--keep-nvram")
        if self.exists("network", self.m["name"]):
            if self.m["name"] in self.virsh("net-list", "--name").splitlines():
                self.virsh("net-destroy", self.m["name"])
            self.virsh("net-undefine", self.m["name"])
        for name in self.m["volumes"]:
            if name in self.volume_names():
                self.virsh("vol-delete", name, "--pool", self.m["pool"])
        self.m["phase"] = "destroyed"
        self.save()
        print("Owned guests, network and volumes removed; private state retained", flush=True)

    def test(self):
        # Never take over or clean up an existing persistent/debugging lab.
        if self.m is not None or any(p.name != "lock" for p in self.state.iterdir()):
            raise LabError("test requires new empty state; existing labs are never adopted")
        output = self.a.output
        if output.exists():
            raise LabError("test output must be a new directory")
        print(f"Disposable lab state: {self.state}", flush=True)
        try:
            self.create()
            cmd = [str(ROOT / "scripts/validate/wireguard.sh"), "--fixture",
                   str(self.private / "fixture.json"), "--start-remora",
                   "--seconds", str(self.a.seconds), "--output", str(output)]
            # Isolate the controller from duplicate terminal signals. On
            # interruption give its finally block time to stop owned test units.
            with subprocess.Popen(cmd, start_new_session=True) as process:
                try:
                    code = process.wait(timeout=self.a.seconds + 120)
                except BaseException:
                    process.send_signal(signal.SIGINT)
                    try:
                        process.wait(timeout=60)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)
                    raise
            if code:
                raise LabError("VM qualification failed; result artifacts retained")
        finally:
            previous = {s: signal.signal(s, signal.SIG_IGN) for s in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
            try:
                if self.m is not None:
                    print("Removing disposable guests, network and disks", flush=True)
                    self.destroy(force=True)
                    for directory in (self.private, self.cache):
                        if directory.exists():
                            shutil.rmtree(directory)
            except Exception:
                print(f"Cleanup incomplete; recover with: ./scripts/validate/libvirt-lab.sh "
                      f"destroy --state {shlex.quote(str(self.state))}", flush=True)
                raise
            finally:
                for s, handler in previous.items():
                    signal.signal(s, handler)

    def adopt(self):
        # Narrow migration of the original manually prepared lab. No arbitrary
        # production domain can be adopted through this command.
        source = self.a.source
        resuming = self.m and self.m["phase"] == "adopting" and not self.m["volumes"]
        if not resuming and (self.m or any(p.name != "lock" for p in self.state.iterdir())):
            raise LabError("adopt requires a new empty state directory")
        if self.a.name != "remora-t037" or not source or source.resolve() != source:
            raise LabError("adopt supports only the original remora-t037 lab and a real source directory")
        self.prerequisites()
        if digest(source / "images/base.qcow2", "sha512") != IMAGE_SHA512:
            raise LabError("old lab base image hash mismatch")
        network = self.xml("network", "remora-t037")
        if network.find("ip").get("address") != "192.168.253.1":
            raise LabError("old lab network does not match the expected profile")
        domains = {}
        xmls = {}
        for role in "ab":
            xml = self.xml("domain", "remora-t037-" + role)
            interface = xml.find("devices/interface")
            if interface.find("mac").get("address") != "52:54:00:37:00:0" + ("a" if role == "a" else "b"):
                raise LabError("old lab MAC mismatch")
            if interface.find("source").get("network") != "remora-t037":
                raise LabError("old lab network attachment mismatch")
            for disk in xml.findall("devices/disk"):
                path = Path(disk.find("source").get("file"))
                if path.parent != source / "images":
                    raise LabError("old lab disk is outside the explicitly supplied source")
            if Path(xml.findtext("os/nvram")).parent != source / "images":
                raise LabError("old lab NVRAM is outside the supplied source")
            domains[role] = {"name": "remora-t037-" + role, "uuid": xml.findtext("uuid"),
                             "mac": interface.find("mac").get("address"),
                             "ip": "192.168.253." + ("10" if role == "a" else "11")}
            xmls[role] = xml
        self.m = {"schema_version": 1, "uri": self.a.uri, "name": "remora-t037", "pool": self.a.pool,
                  "subnet": "192.168.253.0/24", "network_uuid": network.findtext("uuid"),
                  "domains": domains, "volumes": {}, "phase": "adopting"}
        self.private.mkdir(mode=0o700, exist_ok=resuming)
        self.cache.mkdir(mode=0o700, exist_ok=resuming)
        for file in ("ssh-key", "ssh-key.pub", "known_hosts", "a-wg.key", "b-wg.key"):
            old = source / "private" / file
            if old.is_symlink():
                raise LabError("source private file is a symlink")
            shutil.copyfile(old, self.private / file)
            (self.private / file).chmod(0o600)
        self.ssh_config()
        self.save()
        self.idle()
        self.stop()
        for role, xml in xmls.items():
            # Flatten overlays while off: eliminate every dependency on /tmp.
            for disk in xml.findall("devices/disk"):
                src = disk.find("source")
                old = Path(src.get("file"))
                if disk.get("device") == "disk":
                    flat = self.cache / f"{role}.qcow2"
                    run("qemu-img", "convert", "-O", "qcow2", str(old), str(flat), timeout=300)
                    new = self.volume(role + ".qcow2", flat, "qcow2")
                    flat.unlink()
                    backing = disk.find("backingStore")
                    if backing is not None:
                        disk.remove(backing)
                else:
                    new = self.volume(role + "-seed.iso", old, "raw")
                src.set("file", new)
            nvram = xml.find("os/nvram")
            # libvirt-owned NVRAM is usually not readable by the operator.
            # A fresh copy of the matching public template boots the same disk.
            nvram.text = self.volume(role + "-vars.fd", Path(nvram.get("template")), "raw")
            path = self.cache / f"{role}-domain.xml"
            write(path, ET.tostring(xml, encoding="unicode"))
            self.virsh("define", str(path))
        self.m["phase"] = "booting"
        self.save()
        self.start()
        self.configure()


def main():
    os.umask(0o077)
    def interrupted(signum, frame):
        raise KeyboardInterrupt
    for s in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(s, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("test", "cache-image", "create", "start", "configure", "status", "stop", "destroy", "adopt"))
    default = Path(os.environ.get("XDG_STATE_HOME", str(Path.home() / ".local/state"))) / "remora/libvirt-t037"
    parser.add_argument("--state", type=lambda x: Path(x).expanduser().absolute())
    cache = Path(os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache"))) / "remora/cloud-images"
    parser.add_argument("--cache-dir", type=lambda x: Path(x).expanduser().absolute(), default=cache)
    parser.add_argument("--uri", default="qemu:///system", choices=("qemu:///system",))
    parser.add_argument("--pool", default="default")
    parser.add_argument("--name", default="remora-t037")
    parser.add_argument("--subnet", default="192.168.253.0/24")
    parser.add_argument("--binary", type=lambda x: Path(x).absolute(), default=ROOT / "target/release/remora")
    parser.add_argument("--source", type=lambda x: Path(x).absolute(), help="adopt only: old lab directory")
    parser.add_argument("--image", type=lambda x: Path(x).absolute(), help="import image on cache miss; always hash-verified")
    parser.add_argument("--seconds", type=int, default=1800, help="test only: 5..1800 seconds (1800 qualifies duration)")
    parser.add_argument("--output", type=lambda x: Path(x).absolute(), help="test only: new public results directory")
    args = parser.parse_args()
    if args.command == "test" and (not args.output or not 5 <= args.seconds <= 1800):
        parser.error("test requires --output and --seconds 5..1800")
    if args.state is None:
        args.state = (default.parent / "libvirt-runs" / str(uuid.uuid4()) if args.command == "test" else default)
    if not re.fullmatch(r"remora-[a-z0-9-]{1,24}", args.name) or not re.fullmatch(r"[a-zA-Z0-9_-]+", args.pool):
        parser.error("invalid lab or pool name")
    try:
        if args.command == "cache-image":
            print(cached_image(args.cache_dir, args.image), flush=True)
            return 0
        validate_state(args.state)
        with (args.state / "lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            lab = Lab(args)
            if args.command not in ("create", "adopt", "test") and not lab.m:
                raise LabError("state manifest missing; create the lab first")
            if args.command in ("create", "configure", "adopt", "test") and not args.binary.is_file():
                raise LabError("release binary missing; run cargo build --locked --release first")
            if lab.m and lab.m["phase"] == "destroyed" and args.command not in ("destroy", "status"):
                raise LabError("lab destroyed; use a new state directory to create a new lab")
            getattr(lab, args.command)()
        return 0
    except KeyboardInterrupt:
        print(f"Lab interrupted; inspect cleanup output and retained state: {args.state}", flush=True)
        return 130
    except (LabError, subprocess.TimeoutExpired, OSError, ValueError, KeyError, ET.ParseError) as error:
        message = str(error) if isinstance(error, LabError) else type(error).__name__
        print(f"Lab failed: {message}. State retained; no unrelated resources removed.", flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
