//! Checker-spawn regressions for the reviewed helper-lock anchors and JSON depth.
//!
//! The adoption instant, MIT choice, and retained audit identity are fixed in
//! the checker. These cases forge claim files that agree with each other and
//! still must exit nonzero. Depth 64 is the deepest container nesting the
//! parser accepts. Depth 65 and 20000 must return a bound error in the child
//! process, with no signal.

#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const AS_OF: &str = "2026-10-02T12:12:14Z";
const REAL_ADOPT: &str = "2026-10-02T11:10:37Z";
const FORGED_ADOPT: &str = "2026-09-24T17:36:11.278420Z";
const AUDIT_SHA: &str = "f64e43716f72c173879cb30130f73df0f1c802ac32960eb6c3955c15c2209f81";
const ADVISORY_REV_SHA: &str = "ee376fcdd344d86d428ff4f80997e660fe2b2217fdf77139ddfcb5b6e3055cbd";
const RUSTSEC: &str = "117edb3bed98e9be112f277b7615eea3252e7c43";
const FORGED_RUSTSEC: &str = "0000000000000000000000000000000000000000";
const RECORDS: [&str; 5] = [
    "bitflags-2.13.2.json",
    "cfg-if-1.0.5.json",
    "cfg_aliases-0.2.2.json",
    "libc-0.2.189.json",
    "nix-0.31.3.json",
];
const AGES: [(&str, &str); 5] = [
    ("\"age_days_at_adoption\": 22", "\"age_days_at_adoption\": 14"),
    ("\"age_days_at_adoption\": 15", "\"age_days_at_adoption\": 8"),
    ("\"age_days_at_adoption\": 77", "\"age_days_at_adoption\": 70"),
    ("\"age_days_at_adoption\": 72", "\"age_days_at_adoption\": 64"),
    ("\"age_days_at_adoption\": 144", "\"age_days_at_adoption\": 136"),
];

struct Scratch {
    dir: PathBuf,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

struct Ran {
    output: Output,
    millis: u128,
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.file_name().and_then(|name| name.to_str()) == Some("deps") {
        path.pop();
    }
    path.join("helper-evidence-check")
}

fn scratch(label: &str) -> Scratch {
    let dir = PathBuf::from(format!("/tmp/oo-a113-helper-trust-{label}-{}", std::process::id()));
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

fn records_src() -> PathBuf {
    repo().join("testing/harness/containment-supervisor/provenance/audit-records")
}

fn inventory_src() -> PathBuf {
    repo().join("testing/harness/helper-evidence-check/fixtures/inventory.json")
}

fn audit_src() -> PathBuf {
    repo().join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out")
}

fn copy_records(dir: &Path) -> PathBuf {
    let dest = dir.join("records");
    fs::create_dir_all(&dest).expect("records");
    for name in RECORDS {
        copy_file(&records_src().join(name), &dest.join(name));
    }
    dest
}

fn copy_inventory(dir: &Path) -> PathBuf {
    let dest = dir.join("inventory.json");
    copy_file(&inventory_src(), &dest);
    dest
}

fn replace_one(path: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(path).expect("read");
    let count = text.matches(from).count();
    assert_eq!(count, 1, "expected one match in {}", path.display());
    fs::write(path, text.replacen(from, to, 1)).expect("write");
}

fn replace_present(path: &Path, from: &str, to: &str) -> usize {
    let text = fs::read_to_string(path).expect("read");
    let count = text.matches(from).count();
    assert!(count <= 1, "{} has {count} matches", path.display());
    if count == 1 {
        fs::write(path, text.replacen(from, to, 1)).expect("write");
    }
    count
}

fn file_sha256(path: &Path) -> String {
    let output = if Path::new("/usr/bin/sha256sum").is_file() {
        Command::new("/usr/bin/sha256sum").arg(path).output().expect("sha256sum")
    } else {
        Command::new("/usr/bin/shasum").arg("-a").arg("256").arg(path).output().expect("shasum")
    };
    assert!(output.status.success(), "sha failed {}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).expect("sha utf-8");
    text.split_whitespace().next().expect("sha token").to_string()
}

fn nested(depth: usize, kind: &str) -> String {
    let mut text = String::new();
    for index in 0..depth {
        match kind {
            "object" => text.push_str("{\"a\":"),
            "array" => text.push('['),
            "mixed" => {
                if index % 2 == 0 {
                    text.push_str("{\"a\":");
                } else {
                    text.push('[');
                }
            }
            _ => panic!("kind"),
        }
    }
    text.push('0');
    for index in (0..depth).rev() {
        let close_object = match kind {
            "object" => true,
            "array" => false,
            "mixed" => index % 2 == 0,
            _ => panic!("kind"),
        };
        text.push(if close_object { '}' } else { ']' });
    }
    text
}

fn insert_after_brace(path: &Path, insertion: &str) {
    let text = fs::read_to_string(path).expect("read record");
    let rest = text.strip_prefix('{').expect("record object");
    let mut out = String::from("{");
    out.push_str(insertion);
    out.push_str(rest);
    fs::write(path, out).expect("write record");
}

struct Opts {
    inventory: Option<PathBuf>,
    records: Option<PathBuf>,
    audit_json: Option<PathBuf>,
    advisory_rev: Option<PathBuf>,
}

fn invoke(opts: Opts) -> Ran {
    let root = repo();
    let raw = root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw");
    let inventory = opts.inventory.unwrap_or_else(inventory_src);
    let records = opts.records.unwrap_or_else(records_src);
    let audit_json = opts.audit_json.unwrap_or_else(audit_src);
    let advisory_rev = opts.advisory_rev.unwrap_or_else(|| raw.join("advisory-rev.txt"));
    let mut command = Command::new(bin());
    command
        .stdin(std::process::Stdio::null())
        .arg("--inventory")
        .arg(inventory)
        .arg("--records")
        .arg(records)
        .arg("--licenses")
        .arg(root.join("testing/harness/containment-supervisor/provenance/licenses"))
        .arg("--retained-licenses")
        .arg(raw.join("upstream-licenses"))
        .arg("--helper-lock")
        .arg(root.join("testing/harness/containment-supervisor/Cargo.lock"))
        .arg("--helper-manifest")
        .arg(root.join("testing/harness/containment-supervisor/Cargo.toml"))
        .arg("--deny-config")
        .arg(root.join("ci/deny.toml"))
        .arg("--metadata")
        .arg(raw.join("helper-metadata.out"))
        .arg("--age")
        .arg(raw.join("age.txt"))
        .arg("--audit-json")
        .arg(audit_json)
        .arg("--deny-advisories")
        .arg(raw.join("scan-deny-advisories.err"))
        .arg("--advisory-rev")
        .arg(advisory_rev)
        .arg("--version-meta")
        .arg(raw.join("meta"))
        .arg("--repo-root")
        .arg(&root)
        .arg("--as-of")
        .arg(AS_OF);
    let started = Instant::now();
    let output = command.output().expect("spawn helper-evidence-check");
    Ran {
        millis: started.elapsed().as_millis(),
        output,
    }
}

fn signal_of(output: &Output) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        return output.status.signal();
    }
    #[allow(unreachable_code)]
    None
}

fn one_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).replace(['\n', '\r', '\t'], " ");
    if text.len() > 240 {
        format!("{}…", &text[..240])
    } else {
        text
    }
}

fn record_case(name: &str, ran: &Ran) {
    let code = ran.output.status.code();
    let line = format!(
        "{name}\texit={}\tsignal={}\tms={}\tstdout={}\tstderr={}",
        code.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
        signal_of(&ran.output).map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
        ran.millis,
        one_line(&ran.output.stdout),
        one_line(&ran.output.stderr)
    );
    println!("{line}");
    if let Ok(path) = std::env::var("OO_A113_CASE_LOG") {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK.get_or_init(|| Mutex::new(())).lock().expect("case log lock");
        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent).expect("case log dir");
        }
        let mut file = fs::OpenOptions::new().create(true).append(true).open(path).expect("case log");
        writeln!(file, "{line}").expect("case log write");
        drop(guard);
    }
}

fn assert_clean_fail(name: &str, ran: &Ran, exit_code: i32, code_name: &str) {
    record_case(name, ran);
    assert_eq!(signal_of(&ran.output), None, "{name} raised a signal");
    let code = ran.output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&ran.output.stdout);
    let stderr = String::from_utf8_lossy(&ran.output.stderr);
    assert_eq!(code, exit_code, "{name} stdout={stdout} stderr={stderr}");
    assert!(stderr.contains(&format!("code={code_name}")), "{name} stderr={stderr}");
    assert!(!stdout.contains("OK records=5"), "{name} stdout={stdout}");
    assert!(ran.millis < 5_000, "{name} took {} ms", ran.millis);
}

fn assert_clean_ok(name: &str, ran: &Ran) {
    record_case(name, ran);
    assert_eq!(signal_of(&ran.output), None, "{name} raised a signal");
    let code = ran.output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&ran.output.stdout);
    let stderr = String::from_utf8_lossy(&ran.output.stderr);
    assert_eq!(code, 0, "{name} stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("helper-evidence-check: OK records=5 audited=false license_record_status=complete-for-review release_signoff=false"),
        "{name} stdout={stdout}"
    );
    assert!(!stdout.contains("audited=true"), "{name}");
    assert!(ran.millis < 5_000, "{name} took {} ms", ran.millis);
}

fn rewrite_adoption_and_ages(dir: &Path) -> (PathBuf, PathBuf) {
    let inventory = copy_inventory(dir);
    let records = copy_records(dir);
    replace_one(&inventory, REAL_ADOPT, FORGED_ADOPT);
    let mut adoption_records = 0usize;
    for name in RECORDS {
        adoption_records += replace_present(&records.join(name), REAL_ADOPT, FORGED_ADOPT);
    }
    assert_eq!(adoption_records, 5);
    for (from, to) in AGES {
        let mut hits = replace_present(&inventory, from, to);
        for name in RECORDS {
            hits += replace_present(&records.join(name), from, to);
        }
        assert_eq!(hits, 2, "{from}");
    }
    (inventory, records)
}

fn replace_n(path: &Path, from: &str, to: &str, expect: usize) {
    let text = fs::read_to_string(path).expect("read");
    let count = text.matches(from).count();
    assert_eq!(count, expect, "matches in {}", path.display());
    fs::write(path, text.replace(from, to)).expect("write");
}

fn aligned_apache(dir: &Path) -> (PathBuf, PathBuf) {
    let inventory = copy_inventory(dir);
    let records = copy_records(dir);
    replace_n(&inventory, "\"choice\": \"MIT\"", "\"choice\": \"Apache-2.0\"", 3);
    for name in ["bitflags-2.13.2.json", "cfg-if-1.0.5.json", "libc-0.2.189.json"] {
        replace_n(
            &records.join(name),
            "\"dual_license_choice\": \"MIT\"",
            "\"dual_license_choice\": \"Apache-2.0\"",
            1,
        );
    }
    replace_n(
        &records.join("nix-0.31.3.json"),
        "\"dual_license_choice\": \"MIT\"",
        "\"dual_license_choice\": \"Apache-2.0\"",
        3,
    );
    (inventory, records)
}

fn forge_audit(dir: &Path, body: &str) -> (PathBuf, PathBuf, String) {
    let inventory = copy_inventory(dir);
    let path = dir.join("scan-audit.out");
    fs::write(&path, body).expect("audit");
    let digest = file_sha256(&path);
    replace_one(&inventory, AUDIT_SHA, &digest);
    (inventory, path, digest)
}

fn audit_with_status(status_json: &str) -> String {
    let text = fs::read_to_string(audit_src()).expect("audit");
    let trimmed = text.trim_end();
    let body = trimmed.strip_suffix('}').expect("audit object");
    format!("{body},\"status\":{status_json}}}\n")
}

#[test]
fn anchors_match_the_reviewed_baseline() {
    assert_eq!(helper_evidence_check::HELPER_LOCK_COMMIT, "f1a6005971e3dc3075094c87ed670fc9d25e9ae0");
    assert_eq!(helper_evidence_check::ADOPTION_INSTANT, REAL_ADOPT);
    assert_eq!(
        helper_evidence_check::HELPER_LOCK_SHA256,
        "14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1"
    );
    assert_eq!(helper_evidence_check::MAX_DEPTH, 64);
}

#[test]
fn canonical_bundle_stays_unaudited() {
    let ran = invoke(Opts {
        inventory: None,
        records: None,
        audit_json: None,
        advisory_rev: None,
    });
    assert_clean_ok("canonical-bundle", &ran);
}

#[test]
fn aligned_adoption_and_age_forge_is_rejected() {
    let scratch = scratch("adopt");
    let (inventory, records) = rewrite_adoption_and_ages(&scratch.dir);
    let ran = invoke(Opts {
        inventory: Some(inventory),
        records: Some(records),
        audit_json: None,
        advisory_rev: None,
    });
    assert_clean_fail("aligned-adoption-age", &ran, 8, "invalid-time");
}

#[test]
fn aligned_age_with_real_adoption_is_recomputed() {
    let scratch = scratch("age");
    let inventory = copy_inventory(&scratch.dir);
    let records = copy_records(&scratch.dir);
    for (from, _) in AGES {
        let to = from.replace(": 22", ": 21").replace(": 15", ": 14").replace(": 77", ": 76").replace(": 72", ": 71").replace(": 144", ": 143");
        let mut hits = replace_present(&inventory, from, &to);
        for name in RECORDS {
            hits += replace_present(&records.join(name), from, &to);
        }
        assert_eq!(hits, 2, "{from}");
    }
    let ran = invoke(Opts {
        inventory: Some(inventory),
        records: Some(records),
        audit_json: None,
        advisory_rev: None,
    });
    assert_clean_fail("aligned-age-recompute", &ran, 9, "age-floor");
}

#[test]
fn aligned_apache_choice_including_native_is_rejected() {
    let scratch = scratch("choice");
    let (inventory, records) = aligned_apache(&scratch.dir);
    let ran = invoke(Opts {
        inventory: Some(inventory),
        records: Some(records),
        audit_json: None,
        advisory_rev: None,
    });
    assert_clean_fail("aligned-choice-apache", &ran, 4, "wrong-dual-choice");
}

#[test]
fn unverified_audit_with_aligned_digest_is_rejected() {
    let scratch = scratch("unverified");
    let (inventory, audit, digest) = forge_audit(&scratch.dir, &audit_with_status("\"unverified\""));
    assert_ne!(digest, AUDIT_SHA);
    let ran = invoke(Opts {
        inventory: Some(inventory),
        records: None,
        audit_json: Some(audit),
        advisory_rev: None,
    });
    assert_clean_fail("audit-status-unverified-aligned", &ran, 19, "fake-claim");
}

#[test]
fn malformed_audit_status_and_missing_vulnerabilities_fail_closed() {
    let number = scratch("status-number");
    let (inventory, audit, _) = forge_audit(&number.dir, &audit_with_status("1"));
    let ran = invoke(Opts {
        inventory: Some(inventory),
        records: None,
        audit_json: Some(audit),
        advisory_rev: None,
    });
    assert_clean_fail("audit-status-number-aligned", &ran, 21, "malformed-record");

    let blank = scratch("status-blank");
    let (inventory, audit, _) = forge_audit(&blank.dir, &audit_with_status("\" \""));
    let ran = invoke(Opts {
        inventory: Some(inventory),
        records: None,
        audit_json: Some(audit),
        advisory_rev: None,
    });
    assert_clean_fail("audit-status-blank-aligned", &ran, 21, "malformed-record");

    let scratch = scratch("vulns-missing");
    let text = fs::read_to_string(audit_src()).expect("audit");
    let stripped = text.replace(",\"vulnerabilities\":{\"found\":false,\"count\":0,\"list\":[]}", "");
    assert_ne!(stripped, text);
    let (inventory, audit, _) = forge_audit(&scratch.dir, &stripped);
    let ran = invoke(Opts {
        inventory: Some(inventory),
        records: None,
        audit_json: Some(audit),
        advisory_rev: None,
    });
    assert_clean_fail("audit-missing-vulnerabilities-aligned", &ran, 21, "malformed-record");
}

#[test]
fn aligned_audit_commit_cannot_replace_the_a101_anchor() {
    let scratch = scratch("audit-commit");
    let text = fs::read_to_string(audit_src()).expect("audit");
    let forged = text.replace(RUSTSEC, FORGED_RUSTSEC);
    assert_ne!(forged, text);
    let (inventory, audit, _) = forge_audit(&scratch.dir, &forged);
    replace_one(&inventory, RUSTSEC, FORGED_RUSTSEC);
    let rev_src = repo().join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/advisory-rev.txt");
    let rev = scratch.dir.join("advisory-rev.txt");
    let rev_text = fs::read_to_string(&rev_src).expect("rev").replace(RUSTSEC, FORGED_RUSTSEC);
    fs::write(&rev, rev_text).expect("rev write");
    replace_one(&inventory, ADVISORY_REV_SHA, &file_sha256(&rev));
    let records = copy_records(&scratch.dir);
    for name in RECORDS {
        let path = records.join(name);
        let text = fs::read_to_string(&path).expect("record");
        let count = text.matches(RUSTSEC).count();
        assert!(count >= 1, "{}", path.display());
        fs::write(&path, text.replace(RUSTSEC, FORGED_RUSTSEC)).expect("write");
    }
    let ran = invoke(Opts {
        inventory: Some(inventory),
        records: Some(records),
        audit_json: Some(audit),
        advisory_rev: Some(rev),
    });
    assert_clean_fail("aligned-audit-commit", &ran, 19, "fake-claim");
}

fn depth_case(name: &str, kind: &str, inserted: usize, expect_ok: bool) {
    let scratch = scratch(name);
    let records = copy_records(&scratch.dir);
    let nest = nested(inserted, kind);
    insert_after_brace(&records.join("bitflags-2.13.2.json"), &format!("\"nested\":{nest},"));
    let ran = invoke(Opts {
        inventory: None,
        records: Some(records),
        audit_json: None,
        advisory_rev: None,
    });
    if expect_ok {
        assert_clean_ok(name, &ran);
    } else {
        assert_clean_fail(name, &ran, 22, "bound");
    }
}

#[test]
fn depth_64_is_deepest_valid_for_object_array_and_mixed() {
    depth_case("depth-object-64", "object", 63, true);
    depth_case("depth-array-64", "array", 63, true);
    depth_case("depth-mixed-64", "mixed", 63, true);
}

#[test]
fn depth_65_and_20000_exit_clean() {
    for kind in ["object", "array", "mixed"] {
        depth_case(&format!("depth-{kind}-65"), kind, 64, false);
        depth_case(&format!("depth-{kind}-20000"), kind, 20_000, false);
    }
}

#[test]
fn malformed_escape_utf8_and_duplicate_key_do_not_abort() {
    let escape = scratch("escape");
    let records = copy_records(&escape.dir);
    insert_after_brace(&records.join("bitflags-2.13.2.json"), "\"bad\":\"\\q\",");
    let ran = invoke(Opts {
        inventory: None,
        records: Some(records),
        audit_json: None,
        advisory_rev: None,
    });
    assert_clean_fail("malformed-escape", &ran, 21, "malformed-record");

    let utf8 = scratch("utf8");
    let records = copy_records(&utf8.dir);
    let path = records.join("bitflags-2.13.2.json");
    let mut bytes = fs::read(&path).expect("read");
    bytes.push(0xff);
    fs::write(&path, bytes).expect("write");
    let ran = invoke(Opts {
        inventory: None,
        records: Some(records),
        audit_json: None,
        advisory_rev: None,
    });
    assert_clean_fail("invalid-utf8", &ran, 21, "malformed-record");

    let duplicate = scratch("duplicate");
    let records = copy_records(&duplicate.dir);
    insert_after_brace(&records.join("bitflags-2.13.2.json"), "\"audited\": false, ");
    let ran = invoke(Opts {
        inventory: None,
        records: Some(records),
        audit_json: None,
        advisory_rev: None,
    });
    assert_clean_fail("duplicate-key", &ran, 14, "duplicate");
}
