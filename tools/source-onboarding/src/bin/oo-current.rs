//! Select the operator's latest ready Gateway, then execute the existing Rust OO CLI.
#[path = "../io.rs"]
#[allow(dead_code)]
mod io;
use io::*;
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, ExitCode},
};

fn run() -> Result<i32> {
    let exe = env::current_exe()?;
    let config = env::var_os("OO_CURRENT_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| exe.with_extension("json"));
    let config = load(private(config)?)?;
    let ready = load(private(text(&config, "ready_record")?)?)?;
    require(
        ready["ready"] == true && ready["readonly_replica"] == true,
        "gateway_not_ready",
    )?;
    let root = PathBuf::from(text(&ready, "private_root")?);
    let binary_path = text(&config, "cli_binary")?;
    binary(binary_path)?;
    let file = std::path::Path::new(binary_path);
    let executable = format!(
        "/fixed/{}/{}",
        file.parent()
            .and_then(std::path::Path::file_name)
            .and_then(|p| p.to_str())
            .ok_or("binary")?,
        file.file_name().and_then(|p| p.to_str()).ok_or("binary")?
    );
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if !args.iter().any(|a| a == "--gateway") {
        args.extend(strings(&[
            "--gateway",
            &format!(
                "http://{}",
                fs::read_to_string(root.join("listen.addr"))?.trim()
            ),
        ]));
    }
    if !args.iter().any(|a| a == "--token-file") {
        args.extend(strings(&[
            "--token-file",
            "/run-current/current-client.token",
        ]));
    }
    for a in &mut args {
        if *a == root.join("current-client.token").to_string_lossy() {
            *a = "/run-current/current-client.token".into();
        }
    }
    if !args.iter().any(|a| a == "--auth") {
        args.extend(strings(&["--auth", "external"]));
    }
    if !args.iter().any(|a| a == "--transport") {
        args.extend(strings(&["--transport", "local-development"]));
    }
    let status = Command::new("docker")
        .args(["exec", text(&ready, "gateway_container")?, &executable])
        .args(args)
        .status()?;
    Ok(status.code().unwrap_or(2))
}
fn main() -> ExitCode {
    match run() {
        Ok(n) => ExitCode::from(u8::try_from(n).unwrap_or(2)),
        Err(e) => {
            eprintln!("oo-current: {e}");
            ExitCode::from(2)
        }
    }
}
