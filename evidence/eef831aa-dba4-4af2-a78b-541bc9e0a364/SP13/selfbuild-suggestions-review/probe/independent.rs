//! Independent probes for the E50 review.
//!
//! The partial-validation child bytes are a scripted stand-in. They are not a
//! query-pg receipt. The live receipt is captured from the compiled binary.

use selfbuild_suggestions::{
    Error, Gateway, SelectRunner, Spawned, candidate_id, canonical_sha256, parse_document,
    produce, produce_with,
};
use serde_json::{Value, json};

const METRIC_ID: &str = "cand_3cc4bb382160763a05d3d2a16fc20c8d";
const ENTITY_ID: &str = "cand_3238c70833a4054c7c18dc716e7b54c3";
const CANONICAL: &str = "ecf9e98ec0641e23113ff3ce8bdc78d0ddd249886517fd4a7f68cc83d4e65667";
const SELECT_SQL: &str = "SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1";
const ENTRY_SQL: &str = "SELECT account_id FROM shop.entry WHERE account_id IS NOT NULL LIMIT 1";
const PROBE_SQL: &str = "UPDATE shop.account SET label = label";

#[test]
fn scope_refuses_other_model_scope_and_missing_authorization() {
    let mut command = shell("review", "review");
    command["scope"]["model_scope"] = json!("agent");
    let (reason, outcome) = refused(produce(&command));
    assert_eq!(reason, "scope");
    assert_eq!(outcome["scope"]["schema_hash"], Value::Null);
    assert!(outcome["entities"].as_array().unwrap().is_empty());
    assert_closed(&outcome);

    let mut command = shell("review", "review");
    command["authorization"]["authorized"] = json!(false);
    let (reason, outcome) = refused(produce(&command));
    assert_eq!(reason, "authorization");
    assert_eq!(outcome["effects"]["grants_issued"], false);
    assert_eq!(outcome["effects"]["roles_changed"], false);
    assert_closed(&outcome);
}

#[test]
fn seal_mismatch_and_candidate_preimage_are_independent() {
    let twisted = json!({"b": "x", "a": 1});
    assert_eq!(canonical_sha256(&twisted), CANONICAL);
    assert_eq!(
        parse_document(br#"{"a":1,"a":2}"#).unwrap_err(),
        "duplicate_key"
    );
    assert_eq!(
        candidate_id("review", "metric", "shop.account.amount"),
        METRIC_ID
    );

    let mut command = clean();
    let sealed = canonical_sha256(&command["catalog"]);
    command["catalog"]["schema_hash"] =
        json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    assert_ne!(canonical_sha256(&command["catalog"]), sealed);
    let (reason, outcome) = refused(produce(&command));
    assert_eq!(reason, "seal");
    assert!(outcome["metrics"].as_array().unwrap().is_empty());
    assert_closed(&outcome);
}

#[test]
fn duplicate_relationship_and_feedback_collapse_once() {
    let mut command = clean();
    let account = command["relationships"]["items"][0].clone();
    let mut foreign = account.clone();
    foreign["project"] = json!("other");
    command["relationships"]["items"]
        .as_array_mut()
        .unwrap()
        .push(account);
    command["relationships"]["items"]
        .as_array_mut()
        .unwrap()
        .push(foreign);
    command["feedback"]["items"] = json!([
        {"project": "review", "kind": "comment", "text": "same-note", "target": "shop.account"},
        {"project": "review", "kind": "comment", "text": "same-note", "target": "shop.account"},
        {"project": "other", "kind": "gap", "text": "cross-note", "target": "shop.entry"}
    ]);
    seal(&mut command);
    let outcome = ok(produce(&command));
    assert_eq!(outcome["dedup"]["relationships_collapsed"], 1);
    assert_eq!(outcome["scope_drops"]["relationships"], 1);
    assert_eq!(outcome["dedup"]["feedback_collapsed"], 1);
    assert_eq!(outcome["scope_drops"]["feedback"], 1);
    assert_eq!(outcome["feedback"].as_array().unwrap().len(), 1);
    assert_eq!(outcome["examples"].as_array().unwrap().len(), 2);
    assert_closed(&outcome);
}

#[test]
fn graph_metric_and_partial_validation_keep_the_second_example_unrun() {
    let command = clean();
    let first = ok(produce(&command));
    let second = ok(produce(&command));
    assert_eq!(first, second);
    assert_eq!(first["gateway"]["configured"], false);
    assert_eq!(first["gateway"]["probe"], "not_run");
    assert_eq!(first["gateway"]["spawned"], 0);

    let entity = &first["entities"][0];
    assert_eq!(entity["primary"], "shop.party");
    assert_eq!(entity["domain"], "commerce");
    assert_eq!(entity["provenance"], "declared_fk");
    assert_eq!(entity["confidence"], "graph");
    assert_eq!(entity["strong_key_status"], "weak");
    assert_eq!(entity["auto_merge"], false);
    assert_eq!(entity["weak_columns"], json!(["shop.party.phone"]));
    assert_eq!(entity["candidate_id"], ENTITY_ID);
    assert_eq!(entity["prior_overlap"], true);

    let amount = metric(&first, "amount");
    assert_eq!(amount["candidate_id"], METRIC_ID);
    assert_eq!(amount["certified"], false);
    assert_eq!(amount["status"], "draft");
    assert_eq!(amount["owner_needed"], true);
    assert_eq!(amount["control_total_needed"], true);
    assert_eq!(amount["grain"], json!(["month"]));
    assert_eq!(amount["time_dimension"], "posted_on");
    assert_eq!(amount["time_anchor"], "prev_month_start");
    assert_eq!(amount["time_anchor_closed"], false);
    assert_eq!(amount["time_anchor_needed"], false);
    assert_eq!(amount["sql_template"], "SELECT sum(amount) FROM shop.account");
    assert_eq!(amount["validation"], "not_applicable");
    assert_eq!(amount["gateway_admitted"], false);

    let qty = metric(&first, "qty");
    assert_eq!(qty["grain"], json!(["row"]));
    assert_eq!(qty["time_anchor"], Value::Null);
    assert_eq!(qty["time_anchor_needed"], true);
    assert_eq!(qty["owner_needed"], true);
    assert_eq!(qty["control_total_needed"], true);
    assert_eq!(qty["certified"], false);
    assert_eq!(qty["sql_template"], "SELECT sum(qty) FROM shop.entry");

    assert_eq!(first["examples"][0]["sql_template"], party_sql());
    assert_eq!(first["examples"][0]["validation"], "not_run");
    assert_eq!(first["examples"][1]["sql_template"], entry_sql_amount_fixture());
    assert_eq!(first["examples"][1]["validation"], "not_run");
    assert_closed(&first);

    let mut opened = Script {
        calls: Vec::new(),
        answers: vec![Spawned {
            exit: Some(0),
            stdout: b"{}\n".to_vec(),
            timed_out: false,
            started: true,
        }],
    };
    let (reason, refused_outcome) = refused(produce_with(
        &command,
        Some(gate(PROBE_SQL, 1, &mut opened)),
    ));
    assert_eq!(reason, "probe");
    assert_eq!(opened.calls, vec![PROBE_SQL.to_string()]);
    assert_eq!(refused_outcome["gateway"]["probe"], "not_refused");
    assert_eq!(refused_outcome["gateway"]["admitted"], 0);
    assert_eq!(refused_outcome["examples"][0]["validation"], "not_run");
    assert_eq!(refused_outcome["examples"][1]["validation"], "not_run");
    assert_closed(&refused_outcome);

    let mut runner = Script {
        calls: Vec::new(),
        answers: vec![
            Spawned {
                exit: Some(6),
                stdout: Vec::new(),
                timed_out: false,
                started: true,
            },
            Spawned {
                exit: Some(0),
                stdout: scripted_child(),
                timed_out: false,
                started: true,
            },
        ],
    };
    let outcome = ok(produce_with(
        &command,
        Some(gate(PROBE_SQL, 1, &mut runner)),
    ));
    assert_eq!(
        runner.calls,
        vec![PROBE_SQL.to_string(), party_sql()]
    );
    assert!(runner.calls.iter().all(|sql| !sql.contains("sum(")));
    assert_eq!(outcome["gateway"]["probe"], "refused");
    assert_eq!(outcome["gateway"]["probe_exit"], 6);
    assert_eq!(outcome["gateway"]["admitted"], 1);
    assert_eq!(outcome["gateway"]["spawned"], 2);
    assert_eq!(outcome["examples"][0]["validation"], "admitted");
    assert_eq!(outcome["examples"][0]["gateway_admitted"], true);
    assert_eq!(outcome["examples"][0]["knowledge_verified"], false);
    assert_eq!(outcome["examples"][1]["validation"], "not_run");
    assert_eq!(outcome["examples"][1]["gateway_admitted"], false);
    assert_closed(&outcome);
}

#[test]
fn negative_weak_key_does_not_merge_certify_or_publish() {
    let mut command = clean();
    command["profile"]["tables"] = json!([{
        "schema": "shop",
        "name": "party",
        "columns": [{"name": "party_id", "strong_key": "validated"}]
    }]);
    command["negative_pairs"] = json!([["shop.party", "shop.account"]]);
    command["curated_overrides"] = json!([{
        "key": "metric:shop.account.amount",
        "fact_sha256": "0000000000000000000000000000000000000000000000000000000000000000"
    }]);
    command["feedback"]["items"] = json!([{
        "project": "review",
        "kind": "comment",
        "text": "untrusted-note-a364",
        "target": "shop.account"
    }]);
    seal(&mut command);
    let outcome = ok(produce(&command));
    let examples = outcome["examples"].as_array().unwrap();
    assert_eq!(examples.len(), 1);
    assert_eq!(examples[0]["constraint"], "entry_account_fk");
    assert!(
        outcome["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entity| entity["members"] == json!(["shop.party"])
                && entity["provenance"] == "isolated"
                && entity["strong_key_status"] == "weak"
                && entity["auto_merge"] == false)
    );
    let amount = metric(&outcome, "amount");
    assert_eq!(amount["sql_template"], "SELECT sum(amount) FROM shop.account");
    assert_eq!(amount["certified"], false);
    assert!(
        outcome["queue"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["channel"] == "stale_override")
    );
    let feedback = &outcome["feedback"][0];
    assert_eq!(feedback["untrusted"], true);
    assert_eq!(feedback["trusted_metric"], false);
    assert_eq!(feedback["text"], "untrusted-note-a364");
    for metric_item in outcome["metrics"].as_array().unwrap() {
        assert!(!metric_item["sql_template"]
            .as_str()
            .unwrap()
            .contains("untrusted-note-a364"));
    }
    for example in examples {
        assert!(!example["sql_template"]
            .as_str()
            .unwrap()
            .contains("untrusted-note-a364"));
        assert!(!example["question"]
            .as_str()
            .unwrap()
            .contains("untrusted-note-a364"));
    }
    assert_eq!(outcome["effects"]["curated_written"], false);
    assert_eq!(outcome["effects"]["published"], false);
    assert_closed(&outcome);
}

#[test]
fn emit_live_command_when_requested() {
    let Ok(dir) = std::env::var("OO_A364_OUT") else {
        return;
    };
    let command = live_command();
    let outcome = ok(produce(&command));
    assert_eq!(outcome["examples"][0]["sql_template"], SELECT_SQL);
    assert_eq!(outcome["examples"][1]["sql_template"], ENTRY_SQL);
    assert_eq!(outcome["examples"][0]["validation"], "not_run");
    assert_eq!(outcome["examples"][1]["validation"], "not_run");
    assert_eq!(outcome["gateway"]["configured"], false);
    assert_closed(&outcome);
    let path = format!("{dir}/live-command.json");
    std::fs::write(&path, serde_json::to_vec(&command).unwrap()).unwrap();
}

fn party_sql() -> String {
    "SELECT party_id FROM shop.account WHERE party_id IS NOT NULL LIMIT 1".to_string()
}

fn entry_sql_amount_fixture() -> String {
    "SELECT account_id FROM shop.entry WHERE account_id IS NOT NULL LIMIT 1".to_string()
}

fn metric<'a>(outcome: &'a Value, column: &str) -> &'a Value {
    outcome["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["column"] == column)
        .unwrap()
}

fn scripted_child() -> Vec<u8> {
    let row = json!({
        "record": "row",
        "turn": 0,
        "cells": [{"kind": "text", "value": "scripted-not-a-receipt"}]
    });
    let summary = json!({
        "record": "summary",
        "turn": 0,
        "status": "succeeded",
        "returned_rows": 1
    });
    let mut bytes = serde_json::to_vec(&row).unwrap();
    bytes.push(b'\n');
    bytes.extend(serde_json::to_vec(&summary).unwrap());
    bytes.push(b'\n');
    bytes
}

fn gate<'a>(probe: &'a str, max_validated: usize, runner: &'a mut dyn SelectRunner) -> Gateway<'a> {
    Gateway {
        probe_sql: probe,
        max_validated,
        runner,
    }
}

struct Script {
    calls: Vec<String>,
    answers: Vec<Spawned>,
}

impl SelectRunner for Script {
    fn run_select(&mut self, sql: &str) -> Spawned {
        self.calls.push(sql.to_string());
        self.answers.remove(0)
    }
}

fn ok(result: Result<Value, Error>) -> Value {
    match result {
        Ok(outcome) => outcome,
        Err(Error::Malformed(reason)) => panic!("malformed {reason}"),
        Err(Error::Refused { reason, .. }) => panic!("refused {reason}"),
    }
}

fn refused(result: Result<Value, Error>) -> (&'static str, Value) {
    match result {
        Err(Error::Refused { reason, outcome }) => (reason, outcome),
        Ok(_) => panic!("expected refusal"),
        Err(Error::Malformed(reason)) => panic!("malformed {reason}"),
    }
}

fn assert_closed(outcome: &Value) {
    let effects = &outcome["effects"];
    for key in [
        "auto_merge",
        "curated_written",
        "grants_issued",
        "metrics_certified",
        "model_called",
        "published",
        "roles_changed",
        "samples_retained",
        "source_queried",
    ] {
        assert_eq!(effects[key], false, "{key}");
    }
    assert_eq!(effects["knowledge_verified_count"], 0);
    assert_unverified(outcome);
    assert!(!outcome.to_string().contains("\"knowledge_verified\":true"));
}

fn assert_unverified(value: &Value) {
    match value {
        Value::Object(map) => {
            if let Some(flag) = map.get("knowledge_verified") {
                assert_eq!(flag, &Value::Bool(false));
            }
            for item in map.values() {
                assert_unverified(item);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_unverified(item);
            }
        }
        _ => {}
    }
}

fn clean() -> Value {
    let mut command = shell("review", "review");
    command["catalog"] = shop_catalog("review");
    command["relationships"]["items"] = json!([
        rel("review", "account_party_fk", "account", "party_id", "party", "party_id"),
        rel("review", "entry_account_fk", "entry", "account_id", "account", "account_id")
    ]);
    command["prior_candidate_ids"] = json!([ENTITY_ID]);
    seal(&mut command);
    command
}

fn live_command() -> Value {
    let mut command = shell("a364", "a364");
    command["catalog"] = json!({
        "schema": "oo.selfbuild-suggestions.catalog/v1",
        "source_id": "a364",
        "schema_hash": "b364".repeat(16),
        "tables": [
            table("party", "party_key", "text", json!([{"name": "party_key", "type_name": "text"}])),
            table(
                "account",
                "account_id",
                "int8",
                json!([
                    {"name": "account_id", "type_name": "int8"},
                    {"name": "party_key", "type_name": "text"}
                ])
            ),
            table(
                "entry",
                "entry_id",
                "int8",
                json!([
                    {"name": "entry_id", "type_name": "int8"},
                    {"name": "account_id", "type_name": "int8"}
                ])
            )
        ],
        "foreign_keys": [
            fk("account_party_fk", "account", "party_key", "party", "party_key"),
            fk("entry_account_fk", "entry", "account_id", "account", "account_id")
        ]
    });
    command["relationships"]["items"] = json!([
        rel("a364", "account_party_fk", "account", "party_key", "party", "party_key"),
        rel("a364", "entry_account_fk", "entry", "account_id", "account", "account_id")
    ]);
    seal(&mut command);
    command
}

fn shop_catalog(source: &str) -> Value {
    json!({
        "schema": "oo.selfbuild-suggestions.catalog/v1",
        "source_id": source,
        "schema_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "tables": [
            table(
                "party",
                "party_id",
                "int8",
                json!([
                    {"name": "party_id", "type_name": "int8"},
                    {"name": "phone", "type_name": "text"}
                ])
            ),
            table(
                "account",
                "account_id",
                "int8",
                json!([
                    {"name": "account_id", "type_name": "int8"},
                    {"name": "party_id", "type_name": "int8"},
                    {"name": "amount", "type_name": "numeric"},
                    {"name": "posted_on", "type_name": "date"}
                ])
            ),
            table(
                "entry",
                "entry_id",
                "int8",
                json!([
                    {"name": "entry_id", "type_name": "int8"},
                    {"name": "account_id", "type_name": "int8"},
                    {"name": "qty", "type_name": "int8"}
                ])
            )
        ],
        "foreign_keys": [
            fk("account_party_fk", "account", "party_id", "party", "party_id"),
            fk("entry_account_fk", "entry", "account_id", "account", "account_id")
        ]
    })
}

fn table(name: &str, pk: &str, pk_type: &str, columns: Value) -> Value {
    let _ = pk_type;
    json!({
        "schema": "shop",
        "name": name,
        "primary_key": [pk],
        "columns": columns
    })
}

fn fk(name: &str, table_name: &str, column: &str, ref_table: &str, ref_column: &str) -> Value {
    json!({
        "name": name,
        "schema": "shop",
        "table": table_name,
        "columns": [column],
        "ref_schema": "shop",
        "ref_table": ref_table,
        "ref_columns": [ref_column]
    })
}

fn rel(
    project: &str,
    name: &str,
    table_name: &str,
    column: &str,
    ref_table: &str,
    ref_column: &str,
) -> Value {
    json!({
        "project": project,
        "provenance": "declared_fk",
        "review_state": "approved",
        "name": name,
        "schema": "shop",
        "table": table_name,
        "columns": [column],
        "ref_schema": "shop",
        "ref_table": ref_table,
        "ref_columns": [ref_column]
    })
}

fn shell(project: &str, source: &str) -> Value {
    json!({
        "schema": "oo.selfbuild-suggestions.command/v1",
        "scope": {
            "project": project,
            "role": "oo_r_reviewer",
            "evidence_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "source_id": source,
            "schema_version": "oos/v1",
            "model_scope": "none"
        },
        "authorization": {
            "authorized": true,
            "catalog_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "relationships_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "profile_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "feedback_sha256": "0000000000000000000000000000000000000000000000000000000000000000"
        },
        "catalog": {},
        "relationships": {"schema": "oo.selfbuild-suggestions.relationships/v1", "items": []},
        "profile": {"schema": "oo.selfbuild-suggestions.profile/v1", "sample_policy": "none", "tables": []},
        "feedback": {"schema": "oo.selfbuild-suggestions.feedback/v1", "items": []},
        "negative_pairs": [],
        "curated_overrides": [],
        "prior_candidate_ids": []
    })
}

fn seal(command: &mut Value) {
    let catalog = canonical_sha256(&command["catalog"]);
    let relationships = canonical_sha256(&command["relationships"]);
    let profile = canonical_sha256(&command["profile"]);
    let feedback = canonical_sha256(&command["feedback"]);
    command["authorization"]["catalog_sha256"] = json!(catalog);
    command["authorization"]["relationships_sha256"] = json!(relationships);
    command["authorization"]["profile_sha256"] = json!(profile);
    command["authorization"]["feedback_sha256"] = json!(feedback);
    command["scope"]["evidence_sha256"] = json!(catalog);
}
