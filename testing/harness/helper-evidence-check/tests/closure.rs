//! Positive closure of the five records, plus finite negatives on copies under /tmp.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const AS_OF: &str = "2026-10-02T12:12:14Z";
const RECORDS: [&str; 5] = [
    "bitflags-2.13.2.json",
    "cfg-if-1.0.5.json",
    "cfg_aliases-0.2.2.json",
    "libc-0.2.189.json",
    "nix-0.31.3.json",
];
const LICENSE_DIRS: [&str; 5] = [
    "bitflags-2.13.2",
    "cfg-if-1.0.5",
    "cfg_aliases-0.2.2",
    "libc-0.2.189",
    "nix-0.31.3",
];

struct Output {
    code: i32,
    out: String,
    err: String,
}

struct Scratch {
    dir: PathBuf,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.file_name().and_then(|name| name.to_str()) == Some("deps") {
        path.pop();
    }
    path.join("helper-evidence-check")
}

fn rel(path: &Path) -> PathBuf {
    repo().join(path)
}

fn scratch(label: &str) -> Scratch {
    let dir = PathBuf::from(format!(
        "/tmp/oo-a105-helper-evidence-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    Scratch { dir }
}

fn copy_file(src: &Path, dst: &Path) {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).expect("parent");
    }
    let meta = fs::symlink_metadata(src).expect("source metadata");
    assert!(meta.file_type().is_file(), "refusing to copy a non-file");
    fs::copy(src, dst).expect("copy");
}

fn copy_records(dir: &Path) -> PathBuf {
    let dest = dir.join("records");
    fs::create_dir_all(&dest).expect("records");
    for name in RECORDS {
        copy_file(&rel(Path::new("testing/harness/containment-supervisor/provenance/audit-records")).join(name), &dest.join(name));
    }
    dest
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("dir");
    for entry in fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("entry");
        let meta = fs::symlink_metadata(entry.path()).expect("meta");
        let to = dst.join(entry.file_name());
        if meta.file_type().is_symlink() {
            panic!("fixture source contains a symlink");
        } else if meta.is_dir() {
            copy_tree(&entry.path(), &to);
        } else if meta.is_file() {
            fs::copy(entry.path(), &to).expect("copy file");
        }
    }
}

fn replace_one(path: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(path).expect("read");
    let count = text.matches(from).count();
    assert_eq!(count, 1, "expected one match in {}", path.display());
    fs::write(path, text.replacen(from, to, 1)).expect("write");
}

struct Opts {
    records: Option<PathBuf>,
    licenses: Option<PathBuf>,
    repo_root: Option<PathBuf>,
    helper_lock: Option<PathBuf>,
    audit_json: Option<PathBuf>,
    deny_config: Option<PathBuf>,
}

fn stage_license_repo(dir: &Path) -> (PathBuf, PathBuf) {
    let root = dir.join("repo");
    let licenses = root.join("testing/harness/containment-supervisor/provenance/licenses");
    for name in LICENSE_DIRS {
        let src = rel(Path::new("testing/harness/containment-supervisor/provenance/licenses")).join(name);
        copy_tree(&src, &licenses.join(name));
    }
    (root, licenses)
}

fn invoke(opts: Opts) -> Output {
    let root = repo();
    let evidence_root = opts.repo_root.unwrap_or_else(|| root.clone());
    let records = opts.records.unwrap_or_else(|| {
        root.join("testing/harness/containment-supervisor/provenance/audit-records")
    });
    let licenses = opts
        .licenses
        .unwrap_or_else(|| root.join("testing/harness/containment-supervisor/provenance/licenses"));
    let helper_lock = opts
        .helper_lock
        .unwrap_or_else(|| root.join("testing/harness/containment-supervisor/Cargo.lock"));
    let audit_json = opts.audit_json.unwrap_or_else(|| {
        root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out")
    });
    let deny_config = opts.deny_config.unwrap_or_else(|| root.join("ci/deny.toml"));
    let raw = root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw");
    let mut command = Command::new(bin());
    command
        .arg("--inventory")
        .arg(root.join("testing/harness/helper-evidence-check/fixtures/inventory.json"))
        .arg("--records")
        .arg(records)
        .arg("--licenses")
        .arg(licenses)
        .arg("--retained-licenses")
        .arg(raw.join("upstream-licenses"))
        .arg("--helper-lock")
        .arg(helper_lock)
        .arg("--helper-manifest")
        .arg(root.join("testing/harness/containment-supervisor/Cargo.toml"))
        .arg("--deny-config")
        .arg(deny_config)
        .arg("--metadata")
        .arg(raw.join("helper-metadata.out"))
        .arg("--age")
        .arg(raw.join("age.txt"))
        .arg("--audit-json")
        .arg(audit_json)
        .arg("--deny-advisories")
        .arg(raw.join("scan-deny-advisories.err"))
        .arg("--advisory-rev")
        .arg(raw.join("advisory-rev.txt"))
        .arg("--version-meta")
        .arg(raw.join("meta"))
        .arg("--repo-root")
        .arg(&evidence_root)
        .arg("--as-of")
        .arg(AS_OF);
    let output = command.output().expect("spawn helper-evidence-check");
    Output {
        code: output.status.code().unwrap_or(127),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn assert_fail(output: &Output, code_name: &str, exit_code: i32) {
    assert_eq!(output.code, exit_code, "stdout={} stderr={}", output.out, output.err);
    assert!(output.err.contains(&format!("code={code_name}")), "{}", output.err);
    assert!(!output.out.contains("OK records=5"), "{}", output.out);
    assert!(!output.err.contains("Permission is hereby granted"), "{}", output.err);
    assert!(!output.out.contains("Permission is hereby granted"), "{}", output.out);
    assert!(output.err.len() < 500, "stderr too long: {}", output.err);
}

#[test]
fn positive_five_records() {
    let output = invoke(Opts {
        records: None,
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_eq!(output.code, 0, "stdout={} stderr={}", output.out, output.err);
    assert!(output.out.contains("OK records=5"), "{}", output.out);
    assert!(output.out.contains("audited=false"), "{}", output.out);
    assert!(output.out.contains("release_signoff=false"), "{}", output.out);
    assert!(!output.out.contains("audited=true"), "{}", output.out);
    assert!(output.err.is_empty(), "{}", output.err);
}

#[test]
fn usage_requires_flags() {
    let output = Command::new(bin()).output().expect("spawn");
    let code = output.status.code().unwrap_or(127);
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(code, 1);
    assert!(err.contains("code=usage"), "{err}");
}

#[test]
fn missing_license_body() {
    let scratch = scratch("missing-license");
    let (root, licenses) = stage_license_repo(&scratch.dir);
    fs::remove_file(licenses.join("nix-0.31.3/LICENSE")).expect("remove");
    let output = invoke(Opts {
        records: None,
        licenses: Some(licenses),
        repo_root: Some(root),
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "missing-license-body", 2);
}

#[test]
fn hash_mutation_of_license_bytes() {
    let scratch = scratch("hash-mutation");
    let (root, licenses) = stage_license_repo(&scratch.dir);
    let path = licenses.join("nix-0.31.3/LICENSE");
    let mut bytes = fs::read(&path).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&path, bytes).expect("write");
    let output = invoke(Opts {
        records: None,
        licenses: Some(licenses),
        repo_root: Some(root),
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "hash-mismatch", 3);
}

#[test]
fn wrong_dual_choice() {
    let scratch = scratch("wrong-choice");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("bitflags-2.13.2.json"),
        "\"dual_license_choice\": \"MIT\"",
        "\"dual_license_choice\": \"Apache-2.0\"",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "wrong-dual-choice", 4);
}

#[test]
fn undeclared_choice() {
    let scratch = scratch("undeclared-choice");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("libc-0.2.189.json"),
        "\"dual_license_choice\": \"MIT\"",
        "\"dual_license_choice\": null",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "undeclared-choice", 5);
}

#[test]
fn omitted_apache_body() {
    let scratch = scratch("omitted-apache");
    let (root, licenses) = stage_license_repo(&scratch.dir);
    fs::remove_file(licenses.join("bitflags-2.13.2/LICENSE-APACHE")).expect("remove");
    let output = invoke(Opts {
        records: None,
        licenses: Some(licenses),
        repo_root: Some(root),
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "omitted-apache-body", 6);
}

#[test]
fn missing_cfg_aliases_third_party_notice() {
    let scratch = scratch("missing-notice");
    let (root, licenses) = stage_license_repo(&scratch.dir);
    fs::remove_file(licenses.join("cfg_aliases-0.2.2/NOTICE")).expect("remove");
    let output = invoke(Opts {
        records: None,
        licenses: Some(licenses),
        repo_root: Some(root),
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "missing-third-party-notice", 7);
}

#[test]
fn invalid_publisher_time_is_rejected() {
    let scratch = scratch("invalid-time");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("nix-0.31.3.json"),
        "\"publication_date\": \"2026-05-11T07:01:18.650221Z\"",
        "\"publication_date\": \"2021-12-30T16:00:11Z\"",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "invalid-time", 8);
}

#[test]
fn wrong_age_floor() {
    let scratch = scratch("age-floor");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("cfg-if-1.0.5.json"),
        "\"age_days_at_adoption\": 15",
        "\"age_days_at_adoption\": 14",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "age-floor", 9);
}

#[test]
fn changed_helper_lock() {
    let scratch = scratch("helper-lock");
    let lock = scratch.dir.join("Cargo.lock");
    copy_file(
        &rel(Path::new("testing/harness/containment-supervisor/Cargo.lock")),
        &lock,
    );
    let mut bytes = fs::read(&lock).expect("read");
    bytes.extend_from_slice(b"\n# a105 mutated lock\n");
    fs::write(&lock, bytes).expect("write");
    let output = invoke(Opts {
        records: None,
        licenses: None,
        repo_root: None,
        helper_lock: Some(lock),
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "helper-lock-changed", 10);
}

#[test]
fn scanner_output_missing() {
    let scratch = scratch("scanner-missing");
    let missing = scratch.dir.join("scan-audit-missing.out");
    let output = invoke(Opts {
        records: None,
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: Some(missing),
        deny_config: None,
    });
    assert_fail(&output, "scanner-output-missing", 11);
}

#[test]
fn record_digest_hash_wrong() {
    let scratch = scratch("hash-wrong");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("nix-0.31.3.json"),
        "\"digest\": \"sha256:cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3d\"",
        "\"digest\": \"sha256:cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3e\"",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "checksum-mismatch", 18);
}

#[test]
fn path_traversal_in_license_source() {
    let scratch = scratch("traversal");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("nix-0.31.3.json"),
        "\"source\": \"testing/harness/containment-supervisor/provenance/licenses/nix-0.31.3/LICENSE\"",
        "\"source\": \"testing/harness/containment-supervisor/provenance/licenses/nix-0.31.3/../../nix-0.31.3/LICENSE\"",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "path-traversal", 12);
}

#[test]
fn cli_path_traversal() {
    let root = repo();
    let output = Command::new(bin())
        .arg("--inventory")
        .arg(root.join("testing/harness/helper-evidence-check/fixtures/inventory.json"))
        .arg("--records")
        .arg("/tmp/oo-a105-not-created/../records")
        .arg("--licenses")
        .arg(root.join("testing/harness/containment-supervisor/provenance/licenses"))
        .arg("--retained-licenses")
        .arg(root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/upstream-licenses"))
        .arg("--helper-lock")
        .arg(root.join("testing/harness/containment-supervisor/Cargo.lock"))
        .arg("--helper-manifest")
        .arg(root.join("testing/harness/containment-supervisor/Cargo.toml"))
        .arg("--deny-config")
        .arg(root.join("ci/deny.toml"))
        .arg("--metadata")
        .arg(root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/helper-metadata.out"))
        .arg("--age")
        .arg(root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/age.txt"))
        .arg("--audit-json")
        .arg(root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out"))
        .arg("--deny-advisories")
        .arg(root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-deny-advisories.err"))
        .arg("--advisory-rev")
        .arg(root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/advisory-rev.txt"))
        .arg("--version-meta")
        .arg(root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/meta"))
        .arg("--repo-root")
        .arg(&root)
        .arg("--as-of")
        .arg(AS_OF)
        .output()
        .expect("spawn");
    let got = Output {
        code: output.status.code().unwrap_or(127),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    assert_fail(&got, "path-traversal", 12);
}

#[test]
fn symlink_license_is_not_followed() {
    let scratch = scratch("symlink");
    let (root, licenses) = stage_license_repo(&scratch.dir);
    let link = licenses.join("nix-0.31.3/LICENSE");
    fs::remove_file(&link).expect("remove");
    std::os::unix::fs::symlink(
        rel(Path::new("testing/harness/containment-supervisor/provenance/licenses/nix-0.31.3/LICENSE")),
        &link,
    )
    .expect("symlink");
    let output = invoke(Opts {
        records: None,
        licenses: Some(licenses),
        repo_root: Some(root),
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "path-traversal", 12);
    assert!(output.err.contains("refusing symlink"), "{}", output.err);
}

#[test]
fn fake_claim_audited_true() {
    let scratch = scratch("fake-claim");
    let records = copy_records(&scratch.dir);
    replace_one(&records.join("nix-0.31.3.json"), "\"audited\": false", "\"audited\": true");
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "fake-claim", 19);
}

#[test]
fn fresh_scan_claim_is_fake() {
    let scratch = scratch("fresh-scan");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("libc-0.2.189.json"),
        "\"fresh_current_scan\": false",
        "\"fresh_current_scan\": true",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "fake-claim", 19);
}

#[test]
fn unknown_package() {
    let scratch = scratch("unknown");
    let records = copy_records(&scratch.dir);
    fs::write(
        records.join("scan-unknown.json"),
        "{\"package\":{\"name\":\"scan-unknown\"}}\n",
    )
    .expect("write");
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "unknown-package", 13);
}

#[test]
fn duplicate_record() {
    let scratch = scratch("duplicate");
    let records = copy_records(&scratch.dir);
    copy_file(&records.join("nix-0.31.3.json"), &records.join("nix-copy.json"));
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "duplicate", 14);
}

#[test]
fn wrong_source() {
    let scratch = scratch("wrong-source");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("bitflags-2.13.2.json"),
        "\"repository\": \"https://github.com/bitflags/bitflags\"",
        "\"repository\": \"https://example.invalid/bitflags\"",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "wrong-source", 15);
}

#[test]
fn wrong_expression() {
    let scratch = scratch("wrong-expression");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("cfg-if-1.0.5.json"),
        "\"license_spdx\": \"MIT OR Apache-2.0\"",
        "\"license_spdx\": \"ISC\"",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "wrong-expression", 16);
}

#[test]
fn placeholder_obligation() {
    let scratch = scratch("placeholder");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("bitflags-2.13.2.json"),
        "Include the copyright notice and this permission notice in all copies or substantial portions of the Software.",
        "n/a",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "placeholder", 17);
}

#[test]
fn graph_mismatch() {
    let scratch = scratch("graph");
    let records = copy_records(&scratch.dir);
    replace_one(
        &records.join("bitflags-2.13.2.json"),
        "\"graph_role\": \"transitive\"",
        "\"graph_role\": \"direct\"",
    );
    let output = invoke(Opts {
        records: Some(records),
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: None,
    });
    assert_fail(&output, "graph-mismatch", 20);
}

#[test]
fn deny_config_hash_wrong() {
    let scratch = scratch("deny-hash");
    let deny = scratch.dir.join("deny.toml");
    copy_file(&rel(Path::new("ci/deny.toml")), &deny);
    let mut bytes = fs::read(&deny).expect("read");
    bytes.push(b'\n');
    fs::write(&deny, bytes).expect("write");
    let output = invoke(Opts {
        records: None,
        licenses: None,
        repo_root: None,
        helper_lock: None,
        audit_json: None,
        deny_config: Some(deny),
    });
    assert_fail(&output, "hash-mismatch", 3);
}
