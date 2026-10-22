# Explicit size profiles: local evidence

Date: 2026-10-22. Decision: [ADR 0030](../adr/0030-explicit-size-profiles.md).

Historical record: the owner selected new defaults on 2026-10-23 in
[ADR 0031](../adr/0031-default-size-profile.md). Statements below describe the
earlier unchanged-default increment and its executable, not current defaults.

The final scope preserves transport defaults 1350/1200, the WG1120 inner-IPv4
starting example, Cargo manifests/lockfiles and the quiche source policy/pin.
The larger 1452/1400 profile is an explicit operator choice on both endpoints.
An exploratory backend patch was rejected and never integrated or published.

## Checks performed

- Existing local container image `remora-phase2-build`, image ID
  `6da6665e07ee`, runs pinned Rust 1.99.0. Locked offline builds use the existing
  populated Cargo cache. `quiche_inputs.py` verifies the unchanged reviewed
  Git source tree at `154ce1995f23efa434e72dac269f707145be2015`.
- `cargo fmt --all -- --check`, Clippy with all targets/features and warnings
  denied, and `cargo test --locked --all-targets --all-features` pass. Two
  independent-Go tests remain explicitly ignored; they are not new interop
  evidence.
- E2E size coverage preserves default 1200/1201 and tests matched 1452/1400
  endpoints over outer IPv4 and IPv6 loopback. It waits for a full-size exchange,
  verifies byte-identical 1312/1392/1400 application round trips, rejects 1401,
  and checks continued use of distinct existing flows.
- The native selected-profile test observes 1452 PMTU, reduces the verified
  allowance through the public revalidation API, rejects 1312 while too small,
  purges the previously queued oversized message while retaining the empty
  one, submits committed output to the original peer, and observes recovery.
- Diagnostic tests check latest numeric size details after 100 repeated
  rejections despite ordinary event suppression, stale-metadata clearing,
  and 256 distinct worst-case size entries within existing record/ring limits.
- A separate process E2E test uses matched outer 1200/application 1400, verifies
  that 1312 cannot pass either direction, and observes exact correlated
  `size/2` byte counts/ceilings/directions in both roles while small traffic
  remains usable. This was added after the full suite and passed separately;
  format/Clippy were repeated for the final test source.
- All 111 Python tests pass. The first sandboxed attempt had two local UDP
  permission errors; the rerun with socket permission passes. These failures
  were environmental and are not passing attempts.
- Locked release builds without and with the qlog feature pass. Current qlog
  executable SHA-256:
  `48d5eac342896ed356073ec0ddf6ae6bb2ae80d8c1bc6e2520e3449e9cc55882`.
- Touched Markdown file links and `git diff --check` pass.

## Evidence limits

The independent reviewer first reproduced probes above a smaller peer's
advertised cap, then approved the revised opt-in scope with unchanged backend.
That defect remains unresolved. Matched caps avoid that particular conflict;
they do not prove the network or server-to-target route can carry a size.

Native revalidation is a backend-state exercise, not a newly run physical MTU
change/WAN scenario. Outer IPv6 opaque UDP is not inner IPv6 WireGuard testing.
No new VM/WireGuard duration, native ARM64, independent stack, fuzz/ASan or
throughput benchmark is claimed. In particular, the cost of continuous
size-rejection traffic is unmeasured; bounded metadata does not imply zero CPU
cost. Default accepted-packet scheduling/admission and UDP generation are
unchanged, and no performance improvement/regression qualification is claimed.
Historical full qualification remains tied to its original executable.

## Final independent review

Verdict: **APPROVE**, no unresolved substantive findings. The reviewer inspected
all changed/new files and independently repeated five prepared-binary tests:
latest size details under suppression/coalescing and stale-detail clearing;
worst-case diagnostic ring bounds; selected-profile revalidation/recovery;
process E2E directional size rejections on a narrow outer profile; and default
plus selected-profile E2E boundaries over outer IPv4/IPv6. All five pass.
Native/E2E repetitions used the existing container with the repository mounted
read-only and external networking disabled. Initial host attempts could not
prepare the container-path fixture; the corrected container attempts pass.
The reviewer also checked `git diff --check` and unchanged dependency/default
inputs, but did not rerun the full build/lint/Rust/Python matrix.

Two documentation findings were corrected and independently rechecked: a
conservative lower bound is not a necessary per-stream threshold, and the
failure-code table must remain contiguous. The reviewer considers this scope's
checks proportional without a new throughput/WG benchmark; rejection-path CPU
cost and full performance qualification remain explicitly unmeasured.
