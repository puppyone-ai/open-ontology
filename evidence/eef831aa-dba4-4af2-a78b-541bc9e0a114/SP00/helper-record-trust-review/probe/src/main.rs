//! Independent checker spawn probe. Std only. It does not call the author's tests.
//!
//! Expected rows are read from a file written before this process starts.
//! Container depth counts `{` and `[` only. A scalar root stays at depth 0.
//! An object or array root is depth 1.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const OK_LINE: &str = "helper-evidence-check: OK records=5 audited=false license_record_status=complete-for-review release_signoff=false";
const REAL_ADOPT: &str = "2026-10-02T11:10:37Z";
const FORGED_ADOPT: &str = "2026-09-24T17:36:11.278420Z";
const RUSTSEC: &str = "117edb3bed98e9be112f277b7615eea3252e7c43";
const FORGED_RUSTSEC: &str = "0000000000000000000000000000000000000000";
const VULN_OBJECT: &str = ",\"vulnerabilities\":{\"found\":false,\"count\":0,\"list\":[]}";
const RECORDS: [&str; 5] = [
    "bitflags-2.13.2.json",
    "cfg-if-1.0.5.json",
    "cfg_aliases-0.2.2.json",
    "libc-0.2.189.json",
    "nix-0.31.3.json",
];
struct Paths {
    archive: PathBuf,
    checker: PathBuf,
    expected: PathBuf,
    ages: PathBuf,
    out: PathBuf,
}

struct Expect {
    case: String,
    exit: i32,
    signal: String,
    code: String,
    detail: String,
    depth: Option<usize>,
}

struct AgeRow {
    package: String,
    created_at: String,
    stored_age: i64,
    completed_days: i64,
    forged_days: i64,
    at_least_168h: bool,
}

struct Ran {
    exit: Option<i32>,
    signal: Option<i32>,
    millis: u128,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

fn main() {
    let paths = match parse_args() {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("probe-args: {err}");
            std::process::exit(2);
        }
    };
    if let Err(err) = fs::create_dir_all(paths.out.join("cases")) {
        eprintln!("probe-out: {err}");
        std::process::exit(2);
    }
    let mut problems: Vec<String> = Vec::new();
    let mut other_problems = 0usize;
    if let Err(err) = verify_ages(&paths) {
        other_problems += 1;
        problems.push(format!("age-floor: {err}"));
    }
    let expected = match load_expected(&paths.expected) {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!("expected-file: {err}");
            std::process::exit(2);
        }
    };
    let mut lines = Vec::new();
    lines.push("case\texit\tsignal\tms\tdepth\tstdout\tstderr\tverdict".to_string());
    for spec in &expected {
        let (measured, ran, verdict) = run_case(&paths, spec);
        let depth = measured.map(|value| value.to_string()).unwrap_or_default();
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            spec.case,
            ran.exit.map(|code| code.to_string()).unwrap_or_else(|| "none".to_string()),
            ran.signal.map(|sig| sig.to_string()).unwrap_or_else(|| {
                if ran.timed_out {
                    "timeout".to_string()
                } else {
                    "none".to_string()
                }
            }),
            ran.millis,
            depth,
            flatten(&ran.stdout),
            flatten(&ran.stderr),
            verdict
        ));
        write_case_files(&paths, spec, measured, &ran, &verdict);
        if verdict != "match" {
            problems.push(format!("{}: {verdict}", spec.case));
        }
    }
    let tsv = lines.join("\n") + "\n";
    let _ = fs::write(paths.out.join("cases.tsv"), &tsv);
    let case_mismatches = problems.len() - other_problems;
    let matched = expected.len() - case_mismatches;
    let summary = format!(
        "matched={matched} case_mismatches={case_mismatches} other={other_problems} missing=0 cases={}\n{}\n",
        expected.len(),
        if problems.is_empty() { String::new() } else { problems.join("\n") }
    );
    let _ = fs::write(paths.out.join("summary.txt"), &summary);
    print!("{summary}");
    if problems.is_empty() {
        std::process::exit(0);
    }
    std::process::exit(1);
}

fn parse_args() -> Result<Paths, String> {
    let mut archive = None;
    let mut checker = None;
    let mut expected = None;
    let mut ages = None;
    let mut out = None;
    let args: Vec<String> = env::args().skip(1).collect();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        let value = args.get(index + 1).ok_or_else(|| format!("missing value for {name}"))?;
        match name {
            "--archive" => archive = Some(PathBuf::from(value)),
            "--checker" => checker = Some(PathBuf::from(value)),
            "--expected" => expected = Some(PathBuf::from(value)),
            "--ages" => ages = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            other => return Err(format!("unknown arg {other}")),
        }
        index += 2;
    }
    Ok(Paths {
        archive: archive.ok_or("missing --archive")?,
        checker: checker.ok_or("missing --checker")?,
        expected: expected.ok_or("missing --expected")?,
        ages: ages.ok_or("missing --ages")?,
        out: out.ok_or("missing --out")?,
    })
}

fn load_expected(path: &Path) -> Result<Vec<Expect>, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 6 {
            return Err(format!("line {} has {} columns", number + 1, parts.len()));
        }
        let depth = if parts[5].is_empty() {
            None
        } else {
            Some(parts[5].parse::<usize>().map_err(|_| format!("bad depth on line {}", number + 1))?)
        };
        rows.push(Expect {
            case: parts[0].to_string(),
            exit: parts[1].parse::<i32>().map_err(|_| format!("bad exit on line {}", number + 1))?,
            signal: parts[2].to_string(),
            code: parts[3].to_string(),
            detail: parts[4].to_string(),
            depth,
        });
    }
    if rows.len() != 22 {
        return Err(format!("expected 22 cases, found {}", rows.len()));
    }
    Ok(rows)
}

fn verify_ages(paths: &Paths) -> Result<(), String> {
    let text = fs::read_to_string(&paths.ages).map_err(|err| err.to_string())?;
    let mut meta = std::collections::BTreeMap::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            if let Some((key, value)) = rest.split_once('=') {
                meta.insert(key.to_string(), value.to_string());
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 6 {
            return Err(format!("age row has {} columns", parts.len()));
        }
        rows.push(AgeRow {
            package: parts[0].to_string(),
            created_at: parts[1].to_string(),
            stored_age: parts[2].parse().map_err(|_| "stored age")?,
            completed_days: parts[3].parse().map_err(|_| "completed days")?,
            forged_days: parts[4].parse().map_err(|_| "forged days")?,
            at_least_168h: parts[5] == "yes",
        });
    }
    if meta.get("adoption").map(String::as_str) != Some(REAL_ADOPT) {
        return Err("preregistered adoption is not the pinned instant".into());
    }
    if meta.get("forged_adoption").map(String::as_str) != Some(FORGED_ADOPT) {
        return Err("preregistered forged adoption changed".into());
    }
    if meta.get("floor_hours").map(String::as_str) != Some("168") || meta.get("floor_seconds").map(String::as_str) != Some("604800") {
        return Err("preregistered floor is not 168 hours".into());
    }
    let lock = fs::read(paths.archive.join("testing/harness/containment-supervisor/Cargo.lock")).map_err(|err| err.to_string())?;
    let lock_sha = sha256_bytes(&lock)?;
    if meta.get("lock_sha256").map(String::as_str) != Some(lock_sha.as_str())
        || lock_sha != "14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1"
    {
        return Err(format!("helper lock sha is {lock_sha}"));
    }
    let audit_path = paths.archive.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out");
    let audit = fs::read(&audit_path).map_err(|err| err.to_string())?;
    let audit_sha = sha256_bytes(&audit)?;
    if meta.get("audit_sha256").map(String::as_str) != Some(audit_sha.as_str()) {
        return Err(format!("audit sha is {audit_sha}"));
    }
    if audit_has_status_key(&audit)? {
        return Err("canonical audit bytes contain a status key".into());
    }
    let age_text = fs::read_to_string(paths.archive.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/age.txt")).map_err(|err| err.to_string())?;
    if !age_text.lines().any(|line| line == "floor_seconds=604800") {
        return Err("age file floor_seconds is not 604800".into());
    }
    let inventory = fs::read_to_string(paths.archive.join("testing/harness/helper-evidence-check/fixtures/inventory.json")).map_err(|err| err.to_string())?;
    if !inventory.contains(&format!("\"adoption_date\": \"{REAL_ADOPT}\"")) {
        return Err("inventory adoption is not the pinned instant".into());
    }
    let epoch = stamp_nanos("1970-01-01T00:00:00Z")?;
    if epoch != 0 {
        return Err(format!("epoch nanos {epoch}"));
    }
    let adopted = stamp_nanos(REAL_ADOPT)?;
    let forged = stamp_nanos(FORGED_ADOPT)?;
    if rows.len() != 5 {
        return Err(format!("age rows {}", rows.len()));
    }
    let mut report = String::from("package\tcreated_at\tstored\tcomputed\tforged\thours\n");
    for row in &rows {
        let created = version_created_at(&paths.archive, &row.package)?;
        if created != row.created_at {
            return Err(format!("{} version.created_at is {created}", row.package));
        }
        if !inventory_has_publication(&inventory, &row.package, &row.created_at, row.stored_age) {
            return Err(format!("{} inventory publication or stored age disagrees", row.package));
        }
        if !age_text.contains(&format!("version_created_at={}", row.created_at)) {
            return Err(format!("{} missing from age file", row.package));
        }
        let published = stamp_nanos(&created)?;
        let elapsed = adopted - published;
        let computed = completed_days(elapsed);
        let forged_elapsed = forged - published;
        let forged_days = completed_days(forged_elapsed);
        let hours = elapsed as f64 / 3_600_000_000_000.0;
        let floor_ns = 168 * 3_600 * 1_000_000_000;
        if computed != row.completed_days || computed != row.stored_age || forged_days != row.forged_days {
            return Err(format!(
                "{} computed {computed} stored {} forged {forged_days}",
                row.package, row.stored_age
            ));
        }
        if elapsed < floor_ns || !row.at_least_168h || forged_elapsed < floor_ns {
            return Err(format!("{} is under the 168 hour floor", row.package));
        }
        report.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{hours:.3}\n",
            row.package, created, row.stored_age, computed, forged_days
        ));
    }
    fs::write(paths.out.join("age-floor.txt"), report).map_err(|err| err.to_string())?;
    Ok(())
}

fn inventory_has_publication(inventory: &str, package: &str, created_at: &str, age: i64) -> bool {
    let marker = format!("\"name\": \"{package}\"");
    let Some(start) = inventory.find(&marker) else {
        return false;
    };
    let window = &inventory[start..];
    let end = window[marker.len()..].find("\"name\":").map(|at| at + marker.len()).unwrap_or(window.len());
    let slice = &window[..end];
    slice.contains(&format!("\"publication_date\": \"{created_at}\""))
        && slice.contains(&format!("\"age_days_at_adoption\": {age}"))
}

fn version_created_at(archive: &Path, package: &str) -> Result<String, String> {
    let path = archive.join(format!(
        "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/meta/version-{package}.json"
    ));
    let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let start = text.find("\"version\":{").ok_or("version object missing")?;
    let published = text[start..].find("\"published_by\"").ok_or("published_by missing")?;
    let window = &text[start..start + published];
    let key = "\"created_at\":\"";
    let at = window.find(key).ok_or("version created_at missing")?;
    let rest = &window[at + key.len()..];
    let end = rest.find('"').ok_or("created_at unterminated")?;
    Ok(rest[..end].to_string())
}

fn audit_has_status_key(bytes: &[u8]) -> Result<bool, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "audit is not utf-8")?;
    let trimmed = text.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err("audit root is not an object".into());
    }
    Ok(trimmed.contains("\"status\""))
}

fn run_case(paths: &Paths, spec: &Expect) -> (Option<usize>, Ran, String) {
    let built = match prepare(paths, &spec.case) {
        Ok(built) => built,
        Err(err) => {
            return (
                None,
                Ran {
                    exit: None,
                    signal: None,
                    millis: 0,
                    stdout: Vec::new(),
                    stderr: err.into_bytes(),
                    timed_out: false,
                },
                "prepare-failed".to_string(),
            );
        }
    };
    if spec.depth != built.depth {
        return (
            built.depth,
            Ran {
                exit: None,
                signal: None,
                millis: 0,
                stdout: Vec::new(),
                stderr: format!("measured depth {:?} expected {:?}", built.depth, spec.depth).into_bytes(),
                timed_out: false,
            },
            "depth-preregistration-mismatch".to_string(),
        );
    }
    let ran = spawn_checker(paths, &built);
    let verdict = judge(spec, built.depth, &ran);
    (built.depth, ran, verdict)
}

struct Built {
    inventory: PathBuf,
    records: PathBuf,
    audit: PathBuf,
    advisory: PathBuf,
    depth: Option<usize>,
}

fn prepare(paths: &Paths, case: &str) -> Result<Built, String> {
    let scratch = PathBuf::from(format!("/tmp/oo-a114-case-{case}"));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).map_err(|err| err.to_string())?;
    let canonical = canonical_paths(paths);
    match case {
        "canonical-five" => Ok(Built {
            inventory: canonical.inventory,
            records: canonical.records,
            audit: canonical.audit,
            advisory: canonical.advisory,
            depth: None,
        }),
        "paired-adoption-age" => prepare_adoption(&scratch, &canonical, true),
        "forged-age-real-adoption" => prepare_adoption(&scratch, &canonical, false),
        "paired-apache-choice" => prepare_apache(&scratch, &canonical),
        "audit-status-unverified-aligned" => prepare_audit_status(&scratch, &canonical, "\"unverified\""),
        "audit-status-blank-aligned" => prepare_audit_status(&scratch, &canonical, "\" \""),
        "audit-status-number-aligned" => prepare_audit_status(&scratch, &canonical, "1"),
        "audit-missing-vulnerabilities-aligned" => prepare_missing_vuln(&scratch, &canonical),
        "paired-rustsec-identity" => prepare_rustsec(&scratch, &canonical),
        "depth-object-64" => prepare_depth(&scratch, &canonical, "object", 64),
        "depth-array-64" => prepare_depth(&scratch, &canonical, "array", 64),
        "depth-mixed-64" => prepare_depth(&scratch, &canonical, "mixed", 64),
        "depth-object-65" => prepare_depth(&scratch, &canonical, "object", 65),
        "depth-array-65" => prepare_depth(&scratch, &canonical, "array", 65),
        "depth-mixed-65" => prepare_depth(&scratch, &canonical, "mixed", 65),
        "depth-object-20000" => prepare_depth(&scratch, &canonical, "object", 20000),
        "depth-array-20000" => prepare_depth(&scratch, &canonical, "array", 20000),
        "depth-mixed-20000" => prepare_depth(&scratch, &canonical, "mixed", 20000),
        "scalar-root-depth-zero" => prepare_scalar(&scratch, &canonical),
        "malformed-escape" => prepare_insert(&scratch, &canonical, "\"bad\":\"\\q\",", None),
        "invalid-utf8" => prepare_utf8(&scratch, &canonical),
        "duplicate-key" => prepare_insert(&scratch, &canonical, "\"audited\":false,", None),
        other => Err(format!("no builder for {other}")),
    }
}

struct Canon {
    inventory: PathBuf,
    records: PathBuf,
    audit: PathBuf,
    advisory: PathBuf,
}

fn canonical_paths(paths: &Paths) -> Canon {
    let raw = paths.archive.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw");
    Canon {
        inventory: paths.archive.join("testing/harness/helper-evidence-check/fixtures/inventory.json"),
        records: paths.archive.join("testing/harness/containment-supervisor/provenance/audit-records"),
        audit: raw.join("scan-audit.out"),
        advisory: raw.join("advisory-rev.txt"),
    }
}

fn copy_tree(canon: &Canon, scratch: &Path) -> Result<(PathBuf, PathBuf), String> {
    let inventory = scratch.join("inventory.json");
    copy_file(&canon.inventory, &inventory)?;
    let records = scratch.join("records");
    fs::create_dir_all(&records).map_err(|err| err.to_string())?;
    for name in RECORDS {
        copy_file(&canon.records.join(name), &records.join(name))?;
    }
    Ok((inventory, records))
}

fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(src).map_err(|err| err.to_string())?;
    if !meta.file_type().is_file() {
        return Err(format!("{} is not a regular file", src.display()));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::copy(src, dst).map_err(|err| err.to_string())?;
    Ok(())
}

fn rewrite(path: &Path, from: &str, to: &str, expect: usize) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let count = text.matches(from).count();
    if count != expect {
        return Err(format!("{} has {count} matches for {from}, expected {expect}", path.display()));
    }
    fs::write(path, text.replace(from, to)).map_err(|err| err.to_string())?;
    Ok(())
}

fn prepare_adoption(scratch: &Path, canon: &Canon, forge_instant: bool) -> Result<Built, String> {
    let (inventory, records) = copy_tree(canon, scratch)?;
    let ages = [
        ("\"age_days_at_adoption\": 22", 14, 21),
        ("\"age_days_at_adoption\": 15", 8, 14),
        ("\"age_days_at_adoption\": 77", 70, 76),
        ("\"age_days_at_adoption\": 72", 64, 71),
        ("\"age_days_at_adoption\": 144", 136, 143),
    ];
    if forge_instant {
        rewrite(&inventory, REAL_ADOPT, FORGED_ADOPT, 1)?;
        for name in RECORDS {
            rewrite(&records.join(name), REAL_ADOPT, FORGED_ADOPT, 1)?;
        }
    }
    for (token, forged, decremented) in ages {
        let replacement_days = if forge_instant { forged } else { decremented };
        let replacement = token.rsplit(": ").next().map(|_| format!("\"age_days_at_adoption\": {replacement_days}")).unwrap();
        rewrite(&inventory, token, &replacement, 1)?;
        let mut hits = 0;
        for name in RECORDS {
            let path = records.join(name);
            let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
            let count = text.matches(token).count();
            if count > 1 {
                return Err(format!("{} has {count} age tokens", path.display()));
            }
            if count == 1 {
                fs::write(&path, text.replacen(token, &replacement, 1)).map_err(|err| err.to_string())?;
                hits += 1;
            }
        }
        if hits != 1 {
            return Err(format!("{token} found in {hits} records"));
        }
    }
    Ok(Built {
        inventory,
        records,
        audit: canon.audit.clone(),
        advisory: canon.advisory.clone(),
        depth: None,
    })
}

fn prepare_apache(scratch: &Path, canon: &Canon) -> Result<Built, String> {
    let (inventory, records) = copy_tree(canon, scratch)?;
    rewrite(&inventory, "\"choice\": \"MIT\"", "\"choice\": \"Apache-2.0\"", 3)?;
    for name in ["bitflags-2.13.2.json", "cfg-if-1.0.5.json", "libc-0.2.189.json"] {
        rewrite(&records.join(name), "\"dual_license_choice\": \"MIT\"", "\"dual_license_choice\": \"Apache-2.0\"", 1)?;
    }
    rewrite(
        &records.join("nix-0.31.3.json"),
        "\"dual_license_choice\": \"MIT\"",
        "\"dual_license_choice\": \"Apache-2.0\"",
        3,
    )?;
    Ok(Built {
        inventory,
        records,
        audit: canon.audit.clone(),
        advisory: canon.advisory.clone(),
        depth: None,
    })
}

fn prepare_audit_status(scratch: &Path, canon: &Canon, status_json: &str) -> Result<Built, String> {
    let (inventory, records) = copy_tree(canon, scratch)?;
    let text = fs::read_to_string(&canon.audit).map_err(|err| err.to_string())?;
    let trimmed = text.trim_end();
    let body = trimmed.strip_suffix('}').ok_or("audit object does not end with }")?;
    let forged = format!("{body},\"status\":{status_json}}}\n");
    let audit = scratch.join("scan-audit.out");
    fs::write(&audit, &forged).map_err(|err| err.to_string())?;
    let digest = sha256_bytes(forged.as_bytes())?;
    let old = fs::read(&canon.audit).map_err(|err| err.to_string())?;
    let old_digest = sha256_bytes(&old)?;
    if digest == old_digest {
        return Err("status insertion did not change the audit digest".into());
    }
    rewrite(&inventory, &old_digest, &digest, 1)?;
    Ok(Built {
        inventory,
        records,
        audit,
        advisory: canon.advisory.clone(),
        depth: None,
    })
}

fn prepare_missing_vuln(scratch: &Path, canon: &Canon) -> Result<Built, String> {
    let (inventory, records) = copy_tree(canon, scratch)?;
    let text = fs::read_to_string(&canon.audit).map_err(|err| err.to_string())?;
    if text.matches(VULN_OBJECT).count() != 1 {
        return Err("vulnerabilities object count is not 1".into());
    }
    let forged = text.replace(VULN_OBJECT, "");
    let audit = scratch.join("scan-audit.out");
    fs::write(&audit, &forged).map_err(|err| err.to_string())?;
    let digest = sha256_bytes(forged.as_bytes())?;
    let old_digest = sha256_bytes(&fs::read(&canon.audit).map_err(|err| err.to_string())?)?;
    rewrite(&inventory, &old_digest, &digest, 1)?;
    Ok(Built {
        inventory,
        records,
        audit,
        advisory: canon.advisory.clone(),
        depth: None,
    })
}

fn prepare_rustsec(scratch: &Path, canon: &Canon) -> Result<Built, String> {
    let (inventory, records) = copy_tree(canon, scratch)?;
    let audit_text = fs::read_to_string(&canon.audit).map_err(|err| err.to_string())?;
    if audit_text.matches(RUSTSEC).count() != 1 {
        return Err("audit rustsec count".into());
    }
    let audit = scratch.join("scan-audit.out");
    let forged_audit = audit_text.replace(RUSTSEC, FORGED_RUSTSEC);
    fs::write(&audit, &forged_audit).map_err(|err| err.to_string())?;
    let advisory_text = fs::read_to_string(&canon.advisory).map_err(|err| err.to_string())?;
    if advisory_text.matches(RUSTSEC).count() != 4 {
        return Err(format!("advisory rev rustsec count {}", advisory_text.matches(RUSTSEC).count()));
    }
    let advisory = scratch.join("advisory-rev.txt");
    let forged_rev = advisory_text.replace(RUSTSEC, FORGED_RUSTSEC);
    fs::write(&advisory, &forged_rev).map_err(|err| err.to_string())?;
    rewrite(&inventory, RUSTSEC, FORGED_RUSTSEC, 1)?;
    for name in RECORDS {
        let path = records.join(name);
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        if text.matches(RUSTSEC).count() != 1 {
            return Err(format!("{} rustsec count", path.display()));
        }
        fs::write(&path, text.replace(RUSTSEC, FORGED_RUSTSEC)).map_err(|err| err.to_string())?;
    }
    let old_audit = sha256_bytes(audit_text.as_bytes())?;
    let new_audit = sha256_bytes(forged_audit.as_bytes())?;
    let old_rev = sha256_bytes(advisory_text.as_bytes())?;
    let new_rev = sha256_bytes(forged_rev.as_bytes())?;
    rewrite(&inventory, &old_audit, &new_audit, 1)?;
    rewrite(&inventory, &old_rev, &new_rev, 1)?;
    Ok(Built {
        inventory,
        records,
        audit,
        advisory,
        depth: None,
    })
}

fn prepare_depth(scratch: &Path, canon: &Canon, kind: &str, document_depth: usize) -> Result<Built, String> {
    if document_depth == 0 {
        return Err("container depth 0 is the scalar case".into());
    }
    let (inventory, records) = copy_tree(canon, scratch)?;
    let inserted = document_depth - 1;
    let nest = nested(inserted, kind);
    let path = records.join("bitflags-2.13.2.json");
    insert_after_brace(&path, &format!("\"nested\":{nest},"))?;
    let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let measured = max_container_depth(&text)?;
    if measured != document_depth {
        return Err(format!("{kind} measured {measured} wanted {document_depth}"));
    }
    Ok(Built {
        inventory,
        records,
        audit: canon.audit.clone(),
        advisory: canon.advisory.clone(),
        depth: Some(measured),
    })
}

fn prepare_scalar(scratch: &Path, canon: &Canon) -> Result<Built, String> {
    let (inventory, records) = copy_tree(canon, scratch)?;
    let path = records.join("bitflags-2.13.2.json");
    fs::write(&path, "0\n").map_err(|err| err.to_string())?;
    let measured = max_container_depth("0")?;
    if measured != 0 {
        return Err(format!("scalar measured {measured}"));
    }
    Ok(Built {
        inventory,
        records,
        audit: canon.audit.clone(),
        advisory: canon.advisory.clone(),
        depth: Some(0),
    })
}

fn prepare_insert(scratch: &Path, canon: &Canon, insertion: &str, depth: Option<usize>) -> Result<Built, String> {
    let (inventory, records) = copy_tree(canon, scratch)?;
    insert_after_brace(&records.join("bitflags-2.13.2.json"), insertion)?;
    Ok(Built {
        inventory,
        records,
        audit: canon.audit.clone(),
        advisory: canon.advisory.clone(),
        depth,
    })
}

fn prepare_utf8(scratch: &Path, canon: &Canon) -> Result<Built, String> {
    let (inventory, records) = copy_tree(canon, scratch)?;
    let path = records.join("bitflags-2.13.2.json");
    let mut bytes = fs::read(&path).map_err(|err| err.to_string())?;
    bytes.push(0xff);
    fs::write(&path, bytes).map_err(|err| err.to_string())?;
    Ok(Built {
        inventory,
        records,
        audit: canon.audit.clone(),
        advisory: canon.advisory.clone(),
        depth: None,
    })
}

fn insert_after_brace(path: &Path, insertion: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let rest = text.strip_prefix('{').ok_or("record does not start with an object")?;
    let mut out = String::from("{");
    out.push_str(insertion);
    out.push_str(rest);
    fs::write(path, out).map_err(|err| err.to_string())?;
    Ok(())
}

fn nested(containers: usize, kind: &str) -> String {
    let mut text = String::new();
    for index in 0..containers {
        match kind {
            "object" => text.push_str("{\"k\":"),
            "array" => text.push('['),
            "mixed" => {
                if index % 2 == 0 {
                    text.push_str("{\"k\":");
                } else {
                    text.push('[');
                }
            }
            _ => text.push('['),
        }
    }
    text.push('0');
    for index in (0..containers).rev() {
        let object = match kind {
            "object" => true,
            "array" => false,
            "mixed" => index % 2 == 0,
            _ => false,
        };
        text.push(if object { '}' } else { ']' });
    }
    text
}

fn max_container_depth(text: &str) -> Result<usize, String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut depth = 0usize;
    let mut max = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b'{' || byte == b'[' {
            depth += 1;
            max = max.max(depth);
        } else if byte == b'}' || byte == b']' {
            if depth == 0 {
                return Err("depth underflow".into());
            }
            depth -= 1;
        }
        index += 1;
    }
    if in_string || depth != 0 {
        return Err(format!("unbalanced depth={depth} string={in_string}"));
    }
    Ok(max)
}

fn spawn_checker(paths: &Paths, built: &Built) -> Ran {
    let raw = paths.archive.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw");
    let mut command = Command::new(&paths.checker);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir("/tmp")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("TMPDIR", "/tmp")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .arg("--inventory")
        .arg(&built.inventory)
        .arg("--records")
        .arg(&built.records)
        .arg("--licenses")
        .arg(paths.archive.join("testing/harness/containment-supervisor/provenance/licenses"))
        .arg("--retained-licenses")
        .arg(raw.join("upstream-licenses"))
        .arg("--helper-lock")
        .arg(paths.archive.join("testing/harness/containment-supervisor/Cargo.lock"))
        .arg("--helper-manifest")
        .arg(paths.archive.join("testing/harness/containment-supervisor/Cargo.toml"))
        .arg("--deny-config")
        .arg(paths.archive.join("ci/deny.toml"))
        .arg("--metadata")
        .arg(raw.join("helper-metadata.out"))
        .arg("--age")
        .arg(raw.join("age.txt"))
        .arg("--audit-json")
        .arg(&built.audit)
        .arg("--deny-advisories")
        .arg(raw.join("scan-deny-advisories.err"))
        .arg("--advisory-rev")
        .arg(&built.advisory)
        .arg("--version-meta")
        .arg(raw.join("meta"))
        .arg("--repo-root")
        .arg(&paths.archive)
        .arg("--as-of")
        .arg("2026-10-02T12:12:14Z");
    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Ran {
                exit: None,
                signal: None,
                millis: 0,
                stdout: Vec::new(),
                stderr: format!("spawn failed: {err}").into_bytes(),
                timed_out: false,
            };
        }
    };
    let limit = Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let millis = started.elapsed().as_millis();
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_end(&mut stdout);
                }
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_end(&mut stderr);
                }
                #[cfg(unix)]
                let signal = std::os::unix::process::ExitStatusExt::signal(&status);
                #[cfg(not(unix))]
                let signal = None;
                return Ran {
                    exit: status.code(),
                    signal,
                    millis,
                    stdout,
                    stderr,
                    timed_out: false,
                };
            }
            Ok(None) if started.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Ran {
                    exit: None,
                    signal: None,
                    millis: started.elapsed().as_millis(),
                    stdout: Vec::new(),
                    stderr: b"timeout".to_vec(),
                    timed_out: true,
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => {
                return Ran {
                    exit: None,
                    signal: None,
                    millis: started.elapsed().as_millis(),
                    stdout: Vec::new(),
                    stderr: format!("wait failed: {err}").into_bytes(),
                    timed_out: false,
                };
            }
        }
    }
}

fn judge(spec: &Expect, measured: Option<usize>, ran: &Ran) -> String {
    if ran.timed_out {
        return "timeout".into();
    }
    let signal = ran.signal.map(|sig| sig.to_string()).unwrap_or_else(|| "none".to_string());
    if signal != spec.signal {
        return format!("signal {signal}");
    }
    if ran.exit != Some(spec.exit) {
        return format!("exit {:?}", ran.exit);
    }
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let stderr = String::from_utf8_lossy(&ran.stderr);
    if spec.code == "OK" {
        if stdout.trim() != OK_LINE {
            return "stdout".into();
        }
        if stdout.contains("audited=true") || !stderr.trim().is_empty() {
            return "not-unaudited-or-stderr".into();
        }
    } else {
        if stdout.contains("OK records=5") {
            return "accepted-failure".into();
        }
        if !stderr.contains(&format!("code={}", spec.code)) {
            return format!("code missing {}", spec.code);
        }
        if spec.detail.starts_with('!') {
            let banned = &spec.detail[1..];
            if stderr.contains(banned) {
                return format!("banned detail {banned}");
            }
        } else if !spec.detail.is_empty() && !stderr.contains(&spec.detail) {
            return format!("detail missing {}", spec.detail);
        }
    }
    if spec.depth != measured {
        return "depth".into();
    }
    "match".into()
}

fn write_case_files(paths: &Paths, spec: &Expect, measured: Option<usize>, ran: &Ran, verdict: &str) {
    let dir = paths.out.join("cases");
    let _ = fs::write(dir.join(format!("{}.stdout", spec.case)), &ran.stdout);
    let _ = fs::write(dir.join(format!("{}.stderr", spec.case)), &ran.stderr);
    let meta = format!(
        "exit={}\nsignal={}\nms={}\ndepth={}\nverdict={verdict}\n",
        ran.exit.map(|code| code.to_string()).unwrap_or_else(|| "none".to_string()),
        ran.signal.map(|sig| sig.to_string()).unwrap_or_else(|| if ran.timed_out { "timeout" } else { "none" }.to_string()),
        ran.millis,
        measured.map(|value| value.to_string()).unwrap_or_default()
    );
    let _ = fs::write(dir.join(format!("{}.meta", spec.case)), meta);
}

fn flatten(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).replace(['\n', '\r', '\t'], " ");
    if text.len() > 300 {
        format!("{}…", &text[..300])
    } else {
        text
    }
}

fn sha256_bytes(bytes: &[u8]) -> Result<String, String> {
    let path = PathBuf::from(format!("/tmp/oo-a114-sha-{}", std::process::id()));
    fs::write(&path, bytes).map_err(|err| err.to_string())?;
    let output = Command::new("/usr/bin/sha256sum")
        .arg(&path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .output()
        .map_err(|err| err.to_string())?;
    let _ = fs::remove_file(&path);
    if !output.status.success() {
        return Err(format!("sha256sum {}", String::from_utf8_lossy(&output.stderr)));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "sha utf-8")?;
    text.split_whitespace().next().map(str::to_string).ok_or_else(|| "sha token".into())
}

const DAY_NS: i128 = 86_400 * 1_000_000_000;

fn stamp_nanos(value: &str) -> Result<i128, String> {
    let body = value.strip_suffix('Z').ok_or("timestamp is not UTC Z")?;
    if body.len() < 19 {
        return Err("timestamp short".into());
    }
    let (date, time) = body.split_once('T').ok_or("timestamp missing T")?;
    let mut dates = date.split('-');
    let year: i32 = dates.next().ok_or("year")?.parse().map_err(|_| "year")?;
    let month: u32 = dates.next().ok_or("month")?.parse().map_err(|_| "month")?;
    let day: u32 = dates.next().ok_or("day")?.parse().map_err(|_| "day")?;
    if dates.next().is_some() || !valid_ymd(year, month, day) {
        return Err("calendar date".into());
    }
    let (clock, frac) = match time.split_once('.') {
        Some((clock, frac)) => (clock, frac),
        None => (time, ""),
    };
    if clock.len() != 8 {
        return Err("clock".into());
    }
    let hour: u32 = clock[0..2].parse().map_err(|_| "hour")?;
    let minute: u32 = clock[3..5].parse().map_err(|_| "minute")?;
    let second: u32 = clock[6..8].parse().map_err(|_| "second")?;
    if &clock[2..3] != ":" || &clock[5..6] != ":" || hour > 23 || minute > 59 || second > 59 {
        return Err("clock fields".into());
    }
    if !frac.is_empty() && (frac.len() > 9 || !frac.bytes().all(|byte| byte.is_ascii_digit())) {
        return Err("fraction".into());
    }
    let mut nanos_frac: u32 = 0;
    let mut digits = 0u32;
    for byte in frac.bytes() {
        nanos_frac = nanos_frac * 10 + u32::from(byte - b'0');
        digits += 1;
    }
    while digits < 9 {
        nanos_frac *= 10;
        digits += 1;
    }
    let days = days_from_civil(year, month, day);
    Ok(days * DAY_NS
        + i128::from(hour) * 3_600 * 1_000_000_000
        + i128::from(minute) * 60 * 1_000_000_000
        + i128::from(second) * 1_000_000_000
        + i128::from(nanos_frac))
}

fn completed_days(elapsed: i128) -> i64 {
    (elapsed / DAY_NS) as i64
}

fn valid_ymd(year: i32, month: u32, day: u32) -> bool {
    if !(1..=12).contains(&month) || day == 0 || year < 1970 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    day <= max
}

fn days_from_civil(mut year: i32, month: u32, day: u32) -> i128 {
    year -= i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = (year - era * 400) as u32;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i128::from(era) * 146097 + i128::from(doe) - 719468
}
