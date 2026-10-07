//! Compare a fresh restore with the public formulas and the frozen checkpoint literals.

use std::fs;
use std::path::{Path, PathBuf};

use crate::gold::{self, account_line, entry_line, party_line, ACCOUNT_ROWS, ENTRY_ROWS, PARTY_COMMENT, PARTY_ROWS};
use crate::sha256::{self, hex, hash};

const FAMILIES: &[&str] = &[
    "git_mirror",
    "object_store",
    "secret_store",
    "audit_anchors",
    "valkey_buffer",
    "jobs_control",
    "gateway_tokens",
    "file_hosting",
    "ontology_publication",
    "agent_pipeline_schema",
    "entity_materialized_tables",
    "cluster_roles",
    "wal_archive",
    "pgbackrest",
    "enum_calendar_same_database",
];

pub struct Report {
    pub failed: Vec<String>,
    pub lines: Vec<String>,
}

pub fn run(root: &Path) -> Report {
    let mut report = Report { failed: Vec::new(), lines: Vec::new() };
    if !sha256::self_check() {
        report.fail("sha256_self_check");
    } else {
        report.pass("sha256_self_check");
    }
    compare_shop(&mut report, root);
    compare_probes(&mut report, root);
    compare_manifest(&mut report, root);
    compare_reports(&mut report, root);
    compare_schema_probe(&mut report, root);
    report.lines.insert(0, format!("failed={}", report.failed.len()));
    report.lines.insert(1, format!("ok={}", report.failed.is_empty()));
    report
}

fn compare_shop(report: &mut Report, root: &Path) {
    let shop = root.join("shop");
    check_rows(report, &shop.join("party.copy"), PARTY_ROWS, party_line, "shop_party");
    check_rows(report, &shop.join("account.copy"), ACCOUNT_ROWS, account_line, "shop_account");
    check_rows(report, &shop.join("entry.copy"), ENTRY_ROWS, entry_line, "shop_entry");
}

fn check_rows(report: &mut Report, path: &Path, rows: i64, line: fn(i64) -> String, name: &str) {
    match gold::compare_generated(path, rows, line) {
        Ok(()) => {
            if let Ok(bytes) = fs::read(path) {
                report.note(&format!("{name}_sha256"), &hex(&hash(&bytes)));
                report.note(&format!("{name}_bytes"), &bytes.len().to_string());
            }
            report.pass(name);
        }
        Err(detail) => report.fail_detail(name, &detail),
    }
}

fn compare_probes(report: &mut Report, root: &Path) {
    let probes = root.join("probes");
    expect(&probes, report, "party.txt", "2000", "probe_party");
    expect(&probes, report, "account.txt", "8000", "probe_account");
    expect(&probes, report, "entry.txt", "90000", "probe_entry");
    expect(&probes, report, "orphans.txt", "0", "probe_orphans");
    expect(&probes, report, "null_display.txt", "null", "probe_null_display");
    expect(&probes, report, "null_amounts.txt", "3104", "probe_null_amounts");
    expect(&probes, report, "comment.txt", PARTY_COMMENT, "probe_comment");
    expect(&probes, report, "watermark.txt", "100000|txn-a356-1|txn-a356-2|100001", "probe_watermark");
    expect(&probes, report, "closure.txt", "2026-03-31|2026-03", "probe_closure");
    expect(&probes, report, "published.txt", "2026-03-31|unique", "probe_published");
    expect(&probes, report, "session.txt", "3", "probe_session");
    expect(&probes, report, "session_id.txt", "sess-a356-1", "probe_session_id");
    expect(&probes, report, "enum.txt", "confirmed|approved", "probe_enum");
    expect(&probes, report, "negative.txt", "1", "probe_negative");
    expect(&probes, report, "keys.txt", "k-0001|member-a|1\nk-0002|member-b|2", "probe_keys");
    expect(&probes, report, "amounts.txt", &gold::amount_probe(), "probe_amounts");
    expect(&probes, report, "post_party_restore.txt", "0", "probe_post_party_restore");
    expect(&probes, report, "post_key_restore.txt", "0", "probe_post_key_restore");
    expect(&probes, report, "post_party_source.txt", "1", "probe_post_party_source");
    expect(&probes, report, "post_key_source.txt", "1", "probe_post_key_source");
    expect(&probes, report, "source_party.txt", "2001", "probe_source_party");
    expect(&probes, report, "source_keys.txt", "3", "probe_source_keys");
    expect(&probes, report, "missing_dbs.txt", "none", "probe_missing_dbs");
    expect(&probes, report, "corrupt_dump_dbs.txt", "none", "probe_corrupt_dump_dbs");
    expect(&probes, report, "corrupt_manifest_dbs.txt", "none", "probe_corrupt_manifest_dbs");
    expect(&probes, report, "existing_entry.txt", "90000", "probe_existing_entry");
    expect(&probes, report, "existing_party.txt", "p00001|n00001", "probe_existing_party");
    expect(&probes, report, "existing_post.txt", "0", "probe_existing_post");
    let schemas = root.join("schemas");
    expect_hash(&probes, report, "cdc_sha.txt", &schemas.join("cdc.sql"), "probe_cdc_sql");
    expect_hash(&probes, report, "session_sha.txt", &schemas.join("session.sql"), "probe_session_sql");
    expect_hash(&probes, report, "calendar_sha.txt", &schemas.join("calendar.sql"), "probe_calendar_sql");
    expect_hash(&probes, report, "enum_sha.txt", &schemas.join("enum.sql"), "probe_enum_sql");
}

fn compare_manifest(report: &mut Report, root: &Path) {
    let manifest_path = root.join("manifest.json");
    let side_path = root.join("manifest.sha256");
    let Ok(body) = fs::read(&manifest_path) else {
        report.fail("manifest_bytes");
        return;
    };
    let digest = hex(&hash(&body));
    let side = fs::read_to_string(&side_path).unwrap_or_default();
    let side_digest = side.split_whitespace().next().unwrap_or("");
    if side_digest == digest {
        report.pass("manifest_sidecar");
    } else {
        report.fail_detail("manifest_sidecar", &format!("side={side_digest} body={digest}"));
    }
    let text = String::from_utf8_lossy(&body);
    let history = field(&text, "history_insufficient");
    let quiescent = field(&text, "quiescent");
    let wal = field(&text, "wal_moved");
    let before = field(&text, "lsn_before");
    let after = field(&text, "lsn_after");
    let entry_before = field(&text, "entry_count_before");
    let entry_after = field(&text, "entry_count_after");
    report.note("lsn_before", &before);
    report.note("lsn_after", &after);
    report.note("history_insufficient", &history);
    report.note("quiescent", &quiescent);
    report.note("wal_moved", &wal);
    if entry_before == "90000" && entry_after == "90000" {
        report.pass("manifest_entry_count");
    } else {
        report.fail_detail("manifest_entry_count", &format!("{entry_before}/{entry_after}"));
    }
    let quiescent_ok = (quiescent == "true" && history == "false") || (quiescent == "false" && history == "true");
    if quiescent_ok {
        report.pass("manifest_quiescent_flag");
    } else {
        report.fail_detail("manifest_quiescent_flag", &format!("quiescent={quiescent} history={history}"));
    }
    let moved = before != after && !before.is_empty() && !after.is_empty();
    if (wal == "true" && moved) || (wal == "false" && !moved && !before.is_empty()) {
        report.pass("manifest_wal_flag");
    } else {
        report.fail_detail("manifest_wal_flag", &format!("wal={wal} before={before} after={after}"));
    }
    if history == "false" && quiescent == "true" {
        report.pass("manifest_history_sufficient");
    } else {
        report.fail_detail("manifest_history_sufficient", &format!("history={history}"));
    }
    bind_dumps(report, root, &text);
    let last_write = field(&text, "last_write_unix");
    let finished = field(&text, "backup_finished_unix");
    report.note("last_write_unix", &last_write);
    report.note("backup_finished_unix", &finished);
    match (last_write.parse::<u64>(), finished.parse::<u64>()) {
        (Ok(start), Ok(end)) if end >= start => {
            report.note("rpo_from_manifest", &(end - start).to_string());
            report.pass("manifest_rpo_stamps");
        }
        _ => report.fail_detail("manifest_rpo_stamps", &format!("{last_write}->{finished}")),
    }
}

fn bind_dumps(report: &mut Report, root: &Path, manifest: &str) {
    let Some(section) = section(manifest, "dumps") else {
        report.fail("manifest_dump_section");
        return;
    };
    for name in ["oo_a356_agent.dump", "oo_a356_control.dump"] {
        let Some(expect) = named_sha(&section, name) else {
            report.fail_detail("manifest_dump_name", name);
            continue;
        };
        let path = root.join("dumps").join(name);
        match fs::read(&path) {
            Ok(bytes) => {
                let got = hex(&hash(&bytes));
                report.note(&format!("{name}_bytes"), &bytes.len().to_string());
                if got == expect {
                    report.pass(&format!("dump_bind_{name}"));
                } else {
                    report.fail_detail(&format!("dump_bind_{name}"), &format!("manifest={expect} file={got}"));
                }
            }
            Err(err) => report.fail_detail(&format!("dump_bind_{name}"), &err.to_string()),
        }
    }
}

fn compare_reports(report: &mut Report, root: &Path) {
    let reports = root.join("reports");
    let happy_path = reports.join("restore-readiness.json");
    let Ok(happy) = fs::read_to_string(&happy_path) else {
        report.fail("happy_report");
        return;
    };
    expect_field(report, &happy, "replay_ready", "true", "happy_ready");
    expect_field(report, &happy, "offset_published", "100000", "happy_offset");
    expect_field(report, &happy, "offset_min_used", "false", "happy_offset_min");
    expect_field(report, &happy, "history_insufficient", "false", "happy_history");
    expect_field(report, &happy, "pitr_ready", "false", "happy_pitr");
    expect_field(report, &happy, "all_state_certified", "false", "happy_all_state");
    expect_field(report, &happy, "production_slo_certified", "false", "happy_slo");
    expect_field(report, &happy, "production_ops", "false", "happy_ops");
    expect_field(report, &happy, "parent_acceptance", "false", "happy_parent");
    expect_field(report, &happy, "w04_gate", "open", "happy_w04");
    expect_field(report, &happy, "post_cut_absent_on_restore", "true", "happy_post_cut_absent");
    expect_field(report, &happy, "post_cut_present_on_source", "true", "happy_post_cut_source");
    expect_field(report, &happy, "key_reused", "false", "happy_key_reused");
    expect_field(report, &happy, "key_count", "2", "happy_key_count");
    let rpo = field(&happy, "rpo_window_seconds");
    let rto = field(&happy, "rto_seconds");
    report.note("reported_rpo_window_seconds", &rpo);
    report.note("reported_rto_seconds", &rto);
    let from_stamps = field_note(report, "rpo_from_manifest");
    if !rpo.is_empty() && rpo != "null" && rpo == from_stamps {
        report.pass("rpo_matches_stamps");
    } else {
        report.fail_detail("rpo_matches_stamps", &format!("report={rpo} stamps={from_stamps}"));
    }
    if rto.parse::<u64>().is_ok() {
        report.pass("rto_is_a_measurement");
    } else {
        report.fail_detail("rto_is_a_measurement", &rto);
    }
    if happy.contains("\"pitr_ready\": false") {
        report.pass("wal_does_not_certify_pitr");
    } else {
        report.fail("wal_does_not_certify_pitr");
    }
    for family in FAMILIES {
        if happy.contains(family) {
            report.pass(&format!("family_{family}"));
        } else {
            report.fail(&format!("family_{family}"));
        }
    }
    for (file, reason) in [
        ("restore-missing-readiness.json", "manifest_absent"),
        ("restore-corrupt-dump-readiness.json", "dump_hash_mismatch"),
        ("restore-corrupt-manifest-readiness.json", "manifest_hash_mismatch"),
        ("restore-existing-readiness.json", "restore_database_not_fresh"),
    ] {
        match fs::read_to_string(reports.join(file)) {
            Ok(body) => {
                expect_field(report, &body, "replay_ready", "false", &format!("{file}_ready"));
                expect_field(report, &body, "reason", reason, &format!("{file}_reason"));
                expect_field(report, &body, "offset_published", "null", &format!("{file}_offset"));
                expect_field(report, &body, "all_state_certified", "false", &format!("{file}_all_state"));
                expect_field(report, &body, "parent_acceptance", "false", &format!("{file}_parent"));
                if body.contains("\"replay_ready\": true") {
                    report.fail(&format!("{file}_no_true_ready"));
                } else {
                    report.pass(&format!("{file}_no_true_ready"));
                }
            }
            Err(err) => report.fail_detail(file, &err.to_string()),
        }
    }
}

fn compare_schema_probe(report: &mut Report, root: &Path) {
    let exit_text = fs::read_to_string(root.join("schema-probe/exit.txt")).unwrap_or_default();
    let stderr = fs::read_to_string(root.join("schema-probe/stderr.txt")).unwrap_or_default();
    let exit_text = exit_text.trim();
    report.note("schema_probe_exit", exit_text);
    if exit_text != "0" && !exit_text.is_empty() && stderr.contains("oo_dict") {
        report.pass("enum_calendar_same_database_collision");
    } else {
        report.fail_detail(
            "enum_calendar_same_database_collision",
            &format!("exit={exit_text} stderr_has_oo_dict={}", stderr.contains("oo_dict")),
        );
    }
}

fn expect(dir: &Path, report: &mut Report, name: &str, wanted: &str, check: &str) {
    match fs::read_to_string(dir.join(name)) {
        Ok(text) if text.trim_end_matches('\n') == wanted => report.pass(check),
        Ok(text) => report.fail_detail(check, &format!("got {}", text.trim_end_matches('\n'))),
        Err(err) => report.fail_detail(check, &err.to_string()),
    }
}

fn expect_hash(dir: &Path, report: &mut Report, name: &str, file: &Path, check: &str) {
    let Ok(bytes) = fs::read(file) else {
        report.fail_detail(check, "schema file missing");
        return;
    };
    let digest = hex(&hash(&bytes));
    expect(dir, report, name, &digest, check);
}

fn expect_field(report: &mut Report, body: &str, key: &str, wanted: &str, check: &str) {
    let got = field(body, key);
    if got == wanted {
        report.pass(check);
    } else {
        report.fail_detail(check, &format!("{key}={got}"));
    }
}

fn field(text: &str, key: &str) -> String {
    let pattern = format!("\"{key}\": ");
    let Some(start) = text.find(&pattern) else {
        return String::new();
    };
    let rest = &text[start + pattern.len()..];
    if let Some(value) = rest.strip_prefix("null")
        && (value.starts_with([',', '\n', ' ', '}']) || value.is_empty())
    {
        return "null".to_string();
    }
    if rest.starts_with("true") {
        return "true".to_string();
    }
    if rest.starts_with("false") {
        return "false".to_string();
    }
    if let Some(stripped) = rest.strip_prefix('"') {
        let mut end = 0;
        let bytes = stripped.as_bytes();
        while end < bytes.len() {
            if bytes[end] == b'\\' {
                end += 2;
                continue;
            }
            if bytes[end] == b'"' {
                return stripped[..end].to_string();
            }
            end += 1;
        }
        return String::new();
    }
    let end = rest.find([',', '\n', '}']).unwrap_or(rest.len());
    rest[..end].trim().to_string()
}

fn section(text: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\": [");
    let start = text.find(&marker)? + marker.len();
    let rest = &text[start..];
    let end = rest.find(']')?;
    Some(rest[..end].to_string())
}

fn named_sha(section: &str, name: &str) -> Option<String> {
    let needle = format!("\"name\": \"{name}\"");
    let start = section.find(&needle)?;
    let rest = &section[start..];
    let key = "\"sha256\": \"";
    let sha_at = rest.find(key)? + key.len();
    let tail = &rest[sha_at..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

fn field_note(report: &Report, key: &str) -> String {
    let prefix = format!("{key}=");
    report
        .lines
        .iter()
        .find_map(|line| line.strip_prefix(&prefix).map(str::to_string))
        .unwrap_or_default()
}

impl Report {
    fn pass(&mut self, name: &str) {
        self.lines.push(format!("pass {name}"));
    }

    fn fail(&mut self, name: &str) {
        self.failed.push(name.to_string());
        self.lines.push(format!("fail {name}"));
    }

    fn fail_detail(&mut self, name: &str, detail: &str) {
        self.failed.push(name.to_string());
        let flat = detail.replace('\n', " ");
        self.lines.push(format!("fail {name} {flat}"));
    }

    fn note(&mut self, key: &str, value: &str) {
        self.lines.push(format!("{key}={value}"));
    }
}

pub fn write(report: &Report, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("verdict dir: {err}"))?;
    }
    let mut body = report.lines.join("\n");
    body.push('\n');
    fs::write(path, body).map_err(|err| format!("verdict: {err}"))?;
    Ok(())
}

pub fn root_from_arg(arg: &str) -> PathBuf {
    PathBuf::from(arg)
}

#[cfg(test)]
mod tests {
    use super::{field, named_sha, section};
    use crate::gold::{self, write_rows, ACCOUNT_ROWS, ENTRY_ROWS, PARTY_ROWS};
    use crate::sha256::{hash, hex};
    use std::fs;

    #[test]
    fn json_helpers_read_the_author_shape() {
        let text = "{\n  \"offset_published\": \"100000\",\n  \"offset_min_used\": false,\n  \"replay_ready\": true,\n  \"rpo_window_seconds\": 0,\n  \"dumps\": [\n    {\"name\": \"oo_a356_agent.dump\", \"sha256\": \"abc\"}\n  ]\n}\n";
        assert_eq!(field(text, "offset_published"), "100000");
        assert_eq!(field(text, "offset_min_used"), "false");
        assert_eq!(field(text, "rpo_window_seconds"), "0");
        let dumps = section(text, "dumps").unwrap();
        assert_eq!(named_sha(&dumps, "oo_a356_agent.dump").as_deref(), Some("abc"));
    }

    #[test]
    fn generated_files_compare_equal() {
        let dir = std::env::temp_dir().join(format!("a361-oracle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        write_rows(&dir.join("party.copy"), PARTY_ROWS, gold::party_line).unwrap();
        write_rows(&dir.join("account.copy"), ACCOUNT_ROWS, gold::account_line).unwrap();
        write_rows(&dir.join("entry.copy"), ENTRY_ROWS, gold::entry_line).unwrap();
        gold::compare_generated(&dir.join("entry.copy"), ENTRY_ROWS, gold::entry_line).unwrap();
        let bytes = fs::read(dir.join("party.copy")).unwrap();
        assert_eq!(hex(&hash(&bytes)).len(), 64);
        let _ = fs::remove_dir_all(&dir);
    }
}
