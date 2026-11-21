//! Pinned binary artifacts of the Docker-free runtime (D65, ADR-S14): the `uv`
//! launcher and the FFmpeg-free OpenCV wheel. Versions, file names and sha256
//! values live in `tools/doc-parser/runtime-artifacts.json` (embedded here, copied
//! into `supply-chain/`). A file is used only after its sha256 matches the pin.
//! Downloads go through the system `curl`, extraction through the system `tar`;
//! neither adds a crate.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use sha2::{Digest, Sha256};

const ARTIFACTS: &str = include_str!("../../../../tools/doc-parser/runtime-artifacts.json");

/// Operator override: a local archive of the pinned uv release (sha256 still checked).
pub const ENV_UV_ARCHIVE: &str = "OO_DOCS_UV_ARCHIVE";
/// Operator override: a local FFmpeg-free OpenCV wheel (sha256 still checked).
pub const ENV_OPENCV_WHEEL: &str = "OO_DOCS_OPENCV_WHEEL";

fn doc() -> Value {
    serde_json::from_str(ARTIFACTS).expect("runtime-artifacts.json is valid JSON")
}

/// The uv release target of this host.
pub fn uv_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

/// The wheel platform key of this host.
pub fn wheel_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("macosx_arm64"),
        _ => None,
    }
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn verify(path: &Path, want: &str) -> Result<(), String> {
    let got = sha256_file(path)?;
    if got == want {
        Ok(())
    } else {
        Err(format!(
            "{}: sha256 {got} does not match the pinned {want}",
            path.display()
        ))
    }
}

fn curl(url: &str, dest: &Path) -> Result<(), String> {
    let st = Command::new("curl")
        // Bounded: a stalled network must end the download, never hang the caller.
        .args([
            "-fsSL",
            "--retry",
            "2",
            "--connect-timeout",
            "20",
            "--max-time",
            "900",
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("curl: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("download of {url} failed"))
    }
}

/// `<state>/tools/uv-<version>/uv`, the bootstrapped pinned launcher.
pub fn pinned_uv_path(state: &Path) -> PathBuf {
    let ver = doc()["uv"]["version"].as_str().unwrap_or("0").to_owned();
    state.join("tools").join(format!("uv-{ver}")).join("uv")
}

/// Make the pinned uv present under the state directory: download the release
/// archive (or take `OO_DOCS_UV_ARCHIVE`), verify its sha256, unpack the binary.
pub fn bootstrap_uv(state: &Path, say: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let dest = pinned_uv_path(state);
    if dest.is_file() {
        return Ok(dest);
    }
    let d = doc();
    let ver = d["uv"]["version"].as_str().unwrap_or_default();
    let target = uv_target().ok_or(
        "no pinned uv build for this platform: install uv yourself (https://docs.astral.sh/uv/)",
    )?;
    let entry = &d["uv"]["targets"][target];
    let (file, sha) = (
        entry["file"].as_str().unwrap_or_default(),
        entry["sha256"].as_str().unwrap_or_default(),
    );
    let dir = dest.parent().ok_or("uv path")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let archive = dir.join(file);
    let local = std::env::var_os(ENV_UV_ARCHIVE).map(PathBuf::from);
    match &local {
        Some(p) => {
            std::fs::copy(p, &archive).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        None => {
            say(&format!("downloading the pinned uv {ver}"));
            let base = d["uv"]["base_url"].as_str().unwrap_or_default();
            curl(&format!("{base}/{file}"), &archive).map_err(|e| {
                format!(
                    "{e}; install uv yourself (`brew install uv`) or set {ENV_UV_ARCHIVE}=<{file}>"
                )
            })?;
        }
    }
    if let Err(e) = verify(&archive, sha) {
        let _ = std::fs::remove_file(&archive);
        return Err(e);
    }
    let unpack = dir.join("unpack");
    let _ = std::fs::remove_dir_all(&unpack);
    std::fs::create_dir_all(&unpack).map_err(|e| e.to_string())?;
    let st = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&unpack)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("tar: {e}"))?;
    if !st.success() {
        return Err("unpacking the uv archive failed".into());
    }
    let bin = unpack.join(file.trim_end_matches(".tar.gz")).join("uv");
    std::fs::rename(&bin, &dest).map_err(|e| format!("{}: {e}", bin.display()))?;
    let _ = std::fs::remove_dir_all(&unpack);
    let _ = std::fs::remove_file(&archive);
    let out = Command::new(&dest)
        .arg("--version")
        .output()
        .map_err(|e| format!("uv --version: {e}"))?;
    if !String::from_utf8_lossy(&out.stdout).contains(ver) {
        let _ = std::fs::remove_file(&dest);
        return Err(format!("the unpacked uv does not report version {ver}"));
    }
    Ok(dest)
}

/// The FFmpeg-free OpenCV wheel for this host, verified against its pin:
/// `OO_DOCS_OPENCV_WHEEL`, else `<state>/artifacts/<file>`, else the release URL.
pub fn opencv_wheel(state: &Path, say: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let d = doc();
    let plat = wheel_platform().ok_or(
        "no FFmpeg-free OpenCV wheel is pinned for this platform (D65): use the Docker parser (`oo serve --runtime compose`)",
    )?;
    let e = &d["opencv"]["platforms"][plat];
    let (file, sha, url) = (
        e["file"].as_str().unwrap_or_default(),
        e["sha256"].as_str().unwrap_or_default(),
        e["url"].as_str().unwrap_or_default(),
    );
    if let Some(p) = std::env::var_os(ENV_OPENCV_WHEEL).map(PathBuf::from) {
        verify(&p, sha)?;
        return Ok(p);
    }
    let dir = state.join("artifacts");
    let cached = dir.join(file);
    if cached.is_file() && verify(&cached, sha).is_ok() {
        return Ok(cached);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    say("downloading the FFmpeg-free OpenCV wheel");
    let part = dir.join(format!("{file}.partial"));
    curl(url, &part).map_err(|e| {
        format!("{e}; the wheel is not published yet: put {file} (sha256 {sha}) in {} or set {ENV_OPENCV_WHEEL}=<path>", dir.display())
    })?;
    if let Err(e) = verify(&part, sha) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &cached).map_err(|e| e.to_string())?;
    Ok(cached)
}

/// Pinned OpenCV version and the wheel's sha256 for this host (for the marker).
pub fn opencv_pin() -> (String, String) {
    let d = doc();
    let sha = wheel_platform()
        .and_then(|p| d["opencv"]["platforms"][p]["sha256"].as_str())
        .unwrap_or_default();
    (
        d["opencv"]["version"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        sha.to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_file_is_whole() {
        let d = doc();
        assert!(d["uv"]["version"].is_string());
        for (_, t) in d["uv"]["targets"].as_object().unwrap() {
            assert_eq!(t["sha256"].as_str().unwrap().len(), 64);
        }
        let w = &d["opencv"]["platforms"]["macosx_arm64"];
        assert_eq!(w["sha256"].as_str().unwrap().len(), 64);
        assert!(w["file"].as_str().unwrap().ends_with(".whl"));
    }

    #[test]
    fn a_wrong_hash_is_refused() {
        let p = std::env::temp_dir().join(format!("oo-art-{}", std::process::id()));
        std::fs::write(&p, b"x").unwrap();
        assert!(verify(&p, &"0".repeat(64)).is_err());
        let ok = sha256_file(&p).unwrap();
        assert!(verify(&p, &ok).is_ok());
        let _ = std::fs::remove_file(&p);
    }
}
