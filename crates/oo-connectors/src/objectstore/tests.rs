//! fs backend, path rules and dual hash (B-S2-02). No external deps: the path
//! properties use a small deterministic generator (oo-connectors has no
//! proptest dev-dependency; the generator covers the same properties).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::fs::{FsSource, is_settled};
use super::hash::{DualHasher, hash_reader};
use super::path::*;
use super::*;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "oo-objectstore-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
    fn root(&self) -> PathBuf {
        self.0.join("root")
    }
    fn put(&self, rel: &str, data: &[u8]) {
        let p = self.root().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn src(t: &Tmp) -> FsSource {
    std::fs::create_dir_all(t.root()).unwrap();
    FsSource::open(&t.root(), ExcludeSet::default()).unwrap()
}

async fn all(s: &FsSource, page: u32) -> (Vec<ObjectMeta>, bool) {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let p = s.list(cursor.as_ref(), page).await.unwrap();
        out.extend(p.entries);
        if p.next.is_none() {
            return (out, p.complete);
        }
        cursor = p.next;
    }
}

fn keys(v: &[ObjectMeta]) -> Vec<String> {
    v.iter()
        .map(|m| String::from_utf8_lossy(&m.raw_key).into_owned())
        .collect()
}

#[test]
fn objectstore_skip_table() {
    for name in [
        ".git",
        ".DS_Store",
        ".context",
        ".~lock.a.docx#",
        "~$a.docx",
        "Thumbs.db",
        "THUMBS.DB",
        "desktop.ini",
        "a.tmp",
        "b.PART",
        "c.crdownload",
        "node_modules",
        "__pycache__",
    ] {
        assert!(skip_reason(name).is_some(), "{name}");
    }
    for name in [
        "a.md",
        "tmp",
        "partial.txt",
        "my.tmp.md",
        "~a.md",
        "计划.md",
    ] {
        assert!(skip_reason(name).is_none(), "{name}");
    }
    assert_eq!(skip_reason(".context"), Some(SkipReason::ContextDir));
    assert!(is_skipped_path("a/.context/x.md"));
}

#[test]
fn objectstore_exclude_globs_are_additive() {
    let ex = ExcludeSet::new(["drafts/**".to_owned(), "*.bak".to_owned()]);
    assert!(ex.excludes("drafts/a/b.md"));
    assert!(ex.excludes("x.bak"));
    assert!(!ex.excludes("a/x.bak"), "single star stays in one segment");
    assert!(ex.excludes("a/.context/x"), "built-ins cannot be removed");
    assert!(!ex.excludes("notes/a.md"));
    assert!(glob_match("a/?/c", "a/b/c"));
    assert!(!glob_match("a/?/c", "a///c"));
}

#[test]
fn objectstore_normalize_raw_key() {
    let nfd = "e\u{301}.md";
    let p = normalize_raw_key(nfd.as_bytes()).unwrap();
    assert_eq!(p.as_str(), "\u{e9}.md");
    assert!(normalize_raw_key(&[0xff, 0xfe]).is_none());
    assert!(normalize_raw_key(b"a/../b").is_none());
    assert!(normalize_raw_key(b"/a").is_none());
    assert!(normalize_raw_key(b"").is_none());
    assert!(!raw_key_is_safe(b"a/../b") && !raw_key_is_safe(b"/a") && !raw_key_is_safe(b"a\0b"));
    assert!(raw_key_is_safe(b"a/b.md") && raw_key_is_safe(&[b'a', b'/', 0xff]));
}

#[test]
fn objectstore_collisions_and_fold_conflicts() {
    let mk = |raw: &[u8]| ObjectMeta {
        raw_key: raw.to_vec(),
        path: normalize_raw_key(raw),
        kind: EntryKind::File,
        size: 1,
        etag: None,
        last_modified_ns: None,
        version_id: None,
        crc64: None,
        fs_ino: None,
        fs_ctime_ns: None,
    };
    let nfc = "\u{e9}.md".as_bytes();
    let nfd = "e\u{301}.md".as_bytes();
    let entries = vec![
        mk(nfd),
        mk(nfc),
        mk(b"A.md"),
        mk(b"a.md"),
        mk(&[0xff]),
        mk(b"z.md"),
    ];
    let st = annotate(&entries);
    // NFC bytes sort after NFD bytes? the first by raw bytes owns the path.
    let (first, second) = if nfc < nfd { (1, 0) } else { (0, 1) };
    assert_eq!(st[first], PathStatus::Ok);
    assert_eq!(st[second], PathStatus::Collision);
    assert_eq!(st[2], PathStatus::FoldConflict);
    assert_eq!(st[3], PathStatus::FoldConflict);
    assert_eq!(st[4], PathStatus::Invalid);
    assert_eq!(st[5], PathStatus::Ok);
}

// ---- deterministic property checks (KP1) ----

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn random_key(r: &mut Rng) -> Vec<u8> {
    const ATOMS: &[&[u8]] = &[
        b"a",
        b"..",
        b".",
        b"/",
        b"//",
        b"\\",
        b"\0",
        "é".as_bytes(),
        "e\u{301}".as_bytes(),
        b".context",
        b"x.md",
        &[0xff],
        b"%2e",
        b" ",
        "中".as_bytes(),
        b"\n",
    ];
    let n = (r.next() % 12) as usize;
    let mut out = Vec::new();
    for _ in 0..n {
        out.extend_from_slice(ATOMS[(r.next() % ATOMS.len() as u64) as usize]);
    }
    out
}

#[test]
fn objectstore_path_properties() {
    let mut r = Rng(0x9e3779b97f4a7c15);
    for _ in 0..20_000 {
        let raw = random_key(&mut r);
        if let Some(p) = normalize_raw_key(&raw) {
            let s = p.as_str();
            assert!(!s.starts_with('/') && !s.ends_with('/'));
            assert!(s.split('/').all(|x| !x.is_empty() && x != "." && x != ".."));
            assert!(!s.chars().any(char::is_control) && !s.contains('\\'));
            // idempotent
            assert_eq!(normalize_raw_key(s.as_bytes()).unwrap().as_str(), s);
            assert_eq!(
                path_fold(&p),
                path_fold(&normalize_raw_key(s.as_bytes()).unwrap())
            );
        }
        if raw_key_is_safe(&raw) {
            assert!(!raw.starts_with(b"/") && !raw.contains(&0));
            assert!(
                raw.split(|b| *b == b'/')
                    .all(|s| s != b".." && s != b"." && !s.is_empty())
            );
        }
        // `.context` anywhere always excludes, regardless of extra globs.
        let mut with_ctx = b"d/".to_vec();
        with_ctx.extend_from_slice(b".context/");
        with_ctx.extend_from_slice(&raw);
        let text = String::from_utf8_lossy(&with_ctx);
        assert!(ExcludeSet::default().excludes(&text));
    }
}

// ---- hashing ----

#[test]
fn objectstore_dual_hash_vectors_and_streaming() {
    let d = hash_reader(&b""[..]).unwrap();
    assert_eq!(
        d.b3,
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
    );
    assert_eq!(
        d.sha256,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    let mut h = DualHasher::new();
    h.update(b"a");
    h.update(b"bc");
    let d = h.finalize();
    assert_eq!(
        d.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(d.len, 3);
    let big = vec![7u8; 300_000];
    let one = hash_reader(&big[..]).unwrap();
    let mut h = DualHasher::new();
    for chunk in big.chunks(1000) {
        h.update(chunk);
    }
    assert_eq!(one, h.finalize());
}

// ---- fs backend ----

#[tokio::test]
async fn objectstore_fs_lists_skips_and_paginates() {
    let t = Tmp::new("list");
    for f in [
        "a.md",
        "b/c.md",
        "b/d/e.txt",
        "b-x.md",
        ".git/config",
        "b/.DS_Store",
        "Thumbs.db",
        "~$w.docx",
        "n/node_modules/p.js",
        "z.tmp",
        "docs/.context/BRIEF.md",
        "计划.md",
    ] {
        t.put(f, b"x");
    }
    let s = src(&t);
    let (entries, complete) = all(&s, 100).await;
    assert!(complete);
    assert_eq!(
        keys(&entries),
        ["a.md", "b/c.md", "b/d/e.txt", "b-x.md", "计划.md"]
    );
    for page in [1, 2, 3] {
        let (paged, complete) = all(&s, page).await;
        assert!(complete);
        assert_eq!(keys(&paged), keys(&entries), "page size {page}");
    }
    assert!(
        entries
            .iter()
            .all(|m| m.kind == EntryKind::File && m.fs_ino.is_some())
    );
    assert!(
        entries
            .iter()
            .all(|m| m.path.is_some() && m.fingerprint().size == 1)
    );
}

#[tokio::test]
async fn objectstore_fs_context_and_hidden_are_unreadable() {
    let t = Tmp::new("ctx");
    t.put("d/.context/BRIEF.md", b"secret");
    t.put(".git/config", b"cfg");
    t.put("ok.md", b"ok");
    let s = src(&t);
    for k in ["d/.context/BRIEF.md", ".git/config"] {
        assert_eq!(s.stat(k.as_bytes()).await.unwrap(), None);
        assert_eq!(
            s.read(k.as_bytes(), 0, 10, &ReadExpect::default()).await,
            Err(DocSourceError::NotFound)
        );
    }
    assert!(s.stat(b"ok.md").await.unwrap().is_some());
}

#[tokio::test]
async fn objectstore_fs_ranged_read_and_expect() {
    let t = Tmp::new("read");
    t.put("a.md", b"0123456789");
    let s = src(&t);
    let none = ReadExpect::default();
    assert_eq!(s.read(b"a.md", 2, 4, &none).await.unwrap(), b"2345");
    assert_eq!(s.read(b"a.md", 8, 100, &none).await.unwrap(), b"89");
    assert_eq!(s.read(b"a.md", 50, 5, &none).await.unwrap(), b"");
    let wrong = ReadExpect {
        size: Some(9),
        ..Default::default()
    };
    assert_eq!(
        s.read(b"a.md", 0, 1, &wrong).await,
        Err(DocSourceError::Changed)
    );
    assert_eq!(
        s.read(b"missing", 0, 1, &none).await,
        Err(DocSourceError::NotFound)
    );
    assert_eq!(
        s.read(b"../x", 0, 1, &none).await,
        Err(DocSourceError::Protocol("invalid object key"))
    );
    assert!(s.read(b"a.md", 0, u64::MAX, &none).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn objectstore_fs_symlink_escape_fails_closed() {
    use std::os::unix::fs::symlink;
    let t = Tmp::new("sym");
    t.put("a.md", b"in");
    let outside = t.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"TOP SECRET").unwrap();
    symlink(outside.join("secret.txt"), t.root().join("file-link.txt")).unwrap();
    symlink(&outside, t.root().join("dir-link")).unwrap();
    symlink("a.md", t.root().join("inside-link.md")).unwrap();
    let s = src(&t);
    let none = ReadExpect::default();

    // Listing shows symlinks as Symlink and never descends into a dir link.
    let (entries, complete) = all(&s, 100).await;
    assert!(complete);
    let kinds: Vec<_> = entries
        .iter()
        .map(|m| (String::from_utf8_lossy(&m.raw_key).into_owned(), m.kind))
        .collect();
    assert!(kinds.contains(&("dir-link".into(), EntryKind::Symlink)));
    assert!(kinds.contains(&("file-link.txt".into(), EntryKind::Symlink)));
    assert!(!kinds.iter().any(|(k, _)| k.contains("secret")));

    // Reads through a final-component link, an intermediate link and an
    // in-root link all fail closed; stat reports the link without following.
    for k in ["file-link.txt", "dir-link/secret.txt", "inside-link.md"] {
        assert_eq!(
            s.read(k.as_bytes(), 0, 100, &none).await,
            Err(DocSourceError::PermissionDenied),
            "{k}"
        );
        assert!(s.hash_object(k.as_bytes()).await.is_err(), "{k}");
    }
    let m = s.stat(b"file-link.txt").await.unwrap().unwrap();
    assert_eq!(m.kind, EntryKind::Symlink);
    assert_eq!(
        s.stat(b"dir-link/secret.txt").await,
        Err(DocSourceError::PermissionDenied)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn objectstore_fs_special_files_skipped_and_counted() {
    let t = Tmp::new("fifo");
    t.put("a.md", b"x");
    let fifo = t.root().join("pipe");
    let ok = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return; // no mkfifo on this host
    }
    let s = src(&t);
    let (entries, _) = all(&s, 10).await;
    assert_eq!(keys(&entries), ["a.md"]);
    assert_eq!(s.skipped_special(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn objectstore_fs_non_utf8_name_has_no_path() {
    use std::os::unix::ffi::OsStrExt;
    let t = Tmp::new("raw");
    std::fs::create_dir_all(t.root()).unwrap();
    let name = std::ffi::OsStr::from_bytes(&[b'b', 0xff, b'.', b'm', b'd']);
    if std::fs::write(t.root().join(name), b"x").is_err() {
        return; // filesystem refuses non UTF-8 names (macOS APFS)
    }
    let s = src(&t);
    let (entries, _) = all(&s, 10).await;
    assert_eq!(entries.len(), 1);
    assert!(entries[0].path.is_none());
    let m = s.stat(&entries[0].raw_key).await.unwrap().unwrap();
    assert_eq!(m.raw_key, entries[0].raw_key);
}

#[tokio::test]
async fn objectstore_fs_hash_object_and_stability() {
    let t = Tmp::new("hash");
    t.put("a.md", b"abc");
    let s = src(&t);
    let (d, meta) = s.hash_object(b"a.md").await.unwrap();
    assert_eq!(
        d.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(d.b3, blake3::hash(b"abc").to_hex().to_string());
    assert_eq!(meta.size, 3);
    let f1 = meta.fingerprint();
    let mut f2 = f1.clone();
    assert!(is_settled(&f1, &f2));
    f2.last_modified_ns = f2.last_modified_ns.map(|n| n + 1);
    assert!(!is_settled(&f1, &f2));
    f2 = f1.clone();
    f2.size += 1;
    assert!(!is_settled(&f1, &f2));
}

#[tokio::test]
async fn objectstore_fs_root_rules() {
    let t = Tmp::new("root");
    assert_eq!(
        FsSource::open(&t.root(), ExcludeSet::default()).err(),
        Some(DocSourceError::InvalidConfig(
            "source root is missing or not a directory"
        ))
    );
    t.put("f.md", b"x");
    let file_root = t.root().join("f.md");
    assert!(FsSource::open(&file_root, ExcludeSet::default()).is_err());
    let s = src(&t);
    std::fs::remove_dir_all(t.root()).unwrap();
    assert!(
        s.list(None, 10).await.is_err(),
        "vanished root is an error, never an empty listing"
    );
    let _ = Path::new("");
}

#[tokio::test]
async fn objectstore_fs_unreadable_dir_marks_listing_incomplete() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let t = Tmp::new("perm");
        t.put("a.md", b"x");
        t.put("locked/b.md", b"x");
        let locked = t.root().join("locked");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let s = src(&t);
        let readable = std::fs::read_dir(&locked).is_ok(); // running as root
        let (entries, complete) = all(&s, 10).await;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !readable {
            assert_eq!(keys(&entries), ["a.md"]);
            assert!(!complete, "an unreadable subtree must block the sweep");
        }
    }
}

// ---- first level scopes (B-S11-03, 10 11.3) ----

async fn scope_all(s: &FsSource, scope: &str, page: u32) -> (Vec<ObjectMeta>, bool) {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let p = s.list_scope(scope, cursor.as_ref(), page).await.unwrap();
        out.extend(p.entries);
        if p.next.is_none() {
            return (out, p.complete);
        }
        cursor = p.next;
    }
}

#[tokio::test]
async fn objectstore_fs_scopes_partition_the_listing() {
    let t = Tmp::new("scopes");
    for f in [
        "a.md",
        "b/c.md",
        "b/d/e.txt",
        "b-x.md",
        ".git/config",
        "Thumbs.db",
        "z/.DS_Store",
        "计划/x.md",
        "empty/.keep.tmp",
    ] {
        t.put(f, b"x");
    }
    let s = src(&t);
    let scopes = s.list_scopes().await.unwrap().unwrap();
    // `""` first (files directly under the root), then directories by name; skipped names are not scopes.
    assert_eq!(scopes, ["", "b", "empty", "z", "计划"]);
    let (whole, _) = all(&s, 100).await;
    for page in [1, 2, 100] {
        let mut union: Vec<String> = Vec::new();
        for scope in &scopes {
            let (part, complete) = scope_all(&s, scope, page).await;
            assert!(complete, "scope {scope:?} page {page}");
            assert!(
                part.iter().all(|m| {
                    let k = String::from_utf8_lossy(&m.raw_key).into_owned();
                    match k.split_once('/') {
                        Some((first, _)) => first == scope,
                        None => scope.is_empty(),
                    }
                }),
                "scope {scope:?} leaked an entry of another scope"
            );
            union.extend(keys(&part));
        }
        union.sort();
        let mut want = keys(&whole);
        want.sort();
        assert_eq!(
            union, want,
            "page size {page}: the scopes together are the whole listing"
        );
    }
    // A scope that no longer exists lists as nothing, and is complete: its files are gone.
    let (gone, complete) = scope_all(&s, "nope", 10).await;
    assert!(gone.is_empty() && complete);
    // Only one level name is a scope.
    assert!(s.list_scope("b/d", None, 10).await.is_err());
    assert!(s.list_scope("..", None, 10).await.is_err());
}

#[tokio::test]
async fn objectstore_fs_scopes_are_none_for_a_non_utf8_top_level_name() {
    use std::os::unix::ffi::OsStrExt;
    let t = Tmp::new("scopes-bad");
    let s = src(&t);
    let bad = t.root().join(std::ffi::OsStr::from_bytes(b"\xff-dir"));
    if std::fs::create_dir_all(&bad).is_err() {
        return; // the file system refuses the name (macOS APFS): nothing to prove here
    }
    std::fs::write(bad.join("a.md"), b"x").unwrap();
    assert_eq!(s.list_scopes().await.unwrap(), None, "listed whole instead");
}
