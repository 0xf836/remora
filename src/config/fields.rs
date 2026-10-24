//! Field diagnostics use only compile-time names and expectations, never TOML values.
use super::*;
use serde::de::DeserializeOwned;
fn field<T: DeserializeOwned>(
    table: &toml::Table,
    key: &str,
    path: &'static str,
    expected: &'static str,
    required: bool,
) -> Result<(), ConfigError> {
    match table.get(key) {
        Some(value) => value
            .clone()
            .try_into::<T>()
            .map(|_| ())
            .map_err(|_| with_hint(path, ConfigReason::Schema, expected)),
        None if required => Err(with_hint(
            path,
            ConfigReason::Schema,
            "required field is missing",
        )),
        None => Ok(()),
    }
}
fn known(table: &toml::Table, allowed: &[&str], path: &'static str) -> Result<(), ConfigError> {
    if table.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(with_hint(
            path,
            ConfigReason::Schema,
            "unknown field; see the configuration reference",
        ));
    }
    Ok(())
}
macro_rules! table {
    ($root:expr, $section:literal, { $($key:ident: $ty:ty => $required:literal, $hint:literal;)* }) => {
        if let Some(value) = $root.get($section) {
            let table = value.as_table().ok_or_else(|| with_hint($section, ConfigReason::Schema, "expected a table"))?;
            known(table, &[$(stringify!($key)),*], $section)?;
            $(field::<$ty>(table, stringify!($key), concat!($section, ".", stringify!($key)), $hint, $required)?;)*
        }
    };
}
pub(super) fn validate_document_fields(root: &toml::Table) -> Result<(), ConfigError> {
    known(
        root,
        &[
            "schema_version",
            "client",
            "server",
            "limits",
            "io",
            "transport",
            "recovery",
            "diagnostics",
        ],
        "config",
    )?;
    field::<u32>(
        root,
        "schema_version",
        "schema_version",
        "expected an integer schema version",
        true,
    )?;
    table!(root, "client", {
        listen: SocketAddr => true, "expected a numeric IP:port (bracket IPv6)";
        allow_non_loopback: bool => false, "expected true or false";
        uri_template: String => false, "expected a string";
        proxy: String => true, "expected a string";
        connect_address: SocketAddr => false, "expected a numeric IP:port (bracket IPv6)";
        target: String => true, "expected a string";
        ca_file: PathBuf => true, "expected a file path string";
        token_file: PathBuf => true, "expected a file path string";
    });
    table!(root, "server", {
        listen: Vec<SocketAddr> => true, "expected an array of numeric IP:port addresses";
        proxy: String => true, "expected a string";
        path_prefix: String => false, "expected a string";
        certificate_file: PathBuf => true, "expected a file path string";
        private_key_file: PathBuf => true, "expected a file path string";
        credentials_file: PathBuf => true, "expected a file path string";
    });
    table!(root, "limits", {
        max_connections: usize => false, "expected a nonnegative integer";
        max_sessions: usize => false, "expected a nonnegative integer";
        max_sessions_per_identity: usize => false, "expected a nonnegative integer";
        max_sessions_per_connection: usize => false, "expected a nonnegative integer";
        max_pending_setup: usize => false, "expected a nonnegative integer";
        max_relay_sockets: usize => false, "expected a nonnegative integer";
        session_queue_packets: usize => false, "expected a nonnegative integer";
        session_queue_bytes: usize => false, "expected a nonnegative integer";
        application_pool_bytes: usize => false, "expected a nonnegative integer";
        ciphertext_pool_bytes: usize => false, "expected a nonnegative integer";
        scratch_pool_bytes: usize => false, "expected a nonnegative integer";
        quiche_dgram_packets: usize => false, "expected a nonnegative integer";
        connection_dgram_bytes: usize => false, "expected a nonnegative integer";
        idle_session_ms: u64 => false, "expected a nonnegative integer";
        application_residence_ms: u64 => false, "expected a nonnegative integer";
        ciphertext_residence_ms: u64 => false, "expected a nonnegative integer";
    });
    table!(root, "io", {
        mode: IoMode => false, "expected mmsg or single";
        batch_max: usize => false, "expected a nonnegative integer";
        gso: Offload => false, "expected off or auto";
        gro: Offload => false, "expected off or auto";
        outer_socket_buffer_bytes: usize => false, "expected a nonnegative integer";
        relay_socket_buffer_bytes: usize => false, "expected a nonnegative integer";
    });
    table!(root, "transport", {
        cc: CongestionControl => false, "expected cubic, reno or bbr2_gcongestion";
        hystart: bool => false, "expected true or false";
        pacing: Pacing => false, "expected timerfd";
        outer_udp_max_bytes: usize => false, "expected a nonnegative integer";
        application_max_bytes: usize => false, "expected a nonnegative integer";
        idle_timeout_ms: u64 => false, "expected a nonnegative integer";
    });
    table!(root, "recovery", {
        keepalive_interval_ms: u64 => false, "expected a nonnegative integer";
        peer_progress_timeout_ms: u64 => false, "expected a nonnegative integer";
        setup_timeout_ms: u64 => false, "expected a nonnegative integer";
        retry_initial_ms: u64 => false, "expected a nonnegative integer";
        retry_max_ms: u64 => false, "expected a nonnegative integer";
    });
    table!(root, "diagnostics", {
        level: Level => false, "expected error, warn, info, debug or trace";
        format: LogFormat => false, "expected json or text";
        snapshots: bool => false, "expected true or false; full status requires format=json";
        snapshot_interval_ms: u64 => false, "expected a nonnegative integer";
        qlog_directory: PathBuf => false, "expected a file path string";
    });
    Ok(())
}
