use std::process::Command;

fn main() {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .env("TZ", "UTC")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    let timestamp = git(&["log", "-1", "--format=%cd", "--date=format-local:%Y%m%dT%H%M%SZ"])
        .unwrap_or_else(|| chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string());
    println!("cargo:rustc-env=TEAMS_BUILD_VERSION={commit}-{timestamp}");
    // Rerun only when HEAD moves, so an unchanged tree does not recompile the whole app crate.
    if let Some(head) = git(&["rev-parse", "--path-format=absolute", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"])
        .and_then(|reference| git(&["rev-parse", "--path-format=absolute", "--git-path", &reference]))
    {
        println!("cargo:rerun-if-changed={reference}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icon/teams-fast.ico");
        println!("cargo:rerun-if-changed=assets/icon/app.rc");
        embed_resource::compile("assets/icon/app.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
    println!("cargo:rerun-if-changed=build.rs");
}
