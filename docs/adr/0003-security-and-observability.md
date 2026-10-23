# ADR-0003: Verified server trust, bounded authorization and safe diagnostics

Date: 2026-10-10. Revised: 2026-10-11. Status: selected security design; explicit security review and negative tests required before release.

## Context and threat model

Untrusted Internet peers can forge/modify packets, churn sessions, send malformed H3/Capsules/ancillary metadata and saturate configured budgets. A valid credential can attempt unauthorized targets or abuse its allocated quota. DNS may change or mix allowed/forbidden addresses. A local application can submit oversized/bursty traffic. Diagnostics/native-library traces can accidentally leak headers or payloads. Client and server hosts/config/trust provisioning are trusted; compromise of either endpoint or theft of a scoped Bearer credential remains outside tunnel confidentiality guarantees. Server-to-destination traffic is ordinary UDP with separate application confidentiality. Availability during a volumetric link attack is not promised.

## Options considered

Provisioned Bearer credential versus mTLS/account service; CA+DNS validation versus verification bypass; deny-by-default numeric target policy versus DNS-name-only policy; structured sanitized metadata versus raw qlog/trace; controlled restart versus live-reload machinery.

## Decision

Use operator-provisioned random Bearer secrets through protected files and the standard Authorization header, SHA-256 canonical-text digests on server, constant-time matching, explicit scope and bounded identities/rules. Explicitly set verify_peer(true), provide CA bundle and connect(Some(server_name)); wait for verified handshake before credentials. No 0-RTT/skip-verify/keylog or secrets in URLs/argv. Deny target access by default, require host/port and all normalized resolved addresses to be permitted, pin one approved numeric address to a connected socket. No DNS call or relay allocation before authentication. Bounded stateless Retry, handshake/setup/admission budgets and maintained crypto primitives protect pre-auth state.

Separate logs/counters from packet processing through bounded nonblocking output. Only allowlisted fixed classes/fields are exported; do not serialize user headers/config/events with Debug. Sample QUIC stats and kernel overflow counters with correct scope. Optional qlog must parse/filter/reconstruct before disk, removing every H3 event and raw/sensitive fields; disable tracing fail closed on schema/filter failure. Secrets/payload sentinel tests apply at all verbosity levels and to partial qlog writes. Raw qlog is incompatible with FR-023 because upstream HEADERS records include full values.

Revocation/trust/policy update is controlled restart. Old-process exit plus replacement readiness is the completion point and closes affected sessions. No live-reload framework or admin service initially. Accepted A1 preserves mandatory cross-client routing/stale-state isolation and documents the inherent provenance ambiguity of opaque external UDP replies after identical tuple reuse/restart. It requires no fixed quarantine duration; any additional mitigation and its security/resource trade-offs belong in architectural design. Neither a documented information limit nor an early MVP permits weakened authentication, authorization, privacy or active-session isolation.

## Diagnostic and rejection corrections (R5)

Independent review R5: ordinary rate-limited logs alone cannot guarantee correlation for a session that closes before its next snapshot. Choose a fixed 256-entry/64 KiB terminal-summary ring, nonterminal failure summaries in bounded live status, and a 256 KiB status-output reservation inside the existing 1 MiB writer budget. Write terminal records before cleanup, independent of the ordinary event bucket; retire them only after acknowledged completed status output. Distinct generations remain distinguishable; summaries expose only opaque ids and fixed reason/code/time/count fields. The ring and all live/status formatting/bookkeeping fit the existing 2 MiB total diagnostic budget; no history service or new config subsystem is added.

The [diagnostics contract](../../specs/001-masque-udp-tunnel/contracts/diagnostics.md) specifies overflow/failed-write coverage gaps. A functioning collector and workload within bounded status capacity must correlate every SC-010 injection even when ordinary logs are suppressed; any lost required correlation makes that check failed/invalid. Deliberate overflow/blocked-sink robustness verifies finite memory and no packet-loop blocking without claiming complete coverage. Optional qlog filter work follows safe logs/status and the first usable E2E MVP.

Permanent client rejection is one fixed-size admission latch per immutable listener mapping until controlled restart; it survives flow cleanup/source-tuple changes/recovery, blocks new setup and leaves accepted unrelated flows usable. Server rejection remains per request under admission bounds; it does not create an unlimited denial cache. This resolves cleanup versus permanent denial without retaining all rejected sessions.

## Consequences and review obligations

A stolen token grants its allowed destinations until revocation; TLS and protected files are mandatory. Credential provisioning/permission errors fail closed. The native dependency graph and unsafe socket code require review; no current audit is inferred from a version pin. Apply quiche patched resource-limit releases, cap stream/header/windows and drain path events. Negative tests monitor forbidden targets for zero traffic, not just HTTP status. CI/release audit actual Rust/Go locks and preserve unresolved risks explicitly.

Evidence: [security/diagnostic research](../../specs/001-masque-udp-tunnel/research.md), [config contract](../../specs/001-masque-udp-tunnel/contracts/cli-config.md), [diagnostics contract](../../specs/001-masque-udp-tunnel/contracts/diagnostics.md), [TLS configuration](https://docs.rs/quiche/0.30.0/quiche/struct.Config.html), [Bearer security](https://www.rfc-editor.org/rfc/rfc6750.html).


Operator-output amendment: [ADR 0032](0032-operator-log-output.md) makes bounded
full correlation an explicitly selected JSON diagnostic profile. Default human
logs are not a complete per-request audit. This changes default output policy,
not authentication, authorization, privacy or JSON collection guarantees above.
