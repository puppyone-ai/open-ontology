//! Physical mount of a pack (10 §7.7): write `<dir>/.context` atomically.
//!
//! The files go to `<dir>/.context.tmp-<sid>` (0700), then become 0444 files
//! in 0555 directories, are fsynced, and the directory is renamed to
//! `.context`. Any failure removes the temp directory, so no half-written
//! mount is ever visible. An existing `.context` is replaced only by a refresh
//! of the share it holds and deleted only when `SHARE.json` and `mounts.json`
//! agree it is ours (revoke, expiry, the daemon's reconcile).

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

/// The mount directory name inside the target.
pub const MOUNT_DIR: &str = ".context";

/// Why a mount did not happen.
#[derive(Debug, PartialEq, Eq)]
pub enum MountError {
    /// `<dir>/.context` exists (`DOCS_SHARE_TARGET_OCCUPIED`).
    Occupied,
    /// The target is not an acceptable directory (`DOCS_SHARE_TARGET_INVALID`).
    Invalid(&'static str),
    /// A file path of the pack is not a plain relative path.
    BadPath,
    /// Writing failed (`DOCS_SHARE_TARGET_WRITE_FAILED`).
    Write(String),
}

/// Check `--to` (10 §7.7 point 1): an existing absolute directory, resolved
/// through symlinks, that is not `/`, not the home directory and not inside
/// the oo state directory. Returns the canonical path.
pub fn check_target(
    to: &str,
    home: Option<&Path>,
    state: Option<&Path>,
) -> Result<PathBuf, MountError> {
    let p = Path::new(to);
    if !p.is_absolute() {
        return Err(MountError::Invalid("--to must be an absolute path"));
    }
    let real = fs::canonicalize(p).map_err(|_| MountError::Invalid("--to does not exist"))?;
    if !fs::metadata(&real).is_ok_and(|m| m.is_dir()) {
        return Err(MountError::Invalid("--to is not a directory"));
    }
    if real == Path::new("/") {
        return Err(MountError::Invalid("--to must not be the filesystem root"));
    }
    if let Some(h) = home.and_then(|h| fs::canonicalize(h).ok())
        && real == h
    {
        return Err(MountError::Invalid(
            "--to must not be the home directory itself",
        ));
    }
    if let Some(s) = state.and_then(|s| fs::canonicalize(s).ok())
        && real.starts_with(&s)
    {
        return Err(MountError::Invalid(
            "--to must not be inside the oo state directory",
        ));
    }
    Ok(real)
}

/// `rel` is a relative path of normal components only.
pub fn safe_relative(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains(['\\', '\0'])
        && rel
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn io<E: std::fmt::Display>(e: E) -> MountError {
    MountError::Write(e.to_string())
}

/// Make a tree writable again and delete it (it may be 0555 inside).
pub fn remove_tree(dir: &Path) {
    fn open_up(p: &Path) {
        let Ok(m) = fs::symlink_metadata(p) else {
            return;
        };
        if m.file_type().is_symlink() {
            return;
        }
        if m.is_dir() {
            let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o700));
            if let Ok(rd) = fs::read_dir(p) {
                for e in rd.flatten() {
                    open_up(&e.path());
                }
            }
        }
    }
    open_up(dir);
    let _ = fs::remove_dir_all(dir);
}

fn fsync_dir(p: &Path) -> Result<(), MountError> {
    File::open(p).and_then(|f| f.sync_all()).map_err(io)
}

fn write_tree(tmp: &Path, files: &[(String, Vec<u8>)]) -> Result<(), MountError> {
    DirBuilder::new().mode(0o700).create(tmp).map_err(io)?;
    for (rel, bytes) in files {
        let path = tmp.join(rel);
        if let Some(parent) = path.parent() {
            DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(parent)
                .map_err(io)?;
        }
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).map_err(io)?;
    }
    // Directories last, deepest first: 0555 would block writing into them.
    let mut dirs: Vec<PathBuf> = Vec::new();
    for (rel, _) in files {
        let mut p = tmp.join(rel);
        while p.pop() && p.starts_with(tmp) {
            if !dirs.contains(&p) {
                dirs.push(p.clone());
            }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in &dirs {
        fsync_dir(d)?;
        fs::set_permissions(d, fs::Permissions::from_mode(0o555)).map_err(io)?;
    }
    Ok(())
}

/// Install `files` (relative path, bytes) as `<target>/.context`; returns the
/// final path. `tag` makes the temp name unique (the share id).
pub fn install(
    target: &Path,
    tag: &str,
    files: &[(String, Vec<u8>)],
) -> Result<PathBuf, MountError> {
    if files.iter().any(|(p, _)| !safe_relative(p)) {
        return Err(MountError::BadPath);
    }
    let final_path = target.join(MOUNT_DIR);
    if fs::symlink_metadata(&final_path).is_ok() {
        return Err(MountError::Occupied);
    }
    let tmp = target.join(format!("{MOUNT_DIR}.tmp-{tag}"));
    remove_tree(&tmp);
    let outcome = (|| {
        write_tree(&tmp, files)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o555)).map_err(io)?;
        fsync_dir(target)?;
        // A mount that appeared meanwhile still wins: rename onto an existing
        // directory fails for a non-empty one, but check explicitly first.
        if fs::symlink_metadata(&final_path).is_ok() {
            return Err(MountError::Occupied);
        }
        fs::rename(&tmp, &final_path).map_err(io)?;
        fsync_dir(target)
    })();
    match outcome {
        Ok(()) => Ok(final_path),
        Err(e) => {
            remove_tree(&tmp);
            Err(e)
        }
    }
}

// ----------------------------------------------------------- lifecycle ----

use serde_json::{Value, json};

use super::client::Daemon;
use super::secrets;

/// `<state>/mounts.json` (10 §7.7 point 4): `{mount_ref: {share_id, path,
/// created_at, updates_seq}}`, 0600. The engine never learns the path.
pub fn mounts_path(state: &Path) -> PathBuf {
    state.join("mounts.json")
}

/// Load `mounts.json` (a missing or broken file is an empty map).
pub fn load_mounts(state: &Path) -> serde_json::Map<String, Value> {
    fs::read_to_string(mounts_path(state))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Write `mounts.json` atomically.
pub fn save_mounts(state: &Path, m: &serde_json::Map<String, Value>) -> std::io::Result<()> {
    secrets::write_secret_atomic(&mounts_path(state), &Value::Object(m.clone()).to_string())
}

/// Record a mount.
pub fn register(state: &Path, mount_ref: &str, share_id: &str, dir: &Path) -> std::io::Result<()> {
    let mut m = load_mounts(state);
    // One directory holds one mount: drop entries that pointed at it.
    m.retain(|_, v| v["path"].as_str() != dir.to_str());
    m.insert(
        mount_ref.to_owned(),
        json!({"share_id": share_id, "path": dir, "created_at": secrets::rfc3339(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())),
            "updates_seq": 0}),
    );
    save_mounts(state, &m)
}

/// `share_id` written in `<dir>/.context/SHARE.json`.
pub fn share_id_in(target: &Path) -> Option<String> {
    let text = fs::read_to_string(target.join(MOUNT_DIR).join("SHARE.json")).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()?
        .get("share_id")?
        .as_str()
        .map(str::to_owned)
}

/// Delete `<target>/.context` only when its `SHARE.json` names `share_id`
/// (10 §7.7 point 5): what is not ours is never touched, symlinks are not
/// followed. Returns whether a mount was removed.
pub fn remove_if_ours(target: &Path, share_id: &str) -> bool {
    let dir = target.join(MOUNT_DIR);
    if fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return false;
    }
    if share_id_in(target).as_deref() != Some(share_id) {
        return false;
    }
    remove_tree(&dir);
    fs::symlink_metadata(&dir).is_err()
}

/// Replace the mount of `old_share` by `files` (a refresh). The old directory
/// goes to `.context.old-<tag>` first and is deleted once the new one is in;
/// if the new one cannot be put in place the old one is moved back.
pub fn replace(
    target: &Path,
    tag: &str,
    old_share: &str,
    files: &[(String, Vec<u8>)],
) -> Result<PathBuf, MountError> {
    if share_id_in(target).as_deref() != Some(old_share) {
        return Err(MountError::Occupied);
    }
    if files.iter().any(|(p, _)| !safe_relative(p)) {
        return Err(MountError::BadPath);
    }
    let final_path = target.join(MOUNT_DIR);
    let tmp = target.join(format!("{MOUNT_DIR}.tmp-{tag}"));
    let old = target.join(format!("{MOUNT_DIR}.old-{tag}"));
    remove_tree(&tmp);
    remove_tree(&old);
    let built = (|| {
        write_tree(&tmp, files)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o555)).map_err(io)
    })();
    if let Err(e) = built {
        remove_tree(&tmp);
        return Err(e);
    }
    fs::rename(&final_path, &old).map_err(io)?;
    if let Err(e) = fs::rename(&tmp, &final_path) {
        let _ = fs::rename(&old, &final_path);
        remove_tree(&tmp);
        return Err(io(e));
    }
    remove_tree(&old);
    fsync_dir(target)?;
    Ok(final_path)
}

const AIGC_LABEL: &str =
    "> AI生成 · 由 Super Ontology 根据您的文件自动整理，可能有误，请以原文为准。";

fn updates_header(share_id: &str, pack_digest: &str) -> String {
    format!(
        "{AIGC_LABEL}\n\n---\nAIGC:\n  Label: \"1\"\n  ContentProducer: \"<COMPANY>\"\n  ProduceID: \"{share_id}/UPDATES.md\"\n  ReservedCode1: \"{pack_digest}\"\n  ContentPropagator: \"\"\n  PropagateID: \"\"\n  ReservedCode2: \"\"\n---\n\n# UPDATES\n> 本文件只追加；其余文件内容不变。\n"
    )
}

/// One entry of `UPDATES.md` (10 §16.4).
pub fn update_entry(u: &Value) -> String {
    let section = u["section"].as_str().unwrap_or("");
    let section = section.strip_prefix("BRIEF.md#").unwrap_or(section);
    let level = u["level"].as_str().unwrap_or("info");
    let advice = match level {
        "block" => "建议：运行 `oo context check`；刷新前不要提交依赖该决定的改动。",
        "warn" => "建议：提交前运行 `oo context check`，读完本条后用 `oo context refresh` 升级。",
        _ => "仅供参考，不影响已用事实。",
    };
    let head = format!(
        "\n## #{} · v{} → v{} · {}{}\n",
        u["seq"].as_i64().unwrap_or(0),
        u["from_version"].as_i64().unwrap_or(0),
        u["to_version"].as_i64().unwrap_or(0),
        level,
        if section.is_empty() {
            String::new()
        } else {
            format!(" · {section}")
        }
    );
    format!("{head}- {}\n- {advice}\n", u["text"].as_str().unwrap_or(""))
}

/// Append `entries` to `<target>/.context/UPDATES.md` without changing a byte
/// of what is there: the new file (old bytes then the new entries) is written
/// aside and renamed over. The directory is read only; it is opened for the
/// moment of the rename and closed again. A missing directory is not an
/// error (the share's rows in the engine stay the truth).
pub fn append_updates(target: &Path, entries: &[Value]) -> std::io::Result<()> {
    let dir = target.join(MOUNT_DIR);
    if !dir.is_dir() || entries.is_empty() {
        return Ok(());
    }
    let path = dir.join("UPDATES.md");
    let mut text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            let sj: Value = fs::read_to_string(dir.join("SHARE.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            updates_header(
                sj["share_id"].as_str().unwrap_or(""),
                sj["pack_digest"].as_str().unwrap_or(""),
            )
        }
    };
    for e in entries {
        text.push_str(&update_entry(e));
    }
    let mode = fs::metadata(&dir)?.permissions().mode();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    let tmp = dir.join("UPDATES.md.tmp");
    let result = (|| {
        let _ = fs::remove_file(&tmp);
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o444))?;
        fs::rename(&tmp, &path)
    })();
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(mode & 0o777));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// What one reconcile pass did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// Share ids whose directory was deleted (expired or revoked).
    pub removed: Vec<String>,
    /// Entries appended to `UPDATES.md` files.
    pub appended: u32,
}

/// The daemon's pass over `mounts.json` (10 §7.7 point 6, §16.4): delete the
/// directory of every expired or revoked share, forget directories that are
/// gone, and append new share updates. Idempotent.
pub async fn reconcile(d: &Daemon, state: &Path) -> Reconciled {
    let mut out = Reconciled::default();
    let mut m = load_mounts(state);
    let mut changed = false;
    let keys: Vec<String> = m.keys().cloned().collect();
    for k in keys {
        let e = m[&k].clone();
        let (Some(sid), Some(path)) = (e["share_id"].as_str(), e["path"].as_str()) else {
            m.remove(&k);
            changed = true;
            continue;
        };
        let target = PathBuf::from(path);
        let Ok(share) = d.get_json(&format!("/v1/docs/shares/{sid}")).await else {
            continue; // engine down or the share unknown: leave everything
        };
        match share["state"].as_str() {
            Some("expired" | "revoked") => {
                if remove_if_ours(&target, sid) {
                    out.removed.push(sid.to_owned());
                }
                m.remove(&k);
                changed = true;
            }
            Some("refreshed") if share_id_in(&target).as_deref() != Some(sid) => {
                m.remove(&k);
                changed = true;
            }
            Some("active" | "drifted") => {
                if share_id_in(&target).as_deref() != Some(sid) {
                    m.remove(&k);
                    changed = true;
                    continue;
                }
                let since = e["updates_seq"].as_i64().unwrap_or(0);
                let Ok(list) = d
                    .get_json(&format!(
                        "/v1/docs/shares/{sid}/updates?since={since}&limit=200"
                    ))
                    .await
                else {
                    continue;
                };
                let items = list["items"].as_array().cloned().unwrap_or_default();
                if items.is_empty() {
                    continue;
                }
                if append_updates(&target, &items).is_ok() {
                    out.appended += items.len() as u32;
                    let last = items
                        .last()
                        .and_then(|i| i["seq"].as_i64())
                        .unwrap_or(since);
                    m[&k]["updates_seq"] = json!(last);
                    changed = true;
                }
            }
            _ => {}
        }
    }
    if changed {
        let _ = save_mounts(state, &m);
    }
    out
}

/// Mode bits of a path, for tests.
#[cfg(test)]
pub fn mode_of(p: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(p).ok().map(|m| m.mode() & 0o777)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("oo-mounts-{name}-{}", std::process::id()));
        remove_tree(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn files() -> Vec<(String, Vec<u8>)> {
        vec![
            ("BRIEF.md".into(), b"brief\n".to_vec()),
            ("people/a-1234abcd.md".into(), "林川\n".as_bytes().to_vec()),
            ("refs/0001.md".into(), b"ref\n".to_vec()),
        ]
    }

    #[test]
    fn a_mount_is_complete_read_only_and_leaves_no_temp_directory() {
        let b = base("ok");
        let at = install(&b, "s1", &files()).unwrap();
        assert_eq!(at, b.join(".context"));
        assert_eq!(fs::read(at.join("BRIEF.md")).unwrap(), b"brief\n");
        assert_eq!(
            fs::read(at.join("people/a-1234abcd.md")).unwrap(),
            "林川\n".as_bytes()
        );
        assert_eq!(mode_of(&at.join("BRIEF.md")), Some(0o444));
        assert_eq!(mode_of(&at.join("people")), Some(0o555));
        assert_eq!(mode_of(&at), Some(0o555));
        // Read only: nothing can be created inside.
        assert!(
            File::create(at.join("new.md")).is_err()
                || std::env::var("USER").as_deref() == Ok("root")
        );
        let names: Vec<_> = fs::read_dir(&b)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, [".context"]);
        remove_tree(&b);
    }

    #[test]
    fn an_occupied_target_is_refused_and_left_alone() {
        let b = base("occupied");
        fs::create_dir_all(b.join(".context")).unwrap();
        fs::write(b.join(".context/mine.md"), "keep").unwrap();
        assert_eq!(install(&b, "s1", &files()), Err(MountError::Occupied));
        assert_eq!(
            fs::read_to_string(b.join(".context/mine.md")).unwrap(),
            "keep"
        );
        // A symlink named .context counts too, and is not followed.
        let c = base("occupied-link");
        std::os::unix::fs::symlink("/tmp", c.join(".context")).unwrap();
        assert_eq!(install(&c, "s1", &files()), Err(MountError::Occupied));
        remove_tree(&b);
        remove_tree(&c);
    }

    #[test]
    fn a_failure_leaves_neither_a_mount_nor_a_temp_directory() {
        let b = base("fail");
        // Two entries that collide (a file and a directory of the same name).
        let bad = vec![
            ("a".to_owned(), b"x".to_vec()),
            ("a/b.md".to_owned(), b"y".to_vec()),
        ];
        assert!(matches!(install(&b, "s1", &bad), Err(MountError::Write(_))));
        let left: Vec<_> = fs::read_dir(&b).unwrap().flatten().collect();
        assert!(left.is_empty(), "{left:?}");
        remove_tree(&b);
    }

    #[test]
    fn hostile_paths_are_refused_before_anything_is_written() {
        let b = base("paths");
        for p in [
            "../x", "/abs", "a//b", "a/../b", "", "a\\b", "a/", ".", "x\0y",
        ] {
            let r = install(&b, "s", &[(p.to_owned(), b"z".to_vec())]);
            assert_eq!(r, Err(MountError::BadPath), "{p:?}");
        }
        assert_eq!(fs::read_dir(&b).unwrap().count(), 0);
        remove_tree(&b);
    }

    fn with_share(name: &str, sid: &str) -> PathBuf {
        let b = base(name);
        let mut f = files();
        f.push((
            "SHARE.json".into(),
            format!(
                "{{\"share_id\":\"{sid}\",\"pack_digest\":\"{}\"}}\n",
                "a".repeat(64)
            )
            .into_bytes(),
        ));
        install(&b, sid, &f).unwrap();
        b
    }

    fn upd(seq: i64, level: &str) -> Value {
        json!({"seq": seq, "from_version": 1, "to_version": 2, "level": level,
               "section": "BRIEF.md#当前状态与决定", "text": "「方案二已确认」 已被撤回，没有替代。"})
    }

    #[test]
    fn updates_only_append_and_keep_the_mount_read_only() {
        let b = with_share("upd", "s-1");
        append_updates(&b, &[upd(1, "warn")]).unwrap();
        let first = fs::read_to_string(b.join(".context/UPDATES.md")).unwrap();
        assert!(first.starts_with("> AI生成"), "the label is the first line");
        assert!(first.contains("本文件只追加；其余文件内容不变。"));
        assert!(first.contains("## #1 · v1 → v2 · warn · 当前状态与决定"));
        append_updates(&b, &[upd(2, "block")]).unwrap();
        let second = fs::read_to_string(b.join(".context/UPDATES.md")).unwrap();
        assert!(second.starts_with(&first), "existing bytes never change");
        assert!(second.contains("## #2 · v1 → v2 · block · 当前状态与决定"));
        assert!(second.contains("不要提交依赖该决定的改动"));
        assert_eq!(mode_of(&b.join(".context")), Some(0o555));
        assert_eq!(mode_of(&b.join(".context/UPDATES.md")), Some(0o444));
        // Everything else is untouched.
        assert_eq!(fs::read(b.join(".context/BRIEF.md")).unwrap(), b"brief\n");
        assert!(!b.join(".context/UPDATES.md.tmp").exists());
        // A removed directory is not an error.
        remove_tree(&b.join(".context"));
        append_updates(&b, &[upd(3, "info")]).unwrap();
        remove_tree(&b);
    }

    #[test]
    fn only_our_mounts_are_deleted() {
        let b = with_share("ours", "s-2");
        assert!(
            !remove_if_ours(&b, "s-other"),
            "another share's id is not ours"
        );
        assert!(b.join(".context").exists());
        assert!(remove_if_ours(&b, "s-2"));
        assert!(!b.join(".context").exists());
        // A foreign directory without SHARE.json is left alone.
        let c = base("foreign");
        fs::create_dir_all(c.join(".context")).unwrap();
        fs::write(c.join(".context/mine.md"), "keep").unwrap();
        assert!(!remove_if_ours(&c, "s-2"));
        assert!(c.join(".context/mine.md").exists());
        remove_tree(&b);
        remove_tree(&c);
    }

    #[test]
    fn a_refresh_replaces_only_the_share_it_names() {
        let b = with_share("replace", "s-old");
        let mut f = files();
        f.push(("SHARE.json".into(), b"{\"share_id\":\"s-new\"}\n".to_vec()));
        assert_eq!(
            replace(&b, "s-new", "s-wrong", &f),
            Err(MountError::Occupied)
        );
        assert_eq!(share_id_in(&b).as_deref(), Some("s-old"));
        replace(&b, "s-new", "s-old", &f).unwrap();
        assert_eq!(share_id_in(&b).as_deref(), Some("s-new"));
        let names: Vec<_> = fs::read_dir(&b)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, [".context"], "no temp or old directory is left");
        assert_eq!(mode_of(&b.join(".context")), Some(0o555));
        remove_tree(&b);
    }

    #[test]
    fn targets_are_checked() {
        let b = base("target");
        let ok = check_target(b.to_str().unwrap(), None, None).unwrap();
        assert!(ok.is_absolute());
        assert!(check_target("relative", None, None).is_err());
        assert!(check_target("/nonexistent-oo-dir", None, None).is_err());
        assert!(check_target("/", None, None).is_err());
        assert!(check_target(b.to_str().unwrap(), Some(&b), None).is_err());
        let inside = b.join("state").join("x");
        fs::create_dir_all(&inside).unwrap();
        assert!(check_target(inside.to_str().unwrap(), None, Some(&b.join("state"))).is_err());
        let file = b.join("f");
        fs::write(&file, "x").unwrap();
        assert!(check_target(file.to_str().unwrap(), None, None).is_err());
        remove_tree(&b);
    }
}
