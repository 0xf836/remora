# Feature Specification: Remora MASQUE UDP Tunnel

**Feature Branch**: `001-masque-udp-tunnel`

**Created**: 2026-10-10

**Status**: Draft

**Input**: User description: "Build Remora, a lightweight client-server application for tunneling UDP traffic using standards-compliant MASQUE CONNECT-UDP over HTTP/3 and QUIC. The client listens on a local UDP port and forwards datagrams through an authenticated MASQUE connection to a server, which relays them to an authorized UDP endpoint and returns responses. The primary use case is WireGuard-over-MASQUE, but Remora must remain application-agnostic. Prioritize secure operation, high bidirectional throughput, minimal additional packet loss and latency, efficient resource usage, connection recovery, and comprehensive diagnostics. Support multiple concurrent sessions. The initial release targets Linux and focuses exclusively on CONNECT-UDP, without VPN management, TUN interfaces, GUI, or unrelated functionality."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Securely Relay an Existing UDP Application (Priority: P1)

A Linux operator starts a server with an explicitly permitted UDP destination and provisions
client credentials. A client operator configures a local listening address, that destination,
and the trusted server identity. An existing application sends datagrams to the local port and
receives responses through Remora without changes to its application protocol.

**Why this priority**: This is the essential product capability. Authentication and destination
authorization are part of the minimum usable product because a network-facing relay must not
become an open proxy.

**Independent Test**: Run a client and server with a controlled UDP responder; verify bidirectional
payloads and rejection cases. Then point an independently configured WireGuard peer at the local
port and verify traffic through its existing peer. Remora does not configure either peer.

**Acceptance Scenarios**:

1. **Given** valid client credentials, a trusted server, and an authorized reachable destination,
   **When** a local application sends supported-size datagrams, **Then** the destination receives
   unchanged payloads with their original datagram boundaries and responses reach the originating
   application (FR-001, FR-004, FR-005, FR-010).
2. **Given** missing, invalid, or revoked client credentials, **When** a tunnel is requested,
   **Then** it is rejected and no application datagrams are relayed (FR-006, FR-008).
3. **Given** a valid client but an unauthorized destination or an untrusted server identity,
   **When** the client attempts to establish a tunnel, **Then** forwarding is refused and an
   actionable reason is reported (FR-007, FR-008, FR-009).
4. **Given** independently configured WireGuard peers, **When** one uses Remora's local listener
   as its peer endpoint, **Then** its handshake and bidirectional application traffic succeed;
   an unrelated UDP responder also works without application-specific settings (FR-003).
5. **Given** an independently maintained standards-compatible peer with a compatible credential
   arrangement, **When** it communicates with either Remora role, **Then** supported sessions
   interoperate without Remora-specific wire extensions; incompatible capabilities are explicitly
   rejected (FR-002, FR-026).

---

### User Story 2 - Run Concurrent Sessions Without Interference (Priority: P1)

An operator serves multiple clients and local UDP senders concurrently. Each sender receives
only its own responses, including when several senders use the same destination.

**Why this priority**: Concurrent sessions are a stated initial-release requirement. Isolation
prevents information disclosure and incorrect delivery while making the relay useful to more
than one application or user.

**Independent Test**: Run distinguishable traffic from 32 sessions, including multiple senders on
one listener, multiple client processes, identical destinations, and different credentials.
Close, overload, and recreate individual sessions while checking the others.

**Acceptance Scenarios**:

1. **Given** multiple local source addresses and ports on a listener and multiple remote clients,
   **When** they send uniquely tagged datagrams to the same destination, **Then** every received
   response is delivered only to its originating sender (FR-010, FR-011).
2. **Given** active sessions, **When** one session closes or reaches its individual limit,
   **Then** its resources are reclaimed and other sessions remain usable within configured global
   limits (FR-012, FR-013).
3. **Given** configured admission, queue, and idle limits, **When** traffic exceeds them,
   **Then** resources remain bounded, admission or datagram drops are observable, and the process
   remains responsive (FR-012, FR-013, FR-021).
4. **Given** an expired session whose local source tuple is later reused, **When** delayed old
   responses arrive, **Then** those responses do not reach the new session (FR-010, FR-014).

---

### User Story 3 - Sustain Fast Bidirectional Traffic (Priority: P1)

An operator carries sustained application traffic in either direction and simultaneously in both
directions, with predictable overhead and no need to tune application-specific behavior.

**Why this priority**: WireGuard-over-MASQUE is useful only when the additional relay preserves
adequate throughput, latency, and datagram delivery under its documented operating conditions.

**Independent Test**: Compare direct and tunneled traffic on the reference workload in Assumptions,
using the same hosts, routing, destinations, offered loads, and payloads. Measure both directions
separately and simultaneously, including the 32-session case.

**Acceptance Scenarios**:

1. **Given** the reference environment, **When** sustained traffic is sent in each direction and
   both directions together, **Then** delivered throughput meets SC-003 and loss and latency at
   the specified subcapacity load meet SC-004 and SC-005 (FR-027).
2. **Given** bursts or a sender exceeding forwarding capacity, **When** buffers reach their limits,
   **Then** overload follows the documented drop policy without unbounded memory growth or
   indefinite queuing (FR-012, FR-015, FR-028).
3. **Given** payloads at, below, and above the effective size limit, including an empty payload,
   **When** they are submitted, **Then** supported payloads remain unchanged and unsupported
   payloads are dropped and counted without truncation or private fragmentation (FR-004, FR-016).
4. **Given** controlled delay, loss, or a changed path size limit, **When** traffic continues,
   **Then** Remora remains responsive, updates its operating state, and reports observable local
   drops without claiming that unobservable network loss was measured (FR-016, FR-021, FR-022).

---

### User Story 4 - Recover After Connectivity Interruptions (Priority: P2)

An operator can leave the client running through a temporary outage or server restart. New
application traffic resumes automatically once a valid path and authorized server are available.

**Why this priority**: Automatic recovery avoids manual intervention during routine outages while
retaining authentication, authorization, and UDP semantics.

**Independent Test**: Interrupt connectivity, restart the server, change the client's network
address, and revoke credentials. Observe recovery timing, local listener continuity, and absence
of replayed outage traffic.

**Acceptance Scenarios**:

1. **Given** an active tunnel, **When** its connection fails and service later becomes available,
   **Then** the client detects failure, reports recovery attempts, and resumes new datagrams within
   SC-006 without restarting the local application or changing its listening port (FR-017, FR-018).
2. **Given** a disconnected or recovering client, **When** applications continue sending,
   **Then** unavailable-period datagrams are discarded and counted rather than stored for replay
   after recovery (FR-014, FR-017).
3. **Given** credentials or destination permissions revoked before recovery, **When** the client
   reconnects, **Then** authorization is checked again and forwarding remains denied (FR-006,
   FR-009, FR-018).
4. **Given** repeated transient failures or a changed local network address, **When** recovery is
   attempted, **Then** attempts remain bounded and do not busy-loop, and identity validation is
   never bypassed; permanent configuration or authorization failures are reported distinctly
   (FR-007, FR-018, FR-019).

---

### User Story 5 - Operate and Diagnose the Tunnel (Priority: P2)

An operator installs and runs either role on Linux, checks its effective configuration and status,
and distinguishes authentication, destination, connection, size-limit, and resource problems
without inspecting application payloads.

**Why this priority**: Diagnosis is necessary to operate a public networking service and to
establish whether Remora adds loss, latency, or resource pressure.

**Independent Test**: Follow the operator documentation and inject each documented failure class.
Check status, diagnostics, counters, process behavior, and redaction with known test secrets.

**Acceptance Scenarios**:

1. **Given** a clean supported Linux environment and the documented prerequisites, **When** an
   operator follows the setup guide, **Then** both roles can start without elevated privileges
   when using unprivileged ports and exchange traffic; invalid configuration produces a clear
   error before forwarding (FR-020, FR-024, FR-025).
2. **Given** authentication rejection, forbidden destination, failed resolution, unavailable
   destination, connection loss, oversized datagram, or resource exhaustion, **When** the fault
   occurs, **Then** diagnostics identify the relevant cause and session when observable, while
   silent destination nonresponse is not reported as proven reachability or failure (FR-021,
   FR-022).
3. **Given** known credentials and sensitive payloads in test traffic, **When** normal or verbose
   diagnostics and status are collected, **Then** no credentials or payload contents are exposed,
   and log volume remains bounded under repeated faults (FR-023).
4. **Given** active or recovering sessions, **When** the operator stops either role, **Then** it
   stops accepting work and releases its listeners and session resources within SC-008 (FR-014,
   FR-024).
5. **Given** an operator replacing credentials or destination policy, **When** the documented
   update procedure is completed, **Then** removed credentials cannot establish new sessions and
   affected active sessions are closed, including when the procedure requires a restart (FR-006,
   FR-009, FR-025).
6. **Given** the initial-release setup guide and supported feature list, **When** an operator
   runs the tunnel, **Then** no VPN management, TUN interface, GUI, alternative proxy protocol,
   or reliable-transport fallback is needed or included (FR-029).

### Edge Cases

- Empty datagrams, mixed payload sizes, maximum supported size, and path-size reductions.
- Datagram arrival before acceptance, after rejection, during recovery, or after session closure.
- Duplicated or reordered network traffic; Remora preserves payloads but promises neither reliable
  delivery nor ordering and does not deliberately duplicate or retransmit application datagrams.
- Multiple local senders, shared destinations, idle expiry, source-port reuse, and delayed replies.
- Forged responses from a source other than the authorized destination for the session.
- Untrusted, expired, or mismatched server identity and missing, invalid, or revoked credentials.
- Forbidden addresses or ports, unauthorized loopback/private targets, and hostnames resolving to
  a different address after initial authorization, including mixed permitted and forbidden answers.
- Malformed protocol input, unsupported required capabilities, unknown extensions, and early-data
  replay attempts; each receives the standards-required response without unauthorized forwarding.
- Destination silence, resolution failure, refused or unreachable destination, and blocked network
  access; lack of a UDP response alone cannot prove an endpoint is down.
- Client network-address changes, server restart, prolonged outage, idle connection expiry, and
  permanently invalid configuration during recovery.
- Admission saturation, malicious session churn, bursts, queue pressure, and diagnostic overload.
- Invalid bindings, an already occupied port, insufficient permissions, and shutdown during setup.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Remora MUST provide client and server roles for Linux. The client MUST accept local
  UDP datagrams on an operator-configured address and port; the server MUST relay accepted
  datagrams to the session's authorized UDP destination and return its responses.
- **FR-002**: The tunnel MUST use standards-compliant MASQUE CONNECT-UDP over HTTP/3 and QUIC,
  conforming to RFC 9298, RFC 9297, RFC 9221, and their applicable HTTP/3, QUIC, and security
  dependencies. Required capability negotiation, session lifecycle, and error behavior MUST be
  covered by conformance verification. See the standards references below.
- **FR-003**: Remora MUST treat application payloads as opaque. WireGuard and an unrelated UDP
  application MUST work through the same forwarding behavior without application-specific logic.
- **FR-004**: Forwarding MUST preserve datagram boundaries and payload bytes, including empty
  payloads, for every delivered supported-size datagram. Remora MUST NOT alter, truncate,
  coalesce, deliberately duplicate, or retransmit application datagrams.
- **FR-005**: Application payload forwarding MUST use negotiated unreliable datagram delivery.
  Required control-protocol handling MUST remain compliant, but this release MUST reject peers
  lacking required datagram capabilities instead of silently using reliable payload transport.
- **FR-006**: The server MUST authenticate every session using operator-provisioned credentials
  before relaying traffic. Missing, invalid, and revoked credentials MUST be rejected. A
  documented credential replacement/revocation procedure MUST prevent further access, including
  terminating affected existing sessions when that procedure completes.
- **FR-007**: The client MUST validate the server's cryptographic identity against configured trust
  before releasing application payloads or authentication secrets. Initial-release operation MUST
  NOT provide a verification-bypass mode.
- **FR-008**: No application payload MUST be forwarded before both authentication and destination
  authorization succeed. Replayable early application data MUST NOT establish relay access or
  cause destination traffic.
- **FR-009**: Destination authorization MUST deny by default and bind each accepted session to an
  explicitly permitted address and port for its authenticated identity. Hostname resolution,
  changed addresses, address aliases, and reconnects MUST NOT bypass this policy. Loopback or
  private destinations MAY be used only when explicitly permitted. Removing permission through
  the documented update procedure MUST close affected sessions.
- **FR-010**: Responses MUST be associated with the correct local sender and session. Unrelated
  senders, authenticated clients, expired sessions, and recreated sessions MUST NOT receive one
  another's responses. Only responses from the session's authorized destination are eligible.
- **FR-011**: The server MUST support multiple client processes and multiple concurrent sessions
  per client. One local source-address/port pair on a configured listener and destination mapping
  MUST be isolated from other pairs; the initial acceptance scale is at least 32 active sessions.
- **FR-012**: Session counts, pending setup, queued datagrams/bytes, and idle retention MUST have
  documented finite limits, with operator-configurable session and queue budgets. Limit breaches
  MUST cause documented admission rejection or counted drops rather than unbounded growth.
- **FR-013**: A session's failure, closure, idle expiry, or individual overload MUST NOT terminate
  unrelated sessions. Within global budgets, a continuously active session MUST NOT prevent
  another admitted session from making progress.
- **FR-014**: Closed, expired, rejected, or stopped sessions MUST release their resources and
  discard pending datagrams. Data from an earlier session or failed connection MUST NOT be
  replayed into a later session.
- **FR-015**: Forwarding MUST preserve best-effort UDP behavior under congestion and overload,
  obey applicable transport congestion rules, and expose local drops. No reliable delivery or
  ordering guarantee MUST be implied to applications.
- **FR-016**: Both roles MUST respect the effective payload size and path limits. Unsupported-size
  datagrams MUST be dropped and counted without truncation, private fragmentation, or a switch
  to reliable payload delivery. Server forwarding MUST NOT introduce IP fragmentation.
- **FR-017**: The client MUST retain its configured local listener through transient tunnel
  outages, detect unusable connections, and automatically attempt recovery. Datagrams received
  while forwarding is unavailable MUST be dropped and counted, not queued for later replay.
- **FR-018**: Each recovered session MUST repeat server-identity validation, client authentication,
  and destination authorization. Repeated attempts MUST use bounded retry frequency and avoid
  synchronized retry storms; failures MUST never trigger weaker security or another transport.
- **FR-019**: Recovery MUST distinguish transient network or server failures from permanent
  configuration, identity, credential, capability, or policy failures. Permanent failures MUST
  be reported for operator action without an uncontrolled retry loop. A client network-address
  change MUST be handled through preserved connectivity or automatic re-establishment.
- **FR-020**: Client listeners MUST default to loopback. Non-loopback exposure MUST require an
  explicit operator setting. Both roles MUST support configured IPv4 and IPv6 endpoints and
  operate without elevated privileges when using unprivileged addresses and ports.
- **FR-021**: Each role MUST expose structured operational events and machine-readable counters
  for session lifecycle, authentication/policy rejection, transmitted and received datagrams and
  bytes by direction, recovery, observed local drop reasons, and resource-limit pressure.
- **FR-022**: Status and diagnostics MUST distinguish starting, active, recovering, rejected, and
  stopped sessions, with privacy-safe correlation identifiers. They MUST distinguish observed
  local drops from estimated end-to-end loss and MUST NOT equate accepted session setup with
  proven UDP destination responsiveness.
- **FR-023**: All diagnostic levels MUST redact credentials and omit application payload contents.
  High-volume events and correlation metadata MUST have bounded event rates, in-process state,
  and cardinality; sustained malformed traffic MUST NOT cause unbounded log rates or metric
  growth. External log storage and retention remain operator responsibilities.
- **FR-024**: Invalid configuration MUST fail before forwarding with an actionable error. Stopping
  either role MUST complete within the shutdown criterion and release all owned listeners and
  session resources; setup and recovery MUST also be cancellable.
- **FR-025**: Operator documentation MUST cover installation, configuration, secure credential and
  trust provisioning, target authorization, updates/revocation, payload limits, session/resource
  limits, troubleshooting, and a reproducible WireGuard example using independently managed
  peers. Secrets MUST NOT be required in command-line arguments or destination URLs.
- **FR-026**: Verification MUST cover conformance, independent interoperability for both roles,
  bidirectional forwarding, concurrency, negative security cases, malformed input, recovery, and
  resource bounds. If a suitable independent peer is unavailable, that limitation and alternative
  evidence MUST be documented; self-interoperation alone MUST NOT be claimed as independent proof.
- **FR-027**: Performance evidence MUST compare direct and tunneled traffic in each direction and
  simultaneously, using the workload and methodology below. Results MUST include throughput,
  latency, delivered loss, CPU, and memory, and explain variance and test limitations.
- **FR-028**: Sustained operation and session churn MUST meet the stated resource budgets without
  continuous growth. Defaults and limit behavior MUST be usable without per-application tuning.
- **FR-029**: Initial-release scope MUST be limited to the UDP tunnel, its operation, documentation,
  and verification. CONNECT-IP, TUN interfaces, routing/VPN management, WireGuard configuration,
  GUI, HTTP/1.1 or HTTP/2 transport fallback, and unrelated proxy features are excluded.

Standards references: [RFC 9298: Proxying UDP in HTTP](https://www.rfc-editor.org/rfc/rfc9298.html),
[RFC 9297: HTTP Datagrams and the Capsule Protocol](https://www.rfc-editor.org/rfc/rfc9297.html),
and [RFC 9221: An Unreliable Datagram Extension to QUIC](https://www.rfc-editor.org/rfc/rfc9221.html).
FR-002, FR-005, FR-015, and FR-016 define the supported standards profile; limiting transports
and payload delivery does not waive required control handling or standards-defined errors.

### Key Entities *(include if feature involves data)*

- **Operator**: Person responsible for client/server configuration, trust, credentials, destination
  permissions, operational limits, and diagnosis.
- **Client identity and credential**: Provisioned proof of permission to request sessions, with
  associated allowed destinations and a documented revocation lifecycle.
- **Listener mapping**: Local listening address/port, chosen server and trust, client identity, and
  destination for incoming application traffic. Multiple mappings may use separate client processes.
- **Tunnel session**: An isolated association between a local UDP sender, authenticated identity,
  authorized destination, lifecycle state, operating limits, and directional counters.
- **Authorized destination**: A configured host/address and UDP port with permitted resolved
  addresses; a session MUST retain its approved destination association.
- **Datagram**: An opaque application payload and its boundary, direction, originating session,
  and eligibility for forwarding under current size and lifecycle limits.
- **Diagnostic record**: A bounded, privacy-safe event or counter describing observed operation;
  it contains neither authentication secrets nor application payload contents.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Both a generic UDP responder and independently configured WireGuard peers exchange
  bidirectional traffic through Remora for 30 minutes without payload corruption, cross-session
  delivery, or manual client restart. All defined functional acceptance scenarios pass.
- **SC-002**: In the security acceptance matrix, 100% of missing/invalid/revoked-credential,
  untrusted-server, forbidden-destination, changed-resolution, and early-data cases produce zero
  unauthorized application datagrams at monitored destinations. Collected diagnostics contain
  zero occurrences of supplied test secrets or payload contents.
- **SC-003**: For 1,200-byte payloads on the reference workload, sustained delivered payload
  throughput is at least 80% of the matching direct baseline in each direction, tested separately
  and simultaneously, for both one session and 32 sessions in aggregate. Neither simultaneous
  direction may be hidden by reporting only their sum.
- **SC-004**: At the reference subcapacity load, the tunneled loss rate exceeds the matching direct
  baseline by no more than 0.1 percentage points in either direction, including simultaneous
  traffic, for 128-, 512-, and 1,200-byte payloads.
- **SC-005**: At that same load, the increase in end-to-end p95 round-trip latency is at most 5 ms
  over the matching direct baseline for each payload size. One-way latency is reported only when
  clock accuracy is independently established.
- **SC-006**: A broken connection is detected within 15 seconds in the recovery acceptance profile.
  After the server and network are continuously available again, new datagrams resume within
  10 seconds without changing the local listener, weakening authorization, or replaying outage
  datagrams. Credential, identity, and policy rejection cases remain denied.
- **SC-007**: With 32 active sessions carrying an aggregate 500 Mbit/s in each direction of
  1,200-byte payloads, each role uses at most 128 MiB resident memory and an average of two
  logical CPU cores during the reference measurement interval. After 1,000 sequential session
  creation/closure cycles, retained memory is no more than 10 MiB above the post-warm-up baseline
  after the documented reclamation interval, capped at 60 seconds.
- **SC-008**: Each role stops and releases its listeners and sessions within 5 seconds of an
  operator stop request in active, starting, and recovering states.
- **SC-009**: At least 32 sessions remain isolated during a 30-minute test with zero observed
  cross-deliveries. Under 10 minutes of saturated admission, queue pressure, and malformed input,
  configured resource ceilings hold and at least 99% of requests from each admitted low-rate
  probe session receive a response within 1 second while the reference path and destination
  remain responsive. Adversarial tests saturate Remora's configured budgets without exhausting
  external link capacity; they do not promise availability during a volumetric network attack.
- **SC-010**: For every injected observable failure in User Story 5, the operator can identify the
  documented failure class and affected session from supplied diagnostics alone. A documented
  clean-install walkthrough allows an operator to establish an authorized bidirectional session
  without modifying either application or requiring elevated runtime privileges.

## Assumptions

- The initial users are Linux operators comfortable with noninteractive command-line tools and
  configuration files. Exact flags, authentication mechanism, configuration format, libraries,
  process topology, and connection reuse are planning decisions, not specification commitments.
- Operators provision server trust and per-client credentials out of band. This release needs no
  account service, interactive login, credential administration UI, or automatic certificate
  issuance. Revocation and policy updates may require a controlled restart; live reload is not
  required, but the documented completion point MUST be unambiguous.
- One listener mapping selects one configured destination; different mappings may run in separate
  client processes. Each distinct local source-address/port pair forms an isolated session.
  Multiplexing sessions inside one connection is not a requirement on implementation structure.
- Operators manage WireGuard, other applications, routing, firewall rules, and target service
  access separately. Connectivity between client and server and between server and destination
  is an external prerequisite. Remora cannot make a blocked network path available.
- Protection covers the client-to-server tunnel and authenticated relay access. The server-to-UDP
  destination leg and the application's own confidentiality are separate trust considerations;
  Remora does not claim end-to-end application encryption beyond the tunnel.
- Payload limits depend on the negotiated connection and path. A 1,200-byte application payload
  is a reference-test workload, not a guarantee on every path; test paths MUST accommodate it
  including encapsulation overhead. Application MTU adjustments are documented operator actions.
- Numeric performance, scale, resource, and recovery targets are initial acceptance assumptions
  derived for this specification, not user-supplied guarantees or measured results. Planning
  MUST confirm their feasibility on a recorded reference environment or propose an explicit
  specification amendment before implementation/acceptance measurements; targets MUST NOT be
  relaxed retrospectively to fit results.
- The reference environment uses Linux hosts with a recorded CPU model, OS version, configuration,
  and at least four available logical CPU cores per role; a full-duplex 1 Gbit/s bottleneck;
  20 ms client-to-server round-trip delay; an uncongested server-to-destination leg; and no
  injected loss for pass/fail performance measurements. The direct baseline is an unencrypted
  UDP relay across the same two-leg topology without the tunnel, not a shorter route.
- Throughput baselines are established separately for 128-, 512-, and 1,200-byte payloads and
  one/32 sessions. A baseline's sustainable rate is its highest delivered rate with at most
  0.1% loss. Simultaneous runs use matching simultaneous baselines, recording each direction.
  Loss/latency tests offer 50% of the corresponding direct sustainable rate in each direction;
  if Remora cannot sustain this offered load, the loss/latency criteria still apply.
- Performance comparisons use at least five paired runs, each with a 60-second warm-up and a
  five-minute measured interval, stable routing, and identical diagnostic settings. Each run
  MUST meet the thresholds; reports include spread and sample counts. Probe traffic measures
  round-trip latency during load without materially changing it. The resource test records its
  own matching 500 Mbit/s-per-direction load; overload probes use 128-byte request/response
  payloads at one request per second per admitted probe session.
- Impairment characterization additionally covers 50/100 ms round-trip delays, 0.1%/1% injected
  loss, asymmetric rates, bursts, and reduced path sizes. These cases require documented behavior
  and results; reference throughput thresholds are not promises for every impaired network.
- The recovery profile consists of a single 10-second traffic blackhole or server restart,
  followed by a stable reference path with valid credentials and reachable destination. Timing
  starts at failure onset for detection and at restored server/network availability for recovery.
  Longer outages still require bounded retries, not indefinite buffering or a fixed global deadline.
- External prerequisites include a permitted UDP destination, trusted server identity material,
  operator-provisioned credentials, controllable test paths, independently managed WireGuard peers,
  and an independent CONNECT-UDP peer when available. Test suites and architectural decisions
  follow the project constitution; this document creates no implementation or deployment work.
