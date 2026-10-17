//! Immutable schema-v1 startup settings. No file contents enter error messages.
//!
//! Files are read through owned directory descriptors, rejecting symlinks at
//! every component. Provisioning must protect the directory and use atomic
//! replacement; a privileged owner can still edit an already-open inode.

use boring::{pkey::PKey, x509::X509};
use rustix::{
    fs::{Mode, OFlags, openat},
    process::{Resource, geteuid, getrlimit},
};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fmt,
    fs::File,
    io::Read,
    net::{IpAddr, SocketAddr},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

/// Fixed, privacy-safe startup failure. Neither paths nor parser input are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigError {
    field: &'static str,
    pub reason: ConfigReason,
}
impl ConfigError {
    pub fn field(&self) -> &'static str {
        self.field
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigReason {
    Schema,
    Range,
    Arithmetic,
    File,
    Permissions,
    Format,
    Role,
    FdBudget,
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid configuration: {} ({:?})",
            self.field, self.reason
        )
    }
}
impl std::error::Error for ConfigError {}
fn error(field: &'static str, reason: ConfigReason) -> ConfigError {
    ConfigError { field, reason }
}

macro_rules! settings {
    ($name:ident { $($field:ident: $ty:ty = $value:expr),* $(,)? }) => {
        #[derive(Clone, Debug, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),* }
        impl Default for $name { fn default() -> Self { Self { $($field: $value),* } } }
    };
}
macro_rules! choices {
    ($name:ident { $($variant:ident => $text:literal),* }) => {
        #[derive(Clone, Copy, Debug, Deserialize, serde::Serialize, PartialEq, Eq)]
        pub enum $name { $(#[serde(rename=$text)] $variant),* }
    };
}
choices!(IoMode { Mmsg => "mmsg", Single => "single" });
choices!(Offload { Off => "off", Auto => "auto" });
choices!(CongestionControl { Cubic => "cubic", Bbr2Gcongestion => "bbr2_gcongestion", Reno => "reno" });
choices!(Pacing { Timerfd => "timerfd" });
choices!(Level { Error => "error", Warn => "warn", Info => "info", Debug => "debug", Trace => "trace" });
settings!(Limits {
    max_connections: usize = 64,
    max_sessions: usize = 128,
    max_sessions_per_identity: usize = 32,
    max_sessions_per_connection: usize = 32,
    max_pending_setup: usize = 16,
    max_relay_sockets: usize = 256,
    session_queue_packets: usize = 32,
    session_queue_bytes: usize = 65536,
    application_pool_bytes: usize = 8388608,
    ciphertext_pool_bytes: usize = 4194304,
    scratch_pool_bytes: usize = 2097152,
    quiche_dgram_packets: usize = 64,
    connection_dgram_bytes: usize = 262144,
    idle_session_ms: u64 = 120000,
    application_residence_ms: u64 = 20,
    ciphertext_residence_ms: u64 = 100,
});
settings!(IoSettings {
    mode: IoMode = IoMode::Mmsg,
    batch_max: usize = 32,
    gso: Offload = Offload::Off,
    gro: Offload = Offload::Off,
    outer_socket_buffer_bytes: usize = 1048576,
    relay_socket_buffer_bytes: usize = 131072,
});
settings!(TransportSettings {
    cc: CongestionControl = CongestionControl::Cubic,
    pacing: Pacing = Pacing::Timerfd,
    outer_udp_max_bytes: usize = 1350,
    application_max_bytes: usize = 1200,
    idle_timeout_ms: u64 = 5000,
});
settings!(RecoverySettings {
    keepalive_interval_ms: u64 = 2000,
    peer_progress_timeout_ms: u64 = 10000,
    setup_timeout_ms: u64 = 5000,
    retry_initial_ms: u64 = 100,
    retry_max_ms: u64 = 2000,
});
settings!(DiagnosticSettings {
    level: Level = Level::Info, snapshot_interval_ms: u64 = 1000,
    qlog_directory: Option<PathBuf> = None,
});
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    client: Option<ClientDocument>,
    server: Option<ServerDocument>,
    #[serde(default)]
    limits: Limits,
    #[serde(default)]
    io: IoSettings,
    #[serde(default)]
    transport: TransportSettings,
    #[serde(default)]
    recovery: RecoverySettings,
    #[serde(default)]
    diagnostics: DiagnosticSettings,
}
fn parse_document(bytes: &[u8]) -> Result<Document, ConfigError> {
    let text = std::str::from_utf8(bytes).map_err(|_| error("config", ConfigReason::Schema))?;
    let doc: Document = toml::from_str(text).map_err(|_| error("config", ConfigReason::Schema))?;
    if doc.schema_version != 1 {
        return Err(error("schema_version", ConfigReason::Schema));
    }
    Ok(doc)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientDocument {
    listen: SocketAddr,
    #[serde(default)]
    allow_non_loopback: bool,
    server_address: SocketAddr,
    server_name: String,
    proxy_authority: String,
    uri_template: String,
    ca_file: PathBuf,
    token_file: PathBuf,
    target_host: String,
    target_port: u16,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerDocument {
    listen: Vec<SocketAddr>,
    proxy_authority: String,
    path_prefix: String,
    certificate_file: PathBuf,
    private_key_file: PathBuf,
    credentials_file: PathBuf,
}
/// Deliberately has no Debug/Serialize implementation.
pub struct Token([u8; 64]);
impl Token {
    /// Canonical ASCII text, for the authenticated protocol boundary only.
    pub fn canonical_text(&self) -> &[u8; 64] {
        &self.0
    }
}
pub struct ClientConfig {
    pub listen: SocketAddr,
    pub server_address: SocketAddr,
    pub server_name: String,
    pub proxy_authority: String,
    pub uri_template: String,
    pub target_host: String,
    pub target_port: u16,
    pub ca_pem: Vec<u8>,
    pub token: Token,
}
pub struct ServerConfig {
    pub listen: Vec<SocketAddr>,
    pub proxy_authority: String,
    pub path_prefix: String,
    pub certificate_pem: Vec<u8>,
    pub private_key_pem: Vec<u8>,
    pub identities: Vec<Identity>,
}
/// Loaded material is owned by this immutable startup generation, never reloaded.
pub enum RoleConfig {
    Client(ClientConfig),
    Server(ServerConfig),
}
pub struct ValidatedConfig {
    role: RoleConfig,
    limits: Limits,
    io: IoSettings,
    transport: TransportSettings,
    recovery: RecoverySettings,
    diagnostics: DiagnosticSettings,
    required_fds: usize,
}
impl ValidatedConfig {
    /// Validate without binding sockets, resolving DNS, or contacting any peer.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let fd_limit = getrlimit(Resource::Nofile).current.unwrap_or(u64::MAX);
        Self::load_with_fd_limit(path, fd_limit)
    }
    /// Like load, with a caller-supplied *stricter* FD budget for deterministic tests.
    pub fn load_with_fd_limit(path: &Path, fd_limit: u64) -> Result<Self, ConfigError> {
        let bytes = read_file(path, false, false, 256 * 1024, "config")?;
        let mut doc = parse_document(&bytes)?;
        let parent = path.parent().unwrap_or(Path::new("."));
        if let Some(ref mut qlog) = doc.diagnostics.qlog_directory {
            *qlog = parent.join(&*qlog);
            if !cfg!(feature = "qlog") {
                return Err(error("qlog_directory", ConfigReason::Range));
            }
            crate::diagnostics::qlog::open_directory(qlog)
                .map_err(|_| error("qlog_directory", ConfigReason::Permissions))?;
        }
        let role = match (doc.client.take(), doc.server.take()) {
            (Some(c), None) => RoleConfig::Client(load_client(c, parent)?),
            (None, Some(s)) => RoleConfig::Server(load_server(s, parent)?),
            _ => return Err(error("role", ConfigReason::Role)),
        };
        let listeners = match &role {
            RoleConfig::Client(_) => 1,
            RoleConfig::Server(s) => s.listen.len(),
        };
        let required_fds = validate_budgets(&doc, listeners)?;
        let actual = getrlimit(Resource::Nofile).current.unwrap_or(u64::MAX);
        if u64::try_from(required_fds).map_err(|_| error("fds", ConfigReason::Arithmetic))?
            > fd_limit.min(actual)
        {
            return Err(error("fds", ConfigReason::FdBudget));
        }
        Ok(Self {
            role,
            limits: doc.limits,
            io: doc.io,
            transport: doc.transport,
            recovery: doc.recovery,
            diagnostics: doc.diagnostics,
            required_fds,
        })
    }
    pub fn role(&self) -> &RoleConfig {
        &self.role
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    pub fn io(&self) -> &IoSettings {
        &self.io
    }
    pub fn transport(&self) -> &TransportSettings {
        &self.transport
    }
    pub fn recovery(&self) -> &RecoverySettings {
        &self.recovery
    }
    pub fn diagnostics(&self) -> &DiagnosticSettings {
        &self.diagnostics
    }
    pub fn required_fds(&self) -> usize {
        self.required_fds
    }
}

fn read_file(
    path: &Path,
    private: bool,
    root_ok: bool,
    maximum: usize,
    field: &'static str,
) -> Result<Vec<u8>, ConfigError> {
    // Each openat is relative to a held descriptor: renames cannot redirect a
    // later component through a substituted symlink. O_NONBLOCK avoids a FIFO
    // hanging startup before its regular-file check. No raw OS error escapes.
    let fail = || error(field, ConfigReason::File);
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(|_| fail())?.join(path)
    };
    let mut components = absolute
        .components()
        .filter(|c| !matches!(c, Component::RootDir | Component::CurDir))
        .peekable();
    let mut current = File::open("/").map_err(|_| fail())?;
    while let Some(component) = components.next() {
        let flags = OFlags::RDONLY
            | OFlags::CLOEXEC
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | if components.peek().is_some() {
                OFlags::DIRECTORY
            } else {
                OFlags::empty()
            };
        current = File::from(
            openat(&current, component.as_os_str(), flags, Mode::empty()).map_err(|_| fail())?,
        );
    }
    let metadata = current.metadata().map_err(|_| fail())?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(fail());
    }
    if private
        && !private_owner_allowed(metadata.uid(), metadata.mode(), geteuid().as_raw(), root_ok)
    {
        return Err(error(field, ConfigReason::Permissions));
    }
    let mut bytes = Vec::new();
    current
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| fail())?;
    if bytes.len() > maximum {
        return Err(fail());
    }
    Ok(bytes)
}
fn private_owner_allowed(owner: u32, mode: u32, executing_uid: u32, root_ok: bool) -> bool {
    (owner == executing_uid || (root_ok && owner == 0)) && mode & 0o077 == 0
}
fn load_client(c: ClientDocument, parent: &Path) -> Result<ClientConfig, ConfigError> {
    if (!c.listen.ip().is_loopback() && !c.allow_non_loopback)
        || c.listen.port() == 0
        || c.server_address.port() == 0
        || c.server_address.ip().is_unspecified()
        || c.server_address.ip().is_multicast()
    {
        return Err(error("client.address", ConfigReason::Range));
    }
    let server_name = canonical_host(&c.server_name)?;
    let (authority_host, _) = authority(&c.proxy_authority)?;
    if authority_host != server_name {
        return Err(error("proxy_authority", ConfigReason::Format));
    }
    crate::masque::UriTemplate::parse(&c.uri_template, &c.proxy_authority)
        .map_err(|_| error("uri_template", ConfigReason::Format))?;
    let ca_pem = read_file(
        &parent.join(c.ca_file),
        false,
        false,
        1024 * 1024,
        "ca_file",
    )?;
    parse_certificates(&ca_pem, "ca_file")?;
    let bytes = read_file(&parent.join(c.token_file), true, false, 65, "token_file")?;
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let token: [u8; 64] = bytes
        .try_into()
        .map_err(|_| error("token_file", ConfigReason::Format))?;
    if !lower_hex(&token) {
        return Err(error("token_file", ConfigReason::Format));
    }
    let target_host = canonical_host(&c.target_host)?;
    validate_target(&target_host, c.target_port)?;
    Ok(ClientConfig {
        listen: c.listen,
        server_address: c.server_address,
        server_name,
        proxy_authority: c.proxy_authority,
        uri_template: c.uri_template,
        target_host,
        target_port: c.target_port,
        ca_pem,
        token: Token(token),
    })
}
fn parse_certificates(bytes: &[u8], field: &'static str) -> Result<Vec<X509>, ConfigError> {
    let certs = X509::stack_from_pem(bytes).map_err(|_| error(field, ConfigReason::Format))?;
    if certs.is_empty() {
        return Err(error(field, ConfigReason::Format));
    }
    Ok(certs)
}
fn load_server(s: ServerDocument, parent: &Path) -> Result<ServerConfig, ConfigError> {
    let mut unique = HashSet::new();
    if s.listen.is_empty()
        || s.listen.len() > 16
        || s.listen
            .iter()
            .any(|addr| addr.port() == 0 || !unique.insert(*addr))
    {
        return Err(error("server.listen", ConfigReason::Range));
    }
    authority(&s.proxy_authority)?;
    if !s.path_prefix.starts_with('/')
        || !s.path_prefix.is_ascii()
        || s.path_prefix
            .bytes()
            .any(|b| b.is_ascii_control() || b == b' ' || b == b'\\')
        || !s.path_prefix.ends_with('/')
        || s.path_prefix.contains(['?', '#', '{', '}', '\r', '\n'])
    {
        return Err(error("path_prefix", ConfigReason::Format));
    }
    let certificate_pem = read_file(
        &parent.join(s.certificate_file),
        false,
        true,
        1024 * 1024,
        "certificate_file",
    )?;
    let private_key_pem = read_file(
        &parent.join(s.private_key_file),
        true,
        true,
        1024 * 1024,
        "private_key_file",
    )?;
    let certs = parse_certificates(&certificate_pem, "certificate_file")?;
    // Zero avoids prompting but is a valid empty password to BoringSSL. Reject
    // any invocation, including a successfully decrypted empty-password key.
    let mut requested_password = false;
    let key = PKey::private_key_from_pem_callback(&private_key_pem, |_| {
        requested_password = true;
        Ok(0)
    })
    .map_err(|_| error("private_key_file", ConfigReason::Format))?;
    if requested_password {
        return Err(error("private_key_file", ConfigReason::Format));
    }
    if !certs[0]
        .public_key()
        .map_err(|_| error("certificate_file", ConfigReason::Format))?
        .public_eq(&key)
    {
        return Err(error("private_key_file", ConfigReason::Format));
    }
    let credentials = read_file(
        &parent.join(s.credentials_file),
        true,
        true,
        4 * 1024 * 1024,
        "credentials_file",
    )?;
    let identities = parse_credentials(&credentials)?;
    Ok(ServerConfig {
        listen: s.listen,
        proxy_authority: s.proxy_authority,
        path_prefix: s.path_prefix,
        certificate_pem,
        private_key_pem,
        identities,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    identities: Vec<IdentityDocument>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityDocument {
    id: String,
    token_sha256: String,
    targets: Vec<TargetDocument>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetDocument {
    host: String,
    port: u16,
    addresses: Vec<String>,
}
pub struct Identity {
    pub id: String,
    pub token_sha256: [u8; 32],
    pub targets: Vec<TargetRule>,
}
pub struct TargetRule {
    pub host: String,
    pub port: u16,
    pub addresses: Vec<AddressRange>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressRange {
    address: IpAddr,
    prefix: u8,
}
impl AddressRange {
    pub fn address(&self) -> IpAddr {
        self.address
    }
    pub fn prefix(&self) -> u8 {
        self.prefix
    }
    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.address, normalize_ip(address)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                self.prefix == 0
                    || (u32::from(net) ^ u32::from(ip)) >> (32 - u32::from(self.prefix)) == 0
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                self.prefix == 0
                    || (u128::from(net) ^ u128::from(ip)) >> (128 - u32::from(self.prefix)) == 0
            }
            _ => false,
        }
    }
}
fn parse_credentials(bytes: &[u8]) -> Result<Vec<Identity>, ConfigError> {
    let fail = || error("credentials_file", ConfigReason::Format);
    let text = std::str::from_utf8(bytes).map_err(|_| fail())?;
    let doc: Credentials = toml::from_str(text).map_err(|_| fail())?;
    if doc.identities.is_empty() || doc.identities.len() > 128 {
        return Err(fail());
    }
    let mut ids = HashSet::new();
    let mut digests = HashSet::new();
    let mut result = Vec::new();
    for identity in doc.identities {
        if identity.id.is_empty()
            || identity.id.len() > 64
            || !identity
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
            || !ids.insert(identity.id.clone())
            || identity.token_sha256.len() != 64
            || !lower_hex(identity.token_sha256.as_bytes())
            || !digests.insert(identity.token_sha256.clone())
            || identity.targets.is_empty()
            || identity.targets.len() > 128
        {
            return Err(fail());
        }
        let mut digest = [0; 32];
        for (slot, pair) in digest
            .iter_mut()
            .zip(identity.token_sha256.as_bytes().as_chunks::<2>().0)
        {
            let nibble = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
            *slot = nibble(pair[0]) * 16 + nibble(pair[1]);
        }
        let mut targets = Vec::new();
        for target in identity.targets {
            let host = canonical_host(&target.host)?;
            validate_target(&host, target.port)?;
            if target.addresses.is_empty() || target.addresses.len() > 128 {
                return Err(fail());
            }
            let mut addresses = Vec::new();
            for raw in target.addresses {
                let (ip, prefix) = raw.split_once('/').ok_or_else(fail)?;
                let original: IpAddr = ip.parse().map_err(|_| fail())?;
                let mut prefix: u8 = prefix.parse().map_err(|_| fail())?;
                if prefix > if original.is_ipv4() { 32 } else { 128 } {
                    return Err(fail());
                }
                let address = normalize_ip(original);
                if original.is_ipv6() && address.is_ipv4() {
                    prefix = prefix.checked_sub(96).ok_or_else(fail)?;
                }
                addresses.push(AddressRange { address, prefix });
            }
            if let Ok(ip) = host.parse()
                && !addresses.iter().any(|a| a.contains(ip))
            {
                return Err(fail());
            }
            targets.push(TargetRule {
                host,
                port: target.port,
                addresses,
            });
        }
        result.push(Identity {
            id: identity.id,
            token_sha256: digest,
            targets,
        });
    }
    Ok(result)
}
fn lower_hex(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}
pub fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        _ => ip,
    }
}
/// Static canonicalization only: DNS resolution/authorization belongs to policy.rs.
pub fn canonical_host(host: &str) -> Result<String, ConfigError> {
    let fail = || error("host", ConfigReason::Format);
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(normalize_ip(ip).to_string());
    }
    if host.contains([':', '%'])
        || !host.is_ascii()
        || host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(fail());
    }
    let host = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(fail());
    }
    // Reject common nonstandard numeric encodings rather than treating them as DNS.
    if host.split('.').all(|label| {
        label.bytes().all(|b| b.is_ascii_digit())
            || label.strip_prefix("0x").is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit())
            })
    }) {
        return Err(fail());
    }
    Ok(host)
}
fn validate_target(host: &str, port: u16) -> Result<(), ConfigError> {
    if port == 0 {
        return Err(error("target_port", ConfigReason::Range));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && (ip.is_unspecified()
            || ip.is_multicast()
            || matches!(ip, IpAddr::V4(v) if v.is_broadcast())
            || matches!(ip, IpAddr::V6(v) if v.is_unicast_link_local()))
    {
        return Err(error("target_host", ConfigReason::Range));
    }
    Ok(())
}
fn authority(value: &str) -> Result<(String, u16), ConfigError> {
    let fail = || error("proxy_authority", ConfigReason::Format);
    let (host, port) = if let Some(bracketed) = value.strip_prefix('[') {
        let (host, tail) = bracketed.split_once(']').ok_or_else(fail)?;
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(fail());
        }
        (
            host,
            if tail.is_empty() {
                443
            } else {
                tail.strip_prefix(':')
                    .ok_or_else(fail)?
                    .parse()
                    .map_err(|_| fail())?
            },
        )
    } else if let Some((host, port)) = value.split_once(':') {
        (host, port.parse().map_err(|_| fail())?)
    } else {
        (value, 443)
    };
    if port == 0 {
        return Err(fail());
    }
    Ok((canonical_host(host)?, port))
}
fn validate_budgets(doc: &Document, listeners: usize) -> Result<usize, ConfigError> {
    let l = &doc.limits;
    let range = || error("budgets", ConfigReason::Range);
    let arithmetic = || error("budgets", ConfigReason::Arithmetic);
    for value in [
        l.max_connections,
        l.max_sessions,
        l.max_sessions_per_identity,
        l.max_sessions_per_connection,
        l.max_pending_setup,
        l.max_relay_sockets,
        l.session_queue_packets,
        l.session_queue_bytes,
        l.application_pool_bytes,
        l.ciphertext_pool_bytes,
        l.scratch_pool_bytes,
        l.quiche_dgram_packets,
        l.connection_dgram_bytes,
        doc.io.outer_socket_buffer_bytes,
        doc.io.relay_socket_buffer_bytes,
        doc.transport.application_max_bytes,
    ] {
        if value == 0 || value > isize::MAX as usize {
            return Err(range());
        }
    }
    if l.max_sessions_per_identity > l.max_sessions
        || l.max_sessions_per_connection > l.max_sessions
        || !(1..=64).contains(&doc.io.batch_max)
        || !(1200..=65527).contains(&doc.transport.outer_udp_max_bytes)
        || doc.transport.application_max_bytes > 65527
        || doc.io.outer_socket_buffer_bytes > i32::MAX as usize
        || doc.io.relay_socket_buffer_bytes > i32::MAX as usize
    {
        return Err(range());
    }
    let native = l
        .quiche_dgram_packets
        .checked_mul(2)
        .and_then(|n| n.checked_mul(doc.transport.outer_udp_max_bytes))
        .ok_or_else(arithmetic)?;
    if native > l.connection_dgram_bytes {
        return Err(range());
    }
    for ms in [
        l.idle_session_ms,
        l.application_residence_ms,
        l.ciphertext_residence_ms,
        doc.transport.idle_timeout_ms,
        doc.recovery.keepalive_interval_ms,
        doc.recovery.peer_progress_timeout_ms,
        doc.recovery.setup_timeout_ms,
        doc.recovery.retry_initial_ms,
        doc.recovery.retry_max_ms,
        doc.diagnostics.snapshot_interval_ms,
    ] {
        if ms == 0
            || ms.checked_mul(1_000_000).is_none()
            || Instant::now()
                .checked_add(Duration::from_millis(ms))
                .is_none()
        {
            return Err(range());
        }
    }
    if doc.recovery.retry_initial_ms < 100
        || doc.recovery.retry_max_ms < doc.recovery.retry_initial_ms
        || doc.recovery.retry_max_ms > 2000
        || doc.diagnostics.snapshot_interval_ms < 1000
    {
        return Err(range());
    }
    l.max_sessions
        .checked_mul(l.session_queue_packets)
        .and_then(|n| n.checked_mul(2))
        .ok_or_else(arithmetic)?;
    l.max_sessions
        .checked_mul(l.session_queue_bytes)
        .ok_or_else(arithmetic)?;
    l.max_connections
        .checked_mul(l.connection_dgram_bytes)
        .ok_or_else(arithmetic)?;
    l.application_pool_bytes
        .checked_add(l.ciphertext_pool_bytes)
        .and_then(|n| n.checked_add(l.scratch_pool_bytes))
        .and_then(|n| n.checked_add(2 * 1024 * 1024))
        .filter(|n| *n <= isize::MAX as usize)
        .ok_or_else(arithmetic)?;
    let scratch_min = doc
        .io
        .batch_max
        .checked_mul(
            doc.transport
                .outer_udp_max_bytes
                .max(doc.transport.application_max_bytes),
        )
        .ok_or_else(arithmetic)?;
    if l.application_pool_bytes < doc.transport.application_max_bytes
        || l.ciphertext_pool_bytes < doc.transport.outer_udp_max_bytes
        || l.scratch_pool_bytes < scratch_min
    {
        return Err(range());
    }
    // Listener/outer sockets, relay cleanup, two setup FDs each, diagnostics and
    // a 32-FD runtime/control reserve. This is a conservative planned ceiling,
    // not a claim about unbounded inherited descriptors or kernel memory.
    let inherited = std::fs::read_dir("/proc/self/fd")
        .map_err(|_| error("fds", ConfigReason::File))?
        .try_fold(0usize, |n, entry| {
            entry.map_err(|_| error("fds", ConfigReason::File))?;
            n.checked_add(1).ok_or_else(arithmetic)
        })?;
    let fds = listeners
        .checked_add(l.max_connections)
        .and_then(|n| n.checked_add(l.max_relay_sockets))
        .and_then(|n| {
            l.max_pending_setup
                .checked_mul(2)
                .and_then(|setup| n.checked_add(setup))
        })
        .and_then(|n| n.checked_add(8 + 32))
        .and_then(|n| n.checked_add(inherited))
        .ok_or_else(arithmetic)?;
    Ok(fds)
}

#[cfg(test)]
#[path = "config/properties.rs"]
mod properties;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_owner_rules_allow_root_only_for_provisioned_server_material() {
        assert!(private_owner_allowed(1000, 0o100600, 1000, false));
        assert!(!private_owner_allowed(1001, 0o100600, 1000, false));
        assert!(!private_owner_allowed(0, 0o100600, 1000, false));
        assert!(private_owner_allowed(0, 0o100600, 1000, true));
        assert!(!private_owner_allowed(0, 0o100640, 1000, true));
        assert!(!private_owner_allowed(1000, 0o100604, 1000, false));
    }
}
