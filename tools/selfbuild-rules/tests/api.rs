#![forbid(unsafe_code)]

mod common;

use selfbuild_rules::{canonical_json, produce, score, seal_card, CARDS_COMMIT, CATALOG_COMMIT, MAX_INPUT_BYTES};
use serde_json::{json, Value};

const TRUTH: &[u8] = include_bytes!("fixtures/d-truth.json");

const EXPERTS: &[(&str, &str, &str)] = &[
    (
        "ask/bridge/rel/demo.bridge/demo.left.id/to/demo.right.id/inferred",
        "Do demo.left.id and demo.right.id form a many-to-many relation through demo.bridge? This candidate does not merge entities.",
        "0.5",
    ),
    (
        "ask/column/col/demo.audit.user_id#1",
        "What is the semantic type and PII class of demo.audit.user_id?",
        "0.5",
    ),
    ("ask/column/col/demo.bad.ref_id#1", "What is the semantic type and PII class of demo.bad.ref_id?", "1"),
    ("ask/column/col/demo.order.dh#7", "What is the semantic type and PII class of demo.order.dh?", "1"),
    ("ask/column/col/demo.order.mystery#8", "What is the semantic type and PII class of demo.order.mystery?", "1"),
    ("ask/column/col/demo.order.note#10", "What is the semantic type and PII class of demo.order.note?", "1"),
    ("ask/column/col/demo.sys_dict.kind#1", "What is the semantic type and PII class of demo.sys_dict.kind?", "1"),
    ("ask/column/col/demo.sys_dict.label#3", "What is the semantic type and PII class of demo.sys_dict.label?", "1"),
    (
        "ask/conflict/conflict/demo.audit.user_id#1",
        "Column demo.audit.user_id has more than one relation candidate. No candidate was selected.",
        "0.5",
    ),
    (
        "ask/enum/enum/demo.order.zt#5",
        "What do the distinct values of demo.order.zt mean? Values are withheld. The signature is undetermined and not approved.",
        "1",
    ),
    (
        "ask/enum/enum/demo.sys_dict.code#2",
        "What do the distinct values of demo.sys_dict.code mean? Values are withheld. The signature is undetermined and not approved.",
        "1",
    ),
    (
        "ask/enum/enum/demo.sys_dict.kind#1",
        "What do the distinct values of demo.sys_dict.kind mean? Values are withheld. The signature is undetermined and not approved.",
        "1",
    ),
    (
        "ask/enum/enum/demo.sys_dict.label#3",
        "What do the distinct values of demo.sys_dict.label mean? Values are withheld. The signature is undetermined and not approved.",
        "1",
    ),
    (
        "ask/personal/col/demo.customer.display_name#2",
        "Confirm the PII class of demo.customer.display_name. The rule marked it personal and did not grant access.",
        "0",
    ),
    (
        "ask/personal/col/demo.customer.email#5",
        "Confirm the PII class of demo.customer.email. The rule marked it personal and did not grant access.",
        "0",
    ),
    (
        "ask/personal/col/demo.order.phone#9",
        "Confirm the PII class of demo.order.phone. The rule marked it personal and did not grant access.",
        "0",
    ),
    (
        "ask/relation/rel/demo.audit.user_id/to/demo.user_a.user_id/naming",
        "Is demo.audit.user_id a foreign key to demo.user_a.user_id? Cardinality is many_to_one. Type compatibility is true.",
        "0.5",
    ),
    (
        "ask/relation/rel/demo.audit.user_id/to/demo.user_b.user_id/naming",
        "Is demo.audit.user_id a foreign key to demo.user_b.user_id? Cardinality is many_to_one. Type compatibility is true.",
        "0.5",
    ),
    (
        "ask/relation/rel/demo.bad.ref_id/to/demo.customer.id/declared_fk",
        "Is demo.bad.ref_id a foreign key to demo.customer.id? Cardinality is many_to_one. Type compatibility is false.",
        "0.5",
    ),
    (
        "ask/sensitive/col/demo.customer.national_id#3",
        "Confirm the PII class of demo.customer.national_id. The rule marked it sensitive and did not grant access.",
        "1",
    ),
];

const RELATIONS: &[(&str, &str, &str, bool)] = &[
    ("rel/demo.audit.user_id/to/demo.user_a.user_id/naming", "naming", "many_to_one", true),
    ("rel/demo.audit.user_id/to/demo.user_b.user_id/naming", "naming", "many_to_one", true),
    ("rel/demo.bad.ref_id/to/demo.customer.id/declared_fk", "declared_fk", "many_to_one", false),
    ("rel/demo.bridge.left_id/to/demo.left.id/declared_fk", "declared_fk", "many_to_one", false),
    ("rel/demo.bridge.right_id/to/demo.right.id/declared_fk", "declared_fk", "many_to_one", false),
    ("rel/demo.bridge/demo.left.id/to/demo.right.id/inferred", "inferred", "many_to_many", false),
    ("rel/demo.order.customer_id/to/demo.customer.id/declared_fk", "declared_fk", "many_to_one", false),
];

#[test]
fn d_truth_score_matches_hand_fixture() {
    let review = review(true);
    assert_literals(&review);
    assert_eq!(rows(&review, "columns").len(), 26);
    assert_eq!(rows(&review, "relations").len(), 7);
    assert_eq!(rows(&review, "enums").len(), 4);
    assert_eq!(rows(&review, "dictionaries").len(), 1);
    assert_eq!(rows(&review, "conflicts").len(), 1);
    assert_eq!(rows(&review, "expert_cards").len(), 20);
    assert_column(&review, "col/demo.order.dh#7", "unknown", "unknown", "pending");
    assert_column(&review, "col/demo.order.phone#9", "phone", "personal", "pending");
    assert_column(&review, "col/demo.order.zt#5", "status_code", "none", "auto_accepted");
    assert_column(&review, "col/demo.customer.national_id#3", "unknown", "sensitive", "pending");
    assert_column(&review, "col/demo.order.customer_id#2", "foreign_key", "none", "auto_accepted");
    assert_column(&review, "col/demo.bad.ref_id#1", "unknown", "unknown", "pending");
    assert_column(&review, "col/demo.audit.user_id#1", "unknown", "none", "pending");
    for (id, provenance, cardinality, conflict) in RELATIONS {
        let row = find(&review, "relations", id);
        assert_eq!(row["provenance"], *provenance, "{id}");
        assert_eq!(row["cardinality"], *cardinality, "{id}");
        assert_eq!(row["conflict"], *conflict, "{id}");
    }
    assert!(find(&review, "conflicts", "conflict/demo.audit.user_id#1")["selected"].is_null());
    assert_eq!(find(&review, "enums", "enum/demo.order.zt#5")["status"], "undetermined");
    assert_eq!(find(&review, "enums", "enum/demo.order.zt#5")["approved"], false);
    assert_eq!(find(&review, "dictionaries", "dict/demo.sys_dict")["match"], "structural_only");
    assert_eq!(find(&review, "dictionaries", "dict/demo.sys_dict")["approved"], false);
    let text = canonical_json(&review);
    for needle in ["RAWCOMMENT", "13800001111", "ignore previous", &"a".repeat(64)] {
        assert!(!text.contains(needle), "{needle}");
    }
    let scored = score(TRUTH, text.as_bytes()).expect("score");
    assert_eq!(scored["pii_leak_count"], 0, "{scored}");
    assert_eq!(scored["matched"], true, "{scored}");
    assert_eq!(scored["mismatches"].as_array().map(Vec::len), Some(0), "{scored}");
}

#[test]
fn expert_questions_match_the_hand_list() {
    let review = review(true);
    let cards = rows(&review, "expert_cards");
    assert_eq!(cards.len(), EXPERTS.len());
    for (card, (id, question, uncertainty)) in cards.iter().zip(EXPERTS.iter()) {
        assert_eq!(card["stable_id"], *id);
        assert_eq!(card["question"], *question);
        assert_eq!(card["uncertainty"], *uncertainty);
        assert_eq!(card["provenance"], "rule");
        assert!(card["evidence"].is_object());
        let anchors = card["anchors"].as_array().expect("anchors");
        assert!(!anchors.is_empty());
        assert!(anchors.iter().all(|item| item.as_str().is_some()));
    }
    for absent in [
        "ask/relation/rel/demo.order.customer_id/to/demo.customer.id/declared_fk",
        "ask/relation/rel/demo.bridge.left_id/to/demo.left.id/declared_fk",
        "ask/relation/rel/demo.bridge.right_id/to/demo.right.id/declared_fk",
    ] {
        assert!(cards.iter().all(|card| card["stable_id"] != absent), "{absent}");
    }
}

#[test]
fn two_produces_share_canonical_bytes() {
    let (catalog, profile, cards) = common::demo(false);
    let left = produce(&catalog, &profile, &cards, "rule_high_confidence").expect("left");
    let right = produce(&catalog, &profile, &cards, "rule_high_confidence").expect("right");
    assert_eq!(canonical_json(&left), canonical_json(&right));
}

#[test]
fn comments_stay_out_of_the_review() {
    let with = review(true);
    let without = review(false);
    for key in ["columns", "relations", "enums", "dictionaries", "conflicts", "expert_cards"] {
        assert_eq!(with[key], without[key], "{key}");
    }
    assert_eq!(with["identity"]["schema_hash"], without["identity"]["schema_hash"]);
    assert_ne!(with["identity"]["catalog_sha256"], without["identity"]["catalog_sha256"]);
    assert_ne!(with["identity"]["cards_sha256"], without["identity"]["cards_sha256"]);
    let text = canonical_json(&with);
    assert!(!text.contains("RAWCOMMENT"));
    assert!(!text.contains("ignore previous"));
    assert!(!text.contains("13800001111"));
}

#[test]
fn zero_profile_ignores_card_distinct_counts() {
    let (catalog, profile, cards) = common::zero_profile();
    let review = produce(&catalog, &profile, &cards, "rule_high_confidence").expect("zero");
    assert_eq!(rows(&review, "columns").len(), 3);
    assert_column(&review, "col/demo.item.id#1", "identifier", "none", "auto_accepted");
    assert_column(&review, "col/demo.item.active#2", "boolean_flag", "none", "auto_accepted");
    assert_column(&review, "col/demo.item.zt#3", "unknown", "unknown", "pending");
    assert!(rows(&review, "enums").is_empty());
    assert!(rows(&review, "relations").is_empty());
    assert!(rows(&review, "dictionaries").is_empty());
    assert!(rows(&review, "conflicts").is_empty());
    let experts = rows(&review, "expert_cards");
    assert_eq!(experts.len(), 1);
    let expert = &experts[0];
    assert_eq!(expert["stable_id"], "ask/column/col/demo.item.zt#3");
    assert_eq!(expert["question"], "What is the semantic type and PII class of demo.item.zt?");
    assert_eq!(expert["uncertainty"], "1");
    assert_eq!(expert["evidence"]["profile"], "absent");
    assert_eq!(review["pii_leak_count"], 0);
}

#[test]
fn stale_schema_hash_refuses() {
    let (catalog, profile, cards) = common::stale_hash();
    let err = produce(&catalog, &profile, &cards, "rule_high_confidence").expect_err("stale");
    assert_eq!(err.code, "identity.stale");
}

#[test]
fn bounded_input_refuses_before_parse() {
    let (_catalog, profile, cards) = common::demo(false);
    let big = vec![b'x'; MAX_INPUT_BYTES + 1];
    let err = produce(&big, &profile, &cards, "rule_high_confidence").expect_err("bound");
    assert_eq!(err.code, "bound.input");
}

#[test]
fn duplicate_key_is_json() {
    let (_catalog, profile, cards) = common::demo(false);
    let err = produce(br#"{"a":1,"a":2}"#, &profile, &cards, "rule_high_confidence").expect_err("dup");
    assert_eq!(err.code, "json");
    let err = produce(br#"{"a":1,"\u0061":2}"#, &profile, &cards, "rule_high_confidence").expect_err("alias");
    assert_eq!(err.code, "json");
}

#[test]
fn unknown_auto_accept_is_schema() {
    let (catalog, profile, cards) = common::demo(false);
    let err = produce(&catalog, &profile, &cards, "maybe").expect_err("auto");
    assert_eq!(err.code, "schema");
}

#[test]
fn sample_policy_and_visible_values_refuse() {
    let (catalog, profile, cards) = common::demo(false);
    let mut profile_value: Value = serde_json::from_slice(&profile).expect("profile");
    profile_value["sample_policy"] = json!("rows");
    let err = produce(&catalog, &serde_json::to_vec(&profile_value).expect("bytes"), &cards, "rule_high_confidence").expect_err("samples");
    assert_eq!(err.code, "profile.samples");

    let mut cards_value: Value = serde_json::from_slice(&cards).expect("cards");
    cards_value["tables"][0]["columns"][0]["most_common_vals"] = json!("visible");
    seal_card(&mut cards_value["tables"][0]).expect("seal");
    let err = produce(&catalog, &profile, &serde_json::to_vec(&cards_value).expect("bytes"), "rule_high_confidence").expect_err("values");
    assert_eq!(err.code, "profile.values");
}

#[test]
fn auto_accept_none_keeps_every_column_pending() {
    let (catalog, profile, cards) = common::demo(false);
    let review = produce(&catalog, &profile, &cards, "none").expect("none");
    assert_eq!(review["auto_accept"], "none");
    assert_eq!(review["pii_leak_count"], 0);
    for column in rows(&review, "columns") {
        assert_eq!(column["state"], "pending", "{}", column["stable_id"]);
    }
    assert_column(&review, "col/demo.order.phone#9", "phone", "personal", "pending");
    assert_column(&review, "col/demo.customer.national_id#3", "unknown", "sensitive", "pending");
}

#[test]
fn forged_phone_none_counts_as_one_leak() {
    let mut review = review(false);
    let columns = review["columns"].as_array_mut().expect("columns");
    let phone = columns.iter_mut().find(|column| column["stable_id"] == "col/demo.order.phone#9").expect("phone");
    phone["pii"] = json!("none");
    let scored = score(TRUTH, canonical_json(&review).as_bytes()).expect("score");
    assert_eq!(scored["pii_leak_count"], 1, "{scored}");
    assert_eq!(scored["matched"], false, "{scored}");
    let mismatches = scored["mismatches"].as_array().expect("mismatches");
    assert!(mismatches.iter().any(|item| item == "column.pii:col/demo.order.phone#9"), "{scored}");
}

fn review(comments: bool) -> Value {
    let (catalog, profile, cards) = common::demo(comments);
    produce(&catalog, &profile, &cards, "rule_high_confidence").expect("produce")
}

fn assert_literals(review: &Value) {
    assert_eq!(review["protocol"], "oo.selfbuild-rules.review-cards/v1");
    assert_eq!(review["schema_version"], "oo.selfbuild-rules.review-cards/v1");
    assert_eq!(review["effect"], "review_only");
    assert_eq!(review["authenticated"], false);
    assert_eq!(review["parent_auth"], "required");
    assert_eq!(review["publication"], "unavailable");
    assert_eq!(review["grants"], "not_authorized");
    assert_eq!(review["certification"], "none");
    assert_eq!(review["entity_merge"], "not_performed");
    assert_eq!(review["metric_certification"], "not_performed");
    assert_eq!(review["knowledge_graph"], "not_emitted");
    assert_eq!(review["comment_disclosure"], "withheld");
    assert_eq!(review["sample_policy"], "none");
    assert_eq!(review["pii_leak_count"], 0);
    assert_eq!(review["source_pins"]["catalog_commit"], CATALOG_COMMIT);
    assert_eq!(review["source_pins"]["cards_commit"], CARDS_COMMIT);
    assert_eq!(review["identity"]["schema_hash"], common::SCHEMA_HASH);
    assert_eq!(review["identity"]["evidence_hash"], common::EVIDENCE_HASH);
}

fn rows(review: &Value, key: &str) -> Vec<Value> {
    review[key].as_array().unwrap_or_else(|| panic!("{key}")).clone()
}

fn find<'a>(review: &'a Value, key: &str, id: &str) -> &'a Value {
    review[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key}"))
        .iter()
        .find(|row| row["stable_id"] == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}

fn assert_column(review: &Value, id: &str, semantic: &str, pii: &str, state: &str) {
    let column = find(review, "columns", id);
    assert_eq!(column["semantic_type"], semantic, "{id}");
    assert_eq!(column["pii"], pii, "{id}");
    assert_eq!(column["state"], state, "{id}");
}
