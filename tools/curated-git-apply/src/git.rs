//! Fixed-argv Git plumbing. The child environment does not inherit host Git state.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::CuratedGitError;
use crate::identity::{SERVICE_DATE, SERVICE_EMAIL, SERVICE_NAME};
use crate::request::{APPROVED_REF, is_sha1};

const MAX_CAPTURE: usize = 4 * 1_048_576;
const MAX_TREE_RECORDS: usize = 16_384;

pub struct GitSession {
    binary: PathBuf,
    path_dir: PathBuf,
    root: PathBuf,
    git_dir: PathBuf,
    index: PathBuf,
    home: PathBuf,
    tmp: PathBuf,
    hooks: PathBuf,
    deadline: Instant,
    temp: PathBuf,
}

pub struct TreeEntry {
    pub mode: String,
    pub object: String,
}

struct GitOut {
    status: i32,
    stdout: Vec<u8>,
}

struct ChildGuard(Option<Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for GitSession {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp);
    }
}

impl GitSession {
    pub fn open(binary: &Path, root: &Path, deadline: Instant) -> Result<Self, CuratedGitError> {
        if Instant::now() >= deadline {
            return Err(deadline_error());
        }
        if !binary.is_absolute() || binary.file_name().and_then(|name| name.to_str()) != Some("git") {
            return Err(spawn_error());
        }
        let meta = fs::symlink_metadata(binary).map_err(|_| spawn_error())?;
        if !meta.file_type().is_symlink() && !meta.is_file() {
            return Err(spawn_error());
        }
        let path_dir = binary.parent().ok_or_else(spawn_error)?.to_path_buf();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp = std::env::temp_dir().join(format!(
            "curated-git-apply-{}-{nanos}-{seq}",
            std::process::id()
        ));
        if temp.starts_with(root) {
            return Err(CuratedGitError::new(
                "root.escape",
                "a path escapes the checkout",
            ));
        }
        fs::create_dir_all(temp.join("home")).map_err(|_| spawn_error())?;
        fs::create_dir_all(temp.join("tmp")).map_err(|_| spawn_error())?;
        fs::create_dir_all(temp.join("hooks")).map_err(|_| spawn_error())?;
        Ok(Self {
            binary: binary.to_path_buf(),
            path_dir,
            root: root.to_path_buf(),
            git_dir: root.join(".git"),
            index: temp.join("index"),
            home: temp.join("home"),
            tmp: temp.join("tmp"),
            hooks: temp.join("hooks"),
            deadline,
            temp,
        })
    }

    pub fn prepare(&self) -> Result<(), CuratedGitError> {
        let format = self.text(&["rev-parse", "--show-object-format"], None, false)?;
        if format.trim_end() != "sha1" {
            return Err(CuratedGitError::new(
                "object.format",
                "the repository object format is not sha1",
            ));
        }
        self.reject_embedded(&self.tip_or_missing()?)?;
        Ok(())
    }

    pub fn reject_embedded(&self, commit: &str) -> Result<(), CuratedGitError> {
        self.scan_tree(commit)
    }

    pub fn tip(&self) -> Result<String, CuratedGitError> {
        self.tip_or_missing()
    }

    pub fn require_commit(&self, object: &str) -> Result<(), CuratedGitError> {
        if !is_sha1(object) {
            return Err(CuratedGitError::new(
                "head.type",
                "the expected object is not a commit",
            ));
        }
        let out = self.run(&["cat-file", "-t", "--", object], None, false, false)?;
        if out.status != 0 {
            return Err(CuratedGitError::new(
                "head.missing",
                "the expected commit does not exist",
            ));
        }
        if out.stdout.as_slice() != b"commit\n" {
            return Err(CuratedGitError::new(
                "head.type",
                "the expected object is not a commit",
            ));
        }
        Ok(())
    }

    pub fn tree_of(&self, commit: &str) -> Result<String, CuratedGitError> {
        let spec = format!("{commit}^{{tree}}");
        self.sha(&["rev-parse", "--verify", "--end-of-options", &spec])
    }

    pub fn entry(&self, commit: &str, path: &str) -> Result<Option<TreeEntry>, CuratedGitError> {
        let out = self.run(&["ls-tree", "-z", commit, "--", path], None, false, false)?;
        if out.status != 0 {
            return Err(git_exit());
        }
        if out.stdout.is_empty() {
            return Ok(None);
        }
        parse_one_entry(&out.stdout)
    }

    pub fn blob(&self, object: &str) -> Result<Vec<u8>, CuratedGitError> {
        if !is_sha1(object) {
            return Err(git_exit());
        }
        let out = self.run(&["cat-file", "blob", "--", object], None, false, false)?;
        if out.status != 0 {
            return Err(git_exit());
        }
        if out.stdout.len() > 1_048_576 {
            return Err(CuratedGitError::new(
                "limit.bytes",
                "the document exceeds the byte bound",
            ));
        }
        Ok(out.stdout)
    }

    pub fn worktree_dirty(&self, commit: &str) -> Result<bool, CuratedGitError> {
        self.success(&["read-tree", commit], None, false)?;
        // read-tree leaves stat fields empty. diff-files --quiet then treats
        // every path as changed and does not hash it. Refresh only this private index.
        let refreshed = self.run(&["update-index", "--refresh", "-q"], None, true, false)?;
        if refreshed.status != 0 && refreshed.status != 1 {
            return Err(git_exit());
        }
        let tracked = self.run(&["diff-files", "--quiet"], None, true, false)?;
        if tracked.status != 0 && tracked.status != 1 {
            return Err(git_exit());
        }
        let others = self.run(
            &["ls-files", "--others", "--exclude-standard", "-z"],
            None,
            true,
            false,
        )?;
        if others.status != 0 {
            return Err(git_exit());
        }
        Ok(tracked.status == 1 || !others.stdout.is_empty())
    }

    pub fn message(&self, commit: &str) -> Result<String, CuratedGitError> {
        let out = self.run(&["cat-file", "-p", "--", commit], None, false, false)?;
        if out.status != 0 {
            return Err(git_exit());
        }
        let text = String::from_utf8(out.stdout).map_err(|_| git_exit())?;
        let body = text.split_once("\n\n").map(|(_, body)| body).unwrap_or("");
        Ok(body.to_owned())
    }

    pub fn first_parents(&self, tip: &str) -> Result<Vec<String>, CuratedGitError> {
        let out = self.run(
            &["rev-list", "--max-count=32", "--first-parent", tip],
            None,
            false,
            false,
        )?;
        if out.status != 0 {
            return Err(git_exit());
        }
        let text = String::from_utf8(out.stdout).map_err(|_| git_exit())?;
        let mut commits = Vec::new();
        for line in text.lines() {
            if !is_sha1(line) {
                return Err(git_exit());
            }
            commits.push(line.to_owned());
        }
        Ok(commits)
    }

    pub fn build(
        &self,
        parent: &str,
        files: &[(String, Vec<u8>)],
        message: &str,
    ) -> Result<(String, String, Vec<String>), CuratedGitError> {
        self.success(&["read-tree", parent], None, false)?;
        let mut blobs = Vec::with_capacity(files.len());
        for (path, bytes) in files {
            if !curated_dataset_path(path) {
                return Err(CuratedGitError::new(
                    "root.escape",
                    "a path escapes the checkout",
                ));
            }
            let blob = self.sha_in(&["hash-object", "-w", "--no-filters", "--stdin"], Some(bytes), false)?;
            let info = format!("100644,{blob},{path}");
            self.success(
                &["update-index", "--add", "--replace", "--cacheinfo", &info],
                None,
                false,
            )?;
            blobs.push(blob);
        }
        let tree = self.sha(&["write-tree"])?;
        let commit = self.sha_in(
            &["commit-tree", &tree, "-p", parent],
            Some(message.as_bytes()),
            true,
        )?;
        let seen_parent = self.sha(&["rev-parse", "--verify", "--end-of-options", &format!("{commit}^")])?;
        if seen_parent != parent {
            return Err(git_exit());
        }
        for ((path, _), blob) in files.iter().zip(&blobs) {
            let entry = self.entry(&commit, path)?.ok_or_else(git_exit)?;
            if entry.mode != "100644" || entry.object != *blob {
                return Err(git_exit());
            }
        }
        Ok((tree, commit, blobs))
    }

    pub fn cas(&self, new_commit: &str, expected: &str) -> Result<(), CuratedGitError> {
        let out = self.run(
            &["update-ref", "--no-deref", APPROVED_REF, new_commit, expected],
            None,
            false,
            false,
        )?;
        if out.status != 0 {
            return Err(CuratedGitError::new(
                "ref.cas",
                "the ref does not match the expected commit",
            ));
        }
        Ok(())
    }

    fn tip_or_missing(&self) -> Result<String, CuratedGitError> {
        let out = self.run(
            &["rev-parse", "--verify", "--end-of-options", APPROVED_REF],
            None,
            false,
            false,
        )?;
        if out.status != 0 {
            return Err(CuratedGitError::new(
                "ref.missing",
                "the approved ref does not resolve",
            ));
        }
        sha_text(&out.stdout)
    }

    fn scan_tree(&self, commit: &str) -> Result<(), CuratedGitError> {
        let out = self.run(&["ls-tree", "-r", "-z", commit], None, false, false)?;
        if out.status != 0 {
            return Err(git_exit());
        }
        let mut records = 0usize;
        for record in out.stdout.split(|byte| *byte == 0) {
            if record.is_empty() {
                continue;
            }
            records += 1;
            if records > MAX_TREE_RECORDS {
                return Err(CuratedGitError::new(
                    "limit.files",
                    "the change set exceeds the file bound",
                ));
            }
            let mode = record.split(|byte| *byte == b' ').next().unwrap_or(b"");
            if mode == b"160000" {
                return Err(CuratedGitError::new(
                    "root.embedded",
                    "the checkout contains another git directory or submodule",
                ));
            }
            if let Some(path) = record.split(|byte| *byte == b'\t').nth(1)
                && path == b".gitmodules"
            {
                return Err(CuratedGitError::new(
                    "root.embedded",
                    "the checkout contains another git directory or submodule",
                ));
            }
        }
        Ok(())
    }

    fn success(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        author: bool,
    ) -> Result<(), CuratedGitError> {
        let out = self.run(args, stdin, false, author)?;
        if out.status != 0 {
            Err(git_exit())
        } else {
            Ok(())
        }
    }

    fn sha(&self, args: &[&str]) -> Result<String, CuratedGitError> {
        self.sha_in(args, None, false)
    }

    fn sha_in(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        author: bool,
    ) -> Result<String, CuratedGitError> {
        let text = self.text(args, stdin, author)?;
        sha_text(text.as_bytes())
    }

    fn text(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        author: bool,
    ) -> Result<String, CuratedGitError> {
        let out = self.run(args, stdin, false, author)?;
        if out.status != 0 {
            return Err(git_exit());
        }
        String::from_utf8(out.stdout).map_err(|_| git_exit())
    }

    fn run(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        worktree: bool,
        author: bool,
    ) -> Result<GitOut, CuratedGitError> {
        if Instant::now() >= self.deadline {
            return Err(deadline_error());
        }
        let mut command = Command::new(&self.binary);
        command.env_clear();
        command.env("PATH", &self.path_dir);
        command.env("HOME", &self.home);
        command.env("TMPDIR", &self.tmp);
        command.env("GIT_DIR", &self.git_dir);
        command.env("GIT_INDEX_FILE", &self.index);
        command.env("GIT_CONFIG_GLOBAL", "/dev/null");
        command.env("GIT_CONFIG_SYSTEM", "/dev/null");
        command.env("GIT_CONFIG_NOSYSTEM", "1");
        command.env("GIT_TERMINAL_PROMPT", "0");
        command.env("GIT_OPTIONAL_LOCKS", "0");
        command.env("GIT_NO_REPLACE_OBJECTS", "1");
        command.env("GIT_ALLOW_PROTOCOL", "");
        if worktree {
            command.env("GIT_WORK_TREE", &self.root);
        }
        if author {
            command.env("GIT_AUTHOR_NAME", SERVICE_NAME);
            command.env("GIT_AUTHOR_EMAIL", SERVICE_EMAIL);
            command.env("GIT_AUTHOR_DATE", SERVICE_DATE);
            command.env("GIT_COMMITTER_NAME", SERVICE_NAME);
            command.env("GIT_COMMITTER_EMAIL", SERVICE_EMAIL);
            command.env("GIT_COMMITTER_DATE", SERVICE_DATE);
        }
        command.current_dir(&self.root);
        command.arg("-c").arg(format!("core.hooksPath={}", self.hooks.display()));
        command.arg("-c").arg("core.fsmonitor=false");
        command.arg("-c").arg("commit.gpgsign=false");
        command.arg("-c").arg("tag.gpgsign=false");
        command.arg("-c").arg("credential.helper=");
        command.arg("-c").arg("protocol.allow=never");
        command.arg("-c").arg("gc.auto=0");
        command.arg("-c").arg("maintenance.auto=false");
        command.arg("-c").arg("core.autocrlf=false");
        command.arg("-c").arg("core.attributesFile=/dev/null");
        command
            .arg("-c")
            .arg(format!("safe.directory={}", self.root.display()));
        command.args(args);
        command.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|_| spawn_error())?;
        if let Some(bytes) = stdin {
            let mut pipe = child.stdin.take().ok_or_else(spawn_error)?;
            pipe.write_all(bytes).map_err(|_| git_exit())?;
            drop(pipe);
        }
        let mut stdout = child.stdout.take().ok_or_else(spawn_error)?;
        let mut stderr = child.stderr.take().ok_or_else(spawn_error)?;
        let out_thread = thread::spawn(move || drain(&mut stdout));
        let err_thread = thread::spawn(move || drain(&mut stderr));
        let mut guard = ChildGuard(Some(child));
        let status = wait_deadline(guard.0.as_mut().ok_or_else(spawn_error)?, self.deadline)?;
        guard.0.take();
        let stdout = out_thread.join().map_err(|_| git_exit())??;
        let _stderr = err_thread.join().map_err(|_| git_exit())??;
        Ok(GitOut {
            status: status.code().unwrap_or(1),
            stdout,
        })
    }
}

fn wait_deadline(child: &mut Child, deadline: Instant) -> Result<std::process::ExitStatus, CuratedGitError> {
    loop {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(deadline_error());
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => return Err(git_exit()),
        }
    }
}

fn drain(reader: &mut dyn Read) -> Result<Vec<u8>, CuratedGitError> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    let mut overflow = false;
    loop {
        let read = reader.read(&mut buf).map_err(|_| git_exit())?;
        if read == 0 {
            break;
        }
        if overflow || out.len() + read > MAX_CAPTURE {
            overflow = true;
            continue;
        }
        out.extend_from_slice(&buf[..read]);
    }
    if overflow {
        Err(CuratedGitError::new(
            "limit.bytes",
            "the document exceeds the byte bound",
        ))
    } else {
        Ok(out)
    }
}

fn parse_one_entry(bytes: &[u8]) -> Result<Option<TreeEntry>, CuratedGitError> {
    let record = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    if record.is_empty() {
        return Ok(None);
    }
    if record.contains(&0) {
        return Err(git_exit());
    }
    let mut parts = record.split(|byte| *byte == b' ');
    let mode = parts.next().ok_or_else(git_exit)?;
    let kind = parts.next().ok_or_else(git_exit)?;
    let rest = parts.next().ok_or_else(git_exit)?;
    if parts.next().is_some() || kind != b"blob" {
        return Err(CuratedGitError::new(
            "blob.mode",
            "the curated path is not a regular file",
        ));
    }
    let mut object_and_path = rest.split(|byte| *byte == b'\t');
    let object = object_and_path.next().ok_or_else(git_exit)?;
    if object_and_path.next().is_none() {
        return Err(git_exit());
    }
    let mode = std::str::from_utf8(mode).map_err(|_| git_exit())?;
    let object = std::str::from_utf8(object).map_err(|_| git_exit())?;
    if !is_sha1(object) {
        return Err(git_exit());
    }
    Ok(Some(TreeEntry {
        mode: mode.to_owned(),
        object: object.to_owned(),
    }))
}

fn sha_text(bytes: &[u8]) -> Result<String, CuratedGitError> {
    let text = std::str::from_utf8(bytes).map_err(|_| git_exit())?;
    let sha = text.trim_end_matches('\n');
    if text.ends_with('\n') && is_sha1(sha) && !sha.contains('\n') {
        Ok(sha.to_owned())
    } else {
        Err(git_exit())
    }
}

fn curated_dataset_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("ontology/curated/datasets/") else {
        return false;
    };
    let Some((schema, file)) = rest.split_once('/') else {
        return false;
    };
    let Some(table) = file.strip_suffix(".yaml") else {
        return false;
    };
    !schema.is_empty()
        && !table.is_empty()
        && !schema.contains('/')
        && !table.contains('/')
        && !schema.contains('.')
        && !table.contains('.')
        && !path.contains("..")
}

fn deadline_error() -> CuratedGitError {
    CuratedGitError::new(
        "deadline.exceeded",
        "the git operation exceeded the deadline",
    )
}

fn git_exit() -> CuratedGitError {
    CuratedGitError::new("git.exit", "git returned a non-zero status")
}

fn spawn_error() -> CuratedGitError {
    CuratedGitError::new("git.spawn", "git could not be started")
}
