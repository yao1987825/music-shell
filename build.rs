use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=MS_VERSION");

    let version = match std::env::var("MS_VERSION") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    };
    println!("cargo:rustc-env=MS_VERSION={version}");

    println!("cargo:rerun-if-changed=build.rs");
    let _ = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();
}
