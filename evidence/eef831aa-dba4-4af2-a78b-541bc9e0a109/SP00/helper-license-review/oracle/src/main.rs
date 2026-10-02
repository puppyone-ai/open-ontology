//! Independent probe. It calls the helper-evidence-check binary and records
//! the raw exit, stdout, and stderr. It does not set audited on the archive.

#![forbid(unsafe_code)]

use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const AS_OF: &str = "2026-10-02T12:12:14Z";
const REAL_ADOPT: &str = "2026-10-02T11:10:37Z";
const FORGED_ADOPT: &str = "2026-09-24T17:36:11.278420Z";
const DAY_NS: i128 = 86_400 * 1_000_000_000;
const BITFLAGS_SUM: &str = "3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06";
const BITFLAGS_MIT: &str = "6485b8ed310d3f0340bf1ad1f47645069ce4069dcc6bb46c7d5c6faf41de1fdb";
const LIBC_APACHE: &str = "62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a";
const NOTICE_SHA: &str = "1e2b7ade3fb228130408b9990cae6a7618eb314c75aa0b164bfe485d9d9756ee";
const RUSTSEC: &str = "117edb3bed98e9be112f277b7615eea3252e7c43";
const AUDIT_SHA: &str = "f64e43716f72c173879cb30130f73df0f1c802ac32960eb6c3955c15c2209f81";
const DENY_SHA: &str = "02d186150402cda84d64b50da1dea4b586274b3afbd3fbaf078a7c4e389920c3";
const MIT_DUTY: &str = "Include the copyright notice and this permission notice in all copies or substantial portions of the Software.";
const BITFLAGS_SOURCE: &str = "testing/harness/containment-supervisor/provenance/licenses/bitflags-2.13.2/LICENSE-MIT";

struct Outcome {
    exit: i32,
    timed_out: bool,
    millis: u128,
    stdout: String,
    stderr: String,
}

struct Ctx {
    bin: PathBuf,
    archive: PathBuf,
    out: PathBuf,
    scratch: PathBuf,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let bin = PathBuf::from(args.next().expect("checker binary"));
    let archive = PathBuf::from(args.next().expect("archive root"));
    let out = PathBuf::from(args.next().expect("output dir"));
    fs::create_dir_all(&out).expect("out");
    let scratch = PathBuf::from("/tmp/oo-a109-probe");
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).expect("scratch");
    let ctx = Ctx {
        bin,
        archive,
        out,
        scratch,
    };
    if let Err(err) = assert_age_vectors() {
        let _ = fs::write(ctx.out.join("formula.err"), &err);
        eprintln!("{err}");
        std::process::exit(2);
    }
    let mut lines = Vec::new();
    lines.push(run_positive(&ctx));
    lines.push(run_paired_checksum(&ctx));
    lines.push(run_paired_version(&ctx));
    lines.push(run_paired_repo(&ctx));
    lines.push(run_paired_publish(&ctx));
    lines.push(run_paired_adopt(&ctx));
    lines.push(run_paired_feature(&ctx));
    lines.push(run_paired_choice(&ctx));
    lines.push(run_record_only_choice(&ctx));
    lines.push(run_obligation(&ctx));
    lines.push(run_advisory(&ctx));
    lines.push(run_cli_dotdot(&ctx));
    lines.push(run_source_replace(&ctx, "source-dotdot", "../".to_string(), "fail"));
    lines.push(run_source_replace(&ctx, "source-absolute", "/etc/passwd".to_string(), "fail"));
    lines.push(run_source_replace(
        &ctx,
        "source-url-credential",
        "https://user:pass@example.invalid/LICENSE-MIT".to_string(),
        "fail",
    ));
    lines.push(run_source_replace(
        &ctx,
        "source-unicode-dotdot",
        format!("\\u002e\\u002e/\\u002e\\u002e/{BITFLAGS_SOURCE}"),
        "fail",
    ));
    lines.push(run_source_replace(
        &ctx,
        "source-unicode-nul",
        format!("{BITFLAGS_SOURCE}\\u0000"),
        "fail",
    ));
    lines.push(run_symlink(&ctx));
    lines.push(run_fifo_license(&ctx));
    lines.push(run_fifo_lock(&ctx));
    lines.push(run_hardlink(&ctx));
    lines.push(run_duplicate_key(&ctx));
    lines.push(run_duplicate_unicode_key(&ctx));
    lines.push(run_unknown_key(&ctx));
    lines.push(run_invalid_utf8(&ctx));
    lines.push(run_depth(&ctx, "depth-64", 64));
    lines.push(run_depth(&ctx, "depth-20000", 20_000));
    lines.push(run_oversize(&ctx));
    lines.push(run_file_count(&ctx));
    lines.push(run_nan(&ctx));
    lines.push(run_null_publication(&ctx));
    lines.push(run_missing_apache(&ctx));
    lines.push(run_damaged_apache(&ctx));
    lines.push(run_missing_notice(&ctx));
    lines.push(run_damaged_notice(&ctx));
    lines.push(run_flag(&ctx, "flag-audited", "\"audited\": false", "\"audited\": true"));
    lines.push(run_flag(&ctx, "flag-admission", "\"admission\": false", "\"admission\": true"));
    lines.push(run_flag(
        &ctx,
        "flag-fresh-scan",
        "\"fresh_current_scan\": false",
        "\"fresh_current_scan\": true",
    ));
    lines.push(run_flag(
        &ctx,
        "flag-release",
        "\"release_signoff\": false",
        "\"release_signoff\": true",
    ));
    lines.push(run_audit_missing(&ctx));
    lines.push(run_audit_flip(&ctx));
    lines.push(run_audit_unverified(&ctx));
    lines.push(run_deny_errors(&ctx));
    lines.push(run_manifest_oneline(&ctx));
    lines.push(run_path_hijack(&ctx));
    lines.push(run_as_of_before(&ctx));

    let mut report = String::new();
    let mut misses = 0u32;
    let mut hits = 0u32;
    let mut observes = 0u32;
    for line in &lines {
        report.push_str(line);
        report.push('\n');
        if line.contains("\tmiss\t") {
            misses += 1;
        } else if line.contains("\thit\t") {
            hits += 1;
        } else if line.contains("\tobserve\t") {
            observes += 1;
        }
    }
    let summary = format!("hits={hits} misses={misses} observes={observes} cases={}\n", lines.len());
    fs::write(ctx.out.join("cases.tsv"), &report).expect("tsv");
    fs::write(ctx.out.join("summary.txt"), &summary).expect("summary");
    println!("{summary}");
    let _ = fs::remove_dir_all(&ctx.scratch);
}

fn run_positive(ctx: &Ctx) -> String {
    let outcome = invoke(ctx, &Overrides::default(), Duration::from_secs(20), &[]);
    judge("positive", "pass", &outcome, false)
}

fn run_paired_checksum(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("paired-checksum");
    let (inventory, records) = copy_claims(ctx, &dir);
    let forged = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    rewrite(&inventory, BITFLAGS_SUM, forged, 1).expect("inventory sum");
    rewrite_records(&records, BITFLAGS_SUM, forged).expect("record sum");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    judge("paired-checksum", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_paired_version(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("paired-version");
    let (inventory, records) = copy_claims(ctx, &dir);
    rewrite(&inventory, "\"version\": \"2.13.2\"", "\"version\": \"9.9.9\"", 1).expect("inv ver");
    rewrite(&records.join("bitflags-2.13.2.json"), "\"version\": \"2.13.2\"", "\"version\": \"9.9.9\"", 1)
        .expect("rec ver");
    rewrite(&records.join("nix-0.31.3.json"), "\"version\": \"2.13.2\"", "\"version\": \"9.9.9\"", 1).expect("nix ver");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    judge("paired-version", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_paired_repo(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("paired-repo");
    let (inventory, records) = copy_claims(ctx, &dir);
    let from = "https://github.com/bitflags/bitflags";
    let to = "https://user:pass@github.com/bitflags/bitflags";
    rewrite(&inventory, from, to, 1).expect("inv repo");
    rewrite(&records.join("bitflags-2.13.2.json"), from, to, 1).expect("rec repo");
    // nix native_transitive does not repeat the bitflags repository URL.
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    judge("paired-repo-credential", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_paired_publish(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("paired-publish");
    let (inventory, records) = copy_claims(ctx, &dir);
    let from = "2026-09-10T01:11:38.500809Z";
    let to = "2015-01-15T08:35:24.901276Z";
    rewrite(&inventory, from, to, 1).expect("inv pub");
    rewrite(&records.join("bitflags-2.13.2.json"), from, to, 1).expect("rec pub");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    judge("paired-publish-crate-created", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_paired_adopt(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("paired-adopt");
    let (inventory, records) = copy_claims(ctx, &dir);
    let pubs = [
        ("2026-09-10T01:11:38.500809Z", 22i64),
        ("2026-09-16T17:36:11.278420Z", 15),
        ("2026-07-16T14:57:16.699789Z", 77),
        ("2026-07-21T21:33:28.018110Z", 72),
        ("2026-05-11T07:01:18.650221Z", 144),
    ];
    rewrite(&inventory, REAL_ADOPT, FORGED_ADOPT, 1).expect("inv adopt");
    for entry in fs::read_dir(&records).expect("records") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            rewrite(&path, REAL_ADOPT, FORGED_ADOPT, 1).expect("rec adopt");
        }
    }
    for (published, old_age) in pubs {
        let new_age = days_between(published, FORGED_ADOPT).expect("age");
        if new_age < 7 || new_age == old_age {
            return format!("paired-adopt\toracle\tfail\t2\t0\tno\tage formula produced {new_age} from {old_age}\t");
        }
        let from = format!("\"age_days_at_adoption\": {old_age}");
        let to = format!("\"age_days_at_adoption\": {new_age}");
        rewrite(&inventory, &from, &to, 1).expect("inv age");
        rewrite_records(&records, &from, &to).expect("rec age");
    }
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    judge("paired-adopt-not-lock-commit", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_paired_feature(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("paired-feature");
    let (inventory, records) = copy_claims(ctx, &dir);
    rewrite_first(
        &inventory,
        "      \"features_enabled\": [\n        \"default\"\n      ],",
        "      \"features_enabled\": [\n        \"default\",\n        \"forged-feature\"\n      ],",
    )
    .expect("inv feat");
    rewrite_first(
        &records.join("bitflags-2.13.2.json"),
        "    \"features_enabled\": [\n      \"default\"\n    ],",
        "    \"features_enabled\": [\n      \"default\",\n      \"forged-feature\"\n    ],",
    )
    .expect("rec feat");
    rewrite_first(
        &records.join("nix-0.31.3.json"),
        "        \"features_enabled\": [\n          \"default\"\n        ],\n        \"license_spdx\": \"MIT OR Apache-2.0\",",
        "        \"features_enabled\": [\n          \"default\",\n          \"forged-feature\"\n        ],\n        \"license_spdx\": \"MIT OR Apache-2.0\",",
    )
    .expect("nix feat");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    judge("paired-feature", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_paired_choice(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("paired-choice");
    let (inventory, records) = copy_claims(ctx, &dir);
    rewrite_first(&inventory, "\"choice\": \"MIT\"", "\"choice\": \"Apache-2.0\"").expect("inv choice");
    rewrite(
        &records.join("bitflags-2.13.2.json"),
        "\"dual_license_choice\": \"MIT\"",
        "\"dual_license_choice\": \"Apache-2.0\"",
        1,
    )
    .expect("rec choice");
    rewrite_first(
        &records.join("nix-0.31.3.json"),
        "\"dual_license_choice\": \"MIT\"",
        "\"dual_license_choice\": \"Apache-2.0\"",
    )
    .expect("nix choice");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    judge("paired-choice-apache", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_record_only_choice(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("record-only-choice");
    let records = copy_records(ctx, &dir);
    rewrite(
        &records.join("bitflags-2.13.2.json"),
        "\"dual_license_choice\": \"MIT\"",
        "\"dual_license_choice\": \"Apache-2.0\"",
        1,
    )
    .expect("rec only");
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("record-only-choice", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_obligation(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("obligation");
    let records = copy_records(ctx, &dir);
    rewrite(&records.join("bitflags-2.13.2.json"), MIT_DUTY, "n/a", 1).expect("duty");
    rewrite_first(&records.join("nix-0.31.3.json"), MIT_DUTY, "n/a").expect("nix duty");
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("paired-obligation-na", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_advisory(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("advisory");
    let (inventory, records) = copy_claims(ctx, &dir);
    let forged = "0000000000000000000000000000000000000000";
    rewrite(&inventory, RUSTSEC, forged, 1).expect("inv rustsec");
    rewrite_records(&records, RUSTSEC, forged).expect("rec rustsec");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    judge("paired-advisory-commit", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_cli_dotdot(ctx: &Ctx) -> String {
    let mut over = Overrides::default();
    over.records = Some(PathBuf::from("/tmp/oo-a109-probe/../oo-a109-probe"));
    judge("cli-dotdot", "fail", &invoke(ctx, &over, Duration::from_secs(10), &[]), false)
}

fn run_source_replace(ctx: &Ctx, name: &str, source: String, expect: &str) -> String {
    let dir = ctx.case_dir(name);
    let records = copy_records(ctx, &dir);
    let path = records.join("bitflags-2.13.2.json");
    rewrite(&path, BITFLAGS_SOURCE, &source, 1).unwrap_or_else(|err| panic!("{name}: {err}"));
    let mut over = Overrides::default();
    over.records = Some(records);
    judge(name, expect, &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_symlink(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("symlink");
    let (repo, licenses, retained) = stage_trees(ctx, &dir);
    let dest = licenses.join("bitflags-2.13.2/LICENSE-MIT");
    fs::remove_file(&dest).expect("rm");
    let real = ctx.licenses().join("bitflags-2.13.2/LICENSE-MIT");
    symlink(&real, &dest).expect("symlink");
    let mut over = Overrides::default();
    over.repo_root = Some(repo);
    over.licenses = Some(licenses);
    over.retained = Some(retained);
    judge("symlink-license", "fail-fast", &invoke(ctx, &over, Duration::from_secs(3), &[]), false)
}

fn run_fifo_license(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("fifo-license");
    let (repo, licenses, retained) = stage_trees(ctx, &dir);
    let dest = licenses.join("bitflags-2.13.2/LICENSE-MIT");
    fs::remove_file(&dest).expect("rm");
    let status = Command::new("/usr/bin/mkfifo").arg(&dest).status().expect("mkfifo");
    if !status.success() {
        return format!("fifo-license\toracle\tfail-fast\t2\t0\tno\tmkfifo failed\t");
    }
    let mut over = Overrides::default();
    over.repo_root = Some(repo);
    over.licenses = Some(licenses);
    over.retained = Some(retained);
    judge("fifo-license", "fail-fast", &invoke(ctx, &over, Duration::from_secs(3), &[]), false)
}

fn run_fifo_lock(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("fifo-lock");
    let fifo = dir.join("lock.fifo");
    let status = Command::new("/usr/bin/mkfifo").arg(&fifo).status().expect("mkfifo");
    if !status.success() {
        return format!("fifo-lock\toracle\tfail-fast\t2\t0\tno\tmkfifo failed\t");
    }
    let mut over = Overrides::default();
    over.helper_lock = Some(fifo);
    judge("fifo-lock", "fail-fast", &invoke(ctx, &over, Duration::from_secs(3), &[]), false)
}

fn run_hardlink(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("hardlink");
    let (repo, licenses, retained) = stage_trees(ctx, &dir);
    let dest = licenses.join("bitflags-2.13.2/LICENSE-MIT");
    let outside = dir.join("outside-license");
    fs::copy(&dest, &outside).expect("copy out");
    fs::remove_file(&dest).expect("rm");
    fs::hard_link(&outside, &dest).expect("hardlink");
    let nlink = fs::metadata(&dest).expect("meta").nlink();
    let mut over = Overrides::default();
    over.repo_root = Some(repo);
    over.licenses = Some(licenses);
    over.retained = Some(retained);
    let mut line = judge("hardlink-same-bytes", "observe", &invoke(ctx, &over, Duration::from_secs(20), &[]), false);
    line.push_str(&format!("\tnlink={nlink}"));
    line
}

fn run_duplicate_key(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("duplicate-key");
    let records = copy_records(ctx, &dir);
    let path = records.join("bitflags-2.13.2.json");
    insert_after_brace(&path, "\"audited\": false, ").expect("insert");
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("duplicate-key", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_duplicate_unicode_key(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("duplicate-unicode-key");
    let records = copy_records(ctx, &dir);
    let path = records.join("bitflags-2.13.2.json");
    insert_after_brace(&path, "\"\\u0061udited\": true, ").expect("insert");
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("duplicate-key-unicode", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_unknown_key(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("unknown-key");
    let records = copy_records(ctx, &dir);
    let path = records.join("bitflags-2.13.2.json");
    insert_after_brace(&path, "\"reviewer_note\": \"not-a-clearance\", ").expect("insert");
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("unknown-key", "observe", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_invalid_utf8(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("invalid-utf8");
    let records = copy_records(ctx, &dir);
    let path = records.join("bitflags-2.13.2.json");
    let mut bytes = fs::read(&path).expect("read");
    bytes.push(0xff);
    fs::write(&path, bytes).expect("write");
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("invalid-utf8", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_depth(ctx: &Ctx, name: &str, depth: usize) -> String {
    let dir = ctx.case_dir(name);
    let records = copy_records(ctx, &dir);
    let path = records.join("bitflags-2.13.2.json");
    let mut nested = String::from("\"deep\":");
    for _ in 0..depth {
        nested.push_str("{\"a\":");
    }
    nested.push('0');
    for _ in 0..depth {
        nested.push('}');
    }
    nested.push_str(", ");
    insert_after_brace(&path, &nested).expect("depth");
    let mut over = Overrides::default();
    over.records = Some(records);
    let timeout = if depth > 1000 { Duration::from_secs(8) } else { Duration::from_secs(20) };
    judge(name, "observe", &invoke(ctx, &over, timeout, &[]), false)
}

fn run_oversize(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("oversize");
    let (repo, licenses, retained) = stage_trees(ctx, &dir);
    let (inventory, records) = copy_claims(ctx, &dir);
    let dest = licenses.join("bitflags-2.13.2/LICENSE-MIT");
    let mut bytes = fs::read(&dest).expect("read");
    bytes.extend(std::iter::repeat(b' ').take(1_048_576));
    fs::write(&dest, &bytes).expect("write");
    fs::write(retained.join("bitflags-2.13.2/LICENSE-MIT"), &bytes).expect("retained");
    let new_sha = sha256_hex(&dest).expect("sha");
    rewrite(&inventory, BITFLAGS_MIT, &new_sha, 1).expect("inv sha");
    rewrite_records(&records, BITFLAGS_MIT, &new_sha).expect("rec sha");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    over.repo_root = Some(repo);
    over.licenses = Some(licenses);
    over.retained = Some(retained);
    judge("oversize-license", "fail", &invoke(ctx, &over, Duration::from_secs(30), &[]), false)
}

fn run_file_count(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("file-count");
    let records = copy_records(ctx, &dir);
    for index in 0..12 {
        fs::write(records.join(format!("extra-{index}.json")), "{}\n").expect("extra");
    }
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("record-file-count", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_nan(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("nan");
    let records = copy_records(ctx, &dir);
    rewrite(
        &records.join("bitflags-2.13.2.json"),
        "\"age_days_at_adoption\": 22",
        "\"age_days_at_adoption\": NaN",
        1,
    )
    .expect("nan");
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("nan-age", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_null_publication(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("null-publication");
    let records = copy_records(ctx, &dir);
    rewrite(
        &records.join("bitflags-2.13.2.json"),
        "\"publication_date\": \"2026-09-10T01:11:38.500809Z\"",
        "\"publication_date\": null",
        1,
    )
    .expect("null");
    let mut over = Overrides::default();
    over.records = Some(records);
    judge("null-publication", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_missing_apache(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("missing-apache");
    let (repo, licenses, retained) = stage_trees(ctx, &dir);
    fs::remove_file(licenses.join("libc-0.2.189/LICENSE-APACHE")).expect("rm");
    let mut over = Overrides::default();
    over.repo_root = Some(repo);
    over.licenses = Some(licenses);
    over.retained = Some(retained);
    judge("missing-apache", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_damaged_apache(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("damaged-apache");
    let (repo, licenses, retained) = stage_trees(ctx, &dir);
    let (inventory, records) = copy_claims(ctx, &dir);
    let dest = licenses.join("libc-0.2.189/LICENSE-APACHE");
    let text = fs::read_to_string(&dest).expect("read").replace("END OF TERMS AND CONDITIONS", "TERMS CONTINUE");
    if text.contains("END OF TERMS AND CONDITIONS") {
        return "damaged-apache-hash-aligned\toracle\tfail\t2\t0\tno\tphrase remained\t".to_string();
    }
    fs::write(&dest, &text).expect("write");
    fs::write(retained.join("libc-0.2.189/LICENSE-APACHE"), &text).expect("retained");
    let new_sha = sha256_hex(&dest).expect("sha");
    rewrite(&inventory, LIBC_APACHE, &new_sha, 1).expect("inv");
    rewrite_records(&records, LIBC_APACHE, &new_sha).expect("rec");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    over.repo_root = Some(repo);
    over.licenses = Some(licenses);
    over.retained = Some(retained);
    judge("damaged-apache-hash-aligned", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_missing_notice(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("missing-notice");
    let (repo, licenses, retained) = stage_trees(ctx, &dir);
    fs::remove_file(licenses.join("cfg_aliases-0.2.2/NOTICE")).expect("rm");
    let mut over = Overrides::default();
    over.repo_root = Some(repo);
    over.licenses = Some(licenses);
    over.retained = Some(retained);
    judge("missing-cfg-notice", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_damaged_notice(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("damaged-notice");
    let (repo, licenses, retained) = stage_trees(ctx, &dir);
    let (inventory, records) = copy_claims(ctx, &dir);
    let dest = licenses.join("cfg_aliases-0.2.2/NOTICE");
    let text = fs::read_to_string(&dest).expect("read").replace("tectonic_cfg_support", "removed_name");
    fs::write(&dest, &text).expect("notice");
    fs::write(licenses.join("cfg_aliases-0.2.2/NOTICES.md"), &text).expect("notices");
    fs::write(retained.join("cfg_aliases-0.2.2/NOTICES.md"), &text).expect("retained");
    let new_sha = sha256_hex(&dest).expect("sha");
    rewrite(&inventory, NOTICE_SHA, &new_sha, 2).expect("inv notice sha");
    rewrite_records(&records, NOTICE_SHA, &new_sha).expect("rec notice sha");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.records = Some(records);
    over.repo_root = Some(repo);
    over.licenses = Some(licenses);
    over.retained = Some(retained);
    judge("damaged-cfg-notice-hash-aligned", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_flag(ctx: &Ctx, name: &str, from: &str, to: &str) -> String {
    let dir = ctx.case_dir(name);
    let records = copy_records(ctx, &dir);
    rewrite(&records.join("bitflags-2.13.2.json"), from, to, 1).unwrap_or_else(|err| panic!("{name}: {err}"));
    let mut over = Overrides::default();
    over.records = Some(records);
    judge(name, "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_audit_missing(ctx: &Ctx) -> String {
    let mut over = Overrides::default();
    over.audit_json = Some(ctx.scratch.join("missing-audit.json"));
    judge("audit-missing", "fail", &invoke(ctx, &over, Duration::from_secs(10), &[]), false)
}

fn run_audit_flip(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("audit-flip");
    let path = dir.join("scan-audit.out");
    let mut bytes = fs::read(ctx.raw().join("scan-audit.out")).expect("read");
    if let Some(last) = bytes.last_mut() {
        *last ^= 0x01;
    }
    fs::write(&path, bytes).expect("write");
    let mut over = Overrides::default();
    over.audit_json = Some(path);
    judge("audit-hash-wrong", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_audit_unverified(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("audit-unverified");
    let (inventory, _) = copy_claims(ctx, &dir);
    let text = fs::read_to_string(ctx.raw().join("scan-audit.out")).expect("read");
    let trimmed = text.trim_end();
    let Some(body) = trimmed.strip_suffix('}') else {
        return "audit-status-unverified-aligned\toracle\tfail\t2\t0\tno\taudit did not end with object\t".to_string();
    };
    let forged = format!("{body},\"status\":\"unverified\"}}\n");
    let path = dir.join("scan-audit.out");
    fs::write(&path, &forged).expect("write");
    let new_sha = sha256_hex(&path).expect("sha");
    rewrite(&inventory, AUDIT_SHA, &new_sha, 1).expect("inv audit sha");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.audit_json = Some(path);
    judge(
        "audit-status-unverified-aligned",
        "fail",
        &invoke(ctx, &over, Duration::from_secs(20), &[]),
        false,
    )
}

fn run_deny_errors(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("deny-errors");
    let (inventory, _) = copy_claims(ctx, &dir);
    let text = fs::read_to_string(ctx.raw().join("scan-deny-advisories.err")).expect("read");
    let forged = text.replace("\"errors\":0", "\"errors\":1");
    let path = dir.join("scan-deny-advisories.err");
    fs::write(&path, forged).expect("write");
    let new_sha = sha256_hex(&path).expect("sha");
    rewrite(&inventory, DENY_SHA, &new_sha, 1).expect("inv deny sha");
    let mut over = Overrides::default();
    over.inventory = Some(inventory);
    over.deny_advisories = Some(path);
    judge("deny-errors-aligned", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_manifest_oneline(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("manifest-oneline");
    let path = dir.join("Cargo.toml");
    fs::write(&path, "license = \"Apache-2.0\"\n").expect("manifest");
    let mut over = Overrides::default();
    over.helper_manifest = Some(path);
    judge("manifest-oneline", "observe", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

fn run_path_hijack(ctx: &Ctx) -> String {
    let dir = ctx.case_dir("path-hijack");
    let evil = dir.join("evil");
    fs::create_dir_all(&evil).expect("evil");
    let marker = PathBuf::from("/tmp/oo-a109-sha-marker");
    let _ = fs::remove_file(&marker);
    let script = evil.join("sha256sum");
    fs::write(
        &script,
        format!("#!/bin/sh\necho invoked > {}\nprintf '%s\\n' {}\n", marker.display(), "0".repeat(64)),
    )
    .expect("script");
    let mut perms = fs::metadata(&script).expect("meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).expect("chmod");
    let path_env = format!("{}:/usr/bin", evil.display());
    let outcome = invoke(ctx, &Overrides::default(), Duration::from_secs(20), &[("PATH", path_env.as_str())]);
    let marker_hit = marker.exists();
    let _ = fs::remove_file(&marker);
    judge("path-hijack", "pass-nomarker", &outcome, marker_hit)
}

fn run_as_of_before(ctx: &Ctx) -> String {
    let mut over = Overrides::default();
    over.as_of = Some("2026-10-02T11:10:36Z".to_string());
    judge("as-of-before-adoption", "fail", &invoke(ctx, &over, Duration::from_secs(20), &[]), false)
}

struct Overrides {
    inventory: Option<PathBuf>,
    records: Option<PathBuf>,
    licenses: Option<PathBuf>,
    retained: Option<PathBuf>,
    repo_root: Option<PathBuf>,
    helper_lock: Option<PathBuf>,
    helper_manifest: Option<PathBuf>,
    audit_json: Option<PathBuf>,
    deny_advisories: Option<PathBuf>,
    as_of: Option<String>,
}

impl Default for Overrides {
    fn default() -> Self {
        Self {
            inventory: None,
            records: None,
            licenses: None,
            retained: None,
            repo_root: None,
            helper_lock: None,
            helper_manifest: None,
            audit_json: None,
            deny_advisories: None,
            as_of: None,
        }
    }
}

fn invoke(ctx: &Ctx, over: &Overrides, timeout: Duration, env: &[(&str, &str)]) -> Outcome {
    let raw = ctx.raw();
    let inventory = over.inventory.clone().unwrap_or_else(|| ctx.inventory());
    let records = over.records.clone().unwrap_or_else(|| ctx.records());
    let licenses = over.licenses.clone().unwrap_or_else(|| ctx.licenses());
    let retained = over.retained.clone().unwrap_or_else(|| raw.join("upstream-licenses"));
    let repo = over.repo_root.clone().unwrap_or_else(|| ctx.archive.clone());
    let lock = over.helper_lock.clone().unwrap_or_else(|| ctx.helper_lock());
    let manifest = over.helper_manifest.clone().unwrap_or_else(|| ctx.helper_manifest());
    let audit = over.audit_json.clone().unwrap_or_else(|| raw.join("scan-audit.out"));
    let deny_adv = over.deny_advisories.clone().unwrap_or_else(|| raw.join("scan-deny-advisories.err"));
    let as_of = over.as_of.clone().unwrap_or_else(|| AS_OF.to_string());
    let mut command = Command::new(&ctx.bin);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("--inventory")
        .arg(inventory)
        .arg("--records")
        .arg(records)
        .arg("--licenses")
        .arg(licenses)
        .arg("--retained-licenses")
        .arg(retained)
        .arg("--helper-lock")
        .arg(lock)
        .arg("--helper-manifest")
        .arg(manifest)
        .arg("--deny-config")
        .arg(ctx.archive.join("ci/deny.toml"))
        .arg("--metadata")
        .arg(raw.join("helper-metadata.out"))
        .arg("--age")
        .arg(raw.join("age.txt"))
        .arg("--audit-json")
        .arg(audit)
        .arg("--deny-advisories")
        .arg(deny_adv)
        .arg("--advisory-rev")
        .arg(raw.join("advisory-rev.txt"))
        .arg("--version-meta")
        .arg(raw.join("meta"))
        .arg("--repo-root")
        .arg(repo)
        .arg("--as-of")
        .arg(as_of);
    for (key, value) in env {
        command.env(key, value);
    }
    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Outcome {
                exit: 127,
                timed_out: false,
                millis: 0,
                stdout: String::new(),
                stderr: err.to_string(),
            };
        }
    };
    let mut stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let mut out = String::new();
                let mut err = String::new();
                let _ = stdout.read_to_string(&mut out);
                let _ = stderr.read_to_string(&mut err);
                return Outcome {
                    exit: 124,
                    timed_out: true,
                    millis: started.elapsed().as_millis(),
                    stdout: out,
                    stderr: err,
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => {
                return Outcome {
                    exit: 127,
                    timed_out: false,
                    millis: started.elapsed().as_millis(),
                    stdout: String::new(),
                    stderr: err.to_string(),
                };
            }
        }
    };
    let mut out = String::new();
    let mut err = String::new();
    let _ = stdout.read_to_string(&mut out);
    let _ = stderr.read_to_string(&mut err);
    let exit = status.code().unwrap_or_else(|| 128 + status.signal_fallback());
    Outcome {
        exit,
        timed_out: false,
        millis: started.elapsed().as_millis(),
        stdout: out,
        stderr: err,
    }
}

trait SignalFallback {
    fn signal_fallback(&self) -> i32;
}

impl SignalFallback for std::process::ExitStatus {
    fn signal_fallback(&self) -> i32 {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            return self.signal().unwrap_or(0);
        }
        #[allow(unreachable_code)]
        0
    }
}

fn judge(name: &str, expect: &str, outcome: &Outcome, marker: bool) -> String {
    let class = match expect {
        "fail" => {
            if outcome.exit != 0 && !outcome.timed_out {
                "hit"
            } else {
                "miss"
            }
        }
        "fail-fast" => {
            if outcome.exit != 0 && !outcome.timed_out && outcome.millis < 2_000 {
                "hit"
            } else {
                "miss"
            }
        }
        "pass" => {
            if outcome.exit == 0 && !outcome.timed_out {
                "hit"
            } else {
                "miss"
            }
        }
        "pass-nomarker" => {
            if outcome.exit == 0 && !outcome.timed_out && !marker {
                "hit"
            } else {
                "miss"
            }
        }
        _ => "observe",
    };
    let stdout = one_line(&outcome.stdout);
    let stderr = one_line(&outcome.stderr);
    format!(
        "{name}\t{class}\t{expect}\t{}\t{}\t{}\t{stdout}\t{stderr}",
        outcome.exit,
        outcome.millis,
        if marker { "yes" } else { "no" }
    )
}

fn one_line(text: &str) -> String {
    let flat = text.replace(['\n', '\r', '\t'], " ");
    if flat.len() > 360 {
        format!("{}…", &flat[..360])
    } else {
        flat
    }
}

impl Ctx {
    fn case_dir(&self, name: &str) -> PathBuf {
        let dir = self.scratch.join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("case");
        dir
    }

    fn records(&self) -> PathBuf {
        self.archive.join("testing/harness/containment-supervisor/provenance/audit-records")
    }

    fn licenses(&self) -> PathBuf {
        self.archive.join("testing/harness/containment-supervisor/provenance/licenses")
    }

    fn inventory(&self) -> PathBuf {
        self.archive.join("testing/harness/helper-evidence-check/fixtures/inventory.json")
    }

    fn raw(&self) -> PathBuf {
        self.archive.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw")
    }

    fn helper_lock(&self) -> PathBuf {
        self.archive.join("testing/harness/containment-supervisor/Cargo.lock")
    }

    fn helper_manifest(&self) -> PathBuf {
        self.archive.join("testing/harness/containment-supervisor/Cargo.toml")
    }
}

fn copy_records(ctx: &Ctx, dir: &Path) -> PathBuf {
    let dest = dir.join("records");
    copy_tree(&ctx.records(), &dest);
    dest
}

fn copy_claims(ctx: &Ctx, dir: &Path) -> (PathBuf, PathBuf) {
    let inventory = dir.join("inventory.json");
    fs::copy(ctx.inventory(), &inventory).expect("inventory");
    let records = copy_records(ctx, dir);
    (inventory, records)
}

fn stage_trees(ctx: &Ctx, dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let repo = dir.join("repo");
    let licenses = repo.join("testing/harness/containment-supervisor/provenance/licenses");
    copy_tree(&ctx.licenses(), &licenses);
    let retained = dir.join("retained");
    copy_tree(&ctx.raw().join("upstream-licenses"), &retained);
    (repo, licenses, retained)
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("dst");
    for entry in fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("entry");
        let meta = fs::symlink_metadata(entry.path()).expect("meta");
        let to = dst.join(entry.file_name());
        if meta.file_type().is_symlink() {
            panic!("refusing to copy a symlink from {}", entry.path().display());
        } else if meta.is_dir() {
            copy_tree(&entry.path(), &to);
        } else if meta.is_file() {
            fs::copy(entry.path(), &to).expect("copy");
        }
    }
}

fn rewrite(path: &Path, from: &str, to: &str, expect: usize) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let count = text.matches(from).count();
    if count != expect {
        return Err(format!("{} has {count} matches for {from}, want {expect}", path.display()));
    }
    fs::write(path, text.replace(from, to)).map_err(|err| err.to_string())
}

fn rewrite_first(path: &Path, from: &str, to: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let Some(index) = text.find(from) else {
        return Err(format!("missing pattern in {}", path.display()));
    };
    let mut out = String::with_capacity(text.len() + to.len());
    out.push_str(&text[..index]);
    out.push_str(to);
    out.push_str(&text[index + from.len()..]);
    fs::write(path, out).map_err(|err| err.to_string())
}

fn rewrite_records(dir: &Path, from: &str, to: &str) -> Result<(), String> {
    let mut touched = 0;
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let count = text.matches(from).count();
        if count == 0 {
            continue;
        }
        fs::write(&path, text.replace(from, to)).map_err(|err| err.to_string())?;
        touched += count;
    }
    if touched == 0 {
        return Err(format!("no record contained {from}"));
    }
    Ok(())
}

fn insert_after_brace(path: &Path, extra: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let Some(index) = text.find('{') else {
        return Err("no object".to_string());
    };
    let mut out = String::with_capacity(text.len() + extra.len());
    out.push_str(&text[..=index]);
    out.push_str(extra);
    out.push_str(&text[index + 1..]);
    fs::write(path, out).map_err(|err| err.to_string())
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    let output = Command::new("/usr/bin/sha256sum").arg(path).output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err("sha256sum failed".to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let token = text.split_whitespace().next().ok_or("empty sha")?;
    if token.len() != 64 {
        return Err("bad sha".to_string());
    }
    Ok(token.to_string())
}

fn assert_age_vectors() -> Result<(), String> {
    let vectors = [
        ("2026-09-16T17:36:11.278420Z", 15i64),
        ("2026-09-10T01:11:38.500809Z", 22),
        ("2026-07-21T21:33:28.018110Z", 72),
        ("2026-07-16T14:57:16.699789Z", 77),
        ("2026-05-11T07:01:18.650221Z", 144),
    ];
    for (published, age) in vectors {
        let got = days_between(published, REAL_ADOPT)?;
        if got != age {
            return Err(format!("age vector {published} got {got} want {age}"));
        }
    }
    Ok(())
}

fn days_between(published: &str, adopted: &str) -> Result<i64, String> {
    let left = parse_rfc3339(published)?;
    let right = parse_rfc3339(adopted)?;
    Ok(completed_days(right - left))
}

fn completed_days(elapsed: i128) -> i64 {
    (elapsed / DAY_NS) as i64
}

fn parse_rfc3339(value: &str) -> Result<i128, String> {
    let (body, zone) = if let Some(rest) = value.strip_suffix('Z').or_else(|| value.strip_suffix('z')) {
        (rest, true)
    } else if let Some(rest) = value.strip_suffix("+00:00") {
        (rest, true)
    } else {
        (value, false)
    };
    if !zone || body.len() < 19 {
        return Err("bad time".to_string());
    }
    let (date, time) = body.split_once('T').or_else(|| body.split_once('t')).ok_or("bad time")?;
    let mut dates = date.split('-');
    let year: i32 = dates.next().ok_or("bad time")?.parse().map_err(|_| "bad time")?;
    let month: u32 = dates.next().ok_or("bad time")?.parse().map_err(|_| "bad time")?;
    let day: u32 = dates.next().ok_or("bad time")?.parse().map_err(|_| "bad time")?;
    if dates.next().is_some() || !valid_ymd(year, month, day) {
        return Err("bad time".to_string());
    }
    let (clock, frac) = match time.split_once('.') {
        Some((clock, frac)) => (clock, frac),
        None => (time, ""),
    };
    if clock.len() != 8 {
        return Err("bad time".to_string());
    }
    let hour: u32 = clock[0..2].parse().map_err(|_| "bad time")?;
    let minute: u32 = clock[3..5].parse().map_err(|_| "bad time")?;
    let second: u32 = clock[6..8].parse().map_err(|_| "bad time")?;
    if &clock[2..3] != ":" || &clock[5..6] != ":" || hour > 23 || minute > 59 || second > 59 {
        return Err("bad time".to_string());
    }
    if !frac.is_empty() && (frac.len() > 9 || !frac.bytes().all(|byte| byte.is_ascii_digit())) {
        return Err("bad time".to_string());
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
