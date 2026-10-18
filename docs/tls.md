# TLS certificates and trust

The client verifies the server's certificate chain and checks that the
certificate covers the host in `client.proxy`. Keep the same public `proxy` on
both roles. For `https://remora.example.com`, the certificate needs a DNS subject
alternative name (SAN) that covers `remora.example.com`. For a literal IP, it
needs an IP SAN. A fixed [`connect_address`](configuration.md#proxy-name-and-connection-address)
changes where the client connects; the certificate must still cover the host
in `proxy`.

Remora reads certificates at startup. It does not obtain certificates or reload
them automatically, and has no skip-verification option.

## Use an existing publicly trusted certificate

1. Obtain a server certificate using your existing CA/ACME tooling. Provision
   the full chain (leaf first, then intermediates) as `server.pem` and its
   unencrypted PEM private key as `server-key.pem` on the server.
2. On the client, provision a PEM CA bundle as `ca.pem`. Use the appropriate
   public root CA or an up-to-date CA bundle from your operating system.
   `ca_file` is mandatory: Remora trusts **only that file**, without implicitly
   adding system roots. Copy the actual bundle into the config directory as a
   regular file, rather than pointing through a distro symlink.
3. Ensure the certificate SAN matches the host in `proxy`, its validity period
   includes the current time and the server supplies the intermediates needed
   to reach a trusted root.

For an existing renewal directory, copy resolved files through your provisioning
tool into a protected configuration directory. Remora rejects symlinks in any
path component, including common ACME `live/` paths. Keep the server key private
(mode `0600`); client CA and server chain need integrity protection but contain
no secret. Validate the candidate and restart the server after renewal. Update
and restart clients if the trusted CA changes. Do not copy the server private
key to a client.

## Create a private CA

For a private deployment, run these commands in a **fresh provisioning directory**
on a trusted machine. Replace `remora.example.com` with the name used in `proxy`.
The client must be able to resolve it, or use an explicit `connect_address`.

```bash
umask 077
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
  -keyout ca-key.pem -out ca.pem -days 3650 \
  -subj '/CN=Remora private CA' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign'
openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
  -keyout server-key.pem -out server.csr -subj '/CN=remora.example.com'
cat > server.ext <<'EXT'
subjectAltName=DNS:remora.example.com
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth
EXT
openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem \
  -CAcreateserial -out server.pem -days 90 -sha256 -extfile server.ext
openssl verify -CAfile ca.pem -verify_hostname remora.example.com server.pem
```

If `proxy` uses a literal IP, use `subjectAltName=IP:203.0.113.10` instead and
verify with `openssl verify -CAfile ca.pem -verify_ip 203.0.113.10 server.pem`.
A CN alone does not replace the required SAN.

Transfer only `server.pem` and `server-key.pem` to the server, and `ca.pem` to
the client, using an authenticated channel. Keep `ca-key.pem` on the trusted
provisioning machine; neither Remora role needs it. Renew the leaf before its
90-day expiry, atomically install the replacement and restart. The private CA
must be provisioned deliberately on every client; it is not publicly trusted.

`check-config` verifies PEM syntax and server key matching. It does not perform
a handshake or establish that a live peer's identity and chain are trusted.
