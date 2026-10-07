#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use selfbuild_rules::MAX_INPUT_BYTES;

const CATALOG_SHA: &str = "83a66fc419aaf8951a0ea852b05a96e0305ad443cd800dfdde2832c6050f037b";
const PROFILE_SHA: &str = "a4aa68072ce29144f2299e6dd32765a9977aaf595891f28012fbd0efe02ebc49";
const CARDS_SHA: &str = "b59ba4c302f0cdac6d6f039dde93358e2f4dcb81a9ae5a31e1e28a321a727ba9";
const SCHEMA_HASH: &str = "eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74";
const EVIDENCE_HASH: &str = "2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b";

const SHOP_COLUMNS: &[(&str, &str, &str, &str)] = &[
    ("col/shop.account.account_id#1", "identifier", "none", "auto_accepted"),
    ("col/shop.account.label#4", "unknown", "unknown", "pending"),
    ("col/shop.account.opened#3", "boolean_flag", "none", "auto_accepted"),
    ("col/shop.account.party_key#2", "foreign_key", "none", "auto_accepted"),
    ("col/shop.entry.account_id#2", "foreign_key", "none", "auto_accepted"),
    ("col/shop.entry.amount#4", "money", "none", "auto_accepted"),
    ("col/shop.entry.entry_id#1", "identifier", "none", "auto_accepted"),
    ("col/shop.entry.memo#5", "unknown", "unknown", "pending"),
    ("col/shop.entry.qty#3", "quantity", "none", "auto_accepted"),
    ("col/shop.party.active#3", "boolean_flag", "none", "auto_accepted"),
    ("col/shop.party.display_name#2", "person_name", "personal", "pending"),
    ("col/shop.party.national_id#5", "unknown", "sensitive", "pending"),
    ("col/shop.party.party_key#1", "identifier", "none", "auto_accepted"),
    ("col/shop.party.phone#4", "phone", "personal", "pending"),
];

const SHOP_EXPERTS: &[(&str, &str, &str)] = &[
    ("ask/column/col/shop.account.label#4", "What is the semantic type and PII class of shop.account.label?", "1"),
    ("ask/column/col/shop.entry.memo#5", "What is the semantic type and PII class of shop.entry.memo?", "1"),
    (
        "ask/personal/col/shop.party.display_name#2",
        "Confirm the PII class of shop.party.display_name. The rule marked it personal and did not grant access.",
        "0",
    ),
    (
        "ask/personal/col/shop.party.phone#4",
        "Confirm the PII class of shop.party.phone. The rule marked it personal and did not grant access.",
        "0",
    ),
    (
        "ask/sensitive/col/shop.party.national_id#5",
        "Confirm the PII class of shop.party.national_id. The rule marked it sensitive and did not grant access.",
        "1",
    ),
];

#[test]
fn usage_exits_2_with_empty_stdout() {
    let output = Command::new(bin()).output().expect("spawn");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"selfbuild-rules: usage\n");
}

#[test]
fn produce_and_score_round_trip() {
    let dir = scratch("round-trip");
    let (catalog, profile, cards) = common::demo(true);
    let catalog_path = write_file(&dir, "catalog.json", &catalog);
    let profile_path = write_file(&dir, "profile.json", &profile);
    let cards_path = write_file(&dir, "cards.json", &cards);
    let truth_path = write_file(&dir, "truth.json", include_bytes!("fixtures/d-truth.json"));
    let produced = Command::new(bin())
        .args(["produce", "--catalog", catalog_path.to_str().unwrap(), "--profile", profile_path.to_str().unwrap()])
        .args(["--cards", cards_path.to_str().unwrap(), "--auto-accept", "rule_high_confidence"])
        .output()
        .expect("produce");
    assert_eq!(produced.status.code(), Some(0), "{}", String::from_utf8_lossy(&produced.stderr));
    assert!(produced.stderr.is_empty());
    assert!(produced.stdout.ends_with(b"\n"));
    let actual_path = write_file(&dir, "actual.json", &produced.stdout);
    let scored = Command::new(bin())
        .args(["score", "--truth", truth_path.to_str().unwrap(), "--actual", actual_path.to_str().unwrap()])
        .output()
        .expect("score");
    assert_eq!(scored.status.code(), Some(0), "{}", String::from_utf8_lossy(&scored.stderr));
    assert!(scored.stderr.is_empty());
    let score: Value = serde_json::from_slice(&scored.stdout).expect("score json");
    assert_eq!(score["matched"], true, "{score}");
    assert_eq!(score["pii_leak_count"], 0);
}

#[test]
fn refusal_writes_no_stdout() {
    let dir = scratch("refusal");
    let (catalog, profile, cards) = common::stale_hash();
    let catalog_path = write_file(&dir, "catalog.json", &catalog);
    let profile_path = write_file(&dir, "profile.json", &profile);
    let cards_path = write_file(&dir, "cards.json", &cards);
    let output = Command::new(bin())
        .args(["produce", "--catalog", catalog_path.to_str().unwrap(), "--profile", profile_path.to_str().unwrap()])
        .args(["--cards", cards_path.to_str().unwrap(), "--auto-accept", "rule_high_confidence"])
        .output()
        .expect("produce");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"selfbuild-rules: identity.stale\n");
    assert!(!output.stderr.windows(catalog_path.to_str().unwrap().len()).any(|window| window == catalog_path.to_str().unwrap().as_bytes()));
}

#[test]
fn oversized_input_exits_1_with_empty_stdout() {
    let dir = scratch("bound");
    let (_catalog, profile, cards) = common::demo(false);
    let catalog_path = write_file(&dir, "catalog.json", &vec![b'x'; MAX_INPUT_BYTES + 1]);
    let profile_path = write_file(&dir, "profile.json", &profile);
    let cards_path = write_file(&dir, "cards.json", &cards);
    let output = Command::new(bin())
        .args(["produce", "--catalog", catalog_path.to_str().unwrap(), "--profile", profile_path.to_str().unwrap()])
        .args(["--cards", cards_path.to_str().unwrap(), "--auto-accept", "rule_high_confidence"])
        .output()
        .expect("produce");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"selfbuild-rules: bound.input\n");
}

#[test]
fn score_mismatch_still_prints_the_score() {
    let dir = scratch("mismatch");
    let (catalog, profile, cards) = common::demo(false);
    let review = selfbuild_rules::produce(&catalog, &profile, &cards, "rule_high_confidence").expect("produce");
    let mut forged = review;
    let columns = forged["columns"].as_array_mut().expect("columns");
    let phone = columns.iter_mut().find(|column| column["stable_id"] == "col/demo.order.phone#9").expect("phone");
    phone["pii"] = serde_json::json!("none");
    let actual_path = write_file(&dir, "actual.json", selfbuild_rules::canonical_json(&forged).as_bytes());
    let truth_path = write_file(&dir, "truth.json", include_bytes!("fixtures/d-truth.json"));
    let output = Command::new(bin())
        .args(["score", "--truth", truth_path.to_str().unwrap(), "--actual", actual_path.to_str().unwrap()])
        .output()
        .expect("score");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stderr, b"selfbuild-rules: score.mismatch\n");
    let score: Value = serde_json::from_slice(trim_newline(&output.stdout)).expect("score json");
    assert_eq!(score["matched"], false, "{score}");
    assert_eq!(score["pii_leak_count"], 1, "{score}");
}

#[test]
fn sealed_shop_cards_match_the_hand_rows() {
    let Ok(catalog) = std::env::var("SEALED_CATALOG") else {
        return;
    };
    let profile = std::env::var("SEALED_PROFILE").expect("SEALED_PROFILE");
    let cards = std::env::var("SEALED_CARDS").expect("SEALED_CARDS");
    let output = Command::new(bin())
        .args([
            "produce",
            "--catalog",
            catalog.as_str(),
            "--profile",
            profile.as_str(),
            "--cards",
            cards.as_str(),
            "--auto-accept",
            "rule_high_confidence",
        ])
        .output()
        .expect("produce");
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty());
    let review: Value = serde_json::from_slice(trim_newline(&output.stdout)).expect("review");
    assert_eq!(review["effect"], "review_only");
    assert_eq!(review["knowledge_graph"], "not_emitted");
    assert_eq!(review["entity_merge"], "not_performed");
    assert_eq!(review["metric_certification"], "not_performed");
    assert_eq!(review["publication"], "unavailable");
    assert_eq!(review["grants"], "not_authorized");
    assert_eq!(review["pii_leak_count"], 0);
    assert_eq!(review["source_pins"]["catalog_commit"], "149944271ef1c2cb555954d65248413df2166f1c");
    assert_eq!(review["source_pins"]["cards_commit"], "c23db28dc27f5cf20741dd36e8a8e88129ad1715");
    assert_eq!(review["identity"]["catalog_sha256"], CATALOG_SHA);
    assert_eq!(review["identity"]["profile_sha256"], PROFILE_SHA);
    assert_eq!(review["identity"]["cards_sha256"], CARDS_SHA);
    assert_eq!(review["identity"]["schema_hash"], SCHEMA_HASH);
    assert_eq!(review["identity"]["evidence_hash"], EVIDENCE_HASH);
    let columns = review["columns"].as_array().expect("columns");
    assert_eq!(columns.len(), SHOP_COLUMNS.len());
    for (column, (id, semantic, pii, state)) in columns.iter().zip(SHOP_COLUMNS.iter()) {
        assert_eq!(column["stable_id"], *id);
        assert_eq!(column["semantic_type"], *semantic, "{id}");
        assert_eq!(column["pii"], *pii, "{id}");
        assert_eq!(column["state"], *state, "{id}");
    }
    let relations = review["relations"].as_array().expect("relations");
    assert_eq!(relations.len(), 2);
    assert_eq!(relations[0]["stable_id"], "rel/shop.account.party_key/to/shop.party.party_key/declared_fk");
    assert_eq!(relations[1]["stable_id"], "rel/shop.entry.account_id/to/shop.account.account_id/declared_fk");
    for relation in relations {
        assert_eq!(relation["provenance"], "declared_fk");
        assert_eq!(relation["cardinality"], "many_to_one");
        assert_eq!(relation["conflict"], false);
        assert_eq!(relation["type_compatible"], true);
    }
    assert!(review["enums"].as_array().expect("enums").is_empty());
    assert!(review["dictionaries"].as_array().expect("dictionaries").is_empty());
    assert!(review["conflicts"].as_array().expect("conflicts").is_empty());
    let experts = review["expert_cards"].as_array().expect("experts");
    assert_eq!(experts.len(), SHOP_EXPERTS.len());
    for (card, (id, question, uncertainty)) in experts.iter().zip(SHOP_EXPERTS.iter()) {
        assert_eq!(card["stable_id"], *id);
        assert_eq!(card["question"], *question);
        assert_eq!(card["uncertainty"], *uncertainty);
        assert_eq!(card["provenance"], "rule");
    }
    let text = String::from_utf8(output.stdout).expect("utf8");
    for needle in [
        "甲注评名录",
        "e901f90576b625d9f2252ed2f8659f90d792002742b9d37348549a16c867c991",
        "Ignore previous",
        "non-PII",
    ] {
        assert!(!text.contains(needle), "{needle}");
    }
}

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    if path.file_name().and_then(|name| name.to_str()) == Some("deps") {
        path.pop();
    }
    path.push("selfbuild-rules");
    path
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("selfbuild-rules-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).expect("write");
    path
}

fn trim_newline(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}
