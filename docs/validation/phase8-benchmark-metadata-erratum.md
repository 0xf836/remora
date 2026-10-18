# Phase 8 benchmark requested-buffer metadata correction

Recorded 2026-10-17 following the independent decision review.

## Defect and correction

The selected T077 manifest reports old requested socket buffers: outer 262144
bytes and application 65536 bytes. The measured requests were outer 1048576 and
application 131072 bytes, from the shared `IoSettings` defaults in the measured
`src/config.rs`. Reference fixture IO and baseline argv do not override these
fields; the baseline also uses that shared Rust type.

`reference.py` previously duplicated the old literals in the manifest. The runner
now reads the literal values from the same hashed `IoSettings` declaration,
records their source, and rejects an unrecognized declaration or out-of-range
value instead of guessing. This is deliberately not a general Rust evaluator:
a future computed/default declaration needs an explicit metadata implementation.
No socket request, role configuration, workload, numerical criterion or production
Rust behavior changes in this correction.

A regression invokes manifest construction without starting any traffic. Before
the fix it fails with the old numbers; after the fix it passes. Additional checks
verify changed shared literals and fail-closed handling of unsupported/invalid
declarations. All 24 reference-method tests and all 98 Python tests pass. No Rust
rebuild or runtime benchmark was needed or run.

## Historical evidence retained

Original run:
`benchmarks/results/release/phase8-x86-cubic-20261016/reference/benchmark/`.

- The original `manifest.json` is untouched; SHA-256: `3d858f4bcd6739641c4a4af56abbbffa819d50ddd3bb7ae0a2584a64fcaa8de4`.
- Its measured runner is archived as
  `benchmarks/results/phase8-buffer-metadata-erratum-20261017/reference-before.py`,
  SHA-256 `2c5a44216c335c47dbd5316a806b7645fa85eec637474e928a27416ac30902ba`, matching the original manifest's source hash.
- The original T078 release-report snapshot is retained as `release-report-before.md`
  in that correction directory. It matches the historical reconciliation record's
  report hash. Existing qualification/reconciliation records keep their original
  source/report identities; they are not relabelled as runs of the corrected runner.
- An additive `manifest.socket-buffers-erratum.json` alongside the original manifest
  records the original hash, corrected requests, source identity and hashes of all
  150 paired diagnostic files. A copy is retained as `erratum.json` in the correction
  directory. The original suite/criteria/spread and measured result bytes stay intact.

The independent review and correction both checked all 75 baseline/tunnel pairs:
**150 diagnostic files, 300 role records**. Every record has effective outer RX/TX
2097152 bytes each and application RX/TX 262144 bytes each. Thus actual buffer
settings match between modes. Requested values are established from the bound
source/invocations, not inferred by halving effective kernel values; such inference
would be unsafe under clamping.

The correction fixes reproducibility metadata, not a discovered unequal-buffer
comparison. T076/T077 remain the recorded successful measurements on the original
runner/executable; T078 now links this erratum. No threshold or PASS/FAIL result is
rewritten. Future runs will record requests through the corrected metadata path.
