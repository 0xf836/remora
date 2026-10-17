# Patched quiche dependency

Remora resolves quiche 0.30.0 from
[`0xf836/quiche`](https://github.com/0xf836/quiche), pinned to commit
`154ce1995f23efa434e72dac269f707145be2015` on `fix/ping-minimum-payload`.
The commit is based on upstream tag `0.30.0` and changes only the minimum-payload
guard and its regression test. See [ADR 0016](../docs/adr/0016-quiche-minimum-payload.md)
and [upstream issue 2835](https://github.com/cloudflare/quiche/issues/2835).

Root, traffic and fuzz manifests contain the same Git revision override. The
other upstream workspace crates remain at their existing registry pins: octets
0.3.7 and, when enabled, qlog 0.18.1. Their versions/checksums and all other
resolved package entries are unchanged by this migration. Use the lockfiles;
do not replace the commit pin with a moving branch.

`quiche.json` records the reviewed revision, base, upstream archive provenance,
source hashes and exact upstream README/license symlink targets. The integrity
check resolves the actual Cargo checkout, rejects another revision or unexpected
Git transitive dependencies, and verifies the source bytes without requiring a
developer clone at a particular path. Fetch locked dependencies first:

```sh
cargo fetch --locked
python3 scripts/validate/quiche_inputs.py
python3 scripts/validate/quiche_tests.py --output /tmp/quiche-tests-FRESH
```

`quiche-tests/Cargo.toml` and `Cargo.lock` are the retained metadata from the
published 0.30.0 crate. The helper creates a fresh test package outside the
dependency checkout, linking its sources and fixtures to the verified Git tree.
This preserves the reproducible library test graph without resolving unrelated
upstream workspace members or storing the dependency's Rust sources here.
The separate test lock retains its recorded yanked `yoke-derive` warning; see
the [dependency audit](../docs/validation/dependencies.md).

The earlier vendored tree is archived locally at
`/tmp/remora-quiche-vendor-archive-20261015/vendor`, outside the project.
Historical vendored-binary qualification remains bound to its original inputs.
The migration preserves prior measured binaries under the ignored
`benchmarks/results/phase8-quiche-git-migration-20261015/before` directory.
An upstream release with an equivalent fix can replace the Git override only
after regression, resolved-graph and affected build/runtime checks.
