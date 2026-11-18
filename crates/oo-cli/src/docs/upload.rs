//! `oo docs upload` and `oo docs wait` (D83, WP1).
//!
//! ```text
//! oo docs upload <path>... [--drive <name|id>] [--to <dir>] [--name <file>] [--wait terminal|published|none]
//!                          [--timeout 120s] [--json]
//! oo docs wait [--drive <name|id>] [--state terminal|published] [--rev <n>] [--timeout 120s] [--json]
//! ```
//!
//! The engine has no upload route: files are placed where the Drive reads them
//! and the Drive is told with a targeted `:sync` (D47). For an `fs` Drive the
//! file is copied the way the Desktop's upload service does it: written to a
//! hidden `.part` file, fsynced, then moved into place without overwriting an
//! existing file (a clash becomes `name (2).ext`). Directories are walked,
//! symlinks and `..` are refused. For an object-store Drive the same relative
//! path is `PUT` with the Drive's own credential, if that credential can write.
//! Without `--drive`, the managed Drive `default` is used and created on the
//! first upload: a folder under the oo state directory, no cloud account and no
//! bucket. Neither command ever prompts, so an agent can run them.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::client::{CliError, Daemon, exit};
use super::docs_cmd::{Parsed, bad, drive_id, out, state_of};
use super::s3io::{S3Target, short_id};

/// Name of the managed Drive a first upload creates.
pub const MANAGED_DRIVE: &str = "default";
const MAX_FILES: usize = 5000;
const MAX_S3_OBJECT: u64 = 512 * 1024 * 1024;
const MAX_STDIN: u64 = 256 * 1024 * 1024;
const MAX_DEPTH: usize = 32;
const SYNC_CHUNK: usize = 400;

/// A file to place: where it comes from and its Drive-relative path.
#[derive(Debug)]
pub struct Item {
    pub src: Source,
    pub rel: String,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum Source {
    Path(PathBuf),
    Stdin(Vec<u8>),
}

fn err(code: &str, msg: impl Into<String>) -> CliError {
    CliError::new(exit::ARGS, code, msg)
}

/// `120`, `120s`, `2m`, `1h`.
pub fn parse_duration(text: &str) -> Option<Duration> {
    let t = text.trim();
    let (num, unit) = match t.strip_suffix('s') {
        Some(n) => (n, 1),
        None => match t.strip_suffix('m') {
            Some(n) => (n, 60),
            None => match t.strip_suffix('h') {
                Some(n) => (n, 3600),
                None => (t, 1),
            },
        },
    };
    let n: u64 = num.parse().ok()?;
    (n > 0).then(|| Duration::from_secs(n.saturating_mul(unit)))
}

fn clean_segment(seg: &str) -> Result<(), CliError> {
    if seg.is_empty()
        || seg == "."
        || seg == ".."
        || seg.len() > 255
        || seg.chars().any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err(err(
            "SO_UPLOAD_BAD_PATH",
            format!("`{seg}` is not a usable file name"),
        ));
    }
    Ok(())
}

/// `--to`: a relative directory inside the Drive, no `..`.
pub fn check_to(to: &str) -> Result<String, CliError> {
    let t = to.trim_matches('/');
    if to.starts_with('/') || to.contains("//") {
        return Err(err(
            "SO_UPLOAD_BAD_PATH",
            "--to must be a relative directory inside the Drive",
        ));
    }
    if t.is_empty() {
        return Ok(String::new());
    }
    for seg in t.split('/') {
        clean_segment(seg)?;
    }
    Ok(t.to_owned())
}

fn join_rel(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_owned()
    } else {
        format!("{base}/{name}")
    }
}

fn file_name(p: &Path) -> Result<String, CliError> {
    let n = p
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| err("SO_UPLOAD_BAD_PATH", "a file name is not valid UTF-8"))?;
    clean_segment(n)?;
    Ok(n.to_owned())
}

/// Collect what the arguments name. Symlinks anywhere are an error; hidden
/// files (`.name`) inside a directory are skipped and counted.
pub fn collect(
    paths: &[String],
    to: &str,
    stdin_name: Option<&str>,
) -> Result<(Vec<Item>, usize), CliError> {
    let mut items = Vec::new();
    let mut skipped = 0usize;
    for arg in paths {
        if arg == "-" {
            let name = stdin_name.ok_or_else(|| {
                err(
                    "SO_UPLOAD_NEEDS_NAME",
                    "`-` (stdin) needs --name <file name>",
                )
            })?;
            clean_segment(name)?;
            let mut buf = Vec::new();
            std::io::stdin()
                .take(MAX_STDIN + 1)
                .read_to_end(&mut buf)
                .map_err(|_| err("SO_UPLOAD_READ", "cannot read stdin"))?;
            if buf.len() as u64 > MAX_STDIN {
                return Err(err("SO_UPLOAD_TOO_LARGE", "stdin is over 256 MiB"));
            }
            let bytes = buf.len() as u64;
            items.push(Item {
                src: Source::Stdin(buf),
                rel: join_rel(to, name),
                bytes,
            });
            continue;
        }
        let p = Path::new(arg);
        let meta = fs::symlink_metadata(p)
            .map_err(|_| err("SO_UPLOAD_NOT_FOUND", format!("`{arg}` does not exist")))?;
        if meta.file_type().is_symlink() {
            return Err(err(
                "SO_UPLOAD_SYMLINK",
                format!("`{arg}` is a symbolic link; upload the real file"),
            ));
        }
        if meta.is_file() {
            items.push(Item {
                src: Source::Path(p.to_path_buf()),
                rel: join_rel(
                    to,
                    &file_name(&fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))?,
                ),
                bytes: meta.len(),
            });
        } else if meta.is_dir() {
            let real = fs::canonicalize(p)
                .map_err(|_| err("SO_UPLOAD_NOT_FOUND", "directory vanished"))?;
            let top = join_rel(to, &file_name(&real)?);
            walk(&real, &top, 0, &mut items, &mut skipped)?;
        } else {
            return Err(err(
                "SO_UPLOAD_NOT_FILE",
                format!("`{arg}` is not a regular file or directory"),
            ));
        }
        if items.len() > MAX_FILES {
            return Err(err(
                "SO_UPLOAD_TOO_MANY",
                format!("more than {MAX_FILES} files; upload in parts"),
            ));
        }
    }
    if items.is_empty() {
        return Err(err("SO_UPLOAD_EMPTY", "there is nothing to upload"));
    }
    Ok((items, skipped))
}

fn walk(
    dir: &Path,
    rel: &str,
    depth: usize,
    items: &mut Vec<Item>,
    skipped: &mut usize,
) -> Result<(), CliError> {
    if depth > MAX_DEPTH {
        return Err(err(
            "SO_UPLOAD_TOO_DEEP",
            "directories are nested more than 32 levels",
        ));
    }
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|_| {
            err(
                "SO_UPLOAD_READ",
                format!("cannot read directory {}", dir.display()),
            )
        })?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for e in entries {
        let path = e.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            return Err(err("SO_UPLOAD_BAD_PATH", "a file name is not valid UTF-8"));
        };
        if name.starts_with('.') {
            *skipped += 1;
            continue;
        }
        clean_segment(&name)?;
        let meta =
            fs::symlink_metadata(&path).map_err(|_| err("SO_UPLOAD_READ", "cannot stat a file"))?;
        if meta.file_type().is_symlink() {
            return Err(err(
                "SO_UPLOAD_SYMLINK",
                format!(
                    "{} is a symbolic link; upload the real file",
                    path.display()
                ),
            ));
        }
        let child = join_rel(rel, &name);
        if meta.is_dir() {
            walk(&path, &child, depth + 1, items, skipped)?;
        } else if meta.is_file() {
            items.push(Item {
                src: Source::Path(path),
                rel: child,
                bytes: meta.len(),
            });
            if items.len() > MAX_FILES {
                return Err(err(
                    "SO_UPLOAD_TOO_MANY",
                    format!("more than {MAX_FILES} files; upload in parts"),
                ));
            }
        } else {
            *skipped += 1;
        }
    }
    Ok(())
}

/// `name (2).ext` style candidate for clash number `n` (`n >= 2`).
pub fn clash_name(name: &str, n: u32) -> String {
    match name.rfind('.').filter(|i| *i > 0) {
        Some(i) => format!("{} ({n}){}", &name[..i], &name[i..]),
        None => format!("{name} ({n})"),
    }
}

fn io_err(what: &str, e: &std::io::Error) -> CliError {
    CliError::new(
        exit::OTHER,
        "SO_UPLOAD_WRITE_FAILED",
        format!("{what}: {}", e.kind()),
    )
}

/// Create the parent directories of `root/rel` one level at a time, refusing a
/// symlink at any level. Returns the directory of the file.
fn ensure_parent(root: &Path, rel: &str) -> Result<PathBuf, CliError> {
    let mut dir = root.to_path_buf();
    let mut segs: Vec<&str> = rel.split('/').collect();
    segs.pop();
    for seg in segs {
        dir.push(seg);
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(err(
                    "SO_UPLOAD_SYMLINK",
                    "the Drive contains a symbolic link on the way",
                ));
            }
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                return Err(err(
                    "SO_UPLOAD_BAD_PATH",
                    "a file is in the way of a directory",
                ));
            }
            Err(_) => {
                fs::DirBuilder::new()
                    .mode(0o755)
                    .create(&dir)
                    .map_err(|e| io_err("create directory", &e))?;
            }
        }
    }
    Ok(dir)
}

fn open_source(p: &Path) -> Result<File, CliError> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(p)
        .map_err(|_| err("SO_UPLOAD_READ", format!("cannot open {}", p.display())))?;
    if !f.metadata().is_ok_and(|m| m.is_file()) {
        return Err(err(
            "SO_UPLOAD_NOT_FILE",
            format!("{} is not a regular file", p.display()),
        ));
    }
    Ok(f)
}

/// Place one file under `root`; returns the final Drive-relative path.
pub fn place_fs(root: &Path, item: &Item) -> Result<String, CliError> {
    let dir = ensure_parent(root, &item.rel)?;
    let name = item.rel.rsplit('/').next().unwrap_or(&item.rel).to_owned();
    let part = dir.join(format!(".{name}.oo-upload-{}.part", short_id()));
    let write = || -> Result<(), CliError> {
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&part)
            .map_err(|e| io_err("create part file", &e))?;
        match &item.src {
            Source::Path(p) => {
                let mut src = open_source(p)?;
                std::io::copy(&mut src, &mut out).map_err(|e| io_err("copy", &e))?;
            }
            Source::Stdin(b) => {
                std::io::Write::write_all(&mut out, b).map_err(|e| io_err("write", &e))?;
            }
        }
        out.sync_all().map_err(|e| io_err("fsync", &e))
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    let mut n = 1u32;
    let result = loop {
        let candidate = if n == 1 {
            name.clone()
        } else {
            clash_name(&name, n)
        };
        // hard_link never replaces an existing file: the clash check and the
        // publish are one atomic step.
        match fs::hard_link(&part, dir.join(&candidate)) {
            Ok(()) => break Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
            Err(e) => break Err(io_err("move into place", &e)),
        }
    };
    let _ = fs::remove_file(&part);
    let final_name = result?;
    if let Ok(d) = File::open(&dir) {
        let _ = d.sync_all();
    }
    let parent_rel = item.rel.rsplit_once('/').map(|(p, _)| p);
    Ok(match parent_rel {
        Some(p) => format!("{p}/{final_name}"),
        None => final_name,
    })
}

async fn put_s3(t: &S3Target, item: &Item) -> Result<String, CliError> {
    if item.bytes > MAX_S3_OBJECT {
        return Err(err(
            "SO_UPLOAD_TOO_LARGE",
            "object-store uploads are limited to 512 MiB per file",
        ));
    }
    let body = match &item.src {
        Source::Path(p) => {
            let mut b = Vec::new();
            open_source(p)?
                .read_to_end(&mut b)
                .map_err(|_| err("SO_UPLOAD_READ", "cannot read the file"))?;
            b
        }
        Source::Stdin(b) => b.clone(),
    };
    let name = item.rel.rsplit('/').next().unwrap_or(&item.rel).to_owned();
    let dir = item.rel.rsplit_once('/').map(|(p, _)| p);
    let mut n = 1u32;
    loop {
        let candidate = if n == 1 {
            name.clone()
        } else {
            clash_name(&name, n)
        };
        let rel = match dir {
            Some(d) => format!("{d}/{candidate}"),
            None => candidate,
        };
        let key = t.key(&rel);
        let unavailable =
            |m: String| CliError::new(exit::OTHER, "SO_UPLOAD_BACKEND_UNAVAILABLE", m);
        if t.exists(&key).await.map_err(unavailable)? {
            n += 1;
            if n > 1000 {
                return Err(err("SO_UPLOAD_BAD_PATH", "too many files with that name"));
            }
            continue;
        }
        let reply = t
            .put(&key, body)
            .await
            .map_err(|m| CliError::new(exit::OTHER, "SO_UPLOAD_BACKEND_UNAVAILABLE", m))?;
        return match reply.status {
            200 | 201 | 204 => Ok(rel),
            401 | 403 => Err(CliError::new(
                exit::OTHER,
                "SO_UPLOAD_BACKEND_READ_ONLY",
                format!(
                    "the bucket credential cannot write ({}); this Drive is read-only for oo. Upload with the cloud's own tool, or use a credential with write access",
                    reply.code.as_deref().unwrap_or("denied")
                ),
            )),
            s => Err(CliError::new(
                exit::OTHER,
                "SO_UPLOAD_FAILED",
                format!(
                    "the object store answered {s} {}",
                    reply.code.unwrap_or_default()
                ),
            )),
        };
    }
}

/// The Drive to write to: by name or id, or the managed `default`, created on demand.
async fn resolve_drive(
    d: &Daemon,
    state: &Path,
    want: Option<&str>,
) -> Result<(Value, bool), CliError> {
    let list = d.get_json("/v1/docs/drives?limit=200").await?;
    let name = want.unwrap_or(MANAGED_DRIVE);
    let found = list["items"].as_array().into_iter().flatten().find(|i| {
        i["id"] == name
            || i["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
    });
    if let Some(v) = found {
        return Ok((v.clone(), false));
    }
    if !name.eq_ignore_ascii_case(MANAGED_DRIVE) {
        return Err(CliError::new(
            exit::OTHER,
            "DOCS_SOURCE_DRIVE_NOT_FOUND",
            format!(
                "no drive {name}; see `oo source ls`, or omit --drive to use the managed folder"
            ),
        ));
    }
    let root = state.join("drives").join(MANAGED_DRIVE);
    for p in [state.join("drives"), root.clone()] {
        fs::DirBuilder::new()
            .mode(0o755)
            .recursive(true)
            .create(&p)
            .map_err(|e| io_err("create the managed folder", &e))?;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o755));
    }
    let root = fs::canonicalize(&root).map_err(|e| io_err("resolve the managed folder", &e))?;
    let v = d
        .post_json(
            "/v1/docs/drives",
            &json!({"name": MANAGED_DRIVE, "backend": "fs", "config": {"root": root.display().to_string()}}),
        )
        .await?;
    Ok((v, true))
}

/// Host directory behind an `fs` Drive: the configured root, or, when the
/// compose runtime rewrote it to `/drives/<id>`, the directory it mounts.
pub fn host_root(state: &Path, drive: &Value) -> Result<PathBuf, CliError> {
    let root = drive["config"]["root"].as_str().unwrap_or("");
    if let Some(id) = root.strip_prefix("/drives/") {
        let id = id.trim_end_matches('/');
        return super::compose::read_mounts(state)
            .remove(id)
            .ok_or_else(|| {
                CliError::new(
                    exit::OTHER,
                    "SO_UPLOAD_ROOT_UNKNOWN",
                    "cannot find the host folder of this Drive in the daemon's mounts",
                )
            });
    }
    let p = PathBuf::from(root);
    if p.is_dir() {
        Ok(p)
    } else {
        Err(CliError::new(
            exit::OTHER,
            "SO_UPLOAD_ROOT_UNKNOWN",
            "the Drive's folder is not on this machine",
        ))
    }
}

/// The credential text behind a Drive's `secret_ref`.
pub fn read_drive_secret(config: &Value) -> Result<String, CliError> {
    let reference = config["secret_ref"].as_str().ok_or_else(|| {
        CliError::new(
            exit::OTHER,
            "SO_SOURCE_NO_CREDENTIAL",
            "the Drive has no secret_ref",
        )
    })?;
    let store = oo_stack::secret::SecretStore::from_home().map_err(|c| {
        CliError::new(
            exit::OTHER,
            "SO_SOURCE_NO_CREDENTIAL",
            format!("secret store: {c}"),
        )
    })?;
    store.read(reference).map_err(|c| {
        CliError::new(
            exit::OTHER,
            "SO_SOURCE_NO_CREDENTIAL",
            format!("credential `{reference}` cannot be read ({c})"),
        )
    })
}

pub fn s3_target(drive: &Value) -> Result<S3Target, CliError> {
    let text = read_drive_secret(&drive["config"])?;
    S3Target::from_drive(
        drive["backend"].as_str().unwrap_or("s3"),
        &drive["config"],
        &text,
    )
    .map_err(|m| CliError::new(exit::OTHER, "SO_SOURCE_BAD_CONFIG", m))
}

/// `oo docs upload`.
pub async fn upload(d: &Daemon, p: &Parsed) -> Result<u8, CliError> {
    if p.pos.is_empty() {
        return Err(bad(
            "expected <path>... (files or directories; `-` reads stdin with --name)",
        ));
    }
    let wait_mode = p.opt("--wait").unwrap_or("none");
    if !matches!(wait_mode, "none" | "terminal" | "published") {
        return Err(bad("--wait is none, terminal or published"));
    }
    let timeout = match p.opt("--timeout") {
        Some(t) => parse_duration(t).ok_or_else(|| bad("--timeout is like 120s, 2m or 1h"))?,
        None => Duration::from_secs(120),
    };
    let to = check_to(p.opt("--to").unwrap_or(""))?;
    let (items, skipped) = collect(&p.pos, &to, p.opt("--name"))?;
    let state = state_of(p)?;
    let (drive, created) = resolve_drive(d, &state, p.opt("--drive")).await?;
    let id = drive["id"].as_str().unwrap_or_default().to_owned();
    if drive["status"].as_str().is_some_and(|s| s != "active") {
        return Err(CliError::new(
            exit::OTHER,
            "DOCS_SOURCE_DRIVE_NOT_ACTIVE",
            "the Drive is not active",
        ));
    }
    let backend = drive["backend"].as_str().unwrap_or("fs").to_owned();
    let mut placed: Vec<Value> = Vec::new();
    let mut rels: Vec<String> = Vec::new();
    if backend == "fs" {
        let root = host_root(&state, &drive)?;
        for it in &items {
            let rel = place_fs(&root, it)?;
            placed.push(json!({"path": rel, "bytes": it.bytes, "renamed": rel != it.rel}));
            rels.push(rel);
        }
    } else if matches!(backend.as_str(), "s3" | "oss" | "cos" | "r2") {
        let target = s3_target(&drive)?;
        for it in &items {
            let rel = put_s3(&target, it).await?;
            placed.push(json!({"path": rel, "bytes": it.bytes, "renamed": rel != it.rel}));
            rels.push(rel);
        }
    } else {
        return Err(CliError::new(
            exit::OTHER,
            "SO_UPLOAD_UNSUPPORTED_BACKEND",
            format!("uploading into a `{backend}` Drive is not supported"),
        ));
    }
    let mut added = 0i64;
    let mut changed = 0i64;
    let mut drive_rev = Value::Null;
    for chunk in rels.chunks(SYNC_CHUNK) {
        let v = d
            .post_json(
                &format!("/v1/docs/drives/{id}:sync"),
                &json!({"paths": chunk}),
            )
            .await?;
        added += v["added"].as_i64().unwrap_or(0);
        changed += v["changed"].as_i64().unwrap_or(0);
        drive_rev = v["drive_rev"].clone();
    }
    let mut result = json!({
        "drive": {"id": id, "name": drive["name"], "backend": backend},
        "managed": drive["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(MANAGED_DRIVE)),
        "created_drive": created,
        "files": placed,
        "uploaded": rels.len(),
        "skipped_hidden": skipped,
        "added": added,
        "changed": changed,
        "drive_rev": drive_rev,
    });
    let mut code = exit::OK;
    let mut human = format!(
        "{}uploaded {} file(s) to drive {} (drive_rev {})",
        if created {
            "created the managed drive `default`; "
        } else {
            ""
        },
        rels.len(),
        drive["name"].as_str().unwrap_or(""),
        drive_rev
    );
    if wait_mode != "none" {
        let rev = drive_rev.as_i64();
        let (mut report, c) = wait_for(d, &id, wait_mode, timeout, rev).await?;
        add_reason(&mut report, &state);
        human.push_str(&format!("\n{}", wait_human(&report)));
        result["wait"] = report;
        code = c;
    }
    out(p, "oo.docs-upload/v1", result, human);
    Ok(code)
}

/// Why files are `unsupported`: the parser note `oo serve` wrote to serve.json,
/// when files of that state exist and no parser is configured.
pub fn unsupported_hint(counts: &Value, serve_json: Option<&Value>) -> Option<String> {
    if counts["unsupported"].as_i64().unwrap_or(0) == 0 {
        return None;
    }
    let parser = &serve_json?["engine"]["parser"];
    (parser["mode"] == "none")
        .then(|| parser["note"].as_str().map(str::to_owned))
        .flatten()
}

/// Add `unsupported_reason` to a wait report.
fn add_reason(report: &mut Value, state: &Path) {
    let doc = super::secrets::read_serve_json(state);
    if let Some(why) = unsupported_hint(&report["counts"], doc.as_ref()) {
        report["unsupported_reason"] = json!(why);
    }
}

const IN_FLIGHT: [&str; 6] = [
    "stored",
    "queued",
    "parsing",
    "parsed",
    "extracting",
    "outdated",
];
const FAILED: [&str; 5] = [
    "parse_failed",
    "extract_partial",
    "unsupported",
    "too_large",
    "encrypted",
];

/// Summary of a drive's per-state counts: `(settled, failed)`.
///
/// `indexed` is the step before `published`. It is in flight until the Drive
/// has nothing processing and something in the review queue (`needs_person`);
/// only then does a file still `indexed` wait for a person and count as settled,
/// except with `published`.
pub fn judge(counts: &Value, mode: &str, needs_person: bool) -> (bool, i64) {
    let mut settled = true;
    let mut failed = 0;
    if let Some(m) = counts.as_object() {
        for (state, n) in m {
            let n = n.as_i64().unwrap_or(0);
            if n == 0 {
                continue;
            }
            if IN_FLIGHT.contains(&state.as_str())
                || (state == "indexed" && (mode == "published" || !needs_person))
            {
                settled = false;
            }
            if FAILED.contains(&state.as_str()) {
                failed += n;
            }
        }
    }
    (settled, failed)
}

fn wait_human(r: &Value) -> String {
    format!(
        "{}: {} (counts {})",
        if r["settled"] == true {
            "settled"
        } else {
            "not settled"
        },
        if r["failed"].as_i64().unwrap_or(0) > 0 {
            format!("{} file(s) failed or unsupported", r["failed"])
        } else {
            "no failures".to_owned()
        },
        r["counts"]
    ) + &r["unsupported_reason"]
        .as_str()
        .map(|w| format!("\nunsupported because: {w}"))
        .unwrap_or_default()
}

/// Poll a drive until its files left the in-flight states. Returns the report
/// and the exit code (0 settled without failures, 10 settled with failures,
/// 9 timed out).
pub async fn wait_for(
    d: &Daemon,
    id: &str,
    mode: &str,
    timeout: Duration,
    min_rev: Option<i64>,
) -> Result<(Value, u8), CliError> {
    let start = Instant::now();
    loop {
        let v = d.get_json(&format!("/v1/docs/drives/{id}")).await?;
        let rev = v["drive_rev"].as_i64().unwrap_or(0);
        let sync_running = v["last_sync"]["status"] == "running";
        let fresh = &v["freshness"];
        let needs_person = fresh["processing"].as_i64().unwrap_or(0) == 0
            && (fresh["pending_review"].as_i64().unwrap_or(0) > 0
                || v["pending_review"].as_i64().unwrap_or(0) > 0);
        let (mut settled, failed) = judge(&v["counts"], mode, needs_person);
        settled = settled && !sync_running && min_rev.is_none_or(|m| rev >= m);
        let report = json!({
            "settled": settled,
            "failed": failed,
            "counts": v["counts"],
            "drive_rev": rev,
            "awaiting_review": v["counts"]["indexed"].as_i64().unwrap_or(0),
            "waited_ms": start.elapsed().as_millis() as u64,
        });
        if settled {
            let code = if failed > 0 {
                exit::FILES_FAILED
            } else {
                exit::OK
            };
            return Ok((report, code));
        }
        if start.elapsed() >= timeout {
            return Ok((report, exit::WAIT_TIMEOUT));
        }
        let pause = if start.elapsed() < Duration::from_secs(10) {
            300
        } else {
            1000
        };
        tokio::time::sleep(Duration::from_millis(pause)).await;
    }
}

/// `oo docs wait`.
pub async fn wait(d: &Daemon, p: &Parsed) -> Result<u8, CliError> {
    let mode = p.opt("--state").unwrap_or("terminal");
    if !matches!(mode, "terminal" | "published") {
        return Err(bad("--state is terminal or published"));
    }
    let timeout = match p.opt("--timeout") {
        Some(t) => parse_duration(t).ok_or_else(|| bad("--timeout is like 120s, 2m or 1h"))?,
        None => Duration::from_secs(120),
    };
    let rev = match p.opt("--rev") {
        Some(r) => Some(
            r.parse::<i64>()
                .map_err(|_| bad("--rev is a drive_rev number"))?,
        ),
        None => None,
    };
    let want = p.opt("--drive").unwrap_or(MANAGED_DRIVE);
    let id = drive_id(d, want).await?;
    let (report, code) = wait_for(d, &id, mode, timeout, rev).await?;
    let mut r = report;
    r["drive_id"] = json!(id);
    add_reason(&mut r, &state_of(p)?);
    let human = wait_human(&r);
    out(p, "oo.docs-wait/v1", r, human);
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "oo-upload-{tag}-{}-{}",
            std::process::id(),
            short_id()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn durations_parse() {
        assert_eq!(parse_duration("90"), Some(Duration::from_secs(90)));
        assert_eq!(parse_duration("2m"), Some(Duration::from_secs(120)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("0"), None);
        assert_eq!(parse_duration("x"), None);
    }

    #[test]
    fn to_refuses_parent_and_absolute() {
        assert_eq!(check_to("").unwrap(), "");
        assert_eq!(check_to("a/b/").unwrap(), "a/b");
        assert!(check_to("../x").is_err());
        assert!(check_to("a/../b").is_err());
        assert!(check_to("/abs").is_err());
        assert!(check_to("a//b").is_err());
    }

    #[test]
    fn clash_names_keep_the_extension() {
        assert_eq!(clash_name("a.md", 2), "a (2).md");
        assert_eq!(clash_name("a.tar.gz", 3), "a.tar (3).gz");
        assert_eq!(clash_name("README", 2), "README (2)");
        assert_eq!(clash_name(".env", 2), ".env (2)");
    }

    #[test]
    fn a_directory_is_walked_hidden_files_skipped_symlinks_refused() {
        let d = tmp("walk");
        fs::create_dir_all(d.join("docs/sub")).unwrap();
        fs::write(d.join("docs/a.md"), "a").unwrap();
        fs::write(d.join("docs/sub/b.txt"), "bb").unwrap();
        fs::write(d.join("docs/.DS_Store"), "x").unwrap();
        let (items, skipped) =
            collect(&[d.join("docs").display().to_string()], "in", None).unwrap();
        let mut rels: Vec<_> = items.iter().map(|i| i.rel.clone()).collect();
        rels.sort();
        assert_eq!(rels, ["in/docs/a.md", "in/docs/sub/b.txt"]);
        assert_eq!(skipped, 1);
        symlink(d.join("docs/a.md"), d.join("docs/link.md")).unwrap();
        let e = collect(&[d.join("docs").display().to_string()], "", None).unwrap_err();
        assert_eq!(e.code, "SO_UPLOAD_SYMLINK");
        let e = collect(&[d.join("docs/link.md").display().to_string()], "", None).unwrap_err();
        assert_eq!(e.code, "SO_UPLOAD_SYMLINK");
        let e = collect(&[d.join("nope").display().to_string()], "", None).unwrap_err();
        assert_eq!(e.code, "SO_UPLOAD_NOT_FOUND");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn stdin_needs_a_name() {
        let e = collect(&["-".to_owned()], "", None).unwrap_err();
        assert_eq!(e.code, "SO_UPLOAD_NEEDS_NAME");
        let e = collect(&["-".to_owned()], "", Some("../x")).unwrap_err();
        assert_eq!(e.code, "SO_UPLOAD_BAD_PATH");
    }

    #[test]
    fn placing_never_overwrites_and_leaves_no_part_file() {
        let d = tmp("place");
        let root = d.join("root");
        fs::create_dir_all(&root).unwrap();
        let src = d.join("n.md");
        fs::write(&src, "one").unwrap();
        let item = |rel: &str| Item {
            src: Source::Path(src.clone()),
            rel: rel.to_owned(),
            bytes: 3,
        };
        assert_eq!(place_fs(&root, &item("x/n.md")).unwrap(), "x/n.md");
        assert_eq!(place_fs(&root, &item("x/n.md")).unwrap(), "x/n (2).md");
        assert_eq!(place_fs(&root, &item("x/n.md")).unwrap(), "x/n (3).md");
        assert_eq!(fs::read_to_string(root.join("x/n.md")).unwrap(), "one");
        let names: Vec<_> = fs::read_dir(root.join("x"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.contains(".part")), "{names:?}");
        assert_eq!(names.len(), 3);
        let mode = fs::metadata(root.join("x/n.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o644);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_symlink_inside_the_drive_is_not_followed() {
        let d = tmp("escape");
        let root = d.join("root");
        let outside = d.join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("jump")).unwrap();
        let item = Item {
            src: Source::Stdin(b"x".to_vec()),
            rel: "jump/n.md".into(),
            bytes: 1,
        };
        let e = place_fs(&root, &item).unwrap_err();
        assert_eq!(e.code, "SO_UPLOAD_SYMLINK");
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn settling_follows_the_in_flight_states() {
        let c = json!({"published": 3, "queued": 1});
        assert_eq!(judge(&c, "terminal", true), (false, 0));
        let c = json!({"published": 3, "encrypted": 1, "unsupported": 1});
        assert_eq!(judge(&c, "terminal", true), (true, 2));
        let c = json!({"published": 2, "indexed": 1});
        assert_eq!(judge(&c, "terminal", false), (false, 0), "still publishing");
        assert_eq!(judge(&c, "terminal", true), (true, 0), "waits for a person");
        assert_eq!(judge(&c, "published", true), (false, 0));
        assert_eq!(judge(&json!({}), "terminal", true), (true, 0));
    }

    #[test]
    fn unsupported_reason_needs_files_and_no_parser() {
        let none = json!({"engine": {"parser": {"mode": "none", "note": "no parser sidecar: x"}}});
        let some = json!({"engine": {"parser": {"mode": "configured"}}});
        let c = json!({"unsupported": 2});
        assert_eq!(
            unsupported_hint(&c, Some(&none)).as_deref(),
            Some("no parser sidecar: x")
        );
        assert_eq!(unsupported_hint(&c, Some(&some)), None);
        assert_eq!(
            unsupported_hint(&json!({"published": 1}), Some(&none)),
            None
        );
        assert_eq!(unsupported_hint(&c, None), None);
    }

    #[test]
    fn host_root_maps_a_mounted_drive_through_the_mounts_file() {
        let state = tmp("mounts");
        let host = state.join("host");
        fs::create_dir_all(&host).unwrap();
        let doc = json!({"protocol": "oo.docs.mounts/v1", "mounts": {"abc123": host.display().to_string()}});
        fs::write(state.join("drive-mounts.json"), doc.to_string()).unwrap();
        let drive = json!({"config": {"root": "/drives/abc123"}});
        assert_eq!(host_root(&state, &drive).unwrap(), host);
        let drive = json!({"config": {"root": "/drives/missing"}});
        assert_eq!(
            host_root(&state, &drive).unwrap_err().code,
            "SO_UPLOAD_ROOT_UNKNOWN"
        );
        let plain = json!({"config": {"root": host.display().to_string()}});
        assert_eq!(host_root(&state, &plain).unwrap(), host);
        let _ = fs::remove_dir_all(&state);
    }
}
