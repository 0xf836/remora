//! Compile the actual owner in the test-only package, with the same send hook
//! used by native unit tests. No copied implementation is maintained on disk.
use std::{env, fs, path::PathBuf};
fn replace_once(source: &mut String, old: &str, new: &str) {
    assert_eq!(
        source.matches(old).count(),
        1,
        "reactor test seam changed: {old}"
    );
    *source = source.replacen(old, new, 1);
}
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let reactor = root.join("../src/reactor.rs");
    let support = root.join("support/lifecycle.rs");
    println!("cargo:rerun-if-changed={}", reactor.display());
    println!("cargo:rerun-if-changed={}", support.display());
    let diagnostics = root.join("../src/diagnostics.rs");
    println!("cargo:rerun-if-changed={}", diagnostics.display());
    let mut diag = fs::read_to_string(diagnostics).unwrap();
    diag = diag
        .lines()
        .map(|line| {
            line.strip_prefix("//!")
                .map_or(line.to_owned(), |s| format!("//{s}"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    for module in ["operator", "qlog"] {
        let path = root.join(format!("../src/diagnostics/{module}.rs"));
        println!("cargo:rerun-if-changed={}", path.display());
        replace_once(
            &mut diag,
            &format!("pub mod {module};"),
            &format!("#[path = {path:?}] pub mod {module};"),
        );
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("diagnostics.rs"),
        diag,
    )
    .unwrap();
    let mut source = fs::read_to_string(reactor).unwrap();
    // include! cannot consume an inner module doc comment.
    assert!(source.starts_with("//!"));
    source.replace_range(..3, "//");
    for (old, new) in [
        ("#[cfg(test)]\ntype OuterSendHook", "type OuterSendHook"),
        (
            "    #[cfg(test)]\n    outer_send_hook",
            "    outer_send_hook",
        ),
        (
            "            #[cfg(test)]\n            outer_send_hook",
            "            outer_send_hook",
        ),
        (
            "        #[cfg(test)]\n        let result = if let Some(send)",
            "        let result = if let Some(send)",
        ),
        (
            "        #[cfg(not(test))]\n        let result = socket.send_batch(&packets);\n",
            "",
        ),
    ] {
        replace_once(&mut source, old, new);
    }
    source.push_str(&format!(
        "\n#[path = {:?}]\npub mod lifecycle_harness;\n",
        support
    ));
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("reactor.rs"),
        source,
    )
    .unwrap();
}
