//! Independent review oracle for frozen E49.
//! Expected rows are hand literals. This file does not load a producer document as gold.
//! Card hashes use the public sealer so the input identity check can run.

#![forbid(unsafe_code)]

#[path = "/work/src/sha256.rs"]
mod file_sha;

use std::env;
use std::fs;
use std::process::ExitCode;

use selfbuild_rules::{canonical_json, canonical_sha256, produce, score, seal_card, CARDS_COMMIT, CATALOG_COMMIT};
use serde_json::{json, Value};

const GOLD_SHA256: &str = "2f3adbeeb3037b547d33eb611d043e9ee11b52e32b4983fd3b9148575f928f66";
const SCHEMA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_HASH: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const RAW: &str = "HANDRAWCOMMENT";
const SPEC_AUTO: &[&str] = &[
    "boolean_flag",
    "money",
    "quantity",
    "ratio",
    "event_time",
    "date",
    "audit_time",
    "identifier",
    "foreign_key",
    "status_code",
    "code",
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            println!("oracle_ok");
            ExitCode::SUCCESS
        }
        Err(code) => {
            eprintln!("oracle: {code}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), &'static str> {
    known_hashes()?;
    gold_truth_unchanged()?;
    fixture_is_forty_not_five()?;
    let low = low_card_bridge()?;
    low_card_semantics(&low)?;
    auto_none_forces_pending(&low.catalog, &low.profile, &low.cards)?;
    zero_profile_sensitive_unknown_comment()?;
    weak_relation_conflict()?;
    refusals(&low.catalog, &low.profile, &low.cards)?;
    sealed_shop()?;
    Ok(())
}

fn known_hashes() -> Result<(), &'static str> {
    let empty = file_sha::digest_hex(b"").ok_or("hash.empty")?;
    let abc = file_sha::digest_hex(b"abc").ok_or("hash.abc")?;
    if empty != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" {
        return Err("hash.empty");
    }
    if abc != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" {
        return Err("hash.abc");
    }
    let known = canonical_sha256(&json!({"b": 2, "a": 1})).ok_or("hash.object")?;
    if known != "43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777" {
        return Err("hash.object");
    }
    Ok(())
}

fn gold_truth_unchanged() -> Result<(), &'static str> {
    let bytes = fs::read("/work/tests/fixtures/d-truth.json").map_err(|_| "gold.read")?;
    let digest = file_sha::digest_hex(&bytes).ok_or("gold.hash")?;
    if digest != GOLD_SHA256 {
        return Err("gold.hash");
    }
    let truth: Value = serde_json::from_slice(&bytes).map_err(|_| "gold.json")?;
    if truth["protocol"] != "oo.selfbuild-rules.truth/v1" {
        return Err("gold.protocol");
    }
    let enums = ids(&truth, "enums");
    let want = [
        "enum/demo.order.zt#5",
        "enum/demo.sys_dict.code#2",
        "enum/demo.sys_dict.kind#1",
        "enum/demo.sys_dict.label#3",
    ];
    if enums != want {
        return Err("gold.enums");
    }
    if enums.iter().any(|id: &&str| id.contains("bridge")) {
        return Err("gold.bridge_enum");
    }
    expect_col(&truth, "col/demo.bridge.left_id#1", "foreign_key", "none", "auto_accepted")?;
    expect_col(&truth, "col/demo.bridge.right_id#1", "foreign_key", "none", "auto_accepted")?;
    Ok(())
}

fn fixture_is_forty_not_five() -> Result<(), &'static str> {
    let text = fs::read_to_string("/work/tests/common/mod.rs").map_err(|_| "fixture.read")?;
    if !text.contains("name: \"left_id\", ty: \"int8\", nn: true, nd: Some(\"40\")") {
        return Err("fixture.left");
    }
    if !text.contains("name: \"right_id\", ty: \"int8\", nn: true, nd: Some(\"40\")") {
        return Err("fixture.right");
    }
    if text.contains("Some(\"5\")") {
        return Err("fixture.five");
    }
    Ok(())
}

struct Docs {
    catalog: Vec<u8>,
    profile: Vec<u8>,
    cards: Vec<u8>,
}

fn low_card_bridge() -> Result<Docs, &'static str> {
    let left_fk = json!({
        "name": "bridge_left_fk",
        "columns": [{
            "attnum": 1,
            "name": "left_id",
            "ref_schema": "hand",
            "ref_table": "left",
            "ref_attnum": 1,
            "ref_name": "id"
        }]
    });
    let right_fk = json!({
        "name": "bridge_right_fk",
        "columns": [{
            "attnum": 2,
            "name": "right_id",
            "ref_schema": "hand",
            "ref_table": "right",
            "ref_attnum": 1,
            "ref_name": "id"
        }]
    });
    let tables = [
        table(
            "left",
            "10",
            &[(1, "id")],
            &[column(1, "id", "int8", true, Some("-1"), true)],
            &[],
            true,
        ),
        table(
            "right",
            "10",
            &[(1, "id")],
            &[column(1, "id", "int8", true, Some("-1"), false)],
            &[],
            false,
        ),
        table(
            "bridge",
            "20",
            &[(1, "left_id"), (2, "right_id")],
            &[
                column(1, "left_id", "int8", true, Some("5"), true),
                column(2, "right_id", "int8", true, Some("5"), false),
            ],
            &[left_fk, right_fk],
            false,
        ),
    ];
    pack("hand", &tables, true)
}

fn low_card_semantics(docs: &Docs) -> Result<(), &'static str> {
    let review = produce(&docs.catalog, &docs.profile, &docs.cards, "rule_high_confidence").map_err(|_| "low.produce")?;
    let again = produce(&docs.catalog, &docs.profile, &docs.cards, "rule_high_confidence").map_err(|_| "low.again")?;
    if canonical_json(&review) != canonical_json(&again) {
        return Err("low.deterministic");
    }
    posture(&review)?;
    hashes(&review, &docs.catalog, &docs.profile, &docs.cards)?;
    if rows(&review, "columns").len() != 4 || rows(&review, "relations").len() != 3 || rows(&review, "enums").len() != 2 {
        return Err("low.counts");
    }
    if !rows(&review, "dictionaries").is_empty() || !rows(&review, "conflicts").is_empty() {
        return Err("low.extra");
    }
    expect_out(&review, "col/hand.left.id#1", "identifier", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/hand.right.id#1", "identifier", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/hand.bridge.left_id#1", "foreign_key", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/hand.bridge.right_id#2", "foreign_key", "none", "auto_accepted", "0")?;
    expect_rel(
        &review,
        "rel/hand.bridge.left_id/to/hand.left.id/declared_fk",
        "declared_fk",
        "many_to_one",
        false,
        "0",
        "1",
    )?;
    expect_rel(
        &review,
        "rel/hand.bridge.right_id/to/hand.right.id/declared_fk",
        "declared_fk",
        "many_to_one",
        false,
        "0",
        "1",
    )?;
    expect_rel(
        &review,
        "rel/hand.bridge/hand.left.id/to/hand.right.id/inferred",
        "inferred",
        "many_to_many",
        false,
        "0.5",
        "0.5",
    )?;
    expect_enum(&review, "enum/hand.bridge.left_id#1")?;
    expect_enum(&review, "enum/hand.bridge.right_id#2")?;
    let experts = rows(&review, "expert_cards");
    if experts.len() != 3 {
        return Err("low.experts");
    }
    expect_ask(
        &review,
        "ask/bridge/rel/hand.bridge/hand.left.id/to/hand.right.id/inferred",
        "Do hand.left.id and hand.right.id form a many-to-many relation through hand.bridge? This candidate does not merge entities.",
        "0.5",
    )?;
    expect_ask(
        &review,
        "ask/enum/enum/hand.bridge.left_id#1",
        "What do the distinct values of hand.bridge.left_id mean? Values are withheld. The signature is undetermined and not approved.",
        "1",
    )?;
    expect_ask(
        &review,
        "ask/enum/enum/hand.bridge.right_id#2",
        "What do the distinct values of hand.bridge.right_id mean? Values are withheld. The signature is undetermined and not approved.",
        "1",
    )?;
    let text = canonical_json(&review);
    if text.contains(RAW) || text.contains("current_privileges") {
        return Err("low.redaction");
    }
    let truth = json!({
        "protocol": "oo.selfbuild-rules.truth/v1",
        "columns": [
            {"stable_id": "col/hand.bridge.left_id#1", "semantic_type": "foreign_key", "pii": "none", "state": "auto_accepted"},
            {"stable_id": "col/hand.bridge.right_id#2", "semantic_type": "foreign_key", "pii": "none", "state": "auto_accepted"},
            {"stable_id": "col/hand.left.id#1", "semantic_type": "identifier", "pii": "none", "state": "auto_accepted"},
            {"stable_id": "col/hand.right.id#1", "semantic_type": "identifier", "pii": "none", "state": "auto_accepted"}
        ],
        "relations": [
            {"stable_id": "rel/hand.bridge.left_id/to/hand.left.id/declared_fk", "provenance": "declared_fk", "cardinality": "many_to_one", "conflict": false},
            {"stable_id": "rel/hand.bridge.right_id/to/hand.right.id/declared_fk", "provenance": "declared_fk", "cardinality": "many_to_one", "conflict": false},
            {"stable_id": "rel/hand.bridge/hand.left.id/to/hand.right.id/inferred", "provenance": "inferred", "cardinality": "many_to_many", "conflict": false}
        ],
        "enums": [
            {"stable_id": "enum/hand.bridge.left_id#1", "status": "undetermined", "approved": false},
            {"stable_id": "enum/hand.bridge.right_id#2", "status": "undetermined", "approved": false}
        ],
        "dictionaries": [],
        "conflicts": [],
        "personal_ids": [],
        "sensitive_ids": [],
        "unknown_ids": [],
        "forbidden_substrings": [RAW, "current_privileges"]
    });
    let scored = score(&serde_json::to_vec(&truth).map_err(|_| "low.truth")?, text.as_bytes()).map_err(|_| "low.score")?;
    if scored["matched"] != true || scored["pii_leak_count"] != json!(0) {
        return Err("low.score");
    }
    spec_accepts_only(&review)?;
    fs::write("/evidence/linux/oracle-2/oracle-low.json", &text).map_err(|_| "low.write")?;
    Ok(())
}

fn auto_none_forces_pending(catalog: &[u8], profile: &[u8], cards: &[u8]) -> Result<(), &'static str> {
    let review = produce(catalog, profile, cards, "none").map_err(|_| "none.produce")?;
    if review["auto_accept"] != "none" || review["grants"] != "not_authorized" {
        return Err("none.posture");
    }
    for row in rows(&review, "columns") {
        if row["state"] != "pending" {
            return Err("none.pending");
        }
    }
    if rows(&review, "enums").len() != 2 || rows(&review, "expert_cards").len() != 7 {
        return Err("none.counts");
    }
    if review["pii_leak_count"] != json!(0) {
        return Err("none.leak");
    }
    Ok(())
}

fn zero_profile_sensitive_unknown_comment() -> Result<(), &'static str> {
    let tables = [table(
        "item",
        "4",
        &[(1, "id")],
        &[
            column(1, "id", "int8", true, None, false),
            column(2, "national_id", "text", true, None, false),
            column(3, "phone", "text", false, None, false),
            column(4, "zt", "text", false, Some("3"), false),
            column(5, "mystery", "text", false, None, true),
        ],
        &[],
        true,
    )];
    let docs = pack("z", &tables, false)?;
    let review = produce(&docs.catalog, &docs.profile, &docs.cards, "rule_high_confidence").map_err(|_| "zero.produce")?;
    posture(&review)?;
    hashes(&review, &docs.catalog, &docs.profile, &docs.cards)?;
    expect_out(&review, "col/z.item.id#1", "identifier", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/z.item.national_id#2", "unknown", "sensitive", "pending", "1")?;
    expect_out(&review, "col/z.item.phone#3", "phone", "personal", "pending", "0")?;
    expect_out(&review, "col/z.item.zt#4", "unknown", "unknown", "pending", "1")?;
    expect_out(&review, "col/z.item.mystery#5", "unknown", "unknown", "pending", "1")?;
    if !rows(&review, "enums").is_empty() || !rows(&review, "relations").is_empty() {
        return Err("zero.extra");
    }
    let text = canonical_json(&review);
    if text.contains("RAWNOTE") || text.contains("status_code") {
        return Err("zero.redaction");
    }
    expect_ask(
        &review,
        "ask/sensitive/col/z.item.national_id#2",
        "Confirm the PII class of z.item.national_id. The rule marked it sensitive and did not grant access.",
        "1",
    )?;
    expect_ask(
        &review,
        "ask/personal/col/z.item.phone#3",
        "Confirm the PII class of z.item.phone. The rule marked it personal and did not grant access.",
        "0",
    )?;
    spec_accepts_only(&review)?;
    Ok(())
}

fn weak_relation_conflict() -> Result<(), &'static str> {
    let tables = [
        table("user_a", "10", &[(1, "user_id")], &[column(1, "user_id", "int8", true, Some("-1"), false)], &[], false),
        table("user_b", "10", &[(1, "user_id")], &[column(1, "user_id", "int8", true, Some("-1"), false)], &[], false),
        table("audit", "100", &[], &[column(1, "user_id", "int8", false, Some("-0.4"), false)], &[], false),
    ];
    let docs = pack("w", &tables, true)?;
    let review = produce(&docs.catalog, &docs.profile, &docs.cards, "rule_high_confidence").map_err(|_| "weak.produce")?;
    posture(&review)?;
    expect_out(&review, "col/w.audit.user_id#1", "unknown", "none", "pending", "0.5")?;
    expect_rel(
        &review,
        "rel/w.audit.user_id/to/w.user_a.user_id/naming",
        "naming",
        "many_to_one",
        true,
        "0.5",
        "0.5",
    )?;
    expect_rel(
        &review,
        "rel/w.audit.user_id/to/w.user_b.user_id/naming",
        "naming",
        "many_to_one",
        true,
        "0.5",
        "0.5",
    )?;
    let conflict = find(&review, "conflicts", "conflict/w.audit.user_id#1")?;
    if !conflict["selected"].is_null() {
        return Err("weak.selected");
    }
    if rows(&review, "relations").len() != 2 || !rows(&review, "enums").is_empty() {
        return Err("weak.counts");
    }
    if review["entity_merge"] != "not_performed" || review["metric_certification"] != "not_performed" {
        return Err("weak.grant");
    }
    spec_accepts_only(&review)?;
    Ok(())
}

fn refusals(catalog: &[u8], profile: &[u8], cards: &[u8]) -> Result<(), &'static str> {
    let mut stale: Value = serde_json::from_slice(cards).map_err(|_| "stale.json")?;
    stale["schema_hash"] = json!("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc");
    let stale_bytes = serde_json::to_vec(&stale).map_err(|_| "stale.bytes")?;
    expect_code(produce(catalog, profile, &stale_bytes, "rule_high_confidence"), "identity.stale")?;
    let huge = vec![b'x'; 1_048_577];
    expect_code(produce(&huge, profile, cards, "rule_high_confidence"), "bound.input")?;
    expect_code(produce(catalog, profile, cards, "owner_grant"), "schema")?;
    let mut samples: Value = serde_json::from_slice(profile).map_err(|_| "sample.json")?;
    samples["sample_policy"] = json!("raw");
    let sample_bytes = serde_json::to_vec(&samples).map_err(|_| "sample.bytes")?;
    expect_code(produce(catalog, &sample_bytes, cards, "rule_high_confidence"), "profile.samples")?;
    let mut visible: Value = serde_json::from_slice(cards).map_err(|_| "values.json")?;
    visible["tables"][0]["columns"][0]["most_common_vals"] = json!("visible-values");
    seal_card(&mut visible["tables"][0]).map_err(|_| "values.seal")?;
    let visible_bytes = serde_json::to_vec(&visible).map_err(|_| "values.bytes")?;
    expect_code(produce(catalog, profile, &visible_bytes, "rule_high_confidence"), "profile.values")?;
    Ok(())
}

fn sealed_shop() -> Result<(), &'static str> {
    let catalog = fs::read(env::var("SEALED_CATALOG").map_err(|_| "shop.env")?).map_err(|_| "shop.catalog")?;
    let profile = fs::read(env::var("SEALED_PROFILE").map_err(|_| "shop.env")?).map_err(|_| "shop.profile")?;
    let cards = fs::read(env::var("SEALED_CARDS").map_err(|_| "shop.env")?).map_err(|_| "shop.cards")?;
    let review = produce(&catalog, &profile, &cards, "rule_high_confidence").map_err(|_| "shop.produce")?;
    posture(&review)?;
    hashes(&review, &catalog, &profile, &cards)?;
    if review["identity"]["schema_hash"] != "eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74" {
        return Err("shop.schema");
    }
    if review["identity"]["evidence_hash"] != "2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b" {
        return Err("shop.evidence");
    }
    if rows(&review, "columns").len() != 14
        || rows(&review, "relations").len() != 2
        || !rows(&review, "enums").is_empty()
        || !rows(&review, "dictionaries").is_empty()
        || !rows(&review, "conflicts").is_empty()
        || rows(&review, "expert_cards").len() != 5
    {
        return Err("shop.counts");
    }
    expect_out(&review, "col/shop.party.party_key#1", "identifier", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.party.display_name#2", "person_name", "personal", "pending", "0")?;
    expect_out(&review, "col/shop.party.active#3", "boolean_flag", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.party.phone#4", "phone", "personal", "pending", "0")?;
    expect_out(&review, "col/shop.party.national_id#5", "unknown", "sensitive", "pending", "1")?;
    expect_out(&review, "col/shop.account.account_id#1", "identifier", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.account.party_key#2", "foreign_key", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.account.opened#3", "boolean_flag", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.account.label#4", "unknown", "unknown", "pending", "1")?;
    expect_out(&review, "col/shop.entry.entry_id#1", "identifier", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.entry.account_id#2", "foreign_key", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.entry.qty#3", "quantity", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.entry.amount#4", "money", "none", "auto_accepted", "0")?;
    expect_out(&review, "col/shop.entry.memo#5", "unknown", "unknown", "pending", "1")?;
    expect_rel(
        &review,
        "rel/shop.account.party_key/to/shop.party.party_key/declared_fk",
        "declared_fk",
        "many_to_one",
        false,
        "0",
        "1",
    )?;
    expect_rel(
        &review,
        "rel/shop.entry.account_id/to/shop.account.account_id/declared_fk",
        "declared_fk",
        "many_to_one",
        false,
        "0",
        "1",
    )?;
    expect_ask(
        &review,
        "ask/personal/col/shop.party.phone#4",
        "Confirm the PII class of shop.party.phone. The rule marked it personal and did not grant access.",
        "0",
    )?;
    expect_ask(
        &review,
        "ask/sensitive/col/shop.party.national_id#5",
        "Confirm the PII class of shop.party.national_id. The rule marked it sensitive and did not grant access.",
        "1",
    )?;
    expect_ask(
        &review,
        "ask/column/col/shop.account.label#4",
        "What is the semantic type and PII class of shop.account.label?",
        "1",
    )?;
    let text = canonical_json(&review);
    for needle in [
        "current_privileges",
        "e901f90576b625d9f2252ed2f8659f90d792002742b9d37348549a16c867c991",
        "Ignore previous",
        "non-PII",
    ] {
        if text.contains(needle) {
            return Err("shop.redaction");
        }
    }
    spec_accepts_only(&review)?;
    fs::write("/evidence/linux/oracle-2/oracle-shop.json", &text).map_err(|_| "shop.write")?;
    Ok(())
}

struct Built {
    catalog: Value,
    profile: Option<Value>,
    card: Value,
}

fn column(attnum: i32, name: &str, ty: &str, not_null: bool, nd: Option<&str>, raw: bool) -> Value {
    json!({
        "attnum": attnum,
        "name": name,
        "type_name": ty,
        "not_null": not_null,
        "n_distinct": nd.map(str::to_owned),
        "raw": raw
    })
}

fn table(name: &str, rows: &str, pk: &[(i32, &str)], cols: &[Value], fks: &[Value], raw: bool) -> Built {
    let mut catalog_cols = Vec::new();
    let mut profile_cols = Vec::new();
    let mut card_cols = Vec::new();
    for col in cols {
        let attnum = col["attnum"].clone();
        let col_name = col["name"].clone();
        let ty = col["type_name"].clone();
        let not_null = col["not_null"].clone();
        let nd = col["n_distinct"].clone();
        let comment = if col["raw"] == true {
            json!({"text": if name == "item" { "RAWNOTE" } else { RAW }, "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"})
        } else {
            Value::Null
        };
        let mut catalog_col = json!({
            "attnum": attnum,
            "name": col_name,
            "name_ident": col_name,
            "type_name": ty,
            "not_null": not_null,
            "current_privileges": {"select": true, "insert": false, "update": false}
        });
        if !comment.is_null() {
            catalog_col["comment"] = comment.clone();
        }
        catalog_cols.push(catalog_col);
        profile_cols.push(json!({
            "attnum": attnum,
            "name": col_name,
            "null_frac": Value::Null,
            "n_distinct": nd,
            "correlation": Value::Null
        }));
        card_cols.push(json!({
            "attnum": attnum,
            "name": col_name,
            "type_name": ty,
            "not_null": not_null,
            "untrusted_comment": if comment.is_null() { Value::Null } else { json!(comment["text"].as_str().unwrap_or("")) },
            "null_frac": Value::Null,
            "n_distinct": nd,
            "correlation": Value::Null,
            "most_common_vals": "withheld",
            "histogram_bounds": "withheld"
        }));
    }
    let pk_json: Vec<Value> = pk.iter().map(|(attnum, pk_name)| json!({"attnum": attnum, "name": pk_name})).collect();
    let mut catalog = json!({
        "schema": "pending",
        "schema_ident": "pending",
        "name": name,
        "name_ident": name,
        "columns": catalog_cols,
        "primary_key": pk_json,
        "unique_keys": [],
        "foreign_keys": fks
    });
    if raw {
        catalog["comment"] = json!({"text": if name == "item" { "RAWNOTE" } else { RAW }, "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"});
    }
    let profile = json!({
        "schema": "pending",
        "name": name,
        "reltuples": rows,
        "columns": profile_cols
    });
    let card = json!({
        "dataset": format!("pending.{name}"),
        "schema": "pending",
        "schema_ident": "pending",
        "name": name,
        "name_ident": name,
        "relkind": "r",
        "untrusted_table_comment": Value::Null,
        "comment_authority": "untrusted_data",
        "reltuples": rows,
        "columns": card_cols,
        "foreign_keys": fks
    });
    Built { catalog, profile: Some(profile), card }
}

fn pack(schema: &str, tables: &[Built], profile_on: bool) -> Result<Docs, &'static str> {
    let mut catalog_tables = Vec::new();
    let mut profile_tables = Vec::new();
    let mut card_tables = Vec::new();
    for table in tables {
        let mut catalog = table.catalog.clone();
        catalog["schema"] = json!(schema);
        catalog["schema_ident"] = json!(schema);
        let mut profile = table.profile.clone().unwrap_or(Value::Null);
        if profile_on {
            profile["schema"] = json!(schema);
            profile_tables.push(profile);
        }
        let mut card = table.card.clone();
        card["schema"] = json!(schema);
        card["schema_ident"] = json!(schema);
        card["dataset"] = json!(format!("{schema}.{}", card["name"].as_str().unwrap_or("")));
        if !profile_on {
            card["reltuples"] = Value::Null;
            if let Some(cols) = card["columns"].as_array_mut() {
                for col in cols {
                    col["n_distinct"] = json!("3");
                }
            }
        }
        seal_card(&mut card).map_err(|_| "pack.seal")?;
        catalog_tables.push(catalog);
        card_tables.push(card);
    }
    let catalog = json!({
        "protocol": "oo.source.pgsql/v1",
        "schema_hash": SCHEMA,
        "tables": catalog_tables
    });
    let profile = json!({
        "protocol": "oo.pg-builder.profile/v1",
        "sample_policy": "none",
        "tables": profile_tables
    });
    let cards = json!({
        "protocol": "oo.pg-builder.cards/v1",
        "schema_version": "oo.pg-builder.card/v1",
        "evidence_hash": EVIDENCE_HASH,
        "schema_hash": SCHEMA,
        "sample_policy": "none",
        "tables": card_tables
    });
    Ok(Docs {
        catalog: serde_json::to_vec(&catalog).map_err(|_| "pack.catalog")?,
        profile: serde_json::to_vec(&profile).map_err(|_| "pack.profile")?,
        cards: serde_json::to_vec(&cards).map_err(|_| "pack.cards")?,
    })
}

fn posture(review: &Value) -> Result<(), &'static str> {
    if review["effect"] != "review_only"
        || review["authenticated"] != false
        || review["parent_auth"] != "required"
        || review["publication"] != "unavailable"
        || review["grants"] != "not_authorized"
        || review["certification"] != "none"
        || review["entity_merge"] != "not_performed"
        || review["metric_certification"] != "not_performed"
        || review["knowledge_graph"] != "not_emitted"
        || review["comment_disclosure"] != "withheld"
        || review["sample_policy"] != "none"
        || review["pii_leak_count"] != json!(0)
    {
        return Err("posture");
    }
    if review["source_pins"]["catalog_commit"] != CATALOG_COMMIT || review["source_pins"]["cards_commit"] != CARDS_COMMIT {
        return Err("pins");
    }
    Ok(())
}

fn hashes(review: &Value, catalog: &[u8], profile: &[u8], cards: &[u8]) -> Result<(), &'static str> {
    let catalog_sha = file_sha::digest_hex(catalog).ok_or("hash.catalog")?;
    let profile_sha = file_sha::digest_hex(profile).ok_or("hash.profile")?;
    let cards_sha = file_sha::digest_hex(cards).ok_or("hash.cards")?;
    let schema_hash = review["identity"]["schema_hash"].as_str().unwrap_or("");
    if review["identity"]["catalog_sha256"].as_str() != Some(catalog_sha.as_str())
        || review["identity"]["profile_sha256"].as_str() != Some(profile_sha.as_str())
        || review["identity"]["cards_sha256"].as_str() != Some(cards_sha.as_str())
        || (schema_hash != SCHEMA && schema_hash != "eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74")
    {
        return Err("hash.identity");
    }
    Ok(())
}

fn spec_accepts_only(review: &Value) -> Result<(), &'static str> {
    for row in rows(review, "columns") {
        let state = row["state"].as_str().unwrap_or("");
        let pii = row["pii"].as_str().unwrap_or("");
        let semantic = row["semantic_type"].as_str().unwrap_or("");
        if (pii == "personal" || pii == "sensitive") && state == "auto_accepted" {
            return Err("spec.pii");
        }
        if state == "auto_accepted" && !SPEC_AUTO.contains(&semantic) {
            return Err("spec.allow");
        }
        if pii == "unknown" && state != "pending" {
            return Err("spec.unknown");
        }
    }
    Ok(())
}

fn expect_code(result: Result<Value, selfbuild_rules::Fail>, code: &str) -> Result<(), &'static str> {
    match result {
        Err(err) if err.code == code => Ok(()),
        Err(err) => {
            eprintln!("oracle: refusal.got {}", err.code);
            Err("refusal.code")
        }
        Ok(_) => Err("refusal.stdout"),
    }
}

fn expect_col(doc: &Value, id: &str, semantic: &str, pii: &str, state: &str) -> Result<(), &'static str> {
    let row = find(doc, "columns", id)?;
    if row["semantic_type"] != semantic || row["pii"] != pii || row["state"] != state {
        return Err("column");
    }
    Ok(())
}

fn expect_out(doc: &Value, id: &str, semantic: &str, pii: &str, state: &str, uncertainty: &str) -> Result<(), &'static str> {
    expect_col(doc, id, semantic, pii, state)?;
    let row = find(doc, "columns", id)?;
    if row["uncertainty"] != uncertainty {
        return Err("uncertainty");
    }
    Ok(())
}

fn expect_rel(
    doc: &Value,
    id: &str,
    provenance: &str,
    cardinality: &str,
    conflict: bool,
    uncertainty: &str,
    confidence: &str,
) -> Result<(), &'static str> {
    let row = find(doc, "relations", id)?;
    if row["provenance"] != provenance
        || row["cardinality"] != cardinality
        || row["conflict"] != conflict
        || row["uncertainty"] != uncertainty
        || row["confidence"] != confidence
        || row["status"] != "candidate"
    {
        return Err("relation");
    }
    Ok(())
}

fn expect_enum(doc: &Value, id: &str) -> Result<(), &'static str> {
    let row = find(doc, "enums", id)?;
    if row["status"] != "undetermined"
        || row["approved"] != false
        || row["class"] != "undetermined"
        || row["values"] != "withheld"
        || row["time_window"] != "unobserved"
        || !row["jaccard"].is_null()
    {
        return Err("enum");
    }
    Ok(())
}

fn expect_ask(doc: &Value, id: &str, question: &str, uncertainty: &str) -> Result<(), &'static str> {
    let row = find(doc, "expert_cards", id)?;
    let anchors = row["anchors"].as_array().ok_or("ask.anchors")?;
    if row["question"] != question
        || row["uncertainty"] != uncertainty
        || row["provenance"] != "rule"
        || !row["evidence"].is_object()
        || anchors.is_empty()
    {
        return Err("ask");
    }
    Ok(())
}

fn find<'a>(doc: &'a Value, key: &str, id: &str) -> Result<&'a Value, &'static str> {
    rows(doc, key).into_iter().find(|row| row["stable_id"] == id).ok_or("missing")
}

fn rows<'a>(doc: &'a Value, key: &str) -> Vec<&'a Value> {
    doc.get(key).and_then(Value::as_array).map(|items| items.iter().collect()).unwrap_or_default()
}

fn ids<'a>(doc: &'a Value, key: &str) -> Vec<&'a str> {
    let mut out: Vec<&str> = rows(doc, key).into_iter().filter_map(|row| row["stable_id"].as_str()).collect();
    out.sort_unstable();
    out
}
