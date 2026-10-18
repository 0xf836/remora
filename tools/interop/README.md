# Independent CONNECT-UDP peer

Test-only Go fixture using masque-go v0.6.0 (tag resolves to
`c1cf0e4dd6439aea94d5491b27439a4736f00246`) and quic-go v0.63.0.
Go 1.27.2, `go.mod` and `go.sum` pin the build inputs. Build separately:

```bash
cd tools/interop
go build -mod=readonly -o /tmp/remora-interop .
go vet ./...
```

Pass only a private fixture JSON path via `--config`. The
[runner](../../scripts/validate/interop.sh) provisions this JSON and secrets
inside its private temporary directory, never putting tokens in argv.
The client dials a numeric loopback address through `Transport.DialAddr`,
retaining the generated CA pool and DNS ServerName. Positive tests reuse one
independent H3 connection with two request streams. The server checks canonical
Bearer-token SHA-256 and an exact numeric target before allocating a distinct
connected socket. Wire framing comes from the independent libraries.

This is a local compatibility fixture, not a hardened alternate production
server or a substitute for Remora's DNS/resource policies. Third-party reason
logs are discarded; retained reports contain fixed outcomes. See
[interop evidence](../../docs/validation/interop.md) for executed cases,
limitations and the preserved failure that led to a cleanup regression fix.
