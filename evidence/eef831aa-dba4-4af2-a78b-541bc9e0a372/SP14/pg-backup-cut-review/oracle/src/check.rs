//! File checks for one recorded writer-blocked cut.
//! Marker and entry-count samples cannot satisfy this checker by themselves.
//! Shop gold hashes are the a361 independent oracle values. The checker recomputes them.

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

const CHECKPOINT_GOLD: &[&str] = &[
    "calendar-snapshot.copy",
    "cdc-watermark.copy",
    "session-head.copy",
    "key-issued.copy",
    "enum-published.copy",
    "shop-party.copy",
    "shop-account.copy",
    "shop-entry.copy",
];

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
    require_line(&manifest, "  \"cut_mode\": \"writers_blocked\",")?;
    require_line(&manifest, "  \"history_insufficient\": false,")?;
    require_line(&manifest, "  \"quiescent\": true,")?;
    if manifest.contains("\"history_insufficient\": true") || manifest.contains("\"quiescent\": false") {
        return Err("proven manifest also says the cut is missing".to_string());
    }
    match_dump(&manifest, root, "oo_a356_agent.dump")?;
    match_dump(&manifest, root, "oo_a356_control.dump")?;
    let gold_hash = sha256::hash_path(&root.join("gold").join("shop-entry.copy"))?;
    let entry = manifest
        .lines()
        .find(|line| line.contains("\"relation\": \"shop.entry\""))
        .ok_or("shop.entry digest is missing")?;
    if field(entry, "database") != Some("oo_a356_agent") || field(entry, "gold_sha256") != Some(gold_hash.as_str()) {
        return Err("shop.entry digest is not bound to the gold file".to_string());
    }
    if !is_md5(field(entry, "content_md5").unwrap_or("")) {
        return Err("shop.entry content digest is not md5".to_string());
    }
    if full_gold(root) {
        bind_gold_files(&manifest, root)?;
    }
    require_block(&manifest, "oo_a356_control", "oo_a356_cut_control")?;
    require_block(&manifest, "oo_a356_agent", "oo_a356_cut_agent")?;
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
    require_trim(&root.join("probes").join("restore-amount.txt"), ORIGINAL_AMOUNT)?;
    require_trim(&root.join("probes").join("dbs-before-corrupt.txt"), "none")?;
    require_trim(&root.join("probes").join("dbs-after-corrupt.txt"), "none")?;
    require_trim(&root.join("probes").join("cut-sessions-after.txt"), "0")?;
    let good = read_report(&root.join("reports").join("restore-readiness.json"))?;
    require_contains(&good, "\"replay_ready\": true")?;
    require_contains(&good, "\"cut_proven\": true")?;
    require_contains(&good, "\"history_insufficient\": false")?;
    require_contains(&good, "\"offset_published\": \"100000\"")?;
    require_contains(&good, "\"pitr_ready\": false")?;
    require_contains(&good, "\"parent_acceptance\": false")?;
    require_contains(&good, "\"w04_gate\": \"open\"")?;
    require_contains(&good, "\"production_slo_certified\": false")?;
    require_contains(&good, "\"all_state_certified\": false")?;
    let refused = read_report(&root.join("reports").join("corrupt-dump-readiness.json"))?;
    require_contains(&refused, "\"replay_ready\": false")?;
    require_contains(&refused, "\"cut_proven\": false")?;
    require_contains(&refused, "\"history_insufficient\": true")?;
    require_contains(&refused, "\"offset_published\": null")?;
    require_contains(&refused, "\"reason\": \"dump_hash_mismatch\"")?;
    if refused.contains("\"history_insufficient\": false") || refused.contains("\"replay_ready\": true") {
        return Err("corrupt dump report certifies the cut".to_string());
    }
    Ok(())
}

fn full_gold(root: &Path) -> bool {
    fs::read_to_string(root.join("probes").join("full-gold.txt")).map(|text| text.trim() == "yes").unwrap_or(false)
}

fn bind_gold_files(manifest: &str, root: &Path) -> Result<(), String> {
    let mut found = Vec::new();
    for line in manifest.lines() {
        if !line.contains(".copy\"") || !line.contains("\"sha256\"") {
            continue;
        }
        let Some(name) = field(line, "name") else {
            continue;
        };
        if !name.ends_with(".copy") {
            continue;
        }
        let expect = field(line, "sha256").ok_or_else(|| format!("{name} has no hash"))?;
        let got = sha256::hash_path(&root.join("gold").join(name))?;
        if expect != got {
            return Err(format!("{name} gold hash mismatch"));
        }
        if let Some(known) = known_shop(name)
            && expect != known
        {
            return Err(format!("{name} is not the unchanged a361 gold"));
        }
        found.push(name.to_string());
    }
    for need in CHECKPOINT_GOLD {
        if !found.iter().any(|name| name == *need) {
            return Err(format!("checkpoint gold {need} is missing"));
        }
    }
    Ok(())
}

fn known_shop(name: &str) -> Option<&'static str> {
    match name {
        "shop-party.copy" => Some(SHOP_PARTY),
        "shop-account.copy" => Some(SHOP_ACCOUNT),
        "shop-entry.copy" => Some(SHOP_ENTRY),
        _ => None,
    }
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

fn require_block(manifest: &str, database: &str, application: &str) -> Result<(), String> {
    let line = manifest
        .lines()
        .find(|line| line.contains(&format!("\"database\": \"{database}\"")) && line.contains("\"lock_mode\""))
        .ok_or_else(|| format!("{database} writer block is missing"))?;
    let relation = database_relation(database);
    if field(line, "application_name") != Some(application)
        || field(line, "lock_mode") != Some("SHARE")
        || !line.contains("\"probe_rejected\": true")
        || !line.contains(&format!("\"{relation}\""))
    {
        return Err(format!("{database} writer block is not a rejected SHARE lock"));
    }
    Ok(())
}

fn database_relation(database: &str) -> &'static str {
    if database == "oo_a356_agent" { "shop.entry" } else { "oo_key_fixture.cut_marker" }
}

fn gold_amount_is(body: &str, amount: &str) -> bool {
    body.lines().any(|line| {
        let mut fields = line.split('\t');
        fields.next() == Some("3") && fields.nth(2) == Some(amount)
    })
}

fn is_md5(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\": \"");
    let start = line.find(&needle)? + needle.len();
    let rest = line.get(start..)?;
    let end = rest.find('"')?;
    rest.get(..end)
}

fn require_line(body: &str, line: &str) -> Result<(), String> {
    if body.lines().any(|item| item == line) {
        Ok(())
    } else {
        Err(format!("missing {line}"))
    }
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
    let code = body.trim();
    let ok = code == "0";
    if ok == success { Ok(()) } else { Err(format!("{} exit is {code}", path.display())) }
}

fn read_report(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vectors_match() {
        assert!(sha256::self_check());
    }

    #[test]
    fn marker_stability_without_a_rejected_write_is_not_enough() {
        let root = sample(false);
        let err = check(&root).unwrap_err();
        assert!(err.contains("55P03"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bound_cut_passes() {
        let root = sample(true);
        check(&root).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    fn sample(rejected: bool) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("oo-a372-check-{nanos}-{}", if rejected { "yes" } else { "no" }));
        fs::create_dir_all(root.join("dumps")).unwrap();
        fs::create_dir_all(root.join("gold")).unwrap();
        fs::create_dir_all(root.join("probes")).unwrap();
        fs::create_dir_all(root.join("reports")).unwrap();
        fs::write(root.join("dumps").join("oo_a356_agent.dump"), b"agent-dump").unwrap();
        fs::write(root.join("dumps").join("oo_a356_control.dump"), b"control-dump").unwrap();
        fs::write(root.join("gold").join("shop-entry.copy"), "3\t3\t-44997\t1234567890.1234\tm3\n").unwrap();
        let agent = sha256::hash_path(&root.join("dumps").join("oo_a356_agent.dump")).unwrap();
        let control = sha256::hash_path(&root.join("dumps").join("oo_a356_control.dump")).unwrap();
        let gold = sha256::hash_path(&root.join("gold").join("shop-entry.copy")).unwrap();
        let mut manifest = String::from("{\n  \"cut_mode\": \"writers_blocked\",\n  \"history_insufficient\": false,\n  \"quiescent\": true,\n");
        manifest.push_str("    {\"name\": \"oo_a356_agent.dump\", \"sha256\": \"");
        manifest.push_str(&agent);
        manifest.push_str("\"},\n    {\"name\": \"oo_a356_control.dump\", \"sha256\": \"");
        manifest.push_str(&control);
        manifest.push_str("\"},\n    {\"content_md5\": \"d41d8cd98f00b204e9800998ecf8427e\", \"database\": \"oo_a356_agent\", \"gold_sha256\": \"");
        manifest.push_str(&gold);
        manifest.push_str("\", \"relation\": \"shop.entry\"},\n");
        manifest.push_str("    {\"application_name\": \"oo_a356_cut_control\", \"database\": \"oo_a356_control\", \"lock_mode\": \"SHARE\", \"probe_rejected\": true, \"relations\": [\"oo_key_fixture.cut_marker\"]},\n");
        manifest.push_str("    {\"application_name\": \"oo_a356_cut_agent\", \"database\": \"oo_a356_agent\", \"lock_mode\": \"SHARE\", \"probe_rejected\": true, \"relations\": [\"shop.entry\"]}\n}\n");
        fs::write(root.join("manifest.json"), &manifest).unwrap();
        fs::write(root.join("manifest.sha256"), format!("{}\n", sha256::hex(&sha256::hash(manifest.as_bytes())))).unwrap();
        fs::write(root.join("probes").join("rejected.err"), if rejected { "ERROR: canceling statement due to lock timeout\n55P03\n" } else { "UPDATE 1\n" }).unwrap();
        fs::write(root.join("probes").join("rejected.exit"), if rejected { "1\n" } else { "0\n" }).unwrap();
        fs::write(root.join("probes").join("committed.exit"), "0\n").unwrap();
        fs::write(root.join("probes").join("marker-before.txt"), "cut-a356-1|100000\n").unwrap();
        fs::write(root.join("probes").join("marker-after.txt"), "cut-a356-1|100000\n").unwrap();
        fs::write(root.join("probes").join("count-before.txt"), "90000\n").unwrap();
        fs::write(root.join("probes").join("count-after.txt"), "90000\n").unwrap();
        fs::write(root.join("probes").join("amount-during-lock.txt"), "1234567890.1234\n").unwrap();
        fs::write(root.join("probes").join("source-amount.txt"), "1.0001\n").unwrap();
        fs::write(root.join("probes").join("restore-amount.txt"), "1234567890.1234\n").unwrap();
        fs::write(root.join("probes").join("dbs-before-corrupt.txt"), "none\n").unwrap();
        fs::write(root.join("probes").join("dbs-after-corrupt.txt"), "none\n").unwrap();
        fs::write(root.join("probes").join("cut-sessions-after.txt"), "0\n").unwrap();
        fs::write(
            root.join("reports").join("restore-readiness.json"),
            "{\n  \"all_state_certified\": false,\n  \"cut_proven\": true,\n  \"history_insufficient\": false,\n  \"offset_published\": \"100000\",\n  \"parent_acceptance\": false,\n  \"pitr_ready\": false,\n  \"production_slo_certified\": false,\n  \"replay_ready\": true,\n  \"w04_gate\": \"open\"\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("reports").join("corrupt-dump-readiness.json"),
            "{\n  \"cut_proven\": false,\n  \"history_insufficient\": true,\n  \"offset_published\": null,\n  \"reason\": \"dump_hash_mismatch\",\n  \"replay_ready\": false\n}\n",
        )
        .unwrap();
        root
    }
}
