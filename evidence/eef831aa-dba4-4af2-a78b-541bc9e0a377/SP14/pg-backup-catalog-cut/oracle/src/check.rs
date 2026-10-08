//! File checks for one recorded catalog cut.
//! A refused sequence fence is a recorded cut, not a certificate.
//! Shop gold hashes are the unchanged a361 values when the full gold set is present.

use std::fs;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use crate::sha256;

const MARKER: &str = "cut-a356-1|100000";
const COUNT: &str = "90000";
const ORIGINAL_AMOUNT: &str = "1234567890.1234";
const UPDATED_AMOUNT: &str = "1.0001";
const SHOP_PARTY: &str = "62230764135b6d717fe792dffa77d776d0e7cdc7ff2123fd049ce4f56f06fa5b";
const SHOP_ACCOUNT: &str = "fef89f25c29e9ed5b9c36d0bf37f3056433c33ade60b90978d4f91e7c0c7a958";
const SHOP_ENTRY: &str = "a11369572ab597c2211d47df03b92c2a1b2da88451bfb5439c1d7ecf2cb10450";

pub fn check(root: &Path) -> Result<(), String> {
    if !sha256::self_check() {
        return Err("sha256 self check failed".to_string());
    }
    let manifest_bytes = fs::read(root.join("manifest.json")).map_err(|err| format!("manifest: {err}"))?;
    let manifest = String::from_utf8(manifest_bytes.clone()).map_err(|_| "manifest is not utf-8".to_string())?;
    let side = fs::read_to_string(root.join("manifest.sha256")).map_err(|err| format!("manifest sidecar: {err}"))?;
    let want = side.split_whitespace().next().unwrap_or("");
    if want != sha256::hex(&sha256::hash(&manifest_bytes)) {
        return Err("manifest sidecar mismatch".to_string());
    }
    let catalog_bytes = fs::read(root.join("catalog.txt")).map_err(|err| format!("catalog: {err}"))?;
    let catalog_sha = sha256::hex(&sha256::hash(&catalog_bytes));
    require_contains(&manifest, &format!("\"catalog_sha256\": \"{catalog_sha}\""))?;
    let (control_snap, agent_snap) = snapshot_pair(&manifest)?;
    let held = manifest.lines().any(|line| line == "    \"held\": true,");
    let refused = manifest.lines().any(|line| line == "    \"held\": false,");
    if held == refused {
        return Err("sequence fence held bit is ambiguous".to_string());
    }
    if held {
        require_line(&manifest, "  \"cut_mode\": \"writers_blocked\",")?;
        require_line(&manifest, "  \"history_insufficient\": false,")?;
        require_line(&manifest, "  \"quiescent\": true,")?;
        require_contains(&manifest, "\"sqlstate\": \"00000\"")?;
    } else {
        require_line(&manifest, "  \"cut_mode\": \"unproven\",")?;
        require_line(&manifest, "  \"history_insufficient\": true,")?;
        require_line(&manifest, "  \"quiescent\": false,")?;
        if manifest.contains("\"sqlstate\": \"00000\"") {
            return Err("refused fence records a granted sequence lock".to_string());
        }
        require_contains(&manifest, "\"method\": \"lock_table_share\"")?;
        require_contains(&manifest, "oo_key_fixture.cut_seq")?;
        require_contains(&manifest, "shop.cut_seq")?;
    }
    if !manifest.contains("\"probe_rejected\": true") || !manifest.contains("\"lock_mode\": \"SHARE\"") {
        return Err("writer blocks are missing".to_string());
    }
    match_dump(&manifest, root, "oo_a356_agent.dump")?;
    match_dump(&manifest, root, "oo_a356_control.dump")?;
    let flags = fs::read_to_string(root.join("probes").join("dump-flags.txt")).map_err(|err| format!("dump flags: {err}"))?;
    require_flag(&flags, "oo_a356_control", &control_snap)?;
    require_flag(&flags, "oo_a356_agent", &agent_snap)?;
    if flags.contains("serializable-deferrable") || flags.contains("deferrable=present") {
        return Err("pg_dump kept serializable-deferrable".to_string());
    }
    let gold_hash = sha256::hash_path(&root.join("gold").join("shop-entry.copy"))?;
    let entry = manifest
        .lines()
        .find(|line| line.contains("\"relation\": \"shop.entry\""))
        .ok_or("shop.entry digest is missing")?;
    if field(entry, "gold_sha256") != Some(gold_hash.as_str()) {
        return Err("shop.entry digest is not bound to the gold file".to_string());
    }
    if full_gold(root) {
        bind_known_shop(root)?;
    }
    let gold = fs::read_to_string(root.join("gold").join("shop-entry.copy")).map_err(|err| format!("gold entry: {err}"))?;
    if !gold_amount_is(&gold, ORIGINAL_AMOUNT) {
        return Err("gold entry 3 does not have the pre-update amount".to_string());
    }
    require_text(&root.join("probes").join("rejected.err"), "55P03")?;
    require_exit(&root.join("probes").join("rejected.exit"), false)?;
    require_exit(&root.join("probes").join("committed.exit"), true)?;
    require_trim(&root.join("probes").join("marker-before.txt"), MARKER)?;
    require_trim(&root.join("probes").join("marker-after.txt"), MARKER)?;
    require_trim(&root.join("probes").join("count-before.txt"), COUNT)?;
    require_trim(&root.join("probes").join("count-after.txt"), COUNT)?;
    require_trim(&root.join("probes").join("amount-during-lock.txt"), ORIGINAL_AMOUNT)?;
    require_trim(&root.join("probes").join("source-amount.txt"), UPDATED_AMOUNT)?;
    require_trim(&root.join("probes").join("dump-amount.txt"), ORIGINAL_AMOUNT)?;
    require_trim(&root.join("probes").join("postcut-absent.txt"), "yes")?;
    require_trim(&root.join("probes").join("dbs-before-corrupt.txt"), "none")?;
    require_trim(&root.join("probes").join("dbs-after-corrupt.txt"), "none")?;
    require_trim(&root.join("probes").join("cut-sessions-after.txt"), "0")?;
    check_outcome(root, held)?;
    check_corrupt(root)
}

fn check_outcome(root: &Path, held: bool) -> Result<(), String> {
    if held {
        require_text(&root.join("probes").join("preexisting-nextval.err"), "55P03")?;
        require_exit(&root.join("probes").join("preexisting-nextval.exit"), false)?;
        require_trim(&root.join("probes").join("sequence-before.txt"), "41|f")?;
        require_trim(&root.join("probes").join("sequence-after.txt"), "41|f")?;
        let restore = read_report(&root.join("reports").join("restore-readiness.json"))?;
        require_contains(&restore, "\"cut_proven\": true")?;
        require_contains(&restore, "\"pitr_ready\": false")?;
        require_contains(&restore, "\"parent_acceptance\": false")?;
        require_trim(&root.join("probes").join("restore-amount.txt"), ORIGINAL_AMOUNT)?;
        return Ok(());
    }
    require_exit(&root.join("probes").join("preexisting-nextval.exit"), true)?;
    require_trim(&root.join("probes").join("sequence-before.txt"), "41|f")?;
    let after = fs::read_to_string(root.join("probes").join("sequence-after.txt")).map_err(|err| format!("sequence after: {err}"))?;
    if after.trim() == "41|f" {
        return Err("nextval did not show the missing sequence fence".to_string());
    }
    let restore = read_report(&root.join("reports").join("restore-readiness.json"))?;
    require_contains(&restore, "\"replay_ready\": false")?;
    require_contains(&restore, "\"cut_proven\": false")?;
    require_contains(&restore, "\"history_insufficient\": true")?;
    require_contains(&restore, "\"offset_published\": null")?;
    require_contains(&restore, "\"reason\": \"cut_unproven\"")?;
    require_contains(&restore, "\"pitr_ready\": false")?;
    require_contains(&restore, "\"parent_acceptance\": false")?;
    require_contains(&restore, "\"w04_gate\": \"open\"")?;
    require_contains(&restore, "\"production_slo_certified\": false")?;
    require_contains(&restore, "\"all_state_certified\": false")?;
    Ok(())
}

fn check_corrupt(root: &Path) -> Result<(), String> {
    let corrupt = read_report(&root.join("reports").join("corrupt-dump-readiness.json"))?;
    require_contains(&corrupt, "\"replay_ready\": false")?;
    require_contains(&corrupt, "\"offset_published\": null")?;
    require_contains(&corrupt, "\"reason\": \"dump_hash_mismatch\"")?;
    require_contains(&corrupt, "\"pitr_ready\": false")?;
    if corrupt.contains("\"replay_ready\": true") {
        return Err("corrupt dump report certifies the cut".to_string());
    }
    Ok(())
}

fn snapshot_pair(manifest: &str) -> Result<(String, String), String> {
    let control = snapshot_id(manifest, "oo_a356_control")?;
    let agent = snapshot_id(manifest, "oo_a356_agent")?;
    if control == agent {
        return Err("snapshot ids are not distinct".to_string());
    }
    Ok((control, agent))
}

fn snapshot_id(manifest: &str, database: &str) -> Result<String, String> {
    let needle = format!("\"database\": \"{database}\"");
    let mut found = None;
    for line in manifest.lines() {
        if !line.contains(&needle) || !line.contains("\"isolation\": \"REPEATABLE READ\"") {
            continue;
        }
        let id = field(line, "id").ok_or_else(|| format!("snapshot id missing for {database}"))?;
        if !snapshot_form(id) {
            return Err(format!("snapshot id is refused: {id}"));
        }
        if found.is_some() {
            return Err(format!("snapshot id repeats for {database}"));
        }
        found = Some(id.to_string());
    }
    found.ok_or_else(|| format!("snapshot id missing for {database}"))
}

fn snapshot_form(id: &str) -> bool {
    let mut parts = id.split('-');
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    let third = parts.next().unwrap_or("");
    let hex8 = |text: &str| text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_lowercase());
    parts.next().is_none() && hex8(first) && hex8(second) && !third.is_empty() && third.bytes().all(|byte| byte.is_ascii_digit())
}

fn require_flag(flags: &str, database: &str, snapshot: &str) -> Result<(), String> {
    let needle = format!("dbname={database} snapshot={snapshot} deferrable=absent format=custom");
    if flags.lines().any(|line| line == needle) {
        Ok(())
    } else {
        Err(format!("pg_dump snapshot flag missing for {database}"))
    }
}

fn full_gold(root: &Path) -> bool {
    fs::read_to_string(root.join("probes").join("full-gold.txt")).map(|text| text.trim() == "yes").unwrap_or(false)
}

fn bind_known_shop(root: &Path) -> Result<(), String> {
    for (name, expect) in [("shop-party.copy", SHOP_PARTY), ("shop-account.copy", SHOP_ACCOUNT), ("shop-entry.copy", SHOP_ENTRY)] {
        let got = sha256::hash_path(&root.join("gold").join(name))?;
        if got != expect {
            return Err(format!("{name} is not the unchanged a361 gold"));
        }
    }
    Ok(())
}

fn match_dump(manifest: &str, root: &Path, name: &str) -> Result<(), String> {
    let line = manifest
        .lines()
        .find(|line| line.contains(&format!("\"name\": \"{name}\"")))
        .ok_or_else(|| format!("{name} is missing from the manifest"))?;
    let expect = field(line, "sha256").ok_or_else(|| format!("{name} has no hash"))?;
    let got = sha256::hash_path(&root.join("dumps").join(name))?;
    if expect != got {
        return Err(format!("{name} hash mismatch"));
    }
    Ok(())
}

fn gold_amount_is(body: &str, amount: &str) -> bool {
    body.lines().any(|line| {
        let mut fields = line.split('\t');
        fields.next() == Some("3") && fields.nth(2) == Some(amount)
    })
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\": \"");
    let start = line.find(&needle)? + needle.len();
    let rest = line.get(start..)?;
    let end = rest.find('"')?;
    rest.get(..end)
}

fn require_line(body: &str, line: &str) -> Result<(), String> {
    if body.lines().any(|item| item == line) { Ok(()) } else { Err(format!("missing {line}")) }
}

fn require_contains(body: &str, needle: &str) -> Result<(), String> {
    if body.contains(needle) { Ok(()) } else { Err(format!("missing {needle}")) }
}

fn require_text(path: &Path, needle: &str) -> Result<(), String> {
    let body = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    if body.contains(needle) { Ok(()) } else { Err(format!("{} does not contain {needle}", path.display())) }
}

fn require_trim(path: &Path, expect: &str) -> Result<(), String> {
    let body = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    if body.trim() == expect { Ok(()) } else { Err(format!("{} is not the expected sample", path.display())) }
}

fn require_exit(path: &Path, success: bool) -> Result<(), String> {
    let body = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let ok = body.trim() == "0";
    if ok == success { Ok(()) } else { Err(format!("{} exit is {}", path.display(), body.trim())) }
}

fn read_report(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTROL_SNAP: &str = "00000001-0000000A-1";
    const AGENT_SNAP: &str = "00000002-0000000B-1";

    #[test]
    fn sha256_vectors_match() {
        assert!(sha256::self_check());
    }

    #[test]
    fn a_refused_sequence_fence_is_not_a_certificate() {
        let root = sample(false);
        check(&root).unwrap();
        let mut manifest = fs::read_to_string(root.join("manifest.json")).unwrap();
        manifest = manifest.replace("\"replay_ready\": false", "\"replay_ready\": true");
        fs::write(root.join("reports").join("restore-readiness.json"), &manifest).unwrap();
        let err = check(&root).unwrap_err();
        assert!(err.contains("replay_ready") || err.contains("missing"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn snapshot_ids_come_from_the_manifest() {
        let root = sample(false);
        let mut manifest = fs::read_to_string(root.join("manifest.json")).unwrap();
        let mut flags = fs::read_to_string(root.join("probes").join("dump-flags.txt")).unwrap();
        manifest = manifest.replace(CONTROL_SNAP, "00000003-00000016-1").replace(AGENT_SNAP, "00000004-00000017-1");
        flags = flags.replace(CONTROL_SNAP, "00000003-00000016-1").replace(AGENT_SNAP, "00000004-00000017-1");
        let digest = sha256::hex(&sha256::hash(manifest.as_bytes()));
        fs::write(root.join("manifest.json"), &manifest).unwrap();
        fs::write(root.join("manifest.sha256"), format!("{digest}\n")).unwrap();
        fs::write(root.join("probes").join("dump-flags.txt"), &flags).unwrap();
        check(&root).unwrap();
        flags = flags.replace("00000004-00000017-1", CONTROL_SNAP);
        fs::write(root.join("probes").join("dump-flags.txt"), flags).unwrap();
        let err = check(&root).unwrap_err();
        assert!(err.contains("pg_dump snapshot flag"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn marker_stability_without_a_rejected_write_is_not_enough() {
        let root = sample(false);
        fs::write(root.join("probes").join("rejected.err"), "updated\n").unwrap();
        let err = check(&root).unwrap_err();
        assert!(err.contains("55P03"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    fn sample(held: bool) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("oo-a377-check-{nanos}"));
        fs::create_dir_all(root.join("dumps")).unwrap();
        fs::create_dir_all(root.join("gold")).unwrap();
        fs::create_dir_all(root.join("probes")).unwrap();
        fs::create_dir_all(root.join("reports")).unwrap();
        fs::write(root.join("dumps").join("oo_a356_agent.dump"), b"agent-dump").unwrap();
        fs::write(root.join("dumps").join("oo_a356_control.dump"), b"control-dump").unwrap();
        fs::write(root.join("gold").join("shop-entry.copy"), "3\t3\t-44997\t1234567890.1234\tm3\n").unwrap();
        fs::write(root.join("catalog.txt"), b"table|oo_a356_agent|shop.entry\n").unwrap();
        let agent = sha256::hash_path(&root.join("dumps").join("oo_a356_agent.dump")).unwrap();
        let control = sha256::hash_path(&root.join("dumps").join("oo_a356_control.dump")).unwrap();
        let gold = sha256::hash_path(&root.join("gold").join("shop-entry.copy")).unwrap();
        let catalog = sha256::hex(&sha256::hash(b"table|oo_a356_agent|shop.entry\n"));
        let mode = if held { "writers_blocked" } else { "unproven" };
        let history = if held { "false" } else { "true" };
        let quiescent = if held { "true" } else { "false" };
        let fence_held = if held { "true" } else { "false" };
        let sqlstate = if held { "00000" } else { "42809" };
        let mut manifest = format!(
            "{{\n  \"catalog_sha256\": \"{catalog}\",\n  \"cut_mode\": \"{mode}\",\n  \"history_insufficient\": {history},\n  \"quiescent\": {quiescent},\n"
        );
        manifest.push_str("  \"dumps\": [\n    {\"name\": \"oo_a356_agent.dump\", \"sha256\": \"");
        manifest.push_str(&agent);
        manifest.push_str("\"},\n    {\"name\": \"oo_a356_control.dump\", \"sha256\": \"");
        manifest.push_str(&control);
        manifest.push_str("\"}\n  ],\n");
        manifest.push_str("    {\"content_md5\": \"d41d8cd98f00b204e9800998ecf8427e\", \"database\": \"oo_a356_agent\", \"gold_sha256\": \"");
        manifest.push_str(&gold);
        manifest.push_str("\", \"relation\": \"shop.entry\"},\n");
        manifest.push_str("    {\"application_name\": \"oo_a356_cut_agent\", \"database\": \"oo_a356_agent\", \"lock_mode\": \"SHARE\", \"probe_rejected\": true, \"relations\": [\"shop.entry\"]},\n");
        manifest.push_str(&format!(
            "  \"sequence_fence\": {{\n    \"held\": {fence_held},\n    \"lock_mode\": \"SHARE\",\n    \"method\": \"lock_table_share\",\n    \"sequences\": [\n      {{\"database\": \"oo_a356_agent\", \"held\": {fence_held}, \"relation\": \"shop.cut_seq\", \"sqlstate\": \"{sqlstate}\"}},\n      {{\"database\": \"oo_a356_control\", \"held\": {fence_held}, \"relation\": \"oo_key_fixture.cut_seq\", \"sqlstate\": \"{sqlstate}\"}}\n    ]\n  }},\n"
        ));
        manifest.push_str(&format!(
            "    {{\"database\": \"oo_a356_agent\", \"id\": \"{AGENT_SNAP}\", \"isolation\": \"REPEATABLE READ\"}},\n    {{\"database\": \"oo_a356_control\", \"id\": \"{CONTROL_SNAP}\", \"isolation\": \"REPEATABLE READ\"}},\n"
        ));
        let digest = sha256::hex(&sha256::hash(manifest.as_bytes()));
        fs::write(root.join("manifest.json"), &manifest).unwrap();
        fs::write(root.join("manifest.sha256"), format!("{digest}\n")).unwrap();
        fs::write(
            root.join("probes").join("dump-flags.txt"),
            format!(
                "dbname=oo_a356_agent snapshot={AGENT_SNAP} deferrable=absent format=custom\ndbname=oo_a356_control snapshot={CONTROL_SNAP} deferrable=absent format=custom\n"
            ),
        )
        .unwrap();
        fs::write(root.join("probes").join("rejected.err"), "ERROR: canceling statement due to lock timeout\nSQLSTATE 55P03\n").unwrap();
        fs::write(root.join("probes").join("rejected.exit"), "1\n").unwrap();
        fs::write(root.join("probes").join("committed.exit"), "0\n").unwrap();
        fs::write(root.join("probes").join("marker-before.txt"), format!("{MARKER}\n")).unwrap();
        fs::write(root.join("probes").join("marker-after.txt"), format!("{MARKER}\n")).unwrap();
        fs::write(root.join("probes").join("count-before.txt"), format!("{COUNT}\n")).unwrap();
        fs::write(root.join("probes").join("count-after.txt"), format!("{COUNT}\n")).unwrap();
        fs::write(root.join("probes").join("amount-during-lock.txt"), format!("{ORIGINAL_AMOUNT}\n")).unwrap();
        fs::write(root.join("probes").join("source-amount.txt"), format!("{UPDATED_AMOUNT}\n")).unwrap();
        fs::write(root.join("probes").join("dump-amount.txt"), format!("{ORIGINAL_AMOUNT}\n")).unwrap();
        fs::write(root.join("probes").join("restore-amount.txt"), format!("{ORIGINAL_AMOUNT}\n")).unwrap();
        fs::write(root.join("probes").join("postcut-absent.txt"), "yes\n").unwrap();
        fs::write(root.join("probes").join("dbs-before-corrupt.txt"), "none\n").unwrap();
        fs::write(root.join("probes").join("dbs-after-corrupt.txt"), "none\n").unwrap();
        fs::write(root.join("probes").join("cut-sessions-after.txt"), "0\n").unwrap();
        fs::write(root.join("probes").join("full-gold.txt"), "no\n").unwrap();
        fs::write(root.join("probes").join("sequence-before.txt"), "41|f\n").unwrap();
        if held {
            fs::write(root.join("probes").join("sequence-after.txt"), "41|f\n").unwrap();
            fs::write(root.join("probes").join("preexisting-nextval.err"), "55P03 lock timeout\n").unwrap();
            fs::write(root.join("probes").join("preexisting-nextval.exit"), "1\n").unwrap();
            fs::write(root.join("reports").join("restore-readiness.json"), "{\"cut_proven\": true, \"pitr_ready\": false, \"parent_acceptance\": false}\n").unwrap();
        } else {
            fs::write(root.join("probes").join("sequence-after.txt"), "41|t\n").unwrap();
            fs::write(root.join("probes").join("preexisting-nextval.err"), "").unwrap();
            fs::write(root.join("probes").join("preexisting-nextval.exit"), "0\n").unwrap();
            fs::write(
                root.join("reports").join("restore-readiness.json"),
                "{\"all_state_certified\": false, \"cut_proven\": false, \"history_insufficient\": true, \"offset_published\": null, \"parent_acceptance\": false, \"pitr_ready\": false, \"production_slo_certified\": false, \"reason\": \"cut_unproven\", \"replay_ready\": false, \"w04_gate\": \"open\"}\n",
            )
            .unwrap();
        }
        fs::write(
            root.join("reports").join("corrupt-dump-readiness.json"),
            "{\"offset_published\": null, \"pitr_ready\": false, \"reason\": \"dump_hash_mismatch\", \"replay_ready\": false}\n",
        )
        .unwrap();
        root
    }
}
