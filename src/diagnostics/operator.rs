//! Small human renderer and journal stream framing. No file backend, dynamic
//! labels, incident history, peer strings, or general-purpose logging facade.
use super::{Event, EventName, Failure, Role};
use crate::config::Level;
use std::{
    io::{self, Write},
    os::fd::AsFd,
};

pub const MAX_LINE_BYTES: usize = 1024;

/// JOURNAL_STREAM is only a hint: verify the actual destination descriptor.
pub fn is_journal(fd: impl AsFd) -> bool {
    let Ok(stream) = std::env::var("JOURNAL_STREAM") else {
        return false;
    };
    let Ok(stat) = rustix::fs::fstat(fd) else {
        return false;
    };
    matches_stream(&stream, stat.st_dev, stat.st_ino)
}
fn matches_stream(stream: &str, device: u64, inode: u64) -> bool {
    stream.split_once(':').is_some_and(|(dev, ino)| {
        dev.parse::<u64>() == Ok(device) && ino.parse::<u64>() == Ok(inode)
    })
}
pub fn prefix(out: &mut dyn Write, level: Level, journal: bool) -> io::Result<()> {
    let (name, priority) = match level {
        Level::Error => ("ERROR", 3),
        Level::Warn => ("WARN", 4),
        Level::Info => ("INFO", 6),
        Level::Debug => ("DEBUG", 7),
        Level::Trace => ("TRACE", 7),
    };
    if journal {
        write!(out, "<{priority}>")?;
    }
    write!(out, "{name:<5} ")
}

pub(super) fn render(
    out: &mut dyn Write,
    timestamp: &str,
    role: Role,
    event: &Event,
    journal: bool,
) -> io::Result<()> {
    // All records are bounded before they enter the existing output queue.
    let mut scratch = [0; MAX_LINE_BYTES - 1];
    let mut line = io::Cursor::new(&mut scratch[..]);
    if !journal {
        write!(line, "{timestamp} ")?;
    }
    prefix(&mut line, event.level, journal)?;
    match event.event {
        EventName::ProcessStarted => write!(
            line,
            "Remora {} started in {} mode; UDP listener ready.",
            env!("CARGO_PKG_VERSION"),
            if role == Role::Client {
                "client"
            } else {
                "server"
            }
        )?,
        EventName::ProcessStopped => {
            write!(line, "Remora stopped; UDP listeners and sessions released.")?
        }
        EventName::TransportReady => write!(
            line,
            "Secure connection established; a UDP tunnel is not yet accepted."
        )?,
        EventName::TunnelAccepted => write!(line, "UDP tunnel accepted by the server.")?,
        EventName::TunnelRestored => write!(line, "UDP tunnel re-established.")?,
        EventName::TunnelSetupRestored => {
            write!(line, "A new UDP tunnel was accepted after setup failures.")?
        }
        EventName::TunnelSetupUnavailable | EventName::NewTunnelSetupUnavailable => {
            if event.event == EventName::NewTunnelSetupUnavailable {
                write!(
                    line,
                    "A new UDP tunnel could not be opened; already accepted tunnels may continue. "
                )?;
            } else {
                write!(line, "UDP tunnel setup failed. ")?;
            }
            match event.numeric_code.unwrap_or(0) {
                0 => write!(line, "Setup did not complete before its deadline. ")?,
                code => write!(
                    line,
                    "Server returned HTTP {code}. Check server-side setup errors and resource limits. "
                )?,
            }
            write!(line, "Further application traffic can retry setup.")?;
        }
        EventName::ConnectionLost => write!(
            line,
            "Connection to the server lost. Reconnecting automatically."
        )?,
        EventName::ConnectionUnavailable => write!(
            line,
            "Cannot establish a connection to the server. Retrying automatically; check server availability and the network."
        )?,
        EventName::NewTunnelsBlocked => {
            write_reason(&mut line, event)?;
            write!(
                line,
                " New tunnel setup is blocked until restart; already accepted tunnels may continue. Correct the configuration before restarting."
            )?;
        }
        EventName::Failure => write_reason(&mut line, event)?,
        EventName::Starting => write!(line, "Starting connection setup.")?,
        EventName::Active => write!(
            line,
            "QUIC and HTTP/3 settings are ready; tunnel acceptance is separate."
        )?,
        EventName::Recovering => write!(line, "Connection setup will be retried.")?,
        EventName::Rejected => write!(line, "New tunnel setup has been rejected.")?,
        EventName::Stopped => write!(line, "Client transport stopped.")?,
        EventName::OffloadState => write!(line, "UDP offload configuration recorded.")?,
    }
    if matches!(event.level, Level::Debug | Level::Trace) {
        if let Some(class) = event.failure {
            write!(line, " reason={class:?}")?;
        }
        if let Some(code) = event.numeric_code {
            write!(line, " code={code}")?;
        }
        if let Some(id) = event.connection_id {
            write!(line, " connection=c{}", id.serial)?;
        }
        if let Some(id) = event.session_id {
            write!(line, " session=s{}", id.serial)?;
        }
    }
    let length = line.position() as usize;
    out.write_all(&scratch[..length])
}
fn write_reason(out: &mut dyn Write, event: &Event) -> io::Result<()> {
    let code = event.numeric_code.unwrap_or(0);
    match event.failure {
        Some(Failure::Size) => {
            if let Some(size) = event.size {
                let limit = if code == 1 {
                    size.application_cap_bytes
                } else {
                    size.outgoing_ceiling_bytes
                };
                write!(
                    out,
                    "Datagram dropped: {} bytes exceeds the {} limit of {} bytes. Reduce the application's datagram size.",
                    size.datagram_bytes,
                    if code == 1 {
                        "configured application"
                    } else {
                        "current QUIC path"
                    },
                    limit
                )
            } else {
                write!(
                    out,
                    "UDP datagram dropped because it exceeds a local size limit. Reduce the application's datagram size; enable debug logging for details."
                )
            }
        }
        Some(Failure::Authentication) => write!(
            out,
            "Tunnel authentication rejected (HTTP {code}). Check the client token and server credentials."
        ),
        Some(Failure::Authorization) => match code {
            403 => write!(
                out,
                "Tunnel destination denied by server policy (HTTP 403). Check the server allowlist."
            ),
            404 => write!(
                out,
                "Tunnel route not found (HTTP 404). Check the proxy origin and MASQUE path configuration."
            ),
            _ => write!(
                out,
                "Tunnel request rejected by the server (HTTP {code}). Check the request and server configuration."
            ),
        },
        Some(Failure::Trust) => write!(
            out,
            "Cannot establish a trusted TLS connection. Check the server identity, certificate chain, CA file and system clock."
        ),
        Some(Failure::Capability) => write!(
            out,
            "Peer does not support the required HTTP/3 CONNECT-UDP capabilities. Check peer compatibility."
        ),
        Some(Failure::Resolution) => write!(
            out,
            "Name resolution failed during tunnel setup. Check DNS resolver availability."
        ),
        Some(Failure::SetupTimeout) => write!(out, "Tunnel setup timed out before completion."),
        Some(Failure::PeerTimeout) => write!(
            out,
            "Server connection timed out without authenticated peer progress."
        ),
        Some(Failure::Socket) => match code {
            111 => write!(
                out,
                "UDP operation failed: connection refused (ECONNREFUSED, 111). Check the relevant UDP service and firewall rules."
            ),
            90 => write!(
                out,
                "UDP send failed: message too large (EMSGSIZE, 90). Check application size and path MTU."
            ),
            _ => write!(out, "UDP socket operation failed (errno {code})."),
        },
        Some(Failure::Resource) => write!(
            out,
            "Operation rejected by a local or peer resource limit. Check configured limits and offered load."
        ),
        Some(Failure::Malformed) => write!(out, "Invalid protocol input rejected."),
        Some(Failure::Transport) => write!(out, "QUIC or HTTP/3 operation failed (code {code})."),
        Some(Failure::Internal) => write!(out, "Internal operation failed."),
        Some(Failure::InvalidConfiguration) => write!(
            out,
            "Configuration rejected. Run check-config and correct the reported field."
        ),
        Some(Failure::Stopped) => write!(out, "Session stopped."),
        None => write!(out, "Operation completed."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_hint_must_match_actual_stream_identity() {
        assert!(matches_stream("12:34", 12, 34));
        for value in ["12:35", "13:34", "12", "12:34:56", "x:34", "-1:34"] {
            assert!(!matches_stream(value, 12, 34));
        }
    }
    #[test]
    fn framing_is_human_readable_bounded_and_has_real_priority() {
        let event = Event::lifecycle(EventName::ConnectionLost, Level::Warn);
        for journal in [false, true] {
            let mut out = Vec::new();
            render(
                &mut out,
                "2026-10-23T00:00:00Z",
                Role::Client,
                &event,
                journal,
            )
            .unwrap();
            assert!(out.len() < MAX_LINE_BYTES);
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains("Connection to the server lost."));
            assert!(!text.contains("epoch") && !text.contains("sequence") && !text.contains('\n'));
            assert_eq!(text.starts_with("<4>WARN"), journal);
            assert_eq!(text.contains("2026-10-23"), !journal);
        }
    }
}
