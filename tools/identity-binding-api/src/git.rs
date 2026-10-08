//! Local config Git writer. The allowlist is init, rev-parse, hash-object, update-index,
//! write-tree, commit-tree, update-ref, cat-file, and ls-tree. There is no push, fetch, or checkout.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static INDEX_SEQ: AtomicU64 = AtomicU64::new(1);

use sha2::{Digest, Sha256};

use crate::error::Fault;

const AUTHOR_NAME: &str = "oo-identity-binding";
const AUTHOR_EMAIL: &str = "identity-binding@open-ontology.invalid";
const AUTHOR_DATE: &str = "1791417600 +0000";
const FILE_PATH: &str = "management/bindings.json";

pub struct GitSpec {
    pub binary: PathBuf,
    pub root: PathBuf,
    pub reference: String,
}

pub fn seed_repository(git: &GitSpec, document: &[u8]) -> Result<String, Fault> {
    if document.is_empty() || document.len() > 65_536 {
        return Err(Fault::malformed());
    }
    let root = prepare_root(&git.root)?;
    if root.join(".git").exists() {
        return Err(Fault::git_rejected());
    }
    let repo = Repo::new(git, root)?;
    run(&repo, &["init"], None)?;
    write_commit(&repo, &git.reference, None, document, &format!("oo.identity-binding/v1 seed {}", sha256_hex(document)))
}

pub fn cas_commit(git: &GitSpec, expected_head: &str, document: &[u8], idempotency_key: &str, document_sha256: &str) -> Result<String, Fault> {
    if !hex40(expected_head) || !hex64(document_sha256) || !key_ok(idempotency_key) || document.is_empty() || document.len() > 65_536 {
        return Err(Fault::git_rejected());
    }
    if sha256_hex(document) != document_sha256 {
        return Err(Fault::git_rejected());
    }
    let repo = open_repo(git)?;
    let message = format!("oo.identity-binding/v1 {idempotency_key} {document_sha256}");
    let new_commit = commit_object(&repo, Some(expected_head), document, &message)?;
    let head = rev_parse(&repo, &git.reference)?;
    if head == new_commit {
        return Ok(new_commit);
    }
    if head != expected_head {
        return Err(Fault::git_cas());
    }
    let status = run(&repo, &["update-ref", "--no-deref", &git.reference, &new_commit, expected_head], None);
    match status {
        Ok(_) => Ok(new_commit),
        Err(Fault { code: "git.rejected", .. }) => Err(Fault::git_cas()),
        Err(fault) => Err(fault),
    }
}

fn write_commit(repo: &Repo, reference: &str, parent: Option<&str>, document: &[u8], message: &str) -> Result<String, Fault> {
    let new_commit = commit_object(repo, parent, document, message)?;
    let zeros = "0".repeat(40);
    run(repo, &["update-ref", "--no-deref", reference, &new_commit, &zeros], None)?;
    Ok(new_commit)
}

fn commit_object(repo: &Repo, parent: Option<&str>, document: &[u8], message: &str) -> Result<String, Fault> {
    let blob_bytes = run(repo, &["hash-object", "-w", "--stdin"], Some(document))?;
    let blob = parse_hex40(&blob_bytes)?;
    let index = repo.scratch.join(format!("index-{}-{}", std::process::id(), INDEX_SEQ.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_file(&index);
    run_index(repo, &index, &["update-index", "--add", "--cacheinfo", "100644", &blob, FILE_PATH])?;
    let tree_bytes = run_index(repo, &index, &["write-tree"])?;
    let _ = std::fs::remove_file(&index);
    let tree = parse_hex40(&tree_bytes)?;
    let mut args = vec!["commit-tree".to_string(), tree, "-m".to_string(), message.to_string()];
    if let Some(parent) = parent {
        args.insert(1, parent.to_string());
        args.insert(1, "-p".to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    parse_hex40(&run(repo, &arg_refs, None)?)
}

fn rev_parse(repo: &Repo, reference: &str) -> Result<String, Fault> {
    parse_hex40(&run(repo, &["rev-parse", "--verify", reference], None)?)
}

struct Repo {
    binary: PathBuf,
    root: PathBuf,
    git_dir: PathBuf,
    scratch: PathBuf,
}

impl Repo {
    fn new(git: &GitSpec, root: PathBuf) -> Result<Self, Fault> {
        let binary_meta = std::fs::metadata(&git.binary).map_err(|_| Fault::git_rejected())?;
        if !binary_meta.is_file() {
            return Err(Fault::git_rejected());
        }
        let scratch = std::env::temp_dir().join(format!("oo-a402-git-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).map_err(|_| Fault::git_rejected())?;
        Ok(Self { binary: git.binary.clone(), root: root.clone(), git_dir: root.join(".git"), scratch })
    }
}

fn open_repo(git: &GitSpec) -> Result<Repo, Fault> {
    let root = prepare_root(&git.root)?;
    let git_dir = root.join(".git");
    let git_meta = std::fs::symlink_metadata(&git_dir).map_err(|_| Fault::git_rejected())?;
    if !git_meta.is_dir() {
        return Err(Fault::git_rejected());
    }
    let objects = git_dir.join("objects");
    let objects_meta = std::fs::symlink_metadata(&objects).map_err(|_| Fault::git_rejected())?;
    if !objects_meta.is_dir() || git_dir.join("objects/info/alternates").exists() {
        return Err(Fault::git_rejected());
    }
    Repo::new(git, root)
}

fn prepare_root(path: &Path) -> Result<PathBuf, Fault> {
    if !path.is_absolute() {
        return Err(Fault::git_rejected());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| Fault::git_rejected())?;
    if !meta.is_dir() {
        return Err(Fault::git_rejected());
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| Fault::git_rejected())?;
    if canonical != path { Err(Fault::git_rejected()) } else { Ok(canonical) }
}

fn run(repo: &Repo, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, Fault> {
    invoke(repo, &repo.scratch.join("unused-index"), args, input)
}

fn run_index(repo: &Repo, index: &Path, args: &[&str]) -> Result<Vec<u8>, Fault> {
    invoke(repo, index, args, None)
}

fn invoke(repo: &Repo, index: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, Fault> {
    let subcommand = args.first().copied().ok_or_else(Fault::git_rejected)?;
    if !matches!(subcommand, "init" | "rev-parse" | "hash-object" | "update-index" | "write-tree" | "commit-tree" | "update-ref" | "cat-file" | "ls-tree") {
        return Err(Fault::git_rejected());
    }
    let mut command = Command::new(&repo.binary);
    command
        .arg("-c")
        .arg(format!("safe.directory={}", repo.root.display()))
        .arg("-c")
        .arg("protocol.allow=never")
        .arg("-c")
        .arg("credential.helper=")
        .arg("-c")
        .arg("gc.auto=0")
        .args(args)
        .current_dir(&repo.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("PATH", repo.binary.parent().unwrap_or(Path::new("/")))
        .env("HOME", &repo.scratch)
        .env("GIT_DIR", &repo.git_dir)
        .env("GIT_INDEX_FILE", index)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_AUTHOR_NAME", AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL)
        .env("GIT_AUTHOR_DATE", AUTHOR_DATE)
        .env("GIT_COMMITTER_NAME", AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL)
        .env("GIT_COMMITTER_DATE", AUTHOR_DATE);
    let mut child = command.spawn().map_err(|_| Fault::git_rejected())?;
    if let Some(mut stdin) = child.stdin.take()
        && let Some(bytes) = input
    {
        use std::io::Write;
        if stdin.write_all(bytes).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Fault::git_rejected());
        }
    }
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > Duration::from_secs(5) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Fault::git_rejected());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return Err(Fault::git_rejected()),
        }
    };
    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        use std::io::Read;
        let _ = pipe.read_to_end(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        use std::io::Read;
        let mut sink = Vec::new();
        let _ = pipe.read_to_end(&mut sink);
    }
    if status.success() { Ok(stdout) } else { Err(Fault::git_rejected()) }
}

fn parse_hex40(bytes: &[u8]) -> Result<String, Fault> {
    let text = trim_text(bytes)?;
    if hex40(&text) { Ok(text) } else { Err(Fault::git_rejected()) }
}

fn trim_text(bytes: &[u8]) -> Result<String, Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::git_rejected())?;
    Ok(text.trim_end_matches(['\n', '\r']).to_string())
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        text.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    text
}

fn hex40(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn key_ok(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{cas_commit, seed_repository, sha256_hex, GitSpec};
    use crate::error::Fault;
    use std::path::PathBuf;

    #[test]
    fn a_second_writer_loses_the_ref_cas() {
        let binary = PathBuf::from("/usr/bin/git");
        assert!(binary.is_file(), "git binary");
        let requested = std::env::temp_dir().join(format!("oo-a402-git-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&requested);
        std::fs::create_dir_all(&requested).expect("dir");
        let root = std::fs::canonicalize(&requested).expect("canonical");
        let git = GitSpec { binary, root: root.clone(), reference: "refs/heads/management".to_string() };
        let base = br#"{"bindings":[],"protocol":"oo.identity-binding/document/v1"}"#;
        let head = seed_repository(&git, base).expect("seed");
        let next_bytes = br#"{"bindings":[{"project_id":"alpha","role":"reviewer","subject":"person"}],"protocol":"oo.identity-binding/document/v1"}"#;
        let next = cas_commit(&git, &head, next_bytes, "grant-1", &sha256_hex(next_bytes)).expect("cas");
        assert_ne!(next, head);
        let other = br#"{"bindings":[{"project_id":"alpha","role":"auditor","subject":"other"}],"protocol":"oo.identity-binding/document/v1"}"#;
        let lost = cas_commit(&git, &head, other, "grant-2", &sha256_hex(other));
        assert_eq!(lost, Err(Fault::git_cas()));
        let replay = cas_commit(&git, &head, next_bytes, "grant-1", &sha256_hex(next_bytes)).expect("idempotent object");
        assert_eq!(replay, next);
        let _ = std::fs::remove_dir_all(&root);
    }
}
