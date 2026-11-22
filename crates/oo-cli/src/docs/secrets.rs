//! State directory, single-instance lock, tokens and `serve.json` (10 §8.6).
//!
//! Everything secret is a 0600 file created with `O_CREAT|O_EXCL|O_NOFOLLOW`
//! and fsynced; the state directory is 0700 and never a symlink. Tokens are
//! `oodt_` (control, for the CLI and Desktop) and `oodi_` (engine, between the
//! daemon and the engine) plus base64url of 32 random bytes, and are rotated at
//! every start. The daemon keeps only SHA-256 of the control token in memory.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// `oo.docs.serve/v1`.
pub const SERVE_PROTOCOL: &str = "oo.docs.serve/v1";
/// Control API version served.
pub const API_VERSION: &str = "1.0";

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding.
pub fn b64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// `prefix` + base64url of 32 bytes from the operating system.
pub fn new_token(prefix: &str) -> io::Result<String> {
    let mut buf = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(format!("{prefix}{}", b64url(&buf)))
}

/// Why the state directory is unusable (exit code 5).
#[derive(Debug, PartialEq, Eq)]
pub struct StateDirError(pub &'static str);

/// Default state directory: `$HOME/.local/state/open-ontology/docs`.
pub fn default_state_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        Path::new(&home)
            .join(".local")
            .join("state")
            .join("open-ontology")
            .join("docs"),
    )
}

/// Create (0700) or check the state directory. Missing levels are created
/// 0700; the final directory must not be a symlink and must not be readable
/// by group or others.
pub fn ensure_state_dir(root: &Path) -> Result<(), StateDirError> {
    if !root.is_absolute() {
        return Err(StateDirError("state directory must be an absolute path"));
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(root)
        .map_err(|_| StateDirError("state directory cannot be created"))?;
    let meta = fs::symlink_metadata(root)
        .map_err(|_| StateDirError("state directory cannot be inspected"))?;
    if !meta.is_dir() {
        return Err(StateDirError("state directory is not a directory"));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(StateDirError("state directory must be mode 0700"));
    }
    Ok(())
}

/// Write `content` to a new 0600 file (`O_EXCL|O_NOFOLLOW`, fsynced). An older
/// file of that name is unlinked first, so the result is always a fresh
/// regular file owned by this process.
pub fn write_secret_file(path: &Path, content: &str) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    f.write_all(content.as_bytes())?;
    f.sync_all()
}

/// Replace `path` atomically with a 0600 file holding `content`.
pub fn write_secret_atomic(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    write_secret_file(&tmp, content)?;
    fs::rename(&tmp, path)
}

/// The single-instance lock `<state>/serve.lock`; released on drop.
pub struct InstanceLock {
    _file: File,
}

/// Outcome of taking the lock.
pub enum LockResult {
    /// This process is the instance.
    Held(InstanceLock),
    /// Another `oo serve` holds it.
    Busy,
}

fn open_lock(state: &Path) -> io::Result<File> {
    let f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(state.join("serve.lock"))?;
    let m = f.metadata()?;
    if !m.is_file() || m.nlink() != 1 || m.mode() & 0o777 != 0o600 {
        return Err(io::Error::other("serve.lock is not a private regular file"));
    }
    Ok(f)
}

/// Take the lock without waiting.
pub fn acquire_lock(state: &Path) -> io::Result<LockResult> {
    let f = open_lock(state)?;
    Ok(match f.try_lock() {
        Ok(()) => LockResult::Held(InstanceLock { _file: f }),
        Err(_) => LockResult::Busy,
    })
}

/// Take the lock, waiting for an instance that is shutting down.
///
/// The lock is held until the old process has exited, and `serve.json` is removed as soon as it begins to stop (a compose
/// runtime needs about 20 s to bring its containers down). A start in that window finds the lock busy and no `serve.json`:
/// that is an instance on its way out (or still starting), not a running one. It is waited for up to `patience`; the moment
/// a `serve.json` appears the instance is running and `Busy` is returned so the caller reuses it.
pub fn acquire_lock_waiting(state: &Path, patience: std::time::Duration) -> io::Result<LockResult> {
    let started = std::time::Instant::now();
    loop {
        match acquire_lock(state)? {
            LockResult::Held(l) => return Ok(LockResult::Held(l)),
            LockResult::Busy => {
                if read_serve_json(state).is_some() || started.elapsed() >= patience {
                    return Ok(LockResult::Busy);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

/// Is an instance holding the lock right now? (The probe takes and drops it.)
pub fn is_running(state: &Path) -> bool {
    match open_lock(state) {
        Ok(f) => f.try_lock().is_err(),
        Err(_) => false,
    }
}

/// Seconds since the epoch as RFC 3339 UTC.
pub fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01 (Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The `serve.json` document (10 §8.6 point 5). `engine` describes how the
/// engine runs; for L2 it is a host process (`"mode": "host_process"`).
pub fn serve_json(
    pid: u32,
    port: u16,
    token_file: &Path,
    state_dir: &Path,
    started_at: &str,
    engine: Value,
) -> Value {
    json!({
        "protocol": SERVE_PROTOCOL,
        "pid": pid,
        "port": port,
        "api_version": API_VERSION,
        "token_file": token_file,
        "state_dir": state_dir,
        "started_at": started_at,
        "engine": engine,
    })
}

/// Read `<state>/serve.json`.
pub fn read_serve_json(state: &Path) -> Option<Value> {
    let text = fs::read_to_string(state.join("serve.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    (v["protocol"] == SERVE_PROTOCOL).then_some(v)
}

/// Remove the files a stopped (or crashed) instance leaves behind.
pub fn clean_runtime_files(state: &Path) {
    for name in [
        "serve.json",
        "serve.token",
        "serve.json.tmp",
        "engine.token",
        "engine.pid",
    ] {
        let _ = fs::remove_file(state.join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("oo-docs-secrets-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn a_start_right_after_a_stop_waits_for_the_old_instance_to_let_go() {
        let base = tmp("restart");
        ensure_state_dir(&base).unwrap();
        // The dying instance: it holds the lock, serve.json is already gone, and it lets go after 700 ms.
        let held = match acquire_lock(&base).unwrap() {
            LockResult::Held(l) => l,
            LockResult::Busy => panic!("free lock reported busy"),
        };
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(700));
            drop(held);
        });
        let t0 = std::time::Instant::now();
        let got = acquire_lock_waiting(&base, std::time::Duration::from_secs(10)).unwrap();
        assert!(
            matches!(got, LockResult::Held(_)),
            "the new instance must get the lock"
        );
        assert!(
            t0.elapsed() >= std::time::Duration::from_millis(600),
            "it waited for the release"
        );
        release.join().unwrap();
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_running_instance_is_reported_at_once_and_a_stuck_one_after_the_patience() {
        let base = tmp("running");
        ensure_state_dir(&base).unwrap();
        let _held = match acquire_lock(&base).unwrap() {
            LockResult::Held(l) => l,
            LockResult::Busy => panic!("free lock reported busy"),
        };
        // Stuck: no serve.json ever appears, so the wait ends after the patience with Busy.
        let t0 = std::time::Instant::now();
        let got = acquire_lock_waiting(&base, std::time::Duration::from_millis(400)).unwrap();
        assert!(matches!(got, LockResult::Busy));
        assert!(t0.elapsed() >= std::time::Duration::from_millis(350));
        // Running: serve.json exists, so Busy comes back at once (the caller reuses that instance).
        write_secret_file(
            &base.join("serve.json"),
            &format!("{{\"protocol\":\"{SERVE_PROTOCOL}\",\"port\":1}}"),
        )
        .unwrap();
        let t1 = std::time::Instant::now();
        let got = acquire_lock_waiting(&base, std::time::Duration::from_secs(30)).unwrap();
        assert!(matches!(got, LockResult::Busy));
        assert!(t1.elapsed() < std::time::Duration::from_secs(2));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn base64url_matches_known_vectors() {
        assert_eq!(b64url(b""), "");
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
        assert_eq!(b64url(&[0xfb, 0xff, 0xfe]), "-__-");
        assert_eq!(b64url(&[0u8; 32]).len(), 43);
    }

    #[test]
    fn tokens_have_the_documented_shape_and_differ() {
        let a = new_token("oodt_").unwrap();
        let b = new_token("oodt_").unwrap();
        assert!(a.starts_with("oodt_") && a.len() == 5 + 43);
        assert_ne!(a, b);
        assert!(
            a[5..]
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
    }

    #[test]
    fn state_dir_is_created_private_and_loose_modes_are_refused() {
        let root = tmp("dir").join("a").join("b");
        ensure_state_dir(&root).unwrap();
        assert_eq!(fs::metadata(&root).unwrap().mode() & 0o777, 0o700);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            ensure_state_dir(&root),
            Err(StateDirError("state directory must be mode 0700"))
        );
        assert!(ensure_state_dir(Path::new("relative")).is_err());
        let _ = fs::remove_dir_all(root.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn state_dir_symlink_is_refused() {
        let base = tmp("link");
        fs::create_dir_all(&base).unwrap();
        let real = base.join("real");
        fs::create_dir(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(ensure_state_dir(&link).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn secret_files_are_0600_fresh_and_never_follow_symlinks() {
        let base = tmp("secret");
        ensure_state_dir(&base).unwrap();
        let f = base.join("serve.token");
        write_secret_file(&f, "one").unwrap();
        assert_eq!(fs::metadata(&f).unwrap().mode() & 0o777, 0o600);
        write_secret_file(&f, "two").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "two");
        // A planted symlink is replaced, never written through.
        let victim = base.join("victim");
        fs::write(&victim, "keep").unwrap();
        fs::remove_file(&f).unwrap();
        std::os::unix::fs::symlink(&victim, &f).unwrap();
        write_secret_file(&f, "three").unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
        assert!(!fs::symlink_metadata(&f).unwrap().file_type().is_symlink());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn second_instance_is_busy_until_the_first_drops() {
        let base = tmp("lock");
        ensure_state_dir(&base).unwrap();
        let first = match acquire_lock(&base).unwrap() {
            LockResult::Held(l) => l,
            LockResult::Busy => panic!("free lock reported busy"),
        };
        assert!(is_running(&base));
        assert!(matches!(acquire_lock(&base).unwrap(), LockResult::Busy));
        drop(first);
        assert!(!is_running(&base));
        assert!(matches!(acquire_lock(&base).unwrap(), LockResult::Held(_)));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rfc3339_known_instants() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_795_000_000), "2026-11-18T11:06:40Z");
    }

    #[test]
    fn serve_json_has_the_documented_fields() {
        let v = serve_json(
            7,
            8123,
            Path::new("/s/serve.token"),
            Path::new("/s"),
            "2026-11-17T00:00:00Z",
            json!({"mode": "host_process"}),
        );
        for k in [
            "protocol",
            "pid",
            "port",
            "api_version",
            "token_file",
            "state_dir",
            "started_at",
            "engine",
        ] {
            assert!(v.get(k).is_some(), "{k}");
        }
        assert_eq!(v["protocol"], "oo.docs.serve/v1");
        assert_eq!(v["api_version"], "1.0");
    }
}
