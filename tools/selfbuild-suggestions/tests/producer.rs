use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use selfbuild_suggestions::{
    DOCUMENT_MAX_BYTES, Error, Gateway, ProcessRunner, SelectRunner, Spawned, candidate_id,
    canonical_sha256, parse_document, produce, produce_with,
};
use serde_json::{Value, json};

const METRIC_ID: &str = "cand_5544a04dbfef7e7def73d6ac050b29d3";
const SCHEMA_HASH: &str = "eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74";

#[test]
fn public_fixture_is_deterministic_and_does_not_publish() {
    let command = public_command();
    let first = ok(produce(&command));
    let second = ok(produce(&command));
    assert_eq!(first, second);
    assert_eq!(first["schema"], "oo.selfbuild-suggestions.outcome/v1");
    assert_eq!(first["effects"], effects());
    assert_closed(&first);
    assert_eq!(first["gateway"]["configured"], false);
    assert_eq!(first["gateway"]["probe"], "not_run");
    assert_eq!(first["gateway"]["spawned"], 0);
    assert_eq!(first["gateway"]["admitted"], 0);
    assert_eq!(first["scope"]["schema_hash"], "unsealed");
    assert_eq!(first["scope"]["model_scope"], "none");

    let entities = first["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 3);
    let connected = entity_with(entities, "entity:logistics.customer|logistics.shipment");
    assert_eq!(
        connected["members"],
        json!(["logistics.customer", "logistics.shipment"])
    );
    assert_eq!(connected["primary"], "logistics.customer");
    assert_eq!(connected["domain"], "transport");
    assert_eq!(connected["provenance"], "declared_fk");
    assert_eq!(connected["confidence"], "graph");
    assert_eq!(connected["auto_merge"], false);
    assert_eq!(connected["strong_key_status"], "weak");
    assert_eq!(
        connected["weak_columns"],
        json!([
            "logistics.customer.display_name",
            "logistics.customer.phone"
        ])
    );
    assert_eq!(
        connected["profile_reltuples"],
        json!([{"reltuples": "12", "table": "logistics.shipment"}])
    );
    let entity_fact = json!({
        "auto_merge": false,
        "confidence": "graph",
        "domain": "transport",
        "members": ["logistics.customer", "logistics.shipment"],
        "primary": "logistics.customer",
        "provenance": "declared_fk",
        "strong_key_status": "weak",
        "weak_columns": ["logistics.customer.display_name", "logistics.customer.phone"],
    });
    assert_eq!(connected["fact_sha256"], canonical_sha256(&entity_fact));
    assert_eq!(
        entity_with(entities, "entity:logistics.alias")["provenance"],
        "isolated"
    );
    assert_eq!(
        entity_with(entities, "entity:logistics.alias")["confidence"],
        "lower"
    );
    assert_eq!(
        entity_with(entities, "entity:logistics.alias")["strong_key_status"],
        "unknown"
    );
    assert_eq!(
        entity_with(entities, "entity:logistics.warehouse")["members"],
        json!(["logistics.warehouse"])
    );

    assert_eq!(first["domains"].as_array().unwrap().len(), 1);
    assert_eq!(first["domains"][0]["domain_id"], "transport");
    assert_eq!(
        first["domains"][0]["members"],
        json!([
            "logistics.alias",
            "logistics.customer",
            "logistics.shipment",
            "logistics.warehouse"
        ])
    );

    let metrics = first["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 1);
    let metric = &metrics[0];
    assert_eq!(metric["candidate_id"], METRIC_ID);
    assert_eq!(
        candidate_id("public", "metric", "logistics.shipment.amount"),
        METRIC_ID
    );
    assert_eq!(metric["key"], "metric:logistics.shipment.amount");
    assert_eq!(metric["certified"], false);
    assert_eq!(metric["status"], "draft");
    assert_eq!(metric["owner_needed"], true);
    assert_eq!(metric["control_total_needed"], true);
    assert_eq!(metric["grain"], json!(["month"]));
    assert_eq!(metric["time_dimension"], "received_at");
    assert_eq!(metric["time_anchor"], "prev_month_start");
    assert_eq!(metric["time_anchor_closed"], false);
    assert_eq!(metric["time_anchor_needed"], false);
    assert_eq!(
        metric["sql_template"],
        "SELECT sum(amount) FROM logistics.shipment"
    );
    assert_eq!(metric["validation"], "not_applicable");
    assert_eq!(metric["gateway_admitted"], false);
    assert_eq!(metric["prior_overlap"], true);
    let metric_fact = json!({
        "certified": false,
        "column": "amount",
        "control_total_needed": true,
        "grain": ["month"],
        "owner_needed": true,
        "sql_template": "SELECT sum(amount) FROM logistics.shipment",
        "status": "draft",
        "table": "logistics.shipment",
        "time_anchor": "prev_month_start",
        "time_anchor_closed": false,
        "time_anchor_needed": false,
        "time_dimension": "received_at",
    });
    assert_eq!(metric["fact_sha256"], canonical_sha256(&metric_fact));

    let examples = first["examples"].as_array().unwrap();
    assert_eq!(examples.len(), 1);
    assert_eq!(examples[0]["constraint"], "shipment_customer_fk");
    assert_eq!(
        examples[0]["sql_template"],
        "SELECT customer_id FROM logistics.shipment WHERE customer_id IS NOT NULL LIMIT 1"
    );
    assert_eq!(
        examples[0]["question"],
        "Which values of logistics.shipment.customer_id does shipment_customer_fk relate?"
    );
    assert_eq!(examples[0]["validation"], "not_run");
    assert_eq!(examples[0]["gateway_admitted"], false);

    let feedback = first["feedback"].as_array().unwrap();
    assert_eq!(feedback.len(), 1);
    assert_eq!(feedback[0]["text"], "INJECT_GRANT_TOKEN");
    assert_eq!(feedback[0]["untrusted"], true);
    assert_eq!(feedback[0]["trusted_metric"], false);
    assert_eq!(first["dedup"]["feedback_collapsed"], 1);
    assert_eq!(first["dedup"]["relationships_collapsed"], 1);
    assert_eq!(first["dedup"]["relationships_rejected"], 3);
    assert_eq!(first["dedup"]["negative_collapsed"], 0);
    assert_eq!(first["dedup"]["prior_overlaps"], json!([METRIC_ID]));
    assert_eq!(first["scope_drops"]["feedback"], 1);
    assert_eq!(first["scope_drops"]["relationships"], 1);

    let queue = first["queue"].as_array().unwrap();
    let scores: Vec<u64> = queue
        .iter()
        .map(|item| item["score"].as_u64().unwrap())
        .collect();
    assert_eq!(scores, vec![7200, 4200, 3600, 3000, 2000, 1600, 1500, 1500]);
    let channels: Vec<&str> = queue
        .iter()
        .map(|item| item["channel"].as_str().unwrap())
        .collect();
    assert_eq!(
        channels,
        vec![
            "stale_override",
            "metric_hypothesis",
            "example",
            "negative_entity",
            "untrusted_feedback",
            "entity",
            "entity",
            "entity"
        ]
    );
    assert_eq!(queue[0]["key"], "metric:logistics.shipment.amount");
    assert_eq!(queue[1]["candidate_id"], METRIC_ID);
    assert_eq!(queue[2]["uncertainty_millis"], 900);
    assert_eq!(
        queue[5]["key"],
        "entity:logistics.customer|logistics.shipment"
    );
    assert!(queue.iter().all(|item| item["trusted_metric"] == false));
    let body = serde_json::to_string(&first).unwrap();
    assert!(!body.contains("CROSS_PROJECT_TOKEN"));
    assert!(!body.contains("pair_count"));
    assert!(!body.contains("estimated_pairs"));
    assert!(!body.contains("curated/"));
    for value in sql_and_questions(&first) {
        assert!(!value.contains("INJECT_GRANT_TOKEN"));
        assert!(!value.contains(';'));
    }

    let mut matched = command.clone();
    matched["curated_overrides"][0]["fact_sha256"] = metric["fact_sha256"].clone();
    let quiet = ok(produce(&matched));
    assert!(
        quiet["queue"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["channel"] != "stale_override")
    );
    assert_eq!(quiet["effects"]["curated_written"], false);
}

#[test]
fn sealed_shop_catalog_names_party_as_primary_and_refuses_a_tamper() {
    let catalog = sealed_catalog();
    assert_eq!(catalog["schema_hash"], SCHEMA_HASH);
    assert_eq!(catalog["protocol"], "oo.source.pgsql/v1");
    let outcome = ok(produce(&sealed_command(&catalog)));
    assert_eq!(outcome["effects"], effects());
    assert_closed(&outcome);
    assert_eq!(outcome["scope"]["project"], "pubfix");
    assert_eq!(outcome["scope"]["schema_hash"], SCHEMA_HASH);
    let body = serde_json::to_string(&outcome).unwrap();
    assert!(!body.contains("oo_fixture_admin"));
    assert!(!body.contains("current_privileges"));
    let entities = outcome["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 1);
    assert_eq!(
        entities[0]["members"],
        json!(["shop.account", "shop.entry", "shop.party"])
    );
    assert_eq!(entities[0]["primary"], "shop.party");
    assert_eq!(entities[0]["domain"], "commerce");
    assert_eq!(entities[0]["provenance"], "declared_fk");
    assert_eq!(entities[0]["strong_key_status"], "weak");
    assert_eq!(entities[0]["auto_merge"], false);
    assert_eq!(
        entities[0]["weak_columns"],
        json!(["shop.party.display_name", "shop.party.phone"])
    );
    let weak = serde_json::to_string(&entities[0]["weak_columns"]).unwrap();
    assert!(!weak.contains("national_id"));
    let metrics = outcome["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 2);
    assert_eq!(metrics[0]["key"], "metric:shop.entry.amount");
    assert_eq!(metrics[1]["key"], "metric:shop.entry.qty");
    for metric in metrics {
        assert_eq!(metric["grain"], json!(["row"]));
        assert_eq!(metric["time_dimension"], Value::Null);
        assert_eq!(metric["time_anchor"], Value::Null);
        assert_eq!(metric["time_anchor_needed"], true);
        assert_eq!(metric["certified"], false);
        assert_eq!(metric["validation"], "not_applicable");
        assert_eq!(metric["owner_needed"], true);
        assert_eq!(metric["control_total_needed"], true);
    }
    let examples = outcome["examples"].as_array().unwrap();
    assert_eq!(examples.len(), 2);
    assert_eq!(examples[0]["constraint"], "account_party_fk");
    assert_eq!(
        examples[0]["sql_template"],
        "SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1"
    );
    assert_eq!(examples[1]["constraint"], "entry_account_fk");
    assert_eq!(
        examples[1]["sql_template"],
        "SELECT account_id FROM shop.entry WHERE account_id IS NOT NULL LIMIT 1"
    );
    assert!(
        examples
            .iter()
            .all(|item| item["validation"] == "not_run" && item["gateway_admitted"] == false)
    );
    assert!(!body.contains("INJECT_GRANT_TOKEN") || feedback_only(&outcome));
    for value in sql_and_questions(&outcome) {
        assert!(!value.contains("INJECT_GRANT_TOKEN"));
    }

    let mut tampered = sealed_command(&catalog);
    let original = tampered["authorization"]["catalog_sha256"].clone();
    tampered["catalog"]["tables"][2]["columns"][4]["name"] = json!("memo_x");
    tampered["authorization"]["catalog_sha256"] = original.clone();
    tampered["scope"]["evidence_sha256"] = original;
    let refused = refuse(produce(&tampered));
    assert_eq!(refused.0, "seal");
    assert_eq!(refused.1["entities"].as_array().unwrap().len(), 0);
    assert_eq!(refused.1["effects"]["knowledge_verified_count"], 0);
    assert_eq!(refused.1["gateway"]["spawned"], 0);
}

#[test]
fn graph_rules_keep_weak_keys_unmerged() {
    let cycle = ok(produce(&link_command(
        "loop",
        vec![
            table(
                "loop",
                "aa",
                "aa_id",
                &[("aa_id", "int8"), ("bb_id", "int8")],
            ),
            table(
                "loop",
                "bb",
                "bb_id",
                &[("bb_id", "int8"), ("aa_id", "int8")],
            ),
        ],
        vec![
            fk("aa_bb_fk", "loop", "aa", "bb_id", "loop", "bb", "bb_id"),
            fk("bb_aa_fk", "loop", "bb", "aa_id", "loop", "aa", "aa_id"),
        ],
    )));
    let entity = &cycle["entities"][0];
    assert_eq!(entity["primary"], "loop.aa");
    assert_eq!(entity["provenance"], "declared_fk");
    assert_eq!(entity["auto_merge"], false);

    let tie = ok(produce(&link_command(
        "s",
        vec![
            table(
                "s",
                "c",
                "c_id",
                &[("c_id", "int8"), ("m_id", "int8"), ("z_id", "int8")],
            ),
            table("s", "m", "m_id", &[("m_id", "int8")]),
            table("s", "z", "z_id", &[("z_id", "int8")]),
        ],
        vec![
            fk("c_m_fk", "s", "c", "m_id", "s", "m", "m_id"),
            fk("c_z_fk", "s", "c", "z_id", "s", "z", "z_id"),
        ],
    )));
    assert_eq!(tie["entities"][0]["primary"], "s.m");

    let cross = ok(produce(&link_command(
        "public",
        vec![
            table(
                "shop",
                "customer",
                "customer_id",
                &[("customer_id", "int8")],
            ),
            table(
                "logistics",
                "customer",
                "customer_id",
                &[("customer_id", "int8"), ("shop_id", "int8")],
            ),
        ],
        vec![fk(
            "shop_fk",
            "logistics",
            "customer",
            "shop_id",
            "shop",
            "customer",
            "customer_id",
        )],
    )));
    assert_eq!(cross["entities"].as_array().unwrap().len(), 1);
    assert_eq!(cross["domains"][0]["domain_id"], "cross_source");
    assert_eq!(cross["entities"][0]["auto_merge"], false);
    assert_eq!(
        cross["entities"][0]["members"],
        json!(["logistics.customer", "shop.customer"])
    );

    let validated = ok(produce(&profiled_command(false)));
    assert_eq!(
        validated["entities"][0]["strong_key_status"],
        "validated_unmerged"
    );
    assert_eq!(validated["entities"][0]["auto_merge"], false);
    let weak = ok(produce(&profiled_command(true)));
    assert_eq!(weak["entities"][0]["strong_key_status"], "weak");
    assert_eq!(weak["entities"][0]["auto_merge"], false);

    let numeric = ok(produce(&link_command(
        "public",
        vec![table(
            "shop",
            "entry",
            "entry_id",
            &[
                ("entry_id", "int8"),
                ("total", "numeric"),
                ("active", "bool"),
            ],
        )],
        vec![],
    )));
    assert!(numeric["metrics"].as_array().unwrap().is_empty());
    assert_eq!(numeric["entities"][0]["strong_key_status"], "unknown");
}

#[test]
fn catalog_and_scope_refusals_do_not_emit_partial_candidates() {
    let mismatch = link_command(
        "public",
        vec![
            table("shop", "parent", "parent_id", &[("parent_id", "int8")]),
            table(
                "shop",
                "child",
                "child_id",
                &[("child_id", "int8"), ("parent_id", "text")],
            ),
        ],
        vec![fk(
            "child_fk",
            "shop",
            "child",
            "parent_id",
            "shop",
            "parent",
            "parent_id",
        )],
    );
    let refused = refuse(produce(&mismatch));
    assert_eq!(refused.0, "catalog");
    assert!(refused.1["entities"].as_array().unwrap().is_empty());

    let wide = link_command(
        "public",
        vec![
            table(
                "shop",
                "parent",
                "parent_id",
                &[("parent_id", "int8"), ("other_id", "int8")],
            ),
            table(
                "shop",
                "child",
                "child_id",
                &[
                    ("child_id", "int8"),
                    ("parent_id", "int8"),
                    ("other_id", "int8"),
                ],
            ),
        ],
        vec![json!({
            "name": "wide_fk",
            "schema": "shop",
            "table": "child",
            "columns": ["parent_id", "other_id"],
            "ref_schema": "shop",
            "ref_table": "parent",
            "ref_columns": ["parent_id", "other_id"]
        })],
    );
    assert_eq!(refuse(produce(&wide)).0, "catalog");

    let compatible = ok(produce(&link_command(
        "public",
        vec![
            table("shop", "parent", "parent_id", &[("parent_id", "int8")]),
            table(
                "shop",
                "child",
                "child_id",
                &[("child_id", "int8"), ("parent_id", "int4")],
            ),
        ],
        vec![fk(
            "child_fk",
            "shop",
            "child",
            "parent_id",
            "shop",
            "parent",
            "parent_id",
        )],
    )));
    assert_eq!(compatible["entities"][0]["provenance"], "declared_fk");

    let mut command = link_command(
        "public",
        vec![table("shop", "party", "party_id", &[("party_id", "int8")])],
        vec![],
    );
    command["scope"]["model_scope"] = json!("assistant");
    let refused = refuse(produce(&command));
    assert_eq!(refused.0, "scope");
    assert_eq!(refused.1["effects"]["model_called"], false);
    assert_eq!(refused.1["effects"]["knowledge_verified_count"], 0);

    command["scope"]["model_scope"] = json!("none");
    command["authorization"]["authorized"] = json!(false);
    assert_eq!(refuse(produce(&command)).0, "authorization");

    let mut profiled = link_command(
        "public",
        vec![table("shop", "party", "party_id", &[("party_id", "int8")])],
        vec![],
    );
    profiled["profile"]["sample_policy"] = json!("rows");
    profiled = seal(profiled);
    assert_eq!(refuse(produce(&profiled)).0, "profile");

    command["authorization"]["authorized"] = json!(true);
    command["note"] = json!("nope");
    assert_eq!(malformed(produce(&command)), "unknown_field");
}

#[test]
fn gateway_admission_follows_a_refused_probe_and_never_verifies() {
    let mut command = link_command(
        "public",
        vec![
            table("shop", "party", "party_key", &[("party_key", "text")]),
            table(
                "shop",
                "account",
                "account_id",
                &[("account_id", "int8"), ("party_key", "text")],
            ),
            table(
                "shop",
                "entry",
                "entry_id",
                &[
                    ("entry_id", "int8"),
                    ("account_id", "int8"),
                    ("amount", "numeric"),
                ],
            ),
        ],
        vec![
            fk(
                "account_party_fk",
                "shop",
                "account",
                "party_key",
                "shop",
                "party",
                "party_key",
            ),
            fk(
                "entry_account_fk",
                "shop",
                "entry",
                "account_id",
                "shop",
                "account",
                "account_id",
            ),
        ],
    );
    let probe = "UPDATE shop.account SET label = label";
    let mut fake = Fake {
        calls: Vec::new(),
        script: vec![spawned(6, Vec::new()), spawned(0, summary_stdout(1))],
    };
    let outcome = ok(produce_with(&command, Some(gate(probe, 1, &mut fake))));
    assert_eq!(fake.calls.len(), 2);
    assert_eq!(fake.calls[0], probe);
    assert_eq!(
        fake.calls[1],
        "SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1"
    );
    assert!(fake.calls.iter().all(|sql| !sql.contains("sum(")));
    assert_eq!(outcome["gateway"]["probe"], "refused");
    assert_eq!(outcome["gateway"]["probe_exit"], 6);
    assert_eq!(outcome["gateway"]["admitted"], 1);
    assert_eq!(outcome["examples"][0]["validation"], "admitted");
    assert_eq!(outcome["examples"][0]["gateway_admitted"], true);
    assert_eq!(outcome["examples"][1]["validation"], "not_run");
    assert_eq!(outcome["examples"][1]["gateway_admitted"], false);
    let admitted = outcome["queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "example:shop.account.party_key:account_party_fk")
        .unwrap();
    assert_eq!(admitted["uncertainty_millis"], 400);
    assert_closed(&outcome);
    assert_eq!(outcome["effects"]["knowledge_verified_count"], 0);

    let mut rejected = Fake {
        calls: Vec::new(),
        script: vec![spawned(0, summary_stdout(1))],
    };
    let error = refuse(produce_with(&command, Some(gate(probe, 1, &mut rejected))));
    assert_eq!(error.0, "probe");
    assert_eq!(rejected.calls.len(), 1);
    assert!(
        error.1["examples"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["gateway_admitted"] == false)
    );
    assert_eq!(error.1["gateway"]["probe"], "not_refused");
    assert_eq!(error.1["gateway"]["admitted"], 0);
    assert_closed(&error.1);
    assert!(!error.1["metrics"].as_array().unwrap().is_empty());

    let mut dirty = Fake {
        calls: Vec::new(),
        script: vec![spawned(6, b"no\n".to_vec())],
    };
    assert_eq!(
        refuse(produce_with(&command, Some(gate(probe, 1, &mut dirty)))).0,
        "probe"
    );
    assert_eq!(dirty.calls.len(), 1);

    let mut empty = Fake {
        calls: Vec::new(),
        script: vec![spawned(6, Vec::new()), spawned(0, summary_stdout(0))],
    };
    let outcome = ok(produce_with(&command, Some(gate(probe, 1, &mut empty))));
    assert_eq!(outcome["examples"][0]["validation"], "empty");
    assert_eq!(outcome["examples"][0]["gateway_admitted"], false);

    let mut guarded = Fake {
        calls: Vec::new(),
        script: vec![spawned(6, Vec::new()), spawned(6, Vec::new())],
    };
    command["negative_pairs"] = json!([]);
    let outcome = ok(produce_with(&command, Some(gate(probe, 1, &mut guarded))));
    assert_eq!(outcome["examples"][0]["validation"], "guard_refused");
    assert_eq!(outcome["examples"][0]["gateway_admitted"], false);
    assert_closed(&outcome);
}

#[test]
fn cli_exits_match_the_contract() {
    let usage = run(&[], b"");
    assert_eq!(usage.status.code(), Some(1));
    assert!(usage.stdout.is_empty());
    assert_eq!(usage.stderr, b"selfbuild-suggestions: malformed usage\n");

    let extra = run(&["produce", "--config"], b"{}");
    assert_eq!(extra.status.code(), Some(1));
    assert!(extra.stdout.is_empty());

    let duplicate = run(&["produce"], br#"{"a":1,"a":2}"#);
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(duplicate.stdout.is_empty());
    assert_eq!(
        duplicate.stderr,
        b"selfbuild-suggestions: malformed duplicate_key\n"
    );

    let trailing = run(&["produce"], br#"{"a":1}x"#);
    assert_eq!(trailing.status.code(), Some(1));
    assert!(trailing.stdout.is_empty());
    assert_eq!(
        trailing.stderr,
        b"selfbuild-suggestions: malformed trailing\n"
    );

    let empty = run(&["produce"], b"");
    assert_eq!(empty.status.code(), Some(1));
    assert!(empty.stdout.is_empty());
    assert_eq!(empty.stderr, b"selfbuild-suggestions: malformed empty\n");

    let bytes = serde_json::to_vec(&minimal_command()).unwrap();
    let ok_run = run(&["produce"], &bytes);
    assert_eq!(
        ok_run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ok_run.stderr)
    );
    assert!(ok_run.stderr.is_empty());
    let outcome: Value = serde_json::from_slice(&ok_run.stdout).unwrap();
    assert_eq!(outcome["schema"], "oo.selfbuild-suggestions.outcome/v1");
    assert_eq!(outcome["effects"], effects());
    assert!(outcome["entities"].as_array().unwrap().is_empty());

    let mut bad = minimal_command();
    bad["catalog"]["schema_hash"] = json!("unsealed");
    bad["authorization"]["catalog_sha256"] =
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    bad["scope"]["evidence_sha256"] = bad["authorization"]["catalog_sha256"].clone();
    let refused = run(&["produce"], &serde_json::to_vec(&bad).unwrap());
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(refused.stderr, b"selfbuild-suggestions: refused seal\n");
    let outcome: Value = serde_json::from_slice(refused.stdout.trim_ascii_end()).unwrap();
    assert_eq!(outcome["effects"]["published"], false);

    let missing = run(
        &["produce", "--config", "/tmp/oo-a355-missing-config.json"],
        &bytes,
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert_eq!(missing.stderr, b"selfbuild-suggestions: malformed config\n");
}

#[test]
#[ignore]
fn gateway_probe_is_refused_before_one_admitted_select() {
    let bin = std::env::var("OO_A355_QUERY_PG").expect("OO_A355_QUERY_PG");
    let request_path = std::env::var("OO_A355_REQUEST").expect("OO_A355_REQUEST");
    let request = parse_document(&fs::read(&request_path).expect("request")).expect("request json");
    let config = json!({
        "schema": "oo.selfbuild-suggestions.gateway-config/v1",
        "query_pg_bin": bin,
        "guard_probe_sql": "UPDATE shop.account SET label = label",
        "max_validated": 1,
        "request": request,
    });
    let (runner, probe, max_validated) = ProcessRunner::open(&config).expect("config");
    let mut runner = Recording {
        inner: runner,
        calls: Vec::new(),
    };
    let command = sealed_command(&sealed_catalog());
    let outcome = ok(produce_with(
        &command,
        Some(Gateway {
            probe_sql: &probe,
            max_validated,
            runner: &mut runner,
        }),
    ));
    assert_eq!(runner.calls[0], "UPDATE shop.account SET label = label");
    assert_eq!(
        runner.calls[1],
        "SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1"
    );
    assert!(runner.calls.iter().all(|sql| !sql.contains("sum(")));
    assert_eq!(outcome["gateway"]["probe"], "refused");
    assert_eq!(outcome["gateway"]["probe_exit"], 6);
    assert_eq!(outcome["gateway"]["admitted"], 1);
    assert_eq!(outcome["examples"][0]["validation"], "admitted");
    assert_eq!(outcome["examples"][0]["gateway_admitted"], true);
    assert_eq!(outcome["examples"][1]["validation"], "not_run");
    assert_closed(&outcome);
    assert_eq!(outcome["effects"], effects());
}

struct Recording {
    inner: ProcessRunner,
    calls: Vec<String>,
}

impl SelectRunner for Recording {
    fn run_select(&mut self, sql: &str) -> Spawned {
        self.calls.push(sql.to_string());
        self.inner.run_select(sql)
    }
}

struct Fake {
    calls: Vec<String>,
    script: Vec<Spawned>,
}

impl SelectRunner for Fake {
    fn run_select(&mut self, sql: &str) -> Spawned {
        self.calls.push(sql.to_string());
        self.script.remove(0)
    }
}

fn gate<'a>(probe: &'a str, max_validated: usize, runner: &'a mut dyn SelectRunner) -> Gateway<'a> {
    Gateway {
        probe_sql: probe,
        max_validated,
        runner,
    }
}

fn spawned(exit: i32, stdout: Vec<u8>) -> Spawned {
    Spawned {
        exit: Some(exit),
        stdout,
        timed_out: false,
        started: true,
    }
}

fn summary_stdout(rows: u64) -> Vec<u8> {
    format!(
        "{{\"record\":\"row\",\"turn\":0,\"cells\":[]}}\n{{\"record\":\"summary\",\"turn\":0,\"status\":\"succeeded\",\"returned_rows\":{rows}}}\n"
    )
    .into_bytes()
}

fn ok(result: Result<Value, Error>) -> Value {
    match result {
        Ok(value) => value,
        Err(Error::Malformed(reason)) => panic!("malformed {reason}"),
        Err(Error::Refused { reason, .. }) => panic!("refused {reason}"),
    }
}

fn refuse(result: Result<Value, Error>) -> (&'static str, Value) {
    match result {
        Err(Error::Refused { reason, outcome }) => (reason, outcome),
        Ok(_) => panic!("expected refusal"),
        Err(Error::Malformed(reason)) => panic!("malformed {reason}"),
    }
}

fn malformed(result: Result<Value, Error>) -> &'static str {
    match result {
        Err(Error::Malformed(reason)) => reason,
        other => panic!("expected malformed, got {other:?}"),
    }
}

fn effects() -> Value {
    json!({
        "auto_merge": false,
        "curated_written": false,
        "grants_issued": false,
        "knowledge_verified_count": 0,
        "metrics_certified": false,
        "model_called": false,
        "published": false,
        "roles_changed": false,
        "samples_retained": false,
        "source_queried": false,
    })
}

fn assert_closed(value: &Value) {
    match value {
        Value::Object(map) => {
            if let Some(flag) = map.get("knowledge_verified") {
                assert_eq!(flag, &json!(false));
            }
            if let Some(flag) = map.get("certified") {
                assert_eq!(flag, &json!(false));
            }
            if let Some(flag) = map.get("auto_merge") {
                assert_eq!(flag, &json!(false));
            }
            if let Some(flag) = map.get("trusted_metric") {
                assert_eq!(flag, &json!(false));
            }
            for child in map.values() {
                assert_closed(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_closed(item);
            }
        }
        _ => {}
    }
}

fn sql_and_questions(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    walk_text(value, &mut found);
    found
}

fn walk_text(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if (key == "sql_template" || key == "question")
                    && let Some(text) = child.as_str()
                {
                    found.push(text.to_string());
                }
                walk_text(child, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_text(item, found);
            }
        }
        _ => {}
    }
}

fn feedback_only(value: &Value) -> bool {
    serde_json::to_string(&value["feedback"])
        .unwrap()
        .contains("INJECT_GRANT_TOKEN")
}

fn entity_with<'a>(entities: &'a [Value], key: &str) -> &'a Value {
    entities
        .iter()
        .find(|item| item["key"] == key)
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn public_command() -> Value {
    let shipment = fk(
        "shipment_customer_fk",
        "logistics",
        "shipment",
        "customer_id",
        "logistics",
        "customer",
        "customer_id",
    );
    let alias = fk(
        "alias_customer_fk",
        "logistics",
        "alias",
        "customer_id",
        "logistics",
        "customer",
        "customer_id",
    );
    let mut command = projection_command(
        "public",
        vec![
            table(
                "logistics",
                "customer",
                "customer_id",
                &[
                    ("customer_id", "int8"),
                    ("display_name", "text"),
                    ("phone", "text"),
                ],
            ),
            table(
                "logistics",
                "shipment",
                "shipment_id",
                &[
                    ("shipment_id", "int8"),
                    ("customer_id", "int8"),
                    ("amount", "numeric"),
                    ("received_at", "timestamptz"),
                ],
            ),
            table(
                "logistics",
                "warehouse",
                "warehouse_id",
                &[("warehouse_id", "int8"), ("label", "text")],
            ),
            table(
                "logistics",
                "alias",
                "alias_id",
                &[("alias_id", "int8"), ("customer_id", "int8")],
            ),
        ],
        vec![shipment.clone(), alias.clone()],
    );
    command["relationships"]["items"] = json!([
        relationship("public", "declared_fk", "approved", &shipment),
        relationship("public", "declared_fk", "approved", &shipment),
        relationship("public", "declared_fk", "approved", &alias),
        relationship("public", "inferred", "approved", &shipment),
        relationship("public", "declared_fk", "pending", &shipment),
        relationship("otherproj", "declared_fk", "approved", &shipment),
        relationship(
            "public",
            "declared_fk",
            "approved",
            &fk(
                "not_a_fk",
                "logistics",
                "shipment",
                "missing",
                "logistics",
                "customer",
                "customer_id"
            )
        )
    ]);
    command["profile"]["tables"] = json!([{
        "schema": "logistics",
        "name": "shipment",
        "reltuples": "12",
        "columns": []
    }]);
    command["feedback"]["items"] = json!([
        {"project": "public", "kind": "comment", "text": "INJECT_GRANT_TOKEN", "target": "logistics.shipment"},
        {"project": "public", "kind": "comment", "text": "INJECT_GRANT_TOKEN", "target": "logistics.shipment"},
        {"project": "otherproj", "kind": "comment", "text": "CROSS_PROJECT_TOKEN", "target": "logistics.shipment"}
    ]);
    command["negative_pairs"] = json!([["logistics.customer", "logistics.alias"]]);
    command["curated_overrides"] = json!([
        {"key": "metric:logistics.shipment.amount", "fact_sha256": "0000000000000000000000000000000000000000000000000000000000000000"},
        {"key": "metric:no.such.column", "fact_sha256": "1111111111111111111111111111111111111111111111111111111111111111"}
    ]);
    command["prior_candidate_ids"] = json!([METRIC_ID]);
    seal(command)
}

fn sealed_command(catalog: &Value) -> Value {
    let mut items = Vec::new();
    for table in catalog["tables"].as_array().unwrap() {
        for fk in table["foreign_keys"].as_array().unwrap() {
            let pair = &fk["columns"][0];
            items.push(relationship(
                "pubfix",
                "declared_fk",
                "approved",
                &json!({
                    "name": fk["name"],
                    "schema": table["schema"],
                    "table": table["name"],
                    "columns": [pair["name"]],
                    "ref_schema": pair["ref_schema"],
                    "ref_table": pair["ref_table"],
                    "ref_columns": [pair["ref_name"]],
                }),
            ));
        }
    }
    let mut command = shell("pubfix", "pubfix");
    command["catalog"] = catalog.clone();
    command["relationships"]["items"] = Value::Array(items);
    command["feedback"]["items"] = json!([{
        "project": "pubfix",
        "kind": "comment",
        "text": "INJECT_GRANT_TOKEN",
        "target": "shop.entry"
    }]);
    seal(command)
}

fn sealed_catalog() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a349/SP11/real-pg-100k-review/live/run/catalog.json");
    parse_document(&fs::read(&path).expect("sealed catalog")).expect("catalog json")
}

fn link_command(project: &str, tables: Vec<Value>, foreign_keys: Vec<Value>) -> Value {
    seal(projection_command(project, tables, foreign_keys))
}

fn profiled_command(with_phone: bool) -> Value {
    let mut columns = vec![("id", "int8")];
    if with_phone {
        columns.push(("phone", "text"));
    }
    let mut command = projection_command(
        "public",
        vec![table("shop", "party", "id", &columns)],
        vec![],
    );
    let mut profile_columns =
        vec![json!({"name": "id", "semantic": "identifier", "strong_key": "validated"})];
    if with_phone {
        profile_columns.push(json!({"name": "phone"}));
    }
    command["profile"]["tables"] = json!([{
        "schema": "shop",
        "name": "party",
        "columns": profile_columns
    }]);
    seal(command)
}

fn projection_command(project: &str, tables: Vec<Value>, foreign_keys: Vec<Value>) -> Value {
    let mut command = shell(
        project,
        if project == "pubfix" {
            "pubfix"
        } else {
            "examples"
        },
    );
    command["catalog"] = json!({
        "schema": "oo.selfbuild-suggestions.catalog/v1",
        "source_id": command["scope"]["source_id"],
        "schema_hash": "unsealed",
        "tables": tables,
        "foreign_keys": foreign_keys,
    });
    let mut items = Vec::new();
    for fk in command["catalog"]["foreign_keys"].as_array().unwrap() {
        items.push(relationship(project, "declared_fk", "approved", fk));
    }
    command["relationships"]["items"] = Value::Array(items);
    command
}

fn minimal_command() -> Value {
    seal(projection_command("public", vec![], vec![]))
}

fn shell(project: &str, source: &str) -> Value {
    json!({
        "schema": "oo.selfbuild-suggestions.command/v1",
        "scope": {
            "project": project,
            "role": "oo_r_reviewer",
            "evidence_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "source_id": source,
            "schema_version": "oos/v1",
            "model_scope": "none"
        },
        "authorization": {
            "authorized": true,
            "catalog_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "relationships_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "profile_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "feedback_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
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

fn seal(mut command: Value) -> Value {
    let catalog = canonical_sha256(&command["catalog"]);
    let relationships = canonical_sha256(&command["relationships"]);
    let profile = canonical_sha256(&command["profile"]);
    let feedback = canonical_sha256(&command["feedback"]);
    command["authorization"]["catalog_sha256"] = json!(catalog);
    command["authorization"]["relationships_sha256"] = json!(relationships);
    command["authorization"]["profile_sha256"] = json!(profile);
    command["authorization"]["feedback_sha256"] = json!(feedback);
    command["scope"]["evidence_sha256"] = json!(catalog);
    command
}

fn table(schema: &str, name: &str, pk: &str, columns: &[(&str, &str)]) -> Value {
    json!({
        "schema": schema,
        "name": name,
        "primary_key": [pk],
        "columns": columns.iter().map(|(name, type_name)| json!({"name": name, "type_name": type_name})).collect::<Vec<_>>()
    })
}

fn fk(
    name: &str,
    schema: &str,
    table: &str,
    column: &str,
    ref_schema: &str,
    ref_table: &str,
    ref_column: &str,
) -> Value {
    json!({
        "name": name,
        "schema": schema,
        "table": table,
        "columns": [column],
        "ref_schema": ref_schema,
        "ref_table": ref_table,
        "ref_columns": [ref_column]
    })
}

fn relationship(project: &str, provenance: &str, review_state: &str, fk: &Value) -> Value {
    json!({
        "project": project,
        "provenance": provenance,
        "review_state": review_state,
        "name": fk["name"],
        "schema": fk["schema"],
        "table": fk["table"],
        "columns": fk["columns"],
        "ref_schema": fk["ref_schema"],
        "ref_table": fk["ref_table"],
        "ref_columns": fk["ref_columns"],
    })
}

fn suggestion_bin() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_selfbuild_suggestions")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    let mut path = std::env::current_exe().expect("current_exe");
    path.pop();
    path.pop();
    path.push("selfbuild-suggestions");
    path
}

fn run(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut child = Command::new(suggestion_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn parser_rejects_an_oversize_document() {
    let error = parse_document(&vec![b' '; DOCUMENT_MAX_BYTES + 1]).unwrap_err();
    assert_eq!(error, "stdin_too_large");
}
