use remora::config::{RoleConfig, ValidatedConfig};
use std::{path::Path, process::ExitCode, time::Duration};

fn report_error(message: impl std::fmt::Display) {
    use std::io::Write;
    let mut out = std::io::stderr().lock();
    let journal = remora::diagnostics::operator::is_journal(std::io::stderr());
    let _ = remora::diagnostics::operator::prefix(&mut out, remora::config::Level::Error, journal);
    let _ = writeln!(out, "{message}");
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "Remora — UDP tunneling over HTTP/3\n\nUsage: remora <COMMAND> --config PATH\n\nCommands:\n  client        Forward a local UDP listener through a MASQUE proxy\n  server        Accept authenticated CONNECT-UDP requests\n  check-config  Validate configuration and files without network access\n\nOptions:\n  -h, --help     Show help (also available for each command)\n  --version      Show version and build information\n\nLogging: configure [diagnostics] level, format and snapshots in TOML.\nRUST_LOG is not used; see docs/operations.md.\n\nExamples:\n  remora server --config server.toml\n  remora client --config client.toml"
        );
        return ExitCode::SUCCESS;
    }
    if args.len() == 2
        && (args[1] == "--help" || args[1] == "-h")
        && ["client", "server", "check-config"]
            .iter()
            .any(|c| args[0] == *c)
    {
        let command = args[0].to_string_lossy();
        let description = match command.as_ref() {
            "client" => "Forward local UDP traffic through an authenticated MASQUE proxy.",
            "server" => "Accept authenticated CONNECT-UDP requests and enforce target allowlists.",
            _ => {
                "Validate schema, referenced files, permissions and resource budgets.\nDoes not bind sockets, resolve DNS or contact the peer."
            }
        };
        println!(
            "{description}\n\nUsage: remora {command} --config PATH\n\nOptions:\n  --config PATH  TOML configuration file (required)\n  -h, --help     Show this help\n\nLogging: configure [diagnostics] level, format and snapshots in TOML.\nDefaults: level=info, format=text, snapshots=false (quiet service logs).\nRUST_LOG is not used."
        );
        return ExitCode::SUCCESS;
    }
    if args.len() == 1 && args[0] == "--version" {
        println!(
            "remora {} commit={} Rust={} quiche=0.30.0 Linux x86_64/aarch64 portable-udp",
            env!("CARGO_PKG_VERSION"),
            env!("REMORA_BUILD_COMMIT"),
            env!("REMORA_RUST_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    if args.len() != 3
        || args[1] != "--config"
        || !["client", "server", "check-config"]
            .iter()
            .any(|c| args[0] == *c)
    {
        report_error("invalid command; use remora --help");
        return ExitCode::from(2);
    }
    let settings = match ValidatedConfig::load(Path::new(&args[2])) {
        Ok(c) => c,
        Err(e) => {
            report_error(e);
            return ExitCode::from(2);
        }
    };
    if args[0] == "check-config" {
        println!(
            "configuration is valid ({})",
            if matches!(settings.role(), RoleConfig::Client(_)) {
                "client"
            } else {
                "server"
            }
        );
        return ExitCode::SUCCESS;
    }
    if (args[0] == "client") != matches!(settings.role(), RoleConfig::Client(_)) {
        report_error("invalid configuration: command role mismatch");
        return ExitCode::from(2);
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => {
            report_error("runtime initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(remora::reactor::run(&settings));
    // libc DNS cannot be killed; its bounded worker keeps its permit. Limit
    // The owner has already consumed the shared stop/status-flush budget.
    // Do not add a second wait after it. Unkillable bounded DNS workers retain
    // their permits until this executable exits; no reactor callbacks remain.
    runtime.shutdown_timeout(Duration::ZERO);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        // The bounded writer already failed to finish on this same stderr.
        // A synchronous fallback write could hang shutdown indefinitely.
        Err(remora::reactor::RuntimeError::DiagnosticCollectionIncomplete)
            if settings.diagnostics().format == remora::config::LogFormat::Text =>
        {
            ExitCode::FAILURE
        }
        Err(e) => {
            if settings.diagnostics().format == remora::config::LogFormat::Json {
                // Legacy collectors match this fixed marker and typed identity,
                // not localized/human prose. Never prefix it with journal framing.
                eprintln!("runtime failed: {e:?}");
            } else {
                report_error(e);
            }
            ExitCode::FAILURE
        }
    }
}
