//! `oo context mount` against a stand-in daemon: what is written, how, and the
//! exit codes. The daemon is a small std server serving one fixed pack, so the
//! tests need neither a database nor the engine (the L2 script runs the real
//! stack).
#![cfg(feature = "docs")]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const TOKEN: &str = "oodt_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
const DRIVE: &str = "00000000-0000-4000-8000-0000000000d1";
const PACK: &str = "00000000-0000-4000-8000-0000000000a1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    ScopeInvalid,
    Tamper,
    Trimmed,
}

struct Fake {
    port: u16,
    mode: Arc<Mutex<Mode>>,
    requests: Arc<Mutex<Vec<(String, String, String)>>>,
    shares: Arc<AtomicU64>,
}

fn pack_files() -> BTreeMap<&'static str, Vec<u8>> {
    BTreeMap::from([
        (
            "BRIEF.md",
            "# BRIEF\n- 林川负责星火计划 [src:Projects/a/p.md#L1]\n"
                .as_bytes()
                .to_vec(),
        ),
        ("manifest.json", b"{\"files\":[]}\n".to_vec()),
        (
            "people/林川-1234abcd.md",
            "- 工程负责人 [src:People/x.md#L1]\n".as_bytes().to_vec(),
        ),
        ("refs/0001.md", b"ref one\n".to_vec()),
    ])
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn respond(s: &mut TcpStream, status: u16, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn start_fake() -> Fake {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let mode = Arc::new(Mutex::new(Mode::Normal));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let shares = Arc::new(AtomicU64::new(0));
    let (m, r, sh) = (mode.clone(), requests.clone(), shares.clone());
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut s) = conn else { continue };
            let (m, r, sh) = (m.clone(), r.clone(), sh.clone());
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (p + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                let body = String::from_utf8_lossy(&buf[head_end..head_end + len]).into_owned();
                let mut first = head.lines().next().unwrap().split(' ');
                let (method, target) = (
                    first.next().unwrap().to_owned(),
                    first.next().unwrap().to_owned(),
                );
                if !head.contains(&format!("Bearer {TOKEN}"))
                    && !head.contains(&format!("bearer {TOKEN}"))
                {
                    return respond(
                        &mut s,
                        401,
                        "application/json",
                        br#"{"code":"DOCS_ENGINE_UNAUTHENTICATED","message":"no"}"#,
                    );
                }
                r.lock()
                    .unwrap()
                    .push((method.clone(), target.clone(), body.clone()));
                let mode = *m.lock().unwrap();
                let files = pack_files();
                let path = target.split('?').next().unwrap();
                match (method.as_str(), path) {
                    ("GET", "/v1/docs/drives") => respond(
                        &mut s,
                        200,
                        "application/json",
                        json!({"items": [{"id": DRIVE, "name": "drv"}]})
                            .to_string()
                            .as_bytes(),
                    ),
                    ("POST", "/v1/docs/context-packs:compile") | ("POST", "/v1/docs/shares") => {
                        if mode == Mode::ScopeInvalid {
                            return respond(
                                &mut s,
                                422,
                                "application/json",
                                br#"{"code":"DOCS_PACK_SCOPE_INVALID","message":"scope matches no files","retryable":false,"details":{}}"#,
                            );
                        }
                        let trimmed = if mode == Mode::Trimmed {
                            json!({"decisions": {"summary": 2, "title": 0, "removed": 1}})
                        } else {
                            json!({"decisions": {"summary": 0, "title": 0, "removed": 0}})
                        };
                        let mut v = json!({
                            "id": PACK, "pack_digest": "ab".repeat(32), "version": 1, "drive_id": DRIVE,
                            "scope": [], "manifest": {}, "tokens": {"brief": 40, "total": 90, "budget": 800},
                            "trimmed": trimmed, "blocked": {"facts": 0, "entities": 0},
                            "brief_preview": String::from_utf8_lossy(&files["BRIEF.md"]),
                            "created_at": "2026-11-17T00:00:00Z",
                        });
                        if path.ends_with("/shares") {
                            let n = sh.fetch_add(1, Ordering::SeqCst) + 1;
                            let sid = format!("00000000-0000-4000-8000-{n:012x}");
                            let list: Vec<Value> = files
                                .iter()
                                .map(|(p, b)| json!({"path": p, "sha256": hex(&Sha256::digest(b)), "bytes": b.len(), "tokens": 5}))
                                .collect();
                            v["id"] = json!(sid);
                            v["pack_id"] = json!(PACK);
                            v["files"] = json!(list);
                            v["expires_at"] = json!(format!("2026-11-2{n}T00:00:00Z"));
                            v["share_json"] = json!(format!(
                                "{}\n",
                                json!({"protocol": "oo.context-pack.share/v1", "share_id": sid, "created_at": format!("t{n}"),
                                       "expires_at": format!("e{n}"), "pack_digest": "ab".repeat(32),
                                       "refs": {"endpoint": "http://127.0.0.1:1/v1/docs/refs", "credential": format!("oodr_secret{n}"), "expires_at": "x"},
                                       "role": "suggester", "scope": []})
                            ));
                        }
                        respond(&mut s, 201, "application/json", v.to_string().as_bytes());
                    }
                    ("GET", p)
                        if p.starts_with(&format!("/v1/docs/context-packs/{PACK}/files/")) =>
                    {
                        let rel = decode(
                            p.trim_start_matches(&format!("/v1/docs/context-packs/{PACK}/files/")),
                        );
                        match files.get(rel.as_str()) {
                            Some(b) => {
                                let mut b = b.clone();
                                if mode == Mode::Tamper && rel == "BRIEF.md" {
                                    b.push(b'!');
                                }
                                respond(&mut s, 200, "text/markdown", &b)
                            }
                            None => respond(
                                &mut s,
                                404,
                                "application/json",
                                br#"{"code":"DOCS_PACK_NOT_FOUND","message":"x"}"#,
                            ),
                        }
                    }
                    _ => respond(
                        &mut s,
                        404,
                        "application/json",
                        br#"{"code":"DOCS_ENGINE_NOT_FOUND","message":"x"}"#,
                    ),
                }
            });
        }
    });
    Fake {
        port,
        mode,
        requests,
        shares,
    }
}

struct Fx {
    dir: PathBuf,
    state: PathBuf,
}

fn fx(name: &str, port: Option<u16>) -> Fx {
    let dir = std::env::temp_dir().join(format!("oo-docs-ctx-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    if let Some(port) = port {
        let tf = state.join("serve.token");
        fs::write(&tf, TOKEN).unwrap();
        fs::set_permissions(&tf, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(
            state.join("serve.json"),
            json!({"protocol": "oo.docs.serve/v1", "pid": 1, "port": port, "api_version": "1.0",
                   "token_file": tf, "state_dir": state, "started_at": "2026-11-17T00:00:00Z", "engine": {}})
            .to_string(),
        )
        .unwrap();
    }
    Fx { dir, state }
}

impl Fx {
    fn target(&self, n: &str) -> PathBuf {
        let t = self.dir.join(n);
        fs::create_dir_all(&t).unwrap();
        t
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_oo"))
            .arg("context")
            .args(args)
            .arg("--state-dir")
            .arg(&self.state)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .output()
            .unwrap()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        // Mounts are 0555; open them up before removing.
        fn open(p: &Path) {
            let Ok(m) = fs::symlink_metadata(p) else {
                return;
            };
            if m.is_dir() {
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o700));
                if let Ok(rd) = fs::read_dir(p) {
                    for e in rd.flatten() {
                        open(&e.path());
                    }
                }
            }
        }
        open(&self.dir);
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, p: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(p).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn text(o: &std::process::Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn a_mount_is_atomic_read_only_and_complete() {
    let fake = start_fake();
    let f = fx("ok", Some(fake.port));
    let to = f.target("t1");
    let o = f.run(&[
        "mount",
        "--scope",
        "Projects/a",
        "--to",
        to.to_str().unwrap(),
        "--for",
        "写周报",
        "--json",
    ]);
    let (out, err) = text(&o);
    assert_eq!(o.status.code(), Some(0), "{out}\n{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["protocol"], "oo.context-mount/v1");
    assert_eq!(v["tokens"]["brief"], 40);
    assert!(v["share_id"].as_str().unwrap().starts_with("00000000-"));
    assert!(
        !out.contains("oodr_") && !out.contains(TOKEN),
        "no credential in output"
    );
    assert!(!err.contains(TOKEN));

    let ctx = to.join(".context");
    let got = tree(&ctx);
    let want: Vec<&str> = vec![
        "BRIEF.md",
        "SHARE.json",
        "manifest.json",
        "people/林川-1234abcd.md",
        "refs/0001.md",
    ];
    assert_eq!(got.keys().map(String::as_str).collect::<Vec<_>>(), want);
    assert_eq!(got["BRIEF.md"], pack_files()["BRIEF.md"]);
    // Read only all the way down.
    assert_eq!(fs::metadata(&ctx).unwrap().mode() & 0o777, 0o555);
    assert_eq!(
        fs::metadata(ctx.join("people")).unwrap().mode() & 0o777,
        0o555
    );
    assert_eq!(
        fs::metadata(ctx.join("BRIEF.md")).unwrap().mode() & 0o777,
        0o444
    );
    // No temp directory is left.
    let names: Vec<String> = fs::read_dir(&to)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [".context"]);
    // The task and scope reached the engine.
    let reqs = fake.requests.lock().unwrap().clone();
    let share = reqs.iter().find(|r| r.1 == "/v1/docs/shares").unwrap();
    let body: Value = serde_json::from_str(&share.2).unwrap();
    assert_eq!(body["compile"]["scope"], json!(["Projects/a"]));
    assert_eq!(body["compile"]["task"], "写周报");
    assert_eq!(body["compile"]["drive_id"], DRIVE);
    assert_eq!(
        body["mount"]["target_dir"],
        fs::canonicalize(&to).unwrap().to_str().unwrap()
    );
}

#[test]
fn two_mounts_are_byte_identical_except_the_share_fields() {
    let fake = start_fake();
    let f = fx("twice", Some(fake.port));
    let (a, b) = (f.target("t1"), f.target("t2"));
    for t in [&a, &b] {
        let o = f.run(&[
            "mount",
            "--scope",
            "Projects/a",
            "--to",
            t.to_str().unwrap(),
        ]);
        assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    }
    let (ta, tb) = (tree(&a.join(".context")), tree(&b.join(".context")));
    assert_eq!(ta.keys().collect::<Vec<_>>(), tb.keys().collect::<Vec<_>>());
    for (path, bytes) in &ta {
        if path == "SHARE.json" {
            continue;
        }
        assert_eq!(bytes, &tb[path], "{path} differs");
    }
    let strip = |b: &[u8]| {
        let mut v: Value = serde_json::from_slice(b).unwrap();
        for k in ["share_id", "created_at", "expires_at", "refs"] {
            v.as_object_mut().unwrap().remove(k);
        }
        v
    };
    assert_eq!(strip(&ta["SHARE.json"]), strip(&tb["SHARE.json"]));
    assert_ne!(
        ta["SHARE.json"], tb["SHARE.json"],
        "share id and times differ"
    );
}

#[test]
fn occupied_and_invalid_targets_exit_6_and_create_nothing() {
    let fake = start_fake();
    let f = fx("target", Some(fake.port));
    let to = f.target("t1");
    fs::create_dir_all(to.join(".context")).unwrap();
    fs::write(to.join(".context/mine.md"), "keep").unwrap();
    let o = f.run(&["mount", "--to", to.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(6), "{:?}", text(&o));
    assert_eq!(
        fs::read_to_string(to.join(".context/mine.md")).unwrap(),
        "keep"
    );
    for bad in ["relative/dir", "/nonexistent-oo-target", "/"] {
        assert_eq!(
            f.run(&["mount", "--to", bad]).status.code(),
            Some(6),
            "{bad}"
        );
    }
    assert_eq!(
        fake.shares.load(Ordering::SeqCst),
        0,
        "no share was created for a bad target"
    );
}

#[test]
fn engine_errors_map_to_exit_codes() {
    // 3: no serve.json, and a serve.json whose port is closed.
    let none = fx("down", None);
    assert_eq!(
        none.run(&["mount", "--to", none.target("t").to_str().unwrap()])
            .status
            .code(),
        Some(3)
    );
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let dead = fx("dead", Some(closed));
    assert_eq!(
        dead.run(&["mount", "--to", dead.target("t").to_str().unwrap()])
            .status
            .code(),
        Some(3)
    );

    let fake = start_fake();
    let f = fx("errs", Some(fake.port));
    let to = f.target("t1");
    *fake.mode.lock().unwrap() = Mode::ScopeInvalid;
    let o = f.run(&["mount", "--scope", "Nope", "--to", to.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(5), "{:?}", text(&o));
    assert!(!to.join(".context").exists());

    // Wrong token: the daemon says 401, which is not a success.
    fs::write(f.state.join("serve.token"), "oodt_wrong").unwrap();
    *fake.mode.lock().unwrap() = Mode::Normal;
    let o = f.run(&["mount", "--to", to.to_str().unwrap()]);
    assert_ne!(o.status.code(), Some(0));
    assert!(!text(&o).1.contains("oodt_wrong"));

    // A world-readable token file is refused.
    fs::set_permissions(
        f.state.join("serve.token"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(
        f.run(&["mount", "--to", to.to_str().unwrap()])
            .status
            .code(),
        Some(3)
    );
}

#[test]
fn a_file_that_does_not_match_its_manifest_leaves_no_mount() {
    let fake = start_fake();
    let f = fx("tamper", Some(fake.port));
    let to = f.target("t1");
    *fake.mode.lock().unwrap() = Mode::Tamper;
    let o = f.run(&["mount", "--to", to.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{:?}", text(&o));
    assert_eq!(
        fs::read_dir(&to).unwrap().count(),
        0,
        "neither .context nor a temp directory"
    );
}

#[test]
fn strict_budget_and_dry_run() {
    let fake = start_fake();
    let f = fx("strict", Some(fake.port));
    let to = f.target("t1");
    *fake.mode.lock().unwrap() = Mode::Trimmed;
    let o = f.run(&["mount", "--to", to.to_str().unwrap(), "--strict-budget"]);
    assert_eq!(o.status.code(), Some(4), "{:?}", text(&o));
    assert_eq!(
        fake.shares.load(Ordering::SeqCst),
        0,
        "a trimmed pack gets no share under --strict-budget"
    );
    assert!(!to.join(".context").exists());
    // Without strict mode the trim is reported, not fatal.
    let o = f.run(&["mount", "--to", to.to_str().unwrap(), "--json"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<Value>(&o.stdout).unwrap()["trimmed"]["decisions"]["removed"],
        1
    );

    let to2 = f.target("t2");
    let o = f.run(&["mount", "--to", to2.to_str().unwrap(), "--dry-run"]);
    let (out, _) = text(&o);
    assert_eq!(o.status.code(), Some(0));
    assert!(out.contains("[src:"), "{out}");
    assert!(!to2.join(".context").exists());
    assert_eq!(
        fake.shares.load(Ordering::SeqCst),
        1,
        "a dry run creates no share"
    );
}

#[test]
fn argument_errors_exit_2() {
    let f = fx("args", None);
    for bad in [
        &["mount"][..],
        &["mount", "--to", "/tmp", "--budget", "5"],
        &["status"],
        &["refresh"],
        &["revoke"],
        &["refresh", "x", "--group", "t"],
        &["check", "--since", "x"],
        &["list", "--status", "nope"],
        &["mount", "--to", "/tmp", "--group", "bad group"],
        &["mount", "--to", "/tmp", "--refresh-policy", "always"],
        &["bogus"],
        &[],
    ] {
        assert_eq!(f.run(bad).status.code(), Some(2), "{bad:?}");
    }
}

// ---- `oo context mount --remote` (10 §11.3, B-S12-02): the same mount from another OO serve's cloud entry.

fn token_file(f: &Fx, mode: u32) -> PathBuf {
    let p = f.dir.join("remote.token");
    fs::write(&p, TOKEN).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
    p
}

fn remote_run(f: &Fx, port: u16, token: &Path, extra: &[&str]) -> std::process::Output {
    let url = format!("http://127.0.0.1:{port}");
    let mut args = vec![
        "mount",
        "--remote",
        &url,
        "--remote-token-file",
        token.to_str().unwrap(),
        "--drive",
        DRIVE,
        "--agent",
        "agent-7",
    ];
    args.extend_from_slice(extra);
    f.run(&args)
}

#[test]
fn remote_mount_writes_the_same_bytes_as_a_local_mount_without_a_local_engine() {
    let local_fake = start_fake();
    let remote_fake = start_fake();
    let local = fx("rm-local", Some(local_fake.port));
    let remote = fx("rm-remote", None); // no serve.json: there is no local engine at all
    let (a, b) = (local.target("t"), remote.target("t"));
    let o = local.run(&[
        "mount",
        "--scope",
        "Projects/a",
        "--to",
        a.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    let tok = token_file(&remote, 0o600);
    let o = remote_run(
        &remote,
        remote_fake.port,
        &tok,
        &[
            "--scope",
            "Projects/a",
            "--to",
            b.to_str().unwrap(),
            "--pack-digest",
            &"ab".repeat(32),
            "--json",
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["pack_digest"], "ab".repeat(32).as_str());
    let (ta, tb) = (tree(&a.join(".context")), tree(&b.join(".context")));
    assert_eq!(ta.keys().collect::<Vec<_>>(), tb.keys().collect::<Vec<_>>());
    for (path, bytes) in &ta {
        if path != "SHARE.json" {
            assert_eq!(
                bytes, &tb[path],
                "{path} differs between the local and the remote mount"
            );
        }
    }
    // The remote token is in no output and no mounted file.
    let (so, se) = text(&o);
    assert!(!so.contains(TOKEN) && !se.contains(TOKEN));
    assert!(
        tb.values()
            .all(|b| !String::from_utf8_lossy(b).contains(TOKEN))
    );
    // A remote share is not the local daemon's: it keeps no record of it.
    assert!(!remote.state.join("mounts.json").exists());
    // The calls went to the remote, in the order of a mount, and never listed drives.
    let calls: Vec<String> = remote_fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|r| format!("{} {}", r.0, r.1.split('?').next().unwrap()))
        .collect();
    assert!(
        calls.iter().any(|c| c == "POST /v1/docs/shares"),
        "{calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c == "GET /v1/docs/drives"),
        "the cloud entry does not list drives: {calls:?}"
    );
}

#[test]
fn remote_mount_needs_a_private_token_file_a_drive_id_and_the_right_digest() {
    let fake = start_fake();
    let f = fx("rm-refusals", None);
    let t = f.target("t");
    let to = ["--to", t.to_str().unwrap()];
    // A token file others can read is refused (exit 2) and nothing is created.
    let o = remote_run(&f, fake.port, &token_file(&f, 0o644), &to);
    assert_eq!(o.status.code(), Some(2), "{:?}", text(&o));
    assert!(
        fake.requests.lock().unwrap().is_empty(),
        "no request left the machine"
    );
    // No token at all.
    let o = f.run(&[
        "mount",
        "--remote",
        &format!("http://127.0.0.1:{}", fake.port),
        "--drive",
        DRIVE,
        to[0],
        to[1],
    ]);
    assert_eq!(o.status.code(), Some(2), "{:?}", text(&o));
    // A drive name is not enough: the cloud entry does not resolve names.
    let tok = token_file(&f, 0o600);
    let o = f.run(&[
        "mount",
        "--remote",
        &format!("http://127.0.0.1:{}", fake.port),
        "--remote-token-file",
        tok.to_str().unwrap(),
        "--drive",
        "drv",
        to[0],
        to[1],
    ]);
    assert_eq!(o.status.code(), Some(2), "{:?}", text(&o));
    // Not an http URL.
    let o = f.run(&["mount", "--remote", "ftp://x", to[0], to[1]]);
    assert_eq!(o.status.code(), Some(2));
    // A pack with another digest than the one asked for mounts nothing.
    let o = remote_run(
        &f,
        fake.port,
        &tok,
        &[
            "--to",
            t.to_str().unwrap(),
            "--pack-digest",
            &"cd".repeat(32),
        ],
    );
    assert_eq!(o.status.code(), Some(1), "{:?}", text(&o));
    assert!(
        text(&o).1.contains("DOCS_PACK_DIGEST_MISMATCH")
            || text(&o).0.contains("DOCS_PACK_DIGEST_MISMATCH"),
        "{:?}",
        text(&o)
    );
    assert!(!t.join(".context").exists(), "nothing was mounted");
}
