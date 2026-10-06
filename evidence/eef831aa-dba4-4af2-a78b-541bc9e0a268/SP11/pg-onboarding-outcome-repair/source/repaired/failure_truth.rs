#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pg_onboarding::{apply_reviewed, inspect};

fn scratch(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("failure-truth").join(name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn onboarding_bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_pg-onboarding")
        .or(option_env!("CARGO_BIN_EXE_pg_onboarding"))
        .expect("missing pg-onboarding executable path")
}

fn write_cfg(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn duplicate_root_key_refuses() {
    let root = scratch("dup-root");
    let run = root.join("run");
    let cfg = write_cfg(&root, "root.json", br#"{"caller":"trusted_local_admin","caller":"other"}"#);
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "shape");
    assert!(!run.exists());
}

#[test]
fn duplicate_nested_key_refuses() {
    let root = scratch("dup-nested");
    let run = root.join("run");
    let cfg = write_cfg(&root, "nested.json", br#"{"source":{"host":"one","host":"two"}}"#);
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "shape");
    assert!(!run.exists());
}

#[test]
fn duplicate_generation_key_refuses() {
    let root = scratch("dup-gen");
    let run = root.join("run");
    let cfg = write_cfg(
        &root,
        "gen.json",
        br#"{"source_generation":48,"source_generation":1}"#,
    );
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "shape");
    assert!(!run.exists());
}

#[test]
fn duplicate_nested_source_cut_refuses() {
    let root = scratch("dup-cut");
    let run = root.join("run");
    let cfg = write_cfg(
        &root,
        "nested-gen.json",
        br#"{"manifest":{"source_cut":48,"source_cut":1}}"#,
    );
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "shape");
    assert!(!run.exists());
}

#[test]
fn duplicate_role_slug_refuses() {
    let root = scratch("dup-role");
    let run = root.join("run");
    let cfg = write_cfg(&root, "role.json", br#"{"roles":[{"slug":"clerk","slug":"peek"}]}"#);
    let err = apply_reviewed(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "shape");
    assert!(!run.join("steps").exists());
}

#[test]
fn cli_duplicate_config_stderr_is_static() {
    let root = scratch("cli-dup");
    let cfg = write_cfg(
        &root,
        "cfg.json",
        b"{\"password\":\"s3cret-value-aaa\",\"password\":\"s3cret-value-aaa\"}\n",
    );
    let run = root.join("run");
    let output = Command::new(onboarding_bin())
        .arg("inspect")
        .arg("--config")
        .arg(&cfg)
        .arg("--run")
        .arg(&run)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"pg-onboarding: config shape\n");
    assert!(!output.stderr.windows(b"s3cret-value-aaa".len()).any(|window| window == b"s3cret-value-aaa"));
    assert!(!run.exists());
}

#[test]
fn byte_cap_exact_is_shape() {
    let root = scratch("cap");
    let run = root.join("run");
    let cfg = root.join("doc.json");
    fs::write(&cfg, vec![b' '; 1_048_576]).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "shape");
    assert!(!run.exists());
}

#[test]
fn byte_cap_plus_one_is_budget() {
    let root = scratch("cap-plus");
    let run = root.join("run");
    let cfg = root.join("doc.json");
    fs::write(&cfg, vec![b' '; 1_048_577]).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "budget");
    assert!(!run.exists());
}

#[test]
fn depth_31_is_schema_and_32_is_budget() {
    let root = scratch("depth");
    let run = root.join("run");
    let cfg = root.join("doc.json");
    fs::write(&cfg, wrapped(31)).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "config");
    fs::write(&cfg, wrapped(32)).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "budget");
    assert!(!run.exists());
}

#[test]
fn nodes_under_cap_are_schema_and_at_cap_are_budget() {
    let root = scratch("nodes");
    let run = root.join("run");
    let cfg = root.join("doc.json");
    fs::write(&cfg, nulls(49_999)).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "config");
    fs::write(&cfg, nulls(50_000)).unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "budget");
    assert!(!run.exists());
}

#[test]
fn trailing_whitespace_is_schema_and_trailing_bytes_are_shape() {
    let root = scratch("trail");
    let run = root.join("run");
    let cfg = root.join("doc.json");
    fs::write(&cfg, b"{}\n").unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "config");
    fs::write(&cfg, b"{}{}\n").unwrap();
    let err = inspect(&cfg, &run).unwrap_err();
    assert_eq!(err.step, "config");
    assert_eq!(err.detail, "shape");
    assert!(!run.exists());
}

fn wrapped(objects: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..objects {
        out.extend_from_slice(br#"{"a":"#);
    }
    out.push(b'0');
    out.resize(out.len() + objects, b'}');
    out
}

fn nulls(count: usize) -> Vec<u8> {
    let mut out = Vec::from(&b"["[..]);
    for index in 0..count {
        if index > 0 {
            out.push(b',');
        }
        out.extend_from_slice(b"null");
    }
    out.push(b']');
    out
}
