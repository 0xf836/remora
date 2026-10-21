# ADR-0029: Descriptor-based Linux private-file ACL validation

Date: 2026-10-21. Status: implemented and runtime-verified; independent review
of the corrected snapshot remains pending.

## Context

The mode-only private-file check rejects safe POSIX access ACLs because Linux
reports ACL_MASK in the group mode field. The ACL form used by systemd
LoadCredential can retain root ownership and grant only a named service UID
read access. It therefore fails even though the owning group has no access.
Client tokens also had a distinct process-owner-only restriction. See the
[operator guidance](../configuration.md) for the policy.

## Decision

Keep ADR-0005's secure descriptor-relative open, NOFOLLOW at every component,
NONBLOCK, regular-file/length checks, bounded reads and fixed diagnostic fields.
Use the already locked rustix safe fgetxattr API on the same opened descriptor;
no libacl dependency, subprocess or locally authored unsafe is needed.

Read Linux `system.posix_acl_access` with one fixed 64 KiB buffer and one syscall.
An ERANGE, interruption, I/O or permission error fails closed; no retries or
size-query/read loop are necessary. ENODATA (no ACL) and EOPNOTSUPP (no POSIX ACL
support) select the existing strict ordinary-file policy. Neither outcome
permits group/other mode bits or changes the client's ordinary owner rule.

The Linux xattr version-2 decoder validates 8-byte records, canonical tag order,
unique named IDs within each class, undefined base qualifiers,
defined named qualifiers, permission bits, unique/required base entries and
mask. Numeric UID/GID order is unrestricted: kernel namespace/mount translation
can change the numeric order of exported identifiers without reordering entries.
The parser performs two bounded entry scans and checks uniqueness by sorting a
copy of `(tag, ID)` pairs, preserving the original ACL order. The fallible
reservation requests less than 64 KiB for at most 8191 pairs, bounded by the
already checked input. Duplicate detection is deterministic O(n log n); a UID
and GID with the same numeric value remain different qualifier classes.
Owner/mask/other permissions must agree with inode mode.

A base-only ACL preserves the ordinary mode/ownership policy, including the
server-only root exception. For extended ACL-bearing private files:

- The inode owner must be root or the executing UID.
- Trust named root and the executing UID. All unrelated named users, every
  owning/named group and other must have zero effective read/write/execute.
- Apply ACL_MASK to named users and groups, not owner or other. Entries whose
  permissions are entirely masked out do not expose the file.
- A root-owned secret delegated to a different executing UID must contain a
  named entry granting that UID exactly read permission after masking. This
  shared rule includes client tokens, server keys and server credential files.
  Root ownership without that ACL does not widen the ordinary client policy.
- Preserve the executing owner's ordinary permission policy; changing owner
  write/execute rules is outside this compatibility fix. Root is already a
  trusted provisioning principal and can change its files regardless of ACL.

The kernel exports ACL qualifiers in the caller's namespace, including mount
ID translation. Reject UINT32_MAX qualifiers rather than treating unmapped IDs
as trusted identities. Inode metadata can instead report an unmapped owner as
the configurable overflow UID. Read `/proc/sys/kernel/overflowuid` with a
32-byte limit and reject ACL acceptance when this inspection fails or the
inode owner equals the overflow UID. This conservatively rejects a legitimate
owner equal to that value too. No hard-coded assumption that overflowuid is
65534, that namespace root means host root, or that a credential path is trusted.
The trust principal is root within the process's UID namespace, consistent with
the existing metadata policy. Arbitrary remote/NFSv4 ACL models are unsupported.

After permission inspection, recheck owner/group/mode/ctime on the held inode.
An observed change rejects the file. This is not an atomic snapshot against a
trusted owner changing an inode; ADR-0005's trusted owner/directories and atomic
replacement assumptions remain. Contents and raw ACL/procfs errors never enter
routine diagnostics. Public CA/certificate inputs bypass private-file policy.

## Alternatives

- Allow group-readable numerical modes or credential paths: rejected because
  these do not identify which principals can access a secret.
- Use libacl: viable, but adds an unnecessary dependency/packaging or unsafe FFI
  obligation when the existing safe descriptor API exposes the small Linux ABI.
- Copy/chown credentials: an operator workaround, with added lifecycle work,
  rather than support for the reported credential representation.
- Support arbitrary ACL ownership/write delegation: exceeds the read-only
  systemd case and would widen the trust policy without a concrete need.

## Security analysis and verification limits

The boundary is local startup input. Negative tests cover foreign user/group
grants, mask changes, other access, malformed/duplicate records, read errors,
missing delegation, read/write delegation and ambiguous owners. The production
loader is exercised on real ACL files for all three inputs. An explicit ignored
root test spawns a different UID against root-owned ACL fixtures; a separate
real LoadCredential runner must inspect credentials actually produced by systemd.
Do not replace either privileged result with redundant own-UID ACL fixtures.

No packet loop or wire behavior changes. Startup adds a bounded 64 KiB scratch
buffer, one xattr read per private input and a small procfs read for ACL-bearing
inputs. No networking-performance claim is made. This is implementer security
analysis, not an independent security-review sign-off. Independent review of the
initial implementation found an invalid numeric-ID-order requirement; the
corrected snapshot passed the regression tests but was not independently
reviewed again.

Validation passed with the pinned Rust 1.99.0 toolchain: 212 tests, formatting,
Clippy and release build, plus the explicit root-owned ACL test. Real
LoadCredential integration passed on Debian 13.7, systemd 257.13 and Linux
6.12.111: all three credentials were owned by root, had mode 0440 and a named
read ACL for service UID 1000. The fixed executable accepted each input; an
existing pre-fix executable, using its compatible configuration schema, rejected
each with the expected Permissions error. Generated evidence remains local.

## Sources

- [Linux 6.12 ACL export and namespace translation](https://github.com/torvalds/linux/blob/v6.12/fs/posix_acl.c#L790-L883).
- [Linux POSIX ACL semantics](https://man7.org/linux/man-pages/man5/acl.5.html).
- [systemd v257 credential creation](https://github.com/systemd/systemd/blob/v257/src/core/exec-credential.c#L294-L345).
- [Existing startup guarantees](0005-foundation-primitives.md).
