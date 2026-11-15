//! Release-hosted runtime bundle: the fallback when the pinned runtime image
//! is neither local nor pullable (for example a private registry).
//!
//! Trust comes only from the pin shipped beside `oo` (`oo-lite-bundle.json`):
//! size and SHA-256 of the per-architecture asset, the SHA-256 of the manifest
//! inside it, and the image identities it must load as. The download is never
//! executed; the packaged verifier (`deploy/lite/bundle.py`, embedded here)
//! extracts exactly three regular files, checks the archive identity and runs
//! `docker load`, and this module then confirms the loaded image ID.
use crate::outcome::Failure;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub(super) const PIN_SCHEMA: &str = "oo.lite-bundles/v1";
const MAX_ASSET: u64 = 2 * 1024 * 1024 * 1024 - 1;
const PLATFORMS: [&str; 2] = ["linux/amd64", "linux/arm64"];

static OFFLINE_BUNDLE: OnceLock<PathBuf> = OnceLock::new();

/// `--offline-bundle PATH` for this process (`oo up` / `oo quickstart`).
pub(super) fn set_offline_bundle(path: PathBuf) {
    let _ = OFFLINE_BUNDLE.set(path);
}

pub(super) fn offline_bundle() -> Option<&'static Path> {
    OFFLINE_BUNDLE.get().map(PathBuf::as_path)
}

/// `OO_LITE_OFFLINE=1`: never pull or download; only local images and
/// `--offline-bundle` are used.
pub(super) fn offline() -> bool {
    std::env::var("OO_LITE_OFFLINE").is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Entry {
    pub platform: String,
    pub asset: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub manifest_sha256: String,
    pub image_id: String,
    pub config_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Pin {
    pub runtime: String,
    pub entries: Vec<Entry>,
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(hex64)
}

/// Only HTTPS, or plain HTTP to a loopback test server. The SHA-256 in the
/// pin is the trust root either way; transport security is defence in depth.
fn allowed_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.username().is_empty()
        && parsed.password().is_none()
        && match parsed.scheme() {
            "https" => parsed.host_str().is_some(),
            "http" => matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            _ => false,
        }
}

/// Parse and validate a pin. `runtime` is the CLI's `oo-lite-image` pin; a
/// pin for another runtime is refused rather than ignored.
pub(super) fn parse_pin(text: &str, runtime: &str) -> Result<Pin, Failure> {
    let bad = || {
        Failure::local(
            "LITE_BUNDLE_PIN: the packaged oo-lite-bundle.json is invalid; reinstall the package",
        )
    };
    let value: Value = serde_json::from_str(text).map_err(|_| bad())?;
    if value["schema"] != PIN_SCHEMA {
        return Err(bad());
    }
    let pinned = value["runtime"].as_str().ok_or_else(bad)?;
    if pinned != runtime || !super::lite::digest(pinned) || !pinned.contains('@') {
        return Err(Failure::local(
            "LITE_BUNDLE_PIN: oo-lite-bundle.json names a different runtime than oo-lite-image; reinstall the package",
        ));
    }
    let platforms = value["platforms"].as_object().ok_or_else(bad)?;
    if platforms.is_empty() {
        return Err(bad());
    }
    let mut entries = Vec::new();
    for (platform, row) in platforms {
        let text = |key: &str| row[key].as_str().map(str::to_owned).ok_or_else(bad);
        let entry = Entry {
            platform: platform.clone(),
            asset: text("asset")?,
            url: text("url")?,
            size: row["size"].as_u64().ok_or_else(bad)?,
            sha256: text("sha256")?,
            manifest_sha256: text("manifest_sha256")?,
            image_id: text("image_id")?,
            config_id: text("config_id")?,
        };
        let asset_name = entry.asset.len() <= 128
            && !entry.asset.starts_with('.')
            && entry
                .asset
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if !PLATFORMS.contains(&platform.as_str())
            || !asset_name
            || !allowed_url(&entry.url)
            || !entry.url.ends_with(&format!("/{}", entry.asset))
            || entry.size == 0
            || entry.size > MAX_ASSET
            || !hex64(&entry.sha256)
            || !hex64(&entry.manifest_sha256)
            || !image_digest(&entry.image_id)
            || !image_digest(&entry.config_id)
        {
            return Err(bad());
        }
        entries.push(entry);
    }
    Ok(Pin {
        runtime: pinned.to_owned(),
        entries,
    })
}

/// The packaged pin (`OO_LITE_BUNDLE_PIN` overrides the companion file).
/// `Ok(None)` when no pin is installed: the old pull-only behaviour.
pub(super) fn installed_pin(runtime: &str) -> Result<Option<Pin>, Failure> {
    let path = std::env::var_os("OO_LITE_BUNDLE_PIN")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|exe| exe.with_file_name("oo-lite-bundle.json"))
        });
    let Some(path) = path else { return Ok(None) };
    match fs::read_to_string(&path) {
        Ok(text) => parse_pin(&text, runtime).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(Failure::local(
            "LITE_BUNDLE_PIN: oo-lite-bundle.json is unreadable",
        )),
    }
}

/// `docker info` OSType/Architecture as an OCI platform.
pub(super) fn docker_platform(info: &str) -> Option<String> {
    let (os, arch) = info.trim().split_once('/')?;
    let arch = match arch {
        "x86_64" | "amd64" => "amd64",
        "aarch64" | "arm64" => "arm64",
        _ => return None,
    };
    (os == "linux").then(|| format!("linux/{arch}"))
}

impl Pin {
    pub(super) fn entry(&self, platform: Option<&str>) -> Result<&Entry, Failure> {
        let platform = platform.ok_or_else(|| {
            Failure::local(
                "LITE_BUNDLE_PLATFORM: the Docker daemon is not linux/amd64 or linux/arm64",
            )
        })?;
        self.entries
            .iter()
            .find(|e| e.platform == platform)
            .ok_or_else(|| {
                Failure::local(format!(
                    "LITE_BUNDLE_PLATFORM: no runtime bundle for the Docker daemon's {platform}"
                ))
            })
    }
}

fn inspect(id: &str) -> Option<(String, String)> {
    let out = Command::new("docker")
        .args([
            "image",
            "inspect",
            "--format",
            "{{.Id}} {{.Os}}/{{.Architecture}}",
            id,
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let (id, platform) = text.trim().split_once(' ')?;
    Some((id.to_owned(), platform.to_owned()))
}

/// A previously loaded bundle image for this platform, by its pinned IDs.
pub(super) fn local_image(entry: &Entry) -> Option<String> {
    [&entry.image_id, &entry.config_id]
        .into_iter()
        .find_map(|id| {
            inspect(id)
                .filter(|(found, platform)| {
                    (found == &entry.image_id || found == &entry.config_id)
                        && platform == &entry.platform
                })
                .map(|(found, _)| found)
        })
}

/// Private per-user cache: `$XDG_CACHE_HOME` or `~/.cache`, then
/// `open-ontology/lite-bundles`, every created level 0700 and owned by us.
pub(super) fn cache_dir() -> Result<PathBuf, Failure> {
    let fail =
        || Failure::local("LITE_BUNDLE_DOWNLOAD: cannot create the private download directory");
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|h| h.join(".cache"))
        })
        .ok_or_else(fail)?;
    let dir = base.join("open-ontology").join("lite-bundles");
    private_dir(&dir).map_err(|_| fail())?;
    Ok(dir)
}

fn private_dir(dir: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // symlink_metadata: a symlinked cache directory is refused, not followed.
    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(std::io::Error::other("not a directory"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Size, then SHA-256, of a complete local file against its pin entry.
pub(super) fn verify_file(path: &Path, entry: &Entry) -> Result<(), Failure> {
    let size = fs::metadata(path)
        .map_err(|_| Failure::local("LITE_BUNDLE_DOWNLOAD: the bundle file is unreadable"))?
        .len();
    if size != entry.size {
        return Err(Failure::local(format!(
            "LITE_BUNDLE_SHA256: {} is {size} bytes, expected {}",
            entry.asset, entry.size
        )));
    }
    let digest = file_sha256(path)
        .map_err(|_| Failure::local("LITE_BUNDLE_DOWNLOAD: the bundle file is unreadable"))?;
    if digest != entry.sha256 {
        return Err(Failure::local(format!(
            "LITE_BUNDLE_SHA256: {} does not match its pinned SHA-256",
            entry.asset
        )));
    }
    Ok(())
}

struct Progress {
    tty: bool,
    total: u64,
    last: u64,
}

impl Progress {
    fn new(entry: &Entry, start: u64) -> Self {
        let tty = std::io::stderr().is_terminal();
        let mb = entry.size as f64 / 1e6;
        if start > 0 {
            eprintln!(
                "oo up: resuming runtime bundle {} at {:.1} of {mb:.1} MB",
                entry.asset,
                start as f64 / 1e6
            );
        } else {
            eprintln!(
                "oo up: downloading runtime bundle {} ({mb:.1} MB)",
                entry.asset
            );
        }
        Self {
            tty,
            total: entry.size,
            last: 0,
        }
    }

    fn update(&mut self, done: u64) {
        if !self.tty || (done - self.last.min(done) < 1 << 20 && done != self.total) {
            return;
        }
        self.last = done;
        eprint!(
            "\r  {:>3}%  {:.1}/{:.1} MB",
            done * 100 / self.total.max(1),
            done as f64 / 1e6,
            self.total as f64 / 1e6
        );
        if done == self.total {
            eprintln!();
        }
    }
}

fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() < 5 && attempt.url().scheme() == "https" {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

/// Download `entry` into `dir/<sha256>.partial`, resuming an earlier partial
/// file with a Range request. Returns the verified complete file. A size
/// overrun or digest mismatch removes the partial file; an interrupted
/// transfer keeps it for the next attempt.
pub(super) fn download(entry: &Entry, dir: &Path) -> Result<PathBuf, Failure> {
    let partial = dir.join(format!("{}.partial", entry.sha256));
    let network = |detail: &str| {
        Failure::local(format!(
            "LITE_BUNDLE_DOWNLOAD: {detail} ({}); retry oo up to resume, or download it and pass --offline-bundle PATH",
            entry.url
        ))
    };
    let mut start = match fs::symlink_metadata(&partial) {
        Ok(meta) if meta.is_file() && meta.len() <= entry.size => meta.len(),
        Ok(_) => {
            fs::remove_file(&partial).map_err(|_| network("cannot replace the partial file"))?;
            0
        }
        Err(_) => 0,
    };
    if start == entry.size {
        if verify_file(&partial, entry).is_ok() {
            return Ok(partial);
        }
        let _ = fs::remove_file(&partial);
        start = 0;
    }
    let runtime = crate::lifecycle::runtime().map_err(|_| network("no async runtime"))?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .redirect(redirect_policy())
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|_| network("cannot start the HTTP client"))?;
        let mut request = client.get(&entry.url);
        if start > 0 {
            request = request.header("Range", format!("bytes={start}-"));
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| network("the request failed"))?;
        let status = response.status().as_u16();
        let resumed = status == 206
            && start > 0
            && response
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with(&format!("bytes {start}-")));
        if status == 416 || (status == 206 && !resumed) {
            let _ = fs::remove_file(&partial);
            return Err(network("the server refused to resume; the partial file was removed"));
        }
        if status != 200 && !resumed {
            return Err(network(&format!("HTTP {status}")));
        }
        if !resumed {
            start = 0;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&partial)
            .map_err(|_| network("cannot write the partial file"))?;
        let mut done = start;
        let mut progress = Progress::new(entry, start);
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(_) => {
                    let _ = file.flush();
                    return Err(network("the transfer was interrupted"));
                }
            };
            done += chunk.len() as u64;
            if done > entry.size {
                drop(file);
                let _ = fs::remove_file(&partial);
                return Err(Failure::local(format!(
                    "LITE_BUNDLE_SHA256: {} is larger than its pinned {} bytes; the partial file was removed",
                    entry.asset, entry.size
                )));
            }
            file.write_all(&chunk)
                .map_err(|_| network("cannot write the partial file"))?;
            progress.update(done);
        }
        file.sync_all()
            .map_err(|_| network("cannot write the partial file"))?;
        if done < entry.size {
            return Err(network("the transfer ended early"));
        }
        Ok(())
    })?;
    if let Err(error) = verify_file(&partial, entry) {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    Ok(partial)
}

/// Run the embedded verifier on a size- and SHA-checked asset, then confirm
/// the loaded image is the pinned one. Returns the loaded image ID.
pub(super) fn load(asset: &Path, entry: &Entry, dir: &Path) -> Result<String, Failure> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let work = dir.join(format!("work-{}-{nanos}", std::process::id()));
    let result = load_in(asset, entry, &work);
    let _ = fs::remove_dir_all(&work);
    result
}

fn load_in(asset: &Path, entry: &Entry, work: &Path) -> Result<String, Failure> {
    let verify = |detail: String| Failure::local(format!("LITE_BUNDLE_VERIFY: {detail}"));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(work)
        .map_err(|_| verify("cannot create a private work directory".into()))?;
    // Trusted verifier code lives apart from the extracted download.
    let trusted = work.join("verifier");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&trusted)
        .map_err(|_| verify("cannot create a private work directory".into()))?;
    for (name, text) in [
        (
            "bundle.py",
            include_str!("../../../../deploy/lite/bundle.py"),
        ),
        (
            "remote.py",
            include_str!("../../../../deploy/lite/remote.py"),
        ),
    ] {
        fs::write(trusted.join(name), text)
            .map_err(|_| verify("cannot stage the verifier".into()))?;
    }
    eprintln!("oo up: verifying and loading {}", entry.asset);
    let mut command = Command::new("python3");
    command
        .current_dir(&trusted)
        .arg(trusted.join("bundle.py"))
        .arg("load-runtime")
        .arg("--asset")
        .arg(asset)
        .args(["--manifest-sha256", &entry.manifest_sha256])
        .arg("--work")
        .arg(work.join("extract"))
        .args(["--platform", &entry.platform])
        .env("PYTHONDONTWRITEBYTECODE", "1");
    let (exit, stdout, stderr) = super::build_retained::invoke(command, vec![], 1800)
        .map_err(|_| verify("the verifier did not finish".into()))?;
    if exit != 0 {
        let code = String::from_utf8_lossy(&stderr)
            .split(';')
            .next()
            .map(str::trim)
            .filter(|c| c.len() < 64 && c.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
            .unwrap_or("BUNDLE_FAILED")
            .to_owned();
        return Err(verify(format!(
            "{code}; the bundle was not loaded. No services were started."
        )));
    }
    let report: Value =
        serde_json::from_slice(&stdout).map_err(|_| verify("verifier protocol".into()))?;
    let id = report["id"].as_str().unwrap_or_default();
    if report["config_id"] != entry.config_id.as_str()
        || report["platform"] != entry.platform.as_str()
        || !(id == entry.image_id || id == entry.config_id)
    {
        return Err(Failure::local(
            "LITE_BUNDLE_IDENTITY: the loaded image is not the pinned runtime; no services were started",
        ));
    }
    match local_image(entry) {
        Some(found) if found == id => Ok(found),
        _ => Err(Failure::local(
            "LITE_BUNDLE_IDENTITY: the loaded image is not the pinned runtime; no services were started",
        )),
    }
}

/// Use a pre-downloaded asset. The caller's file is never modified or removed.
pub(super) fn install_file(
    path: &Path,
    pin: &Pin,
    platform: Option<&str>,
) -> Result<String, Failure> {
    let entry = pin.entry(platform)?;
    if let Err(error) = verify_file(path, entry) {
        // Name the right file when the user picked the other architecture.
        let other = pin
            .entries
            .iter()
            .find(|e| e.platform != entry.platform && verify_file(path, e).is_ok());
        return Err(match other {
            Some(other) => Failure::local(format!(
                "LITE_BUNDLE_PLATFORM: {} is the {} bundle; the Docker daemon is {}, use {}",
                path.display(),
                other.platform,
                entry.platform,
                entry.asset
            )),
            None => error,
        });
    }
    load(path, entry, &cache_dir()?)
}

/// Download, verify and load the release asset for the daemon platform.
pub(super) fn install_release(pin: &Pin, platform: Option<&str>) -> Result<String, Failure> {
    let entry = pin.entry(platform)?;
    let dir = cache_dir()?;
    let started = std::time::Instant::now();
    let file = download(entry, &dir)?;
    eprintln!(
        "oo up: downloaded and verified {} in {:.1}s",
        entry.asset,
        started.elapsed().as_secs_f64()
    );
    let loaded = load(&file, entry, &dir);
    // Keep nothing once loaded; a failed load keeps the verified file for retry.
    if loaded.is_ok() {
        let _ = fs::remove_file(&file);
    }
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    trait Must<T> {
        fn must(self) -> T;
    }
    impl<T> Must<T> for Result<T, Failure> {
        fn must(self) -> T {
            match self {
                Ok(value) => value,
                Err(error) => panic!("{}", String::from_utf8_lossy(&error.json_bytes())),
            }
        }
    }

    const RUNTIME: &str = "ghcr.io/example/lite@sha256:1111111111111111111111111111111111111111111111111111111111111111";

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn pin_text(url: &str, body: &[u8]) -> String {
        serde_json::json!({
            "schema": PIN_SCHEMA,
            "runtime": RUNTIME,
            "platforms": {"linux/arm64": {
                "asset": "oo-lite-runtime-linux-arm64.tar.gz",
                "url": format!("{url}/oo-lite-runtime-linux-arm64.tar.gz"),
                "size": body.len(),
                "sha256": sha(body),
                "manifest_sha256": "2".repeat(64),
                "image_id": format!("sha256:{}", "3".repeat(64)),
                "config_id": format!("sha256:{}", "4".repeat(64)),
            }}
        })
        .to_string()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oo-bundle-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        dir
    }

    /// A one-shot-per-connection HTTP server honouring `Range` when `ranges`.
    fn serve(
        body: Vec<u8>,
        ranges: bool,
        connections: usize,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for stream in listener.incoming().take(connections) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = None;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = v.trim().trim_end_matches('-').parse::<usize>().ok();
                    }
                }
                seen.push(
                    range
                        .map(|r| format!("range {r}"))
                        .unwrap_or_else(|| "full".into()),
                );
                let (status, part, extra) = match range.filter(|_| ranges) {
                    Some(r) => (
                        "206 Partial Content",
                        &body[r..],
                        format!(
                            "Content-Range: bytes {r}-{}/{}\r\n",
                            body.len() - 1,
                            body.len()
                        ),
                    ),
                    None => ("200 OK", &body[..], String::new()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    part.len()
                );
                let _ = stream.write_all(part);
            }
            seen
        });
        (url, handle)
    }

    #[test]
    fn pin_parsing_requires_matching_runtime_and_complete_entries() {
        let pin = parse_pin(
            &pin_text("https://github.com/o/r/releases/download/v0.2.0", b"x"),
            RUNTIME,
        )
        .must();
        assert_eq!(pin.entries.len(), 1);
        assert_eq!(pin.entries[0].platform, "linux/arm64");
        let other = RUNTIME.replace('1', "5");
        let error = parse_pin(&pin_text("https://h/x", b"x"), &other).unwrap_err();
        assert!(
            error
                .json_bytes()
                .windows(14)
                .any(|w| w == b"LITE_BUNDLE_PI")
        );
        for (key, value) in [
            (
                "url",
                serde_json::json!("ftp://h/oo-lite-runtime-linux-arm64.tar.gz"),
            ),
            (
                "url",
                serde_json::json!("http://example.com/oo-lite-runtime-linux-arm64.tar.gz"),
            ),
            ("url", serde_json::json!("https://h/other.tar.gz")),
            ("size", serde_json::json!(0)),
            ("size", serde_json::json!(3u64 * 1024 * 1024 * 1024)),
            ("sha256", serde_json::json!("ab")),
            ("image_id", serde_json::json!("latest")),
            ("asset", serde_json::json!("../x")),
        ] {
            let mut pin: Value = serde_json::from_str(&pin_text("https://h", b"x")).unwrap();
            pin["platforms"]["linux/arm64"][key] = value;
            let value = pin;
            assert!(parse_pin(&value.to_string(), RUNTIME).is_err(), "{key}");
        }
        let mut value: Value = serde_json::from_str(&pin_text("https://h", b"x")).unwrap();
        value["platforms"]["windows/amd64"] = value["platforms"]["linux/arm64"].clone();
        assert!(parse_pin(&value.to_string(), RUNTIME).is_err());
        assert!(parse_pin("{}", RUNTIME).is_err());
        assert!(parse_pin(&pin_text("http://127.0.0.1:9", b"x"), RUNTIME).is_ok());
    }

    #[test]
    fn daemon_platform_maps_docker_info_architectures() {
        assert_eq!(
            docker_platform("linux/aarch64\n").as_deref(),
            Some("linux/arm64")
        );
        assert_eq!(
            docker_platform("linux/x86_64").as_deref(),
            Some("linux/amd64")
        );
        assert_eq!(docker_platform("windows/x86_64"), None);
        assert_eq!(docker_platform("linux/s390x"), None);
        assert_eq!(docker_platform(""), None);
    }

    #[test]
    fn wrong_architecture_is_a_closed_error() {
        let pin = parse_pin(&pin_text("https://h", b"x"), RUNTIME).must();
        assert!(pin.entry(Some("linux/arm64")).is_ok());
        for platform in [Some("linux/amd64"), None] {
            let error = String::from_utf8(pin.entry(platform).unwrap_err().json_bytes()).unwrap();
            assert!(error.contains("LITE_BUNDLE_PLATFORM"), "{error}");
        }
        // A pre-downloaded file for another platform is named as such.
        let dir = temp("arch");
        let file = dir.join("bundle.tar.gz");
        fs::write(&file, b"amd64 bundle").unwrap();
        let mut value: Value =
            serde_json::from_str(&pin_text("https://h", b"arm64 bundle")).unwrap();
        let mut amd = value["platforms"]["linux/arm64"].clone();
        amd["asset"] = "oo-lite-runtime-linux-amd64.tar.gz".into();
        amd["url"] = "https://h/oo-lite-runtime-linux-amd64.tar.gz".into();
        amd["size"] = 12.into();
        amd["sha256"] = sha(b"amd64 bundle").into();
        value["platforms"]["linux/amd64"] = amd;
        let pin = parse_pin(&value.to_string(), RUNTIME).must();
        let error = String::from_utf8(
            install_file(&file, &pin, Some("linux/arm64"))
                .unwrap_err()
                .json_bytes(),
        )
        .unwrap();
        assert!(
            error.contains("LITE_BUNDLE_PLATFORM") && error.contains("linux/amd64 bundle"),
            "{error}"
        );
        assert!(file.exists(), "a user-supplied file is never removed");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn download_verifies_and_rejects_digest_mismatch() {
        let body = b"fake runtime bundle bytes".repeat(1000);
        let (url, server) = serve(body.clone(), true, 1);
        let pin = parse_pin(&pin_text(&url, &body), RUNTIME).must();
        let dir = temp("ok");
        let file = download(&pin.entries[0], &dir).must();
        assert_eq!(fs::read(&file).unwrap(), body);
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(server.join().unwrap(), ["full"]);

        // Same size, different content: SHA mismatch, partial file removed.
        let mut tampered = body.clone();
        tampered[10] ^= 1;
        let (url, server) = serve(tampered, true, 1);
        let pin = parse_pin(&pin_text(&url, &body), RUNTIME).must();
        let dir = temp("sha");
        let error =
            String::from_utf8(download(&pin.entries[0], &dir).unwrap_err().json_bytes()).unwrap();
        assert!(error.contains("LITE_BUNDLE_SHA256"), "{error}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        server.join().unwrap();

        // Oversized response: refused while streaming, partial removed.
        let (url, server) = serve([body.as_slice(), b"extra"].concat(), true, 1);
        let pin = parse_pin(&pin_text(&url, &body), RUNTIME).must();
        let dir = temp("big");
        let error =
            String::from_utf8(download(&pin.entries[0], &dir).unwrap_err().json_bytes()).unwrap();
        assert!(error.contains("LITE_BUNDLE_SHA256"), "{error}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        server.join().unwrap();
    }

    #[test]
    fn interrupted_download_keeps_partial_and_resumes() {
        let body = b"0123456789abcdef".repeat(4096);
        let dir = temp("resume");
        // Truncated transfer: keeps the partial file, resumable error.
        let (url, server) = serve(body[..20000].to_vec(), true, 1);
        let pin = parse_pin(&pin_text(&url, &body), RUNTIME).must();
        let error =
            String::from_utf8(download(&pin.entries[0], &dir).unwrap_err().json_bytes()).unwrap();
        assert!(error.contains("LITE_BUNDLE_DOWNLOAD"), "{error}");
        server.join().unwrap();
        let partial = dir.join(format!("{}.partial", pin.entries[0].sha256));
        assert_eq!(fs::metadata(&partial).unwrap().len(), 20000);
        // Resume with Range from 20000.
        let (url, server) = serve(body.clone(), true, 1);
        let pin = parse_pin(&pin_text(&url, &body), RUNTIME).must();
        let file = download(&pin.entries[0], &dir).must();
        assert_eq!(fs::read(&file).unwrap(), body);
        assert_eq!(server.join().unwrap(), ["range 20000"]);
        // A server ignoring Range (200) restarts the file from zero.
        fs::write(&partial, &body[..100]).unwrap();
        let (url, server) = serve(body.clone(), false, 1);
        let pin = parse_pin(&pin_text(&url, &body), RUNTIME).must();
        let file = download(&pin.entries[0], &dir).must();
        assert_eq!(fs::read(&file).unwrap(), body);
        assert_eq!(server.join().unwrap(), ["range 100"]);
        // A corrupt partial that already has the full size is discarded.
        let mut wrong = body.clone();
        wrong[0] ^= 1;
        fs::write(&partial, &wrong).unwrap();
        let (url, server) = serve(body.clone(), true, 1);
        let pin = parse_pin(&pin_text(&url, &body), RUNTIME).must();
        let file = download(&pin.entries[0], &dir).must();
        assert_eq!(fs::read(&file).unwrap(), body);
        assert_eq!(server.join().unwrap(), ["full"]);
        // An oversized stale partial is removed before downloading.
        fs::write(&partial, [body.as_slice(), b"zz"].concat()).unwrap();
        let (url, server) = serve(body.clone(), true, 1);
        let pin = parse_pin(&pin_text(&url, &body), RUNTIME).must();
        download(&pin.entries[0], &dir).must();
        assert_eq!(server.join().unwrap(), ["full"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
