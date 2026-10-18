# Releasing Remora

Remora uses Release Please for version changes, changelog entries, tags and
GitHub Releases. The maintainer chooses when to release by merging a release PR.
The ordinary development PR and the generated release PR are separate.

## First release

The first release is **v0.1.0**, matching the initial Cargo package version.
The empty `.release-please-manifest.json` means that no release has happened yet;
it must not be populated with a fictitious previously released version.
`initial-version` applies only to the initial release. `bootstrap-sha` is the
repository's initial commit, excluded from the first changelog scan; subsequent
commits are included. Later releases use the recorded released version.

Before merging the initial development PR:

1. Open the repository's **Settings > Actions > General > Workflow permissions**.
   Enable **Allow GitHub Actions to create and approve pull requests** and save.
   The workflow requests its own job permissions; the repository's default
   workflow permission can remain read-only. No personal access token is needed.
2. Commit the release configuration along with the intended changes and push
   the `001-masque-udp-tunnel` branch to `origin`.
3. Open **Pull requests > New pull request**. Set **base: main** and
   **compare: 001-masque-udp-tunnel**. A suitable title is
   `feat: add authenticated MASQUE CONNECT-UDP tunnel`. Describe the delivered
   behavior, compatibility and completed validation. Create the PR.
4. For this initial PR, use **Create a merge commit** to retain the existing
   implementation commits and their hashes referenced by development evidence.
   Do not push local `main` directly as a substitute for merging the PR.
5. In **Actions**, watch **Release**. It creates a PR proposing version 0.1.0,
   with a changelog and `.release-please-manifest.json` update. It does not
   publish the application when the development PR is merged.
6. Open that release PR and check the version and release notes. GitHub may
   display **Approve workflows to run** for the bot-created PR: approve those
   runs if prompted. Merge the release PR when ready to publish.
7. The next **Release** run creates tag `v0.1.0` and a draft GitHub Release,
   builds its artifacts, uploads them and publishes the release after success.
   The tag exists during this build; a draft release is not yet a completed
   public binary distribution. Check both the successful run and its assets.

Do not create a competing manual tag or manually publish the draft while this
workflow is running. No push, PR, tag or release is created by merely adding
these configuration files locally.

`force-tag-creation` creates the tag even while the release is a draft; GitHub
otherwise defers creating a new tag until publication. This setting creates a
missing tag and does not move an existing one. Before building, the workflow
checks that the tag resolves to the draft release's exact commit SHA.

The linked **Initial development notes** heading in `CHANGELOG.md` preserves
the handwritten initial notes. Release Please inserts versioned entries above
that heading. Those historical notes are not rewritten into a new claim about
the binaries built by GitHub Actions.

## Subsequent development and releases

Use Conventional Commits for messages that land in `main`:

| Message | Version effect while below 1.0 |
| --- | --- |
| `fix(reactor): ...` | Patch: 0.1.0 to 0.1.1 |
| `feat(config): ...` | Minor: 0.1.0 to 0.2.0 |
| `feat(config)!: ...` or a `BREAKING CHANGE:` footer | Minor, with a breaking-change note |
| `docs: ...`, `test: ...`, `build: ...`, `ci: ...`, `chore: ...` | No release on their own by default |

After 1.0, breaking changes use a major bump. The initial-version setting does
not pin later releases to 0.1.0. Do not set a permanent `release-as` override.

Squash merging development PRs is convenient when their intermediate commits
are not useful independently. In that case check the final squash commit's
subject/body, including any breaking-change footer. Plain merge commits are
also supported; preserve Conventional Commit messages in the merged history.
There is no need to rename old setup/documentation commits just to introduce
Release Please. Cosmetic history changes would invalidate commit references.

Release Please maintains a pending release PR as work accumulates. Merge it
when the next release is wanted; no manual `git tag` or `git push --tags` step
is part of this process. The Rust strategy updates root `Cargo.toml` and
`Cargo.lock`; targeted TOML updates also change only the `remora` version in
the traffic and fuzz lockfiles. The tools are unpublished and not independent
release components.

## Published artifacts

The current workflow supports stable `vMAJOR.MINOR.PATCH` tags and publishes
only **x86_64-unknown-linux-gnu**. ARM64 remains outside current release scope.
Introducing `-rc`/`-beta` releases requires a deliberate prerelease configuration
and packaging change; setting a GitHub prerelease checkbox alone is insufficient.

Each release contains:

- `remora-vVERSION-x86_64-unknown-linux-gnu.tar.gz` with `remora`, `LICENSE`,
  `README.txt`, `BUILD-INFO.json` and `build-packages.txt` at archive root.
- `SHA256SUMS` for the compressed archive.

The binary is built from the **tagged commit**, using the pinned Rust/Debian
build image, locked Cargo inputs and `--features qlog`. Runtime tracing is still
opt-in. Linux 6.1+ and glibc 2.36+ are required; this is not a static/musl build.
The `libstdc++.so.6` and `libgcc_s.so.1` runtimes must also be available
(Debian 12's `libstdc++6`/`libgcc-s1` or compatible). The ELF dependency names
are recorded in `BUILD-INFO.json`.
The pinned image is based on Debian Bookworm. The application binary has its
own SHA-256 and embedded source revision; `BUILD-INFO.json` records these and
the toolchain/build-input hashes. The packaging script rejects mismatched
tag/package versions, mismatched embedded revisions, wrong ELF architectures,
changed tracked inputs and pre-existing output directories. Files enter the
archive through an explicit allowlist, never a recursive working-tree archive.

Download both release assets into an empty directory and verify before extracting:

```bash
sha256sum --check SHA256SUMS
tar -xzf remora-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
./remora --version
```

Adjust the filename to the selected version. A checksum detects corruption; it
is not a detached signature or an independent attestation of the publisher.
See the tagged configuration/TLS/operations guides before starting either role.

Packaging fixes tar/gzip timestamps to the source commit time for repeatability
with identical inputs. This is not a promise of bit-identical Rust builds or a
new performance qualification. Existing measurements remain bound to the
executables, environments and workloads recorded in their reports.

## Failed publication and recovery

If compilation, packaging or upload fails, the GitHub Release stays a draft.
Do not delete/move its tag to retry. Existing published releases are not
overwritten by this workflow.

Open **Actions > Release > Run workflow**, select **main**, enter the existing
draft's tag (for example `v0.1.0`) in **release_tag**, and run it. This skips
release-PR creation, checks that the release is still a draft and the tagged
source belongs to `main`, and repeats the tagged build/upload. Partial draft
assets can be replaced; publication happens only after all assets are uploaded.
Leaving the field empty runs normal release-PR maintenance instead.

GitHub's **Re-run failed jobs** can also retain the successful release job's
outputs. **Re-run all jobs** may find that the release was already created,
so use the explicit tag recovery option if the build is then skipped. If a
fix must change the tagged source, publish a new version rather than moving
the original tag.

The workflow builds and publishes in one run because tags/releases created
with `GITHUB_TOKEN` do not trigger ordinary downstream tag/release workflows.
Bot-created PR checks may require manual approval. If fully automatic separate
workflows are introduced later, evaluate a GitHub App token and its permissions
instead of adding a broad personal token by default.

## References

- [Release Please lifecycle](https://github.com/googleapis/release-please#whats-a-release-pr)
- [Release automation decision](adr/0028-release-please.md)
- [Manifest configuration](https://github.com/googleapis/release-please/blob/main/docs/manifest-releaser.md)
- [Release Please Action](https://github.com/googleapis/release-please-action)
- [GitHub workflow triggering and token behavior](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)
