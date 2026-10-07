use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    println!("cargo:rustc-env=TEAMS_BUILD_VERSION={commit}-{timestamp}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icon/teams-fast.ico");
        println!("cargo:rerun-if-changed=assets/icon/app.rc");
        embed_resource::compile("assets/icon/app.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
    println!("cargo:rerun-if-changed=always-rerun-for-fresh-timestamp");
}
