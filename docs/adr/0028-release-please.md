# ADR 0028: Maintainer-controlled releases with Release Please

Date: 2026-10-19.
Status: accepted for implementation at the owner's request; remote publication
is a separate maintainer action.

## Context and alternatives

The initial implementation is ready for integration. The owner requested
Release Please and guidance on development PRs, release PRs, tags and releases.
Manual version/changelog/tag maintenance is possible but duplicates state.
Automatically publishing every feature merge would remove the maintainer's
choice of release boundaries. Production is one unpublished Cargo package;
traffic and fuzz tools have separate lockfiles referencing that package.

## Decision

Use a single root Rust release component, starting at v0.1.0. An ordinary PR
merges development into main. Release Please maintains a separate version and
changelog PR; the maintainer merges it to authorize a release. Before 1.0,
fixes bump patch, features bump minor, and breaking changes bump minor with an
explicit compatibility note. Existing implementation history is preserved,
including commit identities referenced by validation records.

Pin release-please-action v5.0.0 to commit
`45996ed1f6d02564a971a2fa1b5860e934307cf7` (bundled release-please 17.6.0).
This is release tooling, not a new production Rust dependency. The Rust
strategy updates the root manifest/lockfile. Extra TOML updates select only
the Remora entry in traffic/fuzz lockfiles. The pinned TOML updater exposes
tagged primitive values, so the JSONPath name filter uses `name.value`.
Action upgrades must recheck this selection against real lockfiles.

Create a draft release and an explicit tag (`force-tag-creation`); GitHub does
not otherwise create draft release tags immediately. Build the tagged source
using the existing pinned Rust/Debian image, with qlog compiled in. Publish
only Linux GNU x86_64 archives. Packaging and upload finish before the release
is made public. Keep explicit recovery for an existing draft tag; do not move
tags to correct a failed build.

## Trust, permissions and failure boundaries

The third-party action receives contents/issues/PR write permissions only in
its main-branch job, allowing it to maintain PRs, labels, tags and releases.
The publishing job has contents write permission. Use ephemeral GITHUB_TOKEN,
checkout without persisted credentials, and immutable action SHA pins. No
PAT, package-registry token, deployment credential or pull_request_target
execution is introduced. Main-branch membership and exact draft/tag SHA
agreement are checked before building. Workflow inputs enter shell through
quoted environment variables and stable-version tags are validated.

Build and upload belong to one workflow because ordinary tag/release events
created by GITHUB_TOKEN do not start downstream workflows. Bot PR checks may
need the maintainer's approval. Enabling Actions PR creation is a documented
one-time repository setting, not something silently changed by this patch.

Use tools/build as Docker's image-build context. Only explicitly listed public
members enter the binary archive; local fixtures, benchmark outputs, credentials
and working-tree archives are excluded. Tag/version, ELF architecture, embedded
source revision and clean tracked inputs are checked. Upload may replace
partial assets only while the release is a draft. A published release is
rejected by the recovery workflow.

The distribution dynamically links GNU runtime libraries, including libstdc++
and libgcc. Publish Linux/glibc/runtime requirements and per-build metadata,
package inventory and SHA-256 sums. Checksums are integrity checks, not detached
signatures. Packaging does not transfer old qualification to a new executable
or claim reproducible Rust compilation.

## Consequences and verification

Maintainers merge two PRs for the initial release, then repeat the development
and release PR lifecycle. No manual tag push is needed. Stable tags only are
supported initially; prerelease channels and ARM64 require separate decisions.
Source/tag changes after a failed release require a new version.

Verification includes actionlint, the bundled release-please JSON Schema,
offline generation with current commits, first/subsequent version selection,
preservation of handwritten changelog content and byte-for-byte checking that
only Remora's version changes in the three lockfiles. Packaging tests cover
public archive contents/checksums and rejection of mismatched versions,
revisions, architectures, dirty inputs and existing output directories.
The pinned container build and native qlog compilation/packaging are exercised
in an isolated local clone. GitHub Actions execution and publication are
reported separately when they actually occur.
