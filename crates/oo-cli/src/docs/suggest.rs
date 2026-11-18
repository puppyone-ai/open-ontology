//! The `suggest/` directory of a mount and its collection (10 §11.1, B-S7-02).
//!
//! A Share whose role is `suggester` or `editor` gets `.context/suggest/` (0755, the only writable
//! directory of the mount) with a `README.md` that says what to write. The daemon scans every such directory
//! every 10 seconds, `oo context collect <dir>` does it once by hand. A scan never follows a symlink,
//! reads only regular files that are not read-only (0444 and the like are the mount's own), takes
//! `.md` and `.json` up to 64 KiB and at most 200 files per Share, posts what it read to the engine and renames
//! the file to `<name>.collected` (a rejected one to `<name>.rejected`), so a second scan finds nothing new.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::client::{CliError, Daemon};
use super::mounts;

pub const DIR: &str = "suggest";
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_FILES: usize = 200;

pub const README: &str = "# suggest/\n\n\
这是本挂载里唯一可写的目录。Agent 学到了新东西，把建议写在这里（一个文件一条或多条，*.md 或 *.json），\n\
守护进程每 10 秒收集一次并把文件改名为 *.collected。\n\n\
JSON：\n\n\
```json\n\
{\"protocol\":\"oo.suggest/v1\",\"kind\":\"new_fact|supersede|correction|preference|type_change\",\n\
 \"subject\":\"<实体名或 key>\",\"predicate\":\"<核心关系或 attr.*>\",\"object\":\"<实体名或 key>\",\"value\":\"…\",\n\
 \"valid_from\":\"YYYY-MM-DD\",\"supersedes\":\"<BRIEF 中的 [src:…] 或 fact_key>\",\n\
 \"rationale\":\"≤500 字\",\"evidence\":[{\"kind\":\"brief_line|ref|external\",\"ref\":\"…\"}]}\n\
```\n\n\
object 与 value 二选一。Markdown：frontmatter 写同样的键（`evidence: ref:路径#位置` 可重复），正文是理由。\n\n\
限制：单文件 64 KiB，每条建议文件最多 50 条，每个共享最多 200 个文件；不要放符号链接。\n\
建议不会直接改动文件：新事实有范围内证据时自动接受并进入当日摘要，取代已被使用的事实、改偏好或类型结构时由人确认。\n";

/// Add `suggest/README.md` to the files of a mount for a role that may suggest.
pub fn add_dir(files: &mut Vec<(String, Vec<u8>)>, role: &str) {
    if matches!(role, "suggester" | "editor")
        && !files.iter().any(|(p, _)| p.starts_with("suggest/"))
    {
        files.push((format!("{DIR}/README.md"), README.as_bytes().to_vec()));
        files.sort_by(|a, b| a.0.cmp(&b.0));
    }
}

/// Make `<mount>/suggest` writable (the mount builder leaves every directory 0555).
pub fn open_dir(mount: &Path) {
    let d = mount.join(DIR);
    if fs::symlink_metadata(&d).is_ok_and(|m| m.is_dir()) {
        let _ = fs::set_permissions(&d, fs::Permissions::from_mode(0o755));
    }
}

/// One file read from the directory.
#[derive(Debug, PartialEq, Eq)]
pub struct Doc {
    pub path: PathBuf,
    pub name: String,
    pub format: &'static str,
    pub text: String,
}

/// What a scan found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub docs: Vec<Doc>,
    /// `(name, reason)` of files left alone.
    pub skipped: Vec<(String, &'static str)>,
}

/// Read the directory by the rules above. Pure on the file system (no network).
pub fn scan(dir: &Path) -> Scan {
    let mut out = Scan::default();
    // The directory itself must be a real directory, not a link.
    if !fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        return out;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return out;
    };
    let mut names: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let path = dir.join(&name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            out.skipped.push((name, "symlink"));
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let format = match name.rsplit_once('.').map(|(_, e)| e) {
            Some("md") if name != "README.md" => "md",
            Some("json") => "json",
            _ => continue, // README.md, *.collected, *.rejected and anything else
        };
        if meta.permissions().mode() & 0o222 == 0 {
            continue; // read-only: the mount's own
        }
        if meta.len() > MAX_FILE_BYTES {
            out.skipped.push((name, "too_large"));
            continue;
        }
        if out.docs.len() >= MAX_FILES {
            out.skipped.push((name, "over_200_files"));
            continue;
        }
        match fs::read(&path).ok().and_then(|b| String::from_utf8(b).ok()) {
            Some(text) => out.docs.push(Doc {
                path,
                name,
                format,
                text,
            }),
            None => out.skipped.push((name, "not_utf8")),
        }
    }
    out
}

fn rename_to(path: &Path, ext: &str) {
    let mut to = path.as_os_str().to_owned();
    to.push(ext);
    let _ = fs::rename(path, PathBuf::from(to));
}

/// Collect one directory for share `sid`: post, then rename by the engine's answer. Returns the answer
/// extended with `collected`, `rejected` and `skipped` counts.
pub async fn collect_dir(d: &Daemon, sid: &str, dir: &Path) -> Result<Value, CliError> {
    let scan = scan(dir);
    if scan.docs.is_empty() {
        return Ok(
            json!({"share_id": sid, "collected": 0, "rejected": 0, "skipped": scan.skipped.len(),
                         "results": []}),
        );
    }
    let documents: Vec<Value> = scan
        .docs
        .iter()
        .map(|x| json!({"name": x.name, "format": x.format, "text": x.text}))
        .collect();
    let body = json!({"share_id": sid, "documents": documents});
    // The share's own credential, when SHARE.json has one: the engine then records which credential wrote the
    // suggestion (traceability, 30 L5 acceptance 4). Without it the engine token of the daemon does the call.
    let by_cred = dir
        .parent()
        .and_then(|m| fs::read_to_string(m.join("SHARE.json")).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|sj| {
            let cred = sj["refs"]["credential"].as_str()?.to_owned();
            let endpoint = sj["refs"]["endpoint"].as_str()?.to_owned();
            let base = endpoint
                .strip_suffix("/v1/docs/refs")
                .unwrap_or(&endpoint)
                .to_owned();
            Some((base, cred))
        });
    let mut answer = None;
    if let Some((base, cred)) = by_cred
        && let Ok(cd) = Daemon::with_credential(
            &format!("http://{}", base.trim_start_matches("http://")),
            &cred,
        )
        && let Ok(v) = cd.post_json("/v1/docs/suggestions", &body).await
    {
        answer = Some(v);
    }
    let mut answer = match answer {
        Some(v) => v,
        None => d.post_json("/v1/docs/suggestions", &body).await?,
    };
    let results = answer["results"].as_array().cloned().unwrap_or_default();
    let (mut collected, mut rejected) = (0u32, 0u32);
    for doc in &scan.docs {
        let mine: Vec<&Value> = results
            .iter()
            .filter(|r| r["document"] == doc.name.as_str())
            .collect();
        if !mine.is_empty() && mine.iter().all(|r| r["status"] == "invalid") {
            rename_to(&doc.path, ".rejected");
            rejected += 1;
        } else if !mine.is_empty() {
            rename_to(&doc.path, ".collected");
            collected += 1;
        }
    }
    answer["collected"] = json!(collected);
    answer["rejected"] = json!(rejected);
    answer["skipped"] = json!(scan.skipped.len());
    Ok(answer)
}

/// The daemon's pass: every mount in `mounts.json` that has a `suggest/` directory.
pub async fn collect_all(d: &Daemon, state: &Path) -> u32 {
    let mut n = 0;
    for e in mounts::load_mounts(state).values() {
        let (Some(sid), Some(path)) = (e["share_id"].as_str(), e["path"].as_str()) else {
            continue;
        };
        let dir = Path::new(path).join(mounts::MOUNT_DIR).join(DIR);
        if mounts::share_id_in(Path::new(path)).as_deref() != Some(sid) {
            continue;
        }
        if let Ok(v) = collect_dir(d, sid, &dir).await {
            n += v["collected"].as_u64().unwrap_or(0) as u32;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("oo-suggest-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn scan_takes_writable_regular_md_and_json_only() {
        let d = tmp("scan");
        fs::write(d.join("a.json"), "{}").unwrap();
        fs::write(d.join("b.md"), "---\n---\n").unwrap();
        fs::write(d.join("README.md"), "x").unwrap();
        fs::write(d.join("old.json.collected"), "{}").unwrap();
        fs::write(d.join("ro.json"), "{}").unwrap();
        fs::set_permissions(d.join("ro.json"), fs::Permissions::from_mode(0o444)).unwrap();
        fs::write(d.join("big.json"), vec![b'x'; 70_000]).unwrap();
        fs::write(d.join("notes.txt"), "x").unwrap();
        symlink("/etc/passwd", d.join("link.json")).unwrap();
        let s = scan(&d);
        let names: Vec<&str> = s.docs.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["a.json", "b.md"]);
        assert!(s.skipped.contains(&("link.json".into(), "symlink")));
        assert!(s.skipped.contains(&("big.json".into(), "too_large")));
        assert_eq!(s.skipped.len(), 2);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_symlinked_directory_is_not_followed() {
        let real = tmp("real");
        fs::write(real.join("a.json"), "{}").unwrap();
        let base = tmp("linkdir");
        symlink(&real, base.join("suggest")).unwrap();
        assert!(scan(&base.join("suggest")).docs.is_empty());
        fs::remove_dir_all(&real).unwrap();
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn two_hundred_files_at_most() {
        let d = tmp("many");
        for i in 0..205 {
            fs::write(d.join(format!("s{i:03}.json")), "{}").unwrap();
        }
        let s = scan(&d);
        assert_eq!(s.docs.len(), MAX_FILES);
        assert_eq!(s.skipped.len(), 5);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn readme_is_added_for_suggesters_only() {
        let mut f = vec![("BRIEF.md".to_owned(), vec![])];
        add_dir(&mut f, "viewer");
        assert_eq!(f.len(), 1);
        add_dir(&mut f, "suggester");
        assert!(f.iter().any(|(p, _)| p == "suggest/README.md"));
        add_dir(&mut f, "suggester");
        assert_eq!(f.len(), 2);
    }
}
