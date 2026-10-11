use std::process::Command;
fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=.git/refs");
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .filter(|s| s.trim().len() == 40 && s.trim().bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "source-archive".into());
    println!("cargo:rustc-env=REMORA_BUILD_COMMIT={}", revision.trim());
    let rust = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=REMORA_RUST_VERSION={}", rust.trim());
}
