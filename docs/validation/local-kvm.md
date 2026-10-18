# Local two-VM qualification lab

Historical smoke and duration run artifacts remain locally under ignored
`docs/validation/wireguard-results/`; the outcome notes below are retained in
Git. See [artifact retention](README.md).

The explicit operator tool [libvirt-lab.sh](../../scripts/validate/libvirt-lab.sh)
creates two disposable x86_64 KVM guests, prepares independent WireGuard peers,
and writes the private fixture consumed by [wireguard.sh](../../scripts/validate/wireguard.sh).
It is separate from Remora and its qualification runner. Neither the product nor
the runner provisions VPN state. Docker is useful for build checks; this lab uses
libvirt/KVM so it exercises real guest kernels and UDP networking.

**VM tests are opt-in.** Ordinary `cargo test`, the MVP validation script and
CI do not create/start guests or run this qualification. They run only the
unprivileged unit tests of the tooling. Use the VM test when a change to QUIC
timers, UDP I/O, lifecycle, encapsulation or MTU warrants a real guest/WG check,
or when collecting the required release-duration evidence. Routine edits do
not require a two-VM run or an extra development gate.

## Prerequisites

Use a native Linux x86_64 host with `/dev/kvm` access, a working `qemu:///system`
connection, permission to manage its domains/networks/storage, installed UEFI
firmware and an **existing active directory storage pool** (default: `default`).
The script does not install host packages, change pool definitions or configure
unrelated networks. Required host commands: Python 3.11+, `virsh`, `virt-install`
(with the `debian13` OS variant), `qemu-img`, `xorriso`, `curl`, `ssh`, `ssh-keygen`,
`wg`, `openssl`, and `ip`.

Reserve at least 3 GiB available RAM, 18 GiB available in the storage pool and
3 GiB in the state filesystem for staging. Guests use 2 vCPUs, 1024 MiB RAM
and an 8 GiB sparse standalone qcow2 disk each. Internet access is required for
the **first** official image download and guest Debian package installation. The download
honors `HTTPS_PROXY`/`HTTP_PROXY`; no host proxy settings are copied into guests.

The image is pinned to Debian 13 genericcloud amd64 **20261001-2618** at its
[dated official URL](https://cloud.debian.org/images/cloud/trixie/20261001-2618/debian-13-genericcloud-amd64-20261001-2618.qcow2).
The script verifies the embedded SHA-512 before use; it does not perform Debian
checksum-signature verification. Guest package installation uses the configured
Debian repositories, so installed package revisions can evolve. This is a
functional lab, not a frozen benchmark machine image.

Persistent volumes are created/uploaded through the [libvirt storage API](https://www.libvirt.org/manpages/virsh.html#storage-volume-commands),
and firmware/NVRAM follow the [libvirt UEFI domain format](https://www.libvirt.org/formatdomain.html#bios-bootloader).

UEFI is selected from the first boot. The initial local SeaBIOS attempt looped
before reaching Linux; UEFI booted both guests successfully. Changing the CPU
model alone did not fix that attempt. The conservative `qemu64` CPU model is
created with AMD SVM disabled, allowing creation on an Intel KVM host without
requiring nested virtualization. It is
adequate for functional checks; this virtual lab cannot establish native CPU
performance or ARM64 acceptance.

## On-demand test with automatic cleanup

Run as the user authorized to manage system libvirt; do not run the entire
script with `sudo`, which changes the private-state owner and default path.

```bash
cargo build --locked --release
./scripts/validate/libvirt-lab.sh test \
  --seconds 10 --output /tmp/remora-wg-smoke-new
```

Output directories must be new. For the full T037 interval use `--seconds 1800`
and a separate results directory. Preserve the resulting summary, traffic,
observations and Remora status artifacts; only a passing 1800-second run
qualifies the duration. A 10-second smoke does not qualify SC-001.

`test` creates a fresh private state directory under
`${XDG_STATE_HOME:-$HOME/.local/state}/remora/libvirt-runs`, creates/configures
the guests, invokes the traffic qualification and **attempts ownership-verified
cleanup of its domains, network, disks, seed ISOs and NVRAM in a finally block**.
After successful cleanup it also deletes the
completed run's private credentials and staging files. Public results and a
small ownership manifest are retained. The same cleanup runs after test or
provisioning failure, Ctrl+C, SIGTERM and SIGHUP. A nonresponsive disposable
guest is powered off only after the graceful shutdown deadline and renewed
identity checks; persistent/manual `stop` never forces power-off.

The one-shot command refuses existing names or a nonempty supplied state:
it never takes over and later destroys a pre-existing debugging lab. It waits
for the test controller's bounded cleanup on interruption. If cleanup cannot
complete, it reports the exact `destroy --state ...` recovery command and
retains credentials/manifest. SIGKILL or host power loss cannot execute a
finally block; recover such a run from its printed state path. No cron job,
background VM service or automatic host startup is installed.

## Shared image cache

The pinned base image lives in
`${XDG_CACHE_HOME:-$HOME/.cache}/remora/cloud-images`, independently of any lab
or run state. Every creation rechecks its SHA-512 and standalone qcow2 format.
On a valid cache hit **curl is not invoked**. Cleanup never removes this cache.
The first download uses a temporary file and publishes it atomically only
after verification; interrupted/incomplete downloads cannot become a valid
cached image. Cache access is locked. Corruption is an error, not a silent
redownload. `start`, `configure` and the traffic runner do not fetch images.

To prefetch or import an already downloaded matching image without starting VM:

```bash
./scripts/validate/libvirt-lab.sh cache-image
# Optional import on cache miss; otherwise the verified shared cache wins:
./scripts/validate/libvirt-lab.sh cache-image --image /absolute/base.qcow2
```

`--cache-dir /absolute/cache` selects another persistent location. Guest apt
package downloads still occur when provisioning new guests; this cache covers
the approximately 326 MiB base image, not a frozen Debian package repository.

## Explicit persistent debugging lab

Only when keeping guests for investigation is useful, invoke `create`
explicitly. This mode intentionally keeps its stopped disks and definitions
for reuse; it is not the recommended one-shot test command.

```bash
./scripts/validate/libvirt-lab.sh create --state /absolute/private/debug-lab
./scripts/validate/libvirt-lab.sh status --state /absolute/private/debug-lab
```

Creation waits for assigned DHCP leases, SSH and successful cloud-init, uploads
the selected release binary and traffic tool, validates both role configs and
installs the persistent guest-only `remora-t037-wg.service`. Guests use a locked
password and a generated SSH key; the dedicated `lab` account has guest sudo
permission. SSH host keys are accepted on first use only after the owned
domain/network and assigned MAC/IP lease are checked, then pinned with
`StrictHostKeyChecking=yes`. This first-use trust assumes a trusted local host
and bridge. A changed host key is an error, never silently replaced.

Private state lives outside Git (directory 0700,
credential/config files 0600). Disks, seed ISOs and NVRAM live in libvirt's
persistent storage, with no `/tmp` backing files. Seeds contain the SSH public
key and package setup, not WireGuard/TLS private keys or bearer tokens. No lab
resource is enabled for host autostart. Keep the state directory: its manifest
records resource ownership and its SSH key is needed to manage the guests.

## Topology and test profile

| Component | Guest A | Guest B |
| --- | --- | --- |
| Default VM name | `remora-t037-a` | `remora-t037-b` |
| Outer address | `192.168.253.10/24` | `192.168.253.11/24` |
| WireGuard inner address | `10.77.37.1/32` | `10.77.37.2/32` |
| WireGuard peer path | Remora listener `127.0.0.1:41000` | Approved target `127.0.0.1:51820` |
| Generic UDP path | Listener `127.0.0.1:41010` | Approved target `127.0.0.1:41020` |
| QUIC role | Client | Server on UDP 4433 (WG), 4434 (generic) |

The dedicated NAT network has gateway `192.168.253.1`; libvirt manages its
host bridge/NAT rules. The script rejects subnet/route overlaps and existing
resource names. Another lab can use `--name remora-other --subnet 192.168.252.0/24
--state /absolute/private/state` at creation. Subsequent commands use that same
`--state`; the manifest supplies names and addressing. Names/subnets are not
changed in place. The two inner /32 routes and `wg-remora` MTU 1120 are confined
to the guests. This leaves room for IPv4 UDP and WireGuard overhead within
Remora's 1200-byte application ceiling; the outer link MTU is 1500.

Guest B's dedicated nftables table drops non-loopback UDP to 51820 and 41020,
preventing direct outer-network application traffic from bypassing Remora.
Only that table is managed. QUIC GSO/GRO remain off. The WG role fixture uses
`idle_timeout_ms = 15000`, allowing a retry after the first handshake packet is
lost during CONNECT setup; the generic role uses 5000. Setup timeout remains
5000. These are explicit test settings, not changes to product defaults or an
idle-recovery claim. Only WireGuard setup persists across guest reboots;
Remora and traffic processes are owned by each test run and stopped afterward.

## Reuse and cleanup

```bash
./scripts/validate/libvirt-lab.sh stop
./scripts/validate/libvirt-lab.sh start
./scripts/validate/libvirt-lab.sh configure
./scripts/validate/libvirt-lab.sh status
```

These commands manage the manual lab; pass the same `--state` used for its
creation. Without it, they use the legacy default
`${XDG_STATE_HOME:-$HOME/.local/state}/remora/libvirt-t037`. One-shot `test`
runs need no later `stop` or `destroy`.

`start` restores stopped guests/network and checks readiness; it does not
launch Remora. `configure` uploads the current binary/tool, reuses the WG keys,
renews the **two-day test TLS certificates** and bearer tokens, validates configs
and restarts only the guest fixture service. Run it after a rebuild or before
reusing expired fixtures. It refuses to run while test units exist. Clock skew
and installed-source mismatches are also checked by the qualification runner.

`stop` requests graceful shutdown, disables the owned network after guests
stop, and refuses forced power-off on timeout. `start` reactivates that network.
Explicit `destroy` removes only the manifest-owned guests/network/volumes,
after identity checks, and retains private state for inspection. To recreate
after destruction, choose a new empty state directory. Failed creation also
retains the manifest: inspect it and use `destroy` to clean up partially created
resources before retrying with a new state. Do not delete the manifest first.
Concurrent management of one state directory is locked. UUID mismatches, name
collisions and symlinks fail closed; unrelated domains and networks are retained.
Cleanup checks both current and next-boot XML of foreign persistent guests,
and current XML of transient guests, for attached lab networks/disks/NVRAM.
A live-only attachment blocks cleanup before any guest shutdown or deletion;
query failures also fail closed. The state lock does not serialize unrelated
libvirt administrators: avoid concurrent attachment changes during cleanup.

Volume creation saves an intent in the manifest before calling libvirt and
records key/path/device/inode ownership before uploading. An interruption after
creation but before identity capture leaves an unresolved `volume_intents`
entry. If that volume name exists, `destroy` refuses deletion by name and does
not mark the lab destroyed. Inspect the retained manifest and the specified
pool with `virsh vol-list`, `vol-info` and `vol-dumpxml`; independently establish
ownership and check all live/persistent guest references. Only an operator who
has confirmed that the ambiguous volume belongs to this run should remove it
explicitly, then retry the printed `destroy --state ...` command. If ownership
cannot be established, retain it for investigation. Missing intended volumes
need no deletion; the next successful query clears their intent. Never delete
the manifest to bypass recovery. Interruption during upload is different:
ownership is already recorded, so ordinary verified cleanup can remove it.

An optional `create --image /absolute/cached/base.qcow2` imports into the
shared cache on a cache miss, with the same SHA-512/standalone-image checks.

If readiness fails, inspect `virsh -c qemu:///system console remora-t037-a`
(leave with Ctrl+]) and `cloud-init status --long` in the guest. For a guest
fixture-service failure use the pinned SSH config, for example:

```bash
ssh -F /absolute/private/debug-lab/private/ssh-config remora-t037-b \
  'sudo -n journalctl -u remora-t037-wg --no-pager'
```

For a configuration-stage failure, correct its prerequisite and rerun
`configure`; it regenerates matching TLS/token fixtures for both roles.
For an incomplete fresh creation, use the retained manifest to `destroy`, then
retry in new state with the cached image. Do not bypass an identity mismatch.

For the original manually prepared lab only, `adopt --source /absolute/old/lab`
migrates its known MAC/address profile into a new persistent state directory.
It gracefully shuts down both guests, flattens their overlays into new storage
volumes, replaces seed/NVRAM paths, and configures the persistent guest service.
Original temporary files are retained for recovery; do not remove them until
the migration and a restart have passed. This command is not a general-purpose
VM import or production migration tool.

## Recorded verification

The later [review remediation](review-remediation.md) adds deterministic
live/transient attachment and volume-interruption recovery regressions;
all 21 lab-tool unit tests passed in native and pinned-container checks.
No new guests or networks were created for those regressions. The real lab
results below retain their recorded revisions and are not new-binary claims.

On 2026-10-11 the initial two Debian 13 guests completed cloud-init and SSH/WG
setup. The initial successful smoke
reported 100/100 packets in both directions on each application path, no
measured loss/integrity errors and `duration_qualified: false`. Earlier failed
attempts remain in the adjacent results directories. That smoke did not
qualify T037's duration; the full run is recorded below.

The final script was then exercised by creating a separate temporary pair
from the pinned image, completing cloud-init/configuration, passing the
fresh-create traffic smoke,
and destroying only that pair/network/volumes. The main pair was migrated to
persistent storage, gracefully stopped, started again, and passed the
post-restart smoke.
Both runs measured 100/100 packets per direction/channel. Warmup can include
WG send errors before the first handshake; those are recorded separately and
are not counted as measured successes or hidden. Only the 10-second measured
interval passed. Name-collision refusal was checked against the existing lab.
The existing unrelated libvirt domain/networks were retained.

Locked release builds, formatting, Clippy, Rust/traffic tests and all 13 Python
tests passed natively and in the pinned x86_64 build image. Eight Python safety
tests cover subnet/route collision, private-state/symlink rules, identity
replacement refusal and deletion refusal for resources shared by another
domain. These checks run in ordinary CI without creating VM resources.

The opt-in/cache/cleanup follow-up imported the existing verified image into
the shared cache without downloading it. The
one-shot smoke
passed and automatically removed its VM/network/volume resources. A second
one-shot run was interrupted with real Ctrl+C during traffic; its
incomplete qualification result
remains invalid, while the parent exited 130 after deleting its resources.
Both live runs reported a verified cache hit with no image download. Their
private credentials/staging were removed and ownership manifests retained.
Old manual lab resources and duplicate temporary images/credentials were
also removed. Final libvirt/host-link inspection found no Remora domains,
networks, volumes or test bridge; unrelated pre-existing resources remained.

The updated tooling has 17 lab safety/lifecycle/cache unit tests and 22 Python
tests in total, all passing natively and in the pinned build image. New cases
cover a single download across cache reuse, offline import, rejection of
corruption/incomplete downloads, cleanup after provisioning failure and
interrupt, preservation of recovery state on cleanup failure, refusal to
take over an existing lab and owned-network shutdown/shared-network refusal.
An already destroyed manifest cannot reclaim names reused by a later run.
The VM tests stay outside routine CI.

The full T037 run then passed
the complete 1800-second measured interval on 2026-10-11: 18,000/18,000 packets
per direction/channel, zero measured integrity/delivery/send/scheduling errors,
stable Remora units, fresh WG handshakes and increasing bidirectional peer
counters. The command reused the verified local image without downloading it.
Its automatic cleanup returned successfully; independent post-run inspection
found no Remora domains, network, volumes or bridge. The run manifest reached
`destroyed`, and its private credentials and staging were removed. Only the
shared cached image and small ownership manifest remain. See
[application compatibility](application-compatibility.md) for the profile,
separate warmup observations and qualification limits.
