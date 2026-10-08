//! Read-only check of one management config commit. This module never pushes or writes a ref.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::binding::hex40;
use crate::config::BindingGit;
use crate::error::Fault;

pub const AUTHOR_LINE: &str = "oo-identity-binding <identity-binding@open-ontology.invalid> 1791417600 +0000";
const FILE_PATH: &str = "management/bindings.json";

pub fn read_head(git: &BindingGit) -> Result<String, Fault> {
    let repo = open_repo(git)?;
    let output = run(&repo, &["rev-parse", "--verify", git.reference.as_str()], None)?;
    parse_hex40(&output)
}

pub fn verify_new_commit(git: &BindingGit, expected_head: &str, new_commit: &str, document: &[u8], idempotency_key: &str, document_sha256: &str) -> Result<(), Fault> {
    if !hex40(expected_head) || !hex40(new_commit) {
        return Err(Fault::git_rejected());
    }
    let repo = open_repo(git)?;
    let head = parse_hex40(&run(&repo, &["rev-parse", "--verify", git.reference.as_str()], None)?)?;
    if head != new_commit {
        return Err(Fault::git_cas());
    }
    verify_object(&repo, new_commit, expected_head, document, idempotency_key, document_sha256)
}

pub fn verify_tree_blob(git: &BindingGit, commit: &str, document: &[u8]) -> Result<(), Fault> {
    if !hex40(commit) {
        return Err(Fault::git_rejected());
    }
    let repo = open_repo(git)?;
    let blob = blob_of(&repo, commit)?;
    let bytes = run(&repo, &["cat-file", "blob", &blob], None)?;
    if bytes == document { Ok(()) } else { Err(Fault::git_rejected()) }
}

fn verify_object(repo: &Repo, commit: &str, expected_head: &str, document: &[u8], idempotency_key: &str, document_sha256: &str) -> Result<(), Fault> {
    let kind = trim_text(&run(repo, &["cat-file", "-t", commit], None)?)?;
    if kind != "commit" {
        return Err(Fault::git_rejected());
    }
    let text = trim_text(&run(repo, &["cat-file", "commit", commit], None)?)?;
    let mut lines = text.lines();
    let tree = lines.next().ok_or_else(Fault::git_rejected)?;
    let parent = lines.next().ok_or_else(Fault::git_rejected)?;
    let author = lines.next().ok_or_else(Fault::git_rejected)?;
    let committer = lines.next().ok_or_else(Fault::git_rejected)?;
    let blank = lines.next().ok_or_else(Fault::git_rejected)?;
    let message = lines.next().ok_or_else(Fault::git_rejected)?;
    if lines.next().is_some() || !blank.is_empty() {
        return Err(Fault::git_rejected());
    }
    let tree_id = tree.strip_prefix("tree ").ok_or_else(Fault::git_rejected)?;
    let parent_id = parent.strip_prefix("parent ").ok_or_else(Fault::git_rejected)?;
    if !hex40(tree_id) || parent_id != expected_head || author != format!("author {AUTHOR_LINE}") || committer != format!("committer {AUTHOR_LINE}") {
        return Err(Fault::git_rejected());
    }
    let expected_message = format!("oo.identity-binding/v1 {idempotency_key} {document_sha256}");
    if message != expected_message {
        return Err(Fault::git_rejected());
    }
    let listed = trim_text(&run(repo, &["ls-tree", "-r", tree_id], None)?)?;
    let blob = blob_from_listing(&listed)?;
    let bytes = run(repo, &["cat-file", "blob", &blob], None)?;
    if bytes == document { Ok(()) } else { Err(Fault::git_rejected()) }
}

fn blob_of(repo: &Repo, commit: &str) -> Result<String, Fault> {
    let text = trim_text(&run(repo, &["cat-file", "commit", commit], None)?)?;
    let tree = text.lines().next().ok_or_else(Fault::git_rejected)?;
    let tree_id = tree.strip_prefix("tree ").ok_or_else(Fault::git_rejected)?;
    if !hex40(tree_id) {
        return Err(Fault::git_rejected());
    }
    let listed = trim_text(&run(repo, &["ls-tree", "-r", tree_id], None)?)?;
    blob_from_listing(&listed)
}

fn blob_from_listing(listed: &str) -> Result<String, Fault> {
    let mut lines = listed.lines();
    let line = lines.next().ok_or_else(Fault::git_rejected)?;
    if lines.next().is_some() {
        return Err(Fault::git_rejected());
    }
    let Some((meta, path)) = line.split_once('\t') else {
        return Err(Fault::git_rejected());
    };
    if path != FILE_PATH {
        return Err(Fault::git_rejected());
    }
    let mut parts = meta.split(' ');
    let mode = parts.next().ok_or_else(Fault::git_rejected)?;
    let kind = parts.next().ok_or_else(Fault::git_rejected)?;
    let blob = parts.next().ok_or_else(Fault::git_rejected)?;
    if parts.next().is_some() || mode != "100644" || kind != "blob" || !hex40(blob) {
        return Err(Fault::git_rejected());
    }
    Ok(blob.to_string())
}

struct Repo {
    binary: PathBuf,
    root: PathBuf,
    git_dir: PathBuf,
    scratch: PathBuf,
}

fn open_repo(git: &BindingGit) -> Result<Repo, Fault> {
    let root = canonical_dir(&git.root)?;
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
    let binary_meta = std::fs::metadata(&git.binary).map_err(|_| Fault::git_rejected())?;
    if !binary_meta.is_file() {
        return Err(Fault::git_rejected());
    }
    let scratch = std::env::temp_dir().join(format!("oo-identity-git-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).map_err(|_| Fault::git_rejected())?;
    Ok(Repo { binary: git.binary.clone(), root, git_dir, scratch })
}

fn canonical_dir(path: &Path) -> Result<PathBuf, Fault> {
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
    let subcommand = args.first().copied().ok_or_else(Fault::git_rejected)?;
    if !matches!(subcommand, "rev-parse" | "cat-file" | "ls-tree") {
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
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_ALLOW_PROTOCOL", "");
    let mut child = command.spawn().map_err(|_| Fault::git_rejected())?;
    if let Some(bytes) = input {
        use std::io::Write;
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Fault::git_rejected());
        };
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

#[cfg(test)]
mod tests {
    use super::{verify_new_commit, verify_tree_blob};
    use crate::config::BindingGit;
    use crate::crypto::sha256_hex;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    #[test]
    fn nested_binding_file_matches_the_commit() {
        let binary = PathBuf::from("/usr/bin/git");
        assert!(binary.is_file(), "git binary");
        let requested = std::env::temp_dir().join(format!("oo-a402-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&requested);
        std::fs::create_dir_all(&requested).expect("dir");
        let root = std::fs::canonicalize(&requested).expect("canonical");
        let base = br#"{"bindings":[],"protocol":"oo.identity-binding/document/v1"}"#;
        let next = br#"{"bindings":[{"project_id":"alpha","role":"reviewer","subject":"person"}],"protocol":"oo.identity-binding/document/v1"}"#;
        let seed = commit_file(&root, None, base, &format!("oo.identity-binding/v1 seed {}", sha256_hex(base)));
        let child = commit_file(&root, Some(seed.as_str()), next, &format!("oo.identity-binding/v1 grant-1 {}", sha256_hex(next)));
        let git = BindingGit { binary, root: root.clone(), reference: "refs/heads/management".to_string() };
        verify_tree_blob(&git, &seed, base).expect("seed blob");
        verify_new_commit(&git, &seed, &child, next, "grant-1", &sha256_hex(next)).expect("child");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn commit_file(root: &Path, parent: Option<&str>, document: &[u8], message: &str) -> String {
        let doc = root.join("document.bin");
        std::fs::write(&doc, document).expect("doc");
        if parent.is_none() {
            git(root, &["init"]);
        }
        let blob = git(root, &["hash-object", "-w", doc.to_str().expect("doc path")]);
        let index = root.join("index");
        git_index(root, &index, &["update-index", "--add", "--cacheinfo", "100644", &blob, "management/bindings.json"]);
        let tree = git_index(root, &index, &["write-tree"]);
        let mut args = vec!["commit-tree".to_string(), tree, "-m".to_string(), message.to_string()];
        if let Some(parent) = parent {
            args.insert(1, parent.to_string());
            args.insert(1, "-p".to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let commit = git(root, &refs);
        let old = parent.unwrap_or("0000000000000000000000000000000000000000");
        git(root, &["update-ref", "--no-deref", "refs/heads/management", &commit, old]);
        commit
    }

    fn git(root: &Path, args: &[&str]) -> String {
        git_index(root, &root.join("unused-index"), args)
    }

    fn git_index(root: &Path, index: &Path, args: &[&str]) -> String {
        let output = Command::new("/usr/bin/git")
            .args(args)
            .current_dir(root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root)
            .env("GIT_DIR", root.join(".git"))
            .env("GIT_INDEX_FILE", index)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "oo-identity-binding")
            .env("GIT_AUTHOR_EMAIL", "identity-binding@open-ontology.invalid")
            .env("GIT_AUTHOR_DATE", "1791417600 +0000")
            .env("GIT_COMMITTER_NAME", "oo-identity-binding")
            .env("GIT_COMMITTER_EMAIL", "identity-binding@open-ontology.invalid")
            .env("GIT_COMMITTER_DATE", "1791417600 +0000")
            .output()
            .expect("git");
        assert!(output.status.success(), "git {args:?} {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).expect("utf8").trim().to_string()
    }
}
