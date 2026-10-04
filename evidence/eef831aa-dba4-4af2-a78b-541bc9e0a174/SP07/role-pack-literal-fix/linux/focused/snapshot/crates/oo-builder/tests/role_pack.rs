//! Focused role-pack checks against valid OOS documents.
//!
//! The visibility snapshot is an input. These tests do not treat a role name
//! as authentication or as a grant.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use oo_builder::{RolePack, RolePackError, compile_role_pack, normalize_pack_path};

fn compile(effective: &Value, visibility: &Value) -> Result<RolePack, RolePackError> {
    compile_role_pack(
        &serde_json::to_string(effective).expect("effective json"),
        &serde_json::to_string(visibility).expect("visibility json"),
    )
}

fn must(effective: &Value, visibility: &Value) -> RolePack {
    match compile(effective, visibility) {
        Ok(pack) => pack,
        Err(error) => panic!("compile failed: {error}"),
    }
}

fn none_col(name: &str) -> Value {
    json!({
        "name": name,
        "pii": "none",
        "classification": {"state": "confirmed"}
    })
}

fn analyst_documents() -> (Value, Value) {
    let mut phone = none_col("phone");
    phone["pii"] = json!("personal");
    phone["semantic_type"] = json!("phone");
    phone["description"] = json!("dial string for the account");
    phone["review"] = json!({"state": "approved"});
    phone["classification"]["by"] = json!("ops.wang");

    let mut old_name = none_col("old_name");
    old_name["status"] = json!("deprecated");
    old_name["replaced_by"] = json!("sales.customer.name");

    let mut alias_note = none_col("alias_note");
    alias_note["description"] = json!("legacy alias kept for readers");
    alias_note["provenance"] = json!("declared");

    let mut email = none_col("email");
    email["pii"] = json!("personal");
    email["description"] = json!("contact address");
    email["review"] = json!({"state": "approved"});

    let scratch = json!({"name": "scratch"});
    let mut pending_col = none_col("pending_col");
    pending_col["classification"] = json!({"state": "pending"});

    let mut amount = none_col("amount");
    amount["semantic_type"] = json!("money");
    amount["unit"] = json!("CNY");

    let mut status = none_col("status");
    status["semantic_type"] = json!("status_code");
    status["enum"] = json!("sales.order.status");

    let mut received_at = none_col("received_at");
    received_at["semantic_type"] = json!("event_time");

    let mut secret_note = none_col("secret_note");
    secret_note["pii"] = json!("sensitive");

    let mut customer_email = none_col("customer_email");
    customer_email["pii"] = json!("personal");

    let customer = json!({
        "schema_version": "oos/v1",
        "dataset": "sales.customer",
        "label": "Customer",
        "description": "buyer account file",
        "tier": "file",
        "grain": "one row per customer",
        "primary_key": ["customer_id"],
        "row_count_estimate": 100,
        "caveats": ["legacy load caveat"],
        "columns": [
            none_col("customer_id"),
            none_col("name"),
            phone,
            old_name,
            alias_note,
            email,
            scratch,
            pending_col
        ]
    });
    let order = json!({
        "schema_version": "oos/v1",
        "dataset": "sales.order",
        "label": "Order",
        "tier": "file",
        "grain": "one row per order",
        "primary_key": ["order_id"],
        "row_count_estimate": 5000,
        "time_columns": [{"name": "received_at", "role": "event_time"}],
        "columns": [
            none_col("order_id"),
            none_col("customer_id"),
            amount,
            status,
            received_at,
            secret_note
        ]
    });
    let account = json!({
        "schema_version": "oos/v1",
        "dataset": "sales.account",
        "label": "Account",
        "tier": "file",
        "grain": "one row per account",
        "primary_key": ["account_id"],
        "columns": [none_col("account_id"), none_col("customer_id")]
    });
    let invoice = json!({
        "schema_version": "oos/v1",
        "dataset": "sales.invoice",
        "label": "Invoice",
        "tier": "file",
        "grain": "one row per invoice",
        "primary_key": ["invoice_id"],
        "row_count_estimate": 77,
        "columns": [none_col("invoice_id")]
    });
    let ticket = json!({
        "schema_version": "oos/v1",
        "dataset": "support.ticket",
        "label": "Ticket",
        "tier": "file",
        "grain": "one row per ticket",
        "primary_key": ["ticket_id"],
        "row_count_estimate": 4242,
        "columns": [none_col("ticket_id"), none_col("subject"), customer_email]
    });

    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "alpha",
        "ontology_version": "ontology-v3",
        "catalog_version": "catalog-v3",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "acme",
            "locale": "zh-CN",
            "include_schemas": ["sales", "support"],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [customer, order, account, invoice, ticket],
        "relationships": [
            {
                "name": "order_to_customer",
                "from": {"dataset": "sales.order", "columns": ["customer_id"]},
                "to": {"dataset": "sales.customer", "columns": ["customer_id"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "declared_fk",
                "confidence": "0.97",
                "evidence": {
                    "containment": "0.9991",
                    "name_match": "exact",
                    "type_compatible": true,
                    "seen_in_query_log": 17
                }
            },
            {
                "name": "note_link",
                "from": {"dataset": "sales.order", "columns": ["secret_note"]},
                "to": {"dataset": "sales.customer", "columns": ["email"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "declared_fk"
            },
            {
                "name": "order_guessed",
                "from": {"dataset": "sales.order", "columns": ["customer_id"]},
                "to": {"dataset": "sales.customer", "columns": ["customer_id"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "inferred"
            },
            {
                "name": "account_holder",
                "from": {"dataset": "sales.account", "columns": ["customer_id"]},
                "to": {"dataset": "sales.customer", "columns": ["customer_id"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "declared_fk",
                "evidence": {
                    "containment": "0.9991",
                    "name_match": "exact",
                    "type_compatible": true
                }
            }
        ],
        "metrics": [
            {
                "metric": "order_amount",
                "label": "Order amount",
                "description": "sum of visible order amounts",
                "owner": "finance.li",
                "certified": true,
                "unit": "CNY",
                "status": "active",
                "time_dimension": "sales.order.received_at",
                "default_anchor": "month_start",
                "enum_filters": [{
                    "column": "sales.order.status",
                    "enum": "sales.order.status",
                    "classes": ["open"]
                }],
                "sql": "SELECT sum(amount) AS total FROM sales.order WHERE status = 1",
                "caveats": ["unconfirmed codes stay outside every class"],
                "tests": [{"params": {}, "expect": {"rows": 9090}}],
                "control_total": {
                    "as_of": "month_start",
                    "source": "sales",
                    "sql": "SELECT sum(amount) AS control_sum FROM sales.order",
                    "tolerance": {"abs": "1"}
                }
            },
            {
                "metric": "email_roll",
                "label": "Hidden rollup",
                "owner": "finance.li",
                "certified": true,
                "status": "active",
                "default_anchor": "month_start",
                "sql": "SELECT count(email) AS total FROM sales.customer",
                "tests": [{"params": {}, "expect": {"rows": 1}}],
                "control_total": {
                    "as_of": "month_start",
                    "source": "sales",
                    "sql": "SELECT count(email) FROM sales.customer",
                    "tolerance": {"abs": "1"}
                }
            }
        ],
        "glossary": [
            {
                "term": "在途",
                "definition": "an order that has been received and is not closed",
                "maps_to": {"dataset": "sales.order"},
                "status": "approved"
            },
            {
                "term": "工单",
                "definition": "a request recorded on the ticket dataset",
                "maps_to": {"dataset": "support.ticket"},
                "status": "approved"
            },
            {
                "term": "hidden contact",
                "definition": "a stored contact",
                "maps_to": {"dataset": "sales.customer", "filter": "email is not null"},
                "status": "approved"
            },
            {
                "term": "draft term",
                "definition": "queued for a writer",
                "maps_to": {"dataset": "sales.order"},
                "status": "draft"
            },
            {
                "term": "old term",
                "definition": "retired wording",
                "maps_to": {"dataset": "sales.order"},
                "status": "deprecated"
            }
        ],
        "examples": [
            {
                "id": "ex-order",
                "question": "how much has been received",
                "sql": "SELECT sum(amount) AS total FROM sales.order WHERE status = 1",
                "uses": ["dataset:sales.order", "column:sales.order.amount", "metric:order_amount"],
                "verified_by": "pat.reviewer",
                "verified_at": "2026-04-01"
            },
            {
                "id": "ex-email",
                "question": "list hidden contact values",
                "sql": "SELECT email FROM sales.customer",
                "uses": ["dataset:sales.customer", "column:sales.customer.email"],
                "verified_by": "pat.reviewer",
                "verified_at": "2026-04-01"
            },
            {
                "id": "ex-mailcase",
                "question": "list the same hidden column in another case",
                "sql": "SELECT EMAIL FROM sales.customer",
                "uses": ["dataset:sales.customer"],
                "verified_by": "pat.reviewer",
                "verified_at": "2026-04-02"
            },
            {
                "id": "ex-ticket",
                "question": "list open requests",
                "sql": "SELECT subject FROM support.ticket",
                "uses": ["dataset:support.ticket", "column:support.ticket.subject"],
                "verified_by": "pat.reviewer",
                "verified_at": "2026-04-03"
            }
        ],
        "enums": [{
            "schema_version": "oos/v1",
            "enum": "sales.order.status",
            "classes": {"open": "open order", "unexplained": "meaning is not assigned"},
            "values": [
                {"code": "1", "label": "open", "class": "open", "status": "approved"},
                {
                    "code": "9",
                    "label": "unknown",
                    "class": "unexplained",
                    "status": "unexplained",
                    "evidence": {"rows": 1832}
                }
            ],
            "coverage": "0.97",
            "unexplained_values": ["9"]
        }],
        "entities": [
            {
                "schema_version": "oos/v1",
                "entity": "customer",
                "label": "Customer",
                "primary_dataset": "sales.customer",
                "key": "customer_id",
                "display": "name",
                "members": [{
                    "dataset": "sales.customer",
                    "key": ["customer_id"],
                    "name": "name",
                    "phone": "email"
                }],
                "datasets": ["sales.customer"]
            },
            {
                "schema_version": "oos/v1",
                "entity": "ticket",
                "label": "Ticket",
                "primary_dataset": "support.ticket",
                "key": "ticket_id",
                "display": "subject",
                "members": [{
                    "dataset": "support.ticket",
                    "key": ["ticket_id"],
                    "name": "subject"
                }],
                "datasets": ["support.ticket"]
            }
        ],
        "calendar": {
            "schema_version": "oos/v1",
            "business_tz": "Asia/Shanghai",
            "fiscal_year_start_month": 4,
            "week_start": "monday",
            "anchors": ["month_start", "today"],
            "phrases": {
                "本月": {"start": "month_start", "end": "today", "bounds": "[)"}
            }
        }
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "alpha",
        "principal": "ada",
        "role": "oo_r_analyst",
        "ontology_version": "ontology-v3",
        "catalog_version": "catalog-v3",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [
            {
                "dataset": "sales.customer",
                "row_security": "none",
                "global_stats": "permitted",
                "columns": ["customer_id", "name", "phone", "old_name", "alias_note"]
            },
            {
                "dataset": "sales.order",
                "row_security": "rls",
                "global_stats": "denied",
                "columns": ["order_id", "customer_id", "amount", "status", "received_at"]
            },
            {
                "dataset": "sales.account",
                "row_security": "none",
                "global_stats": "permitted",
                "columns": ["account_id", "customer_id"]
            },
            {
                "dataset": "sales.invoice",
                "row_security": "rls",
                "global_stats": "permitted",
                "global_stats_proof": {"ref": "proof-invoice-1", "proven": true},
                "columns": ["invoice_id"]
            }
        ],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (effective, visibility)
}

fn support_documents() -> (Value, Value) {
    let (mut effective, _) = analyst_documents();
    effective["project"] = json!("beta");
    effective["policy"] = json!({"id": "pol-beta", "version": "1"});
    effective["knowledge"] = json!({"id": "know-beta", "version": "1"});
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "beta",
        "principal": "bea",
        "role": "oo_r_support",
        "ontology_version": "ontology-v3",
        "catalog_version": "catalog-v3",
        "policy": {"id": "pol-beta", "version": "1"},
        "knowledge": {"id": "know-beta", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": "support.ticket",
            "row_security": "rls",
            "global_stats": "denied",
            "columns": ["ticket_id", "subject"]
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (effective, visibility)
}

fn prose(pack: &RolePack) -> String {
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact json");
    let mut out = String::new();
    for file in artifact["files"].as_array().expect("files") {
        let path = file["path"].as_str().expect("path");
        if path.ends_with(".json") {
            continue;
        }
        out.push_str(file["text"].as_str().expect("text"));
        out.push('\n');
    }
    out
}

fn assert_absent(text: &str, token: &str) {
    if let Some(index) = text.find(token) {
        let start = index.saturating_sub(48);
        let end = (index + token.len() + 48).min(text.len());
        panic!("found {token} near {:?}", &text[start..end]);
    }
}

fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let marker = format!("## {heading}\n");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("missing {heading}"))
        + marker.len();
    let rest = &text[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    &rest[..end]
}

fn err_code(effective: &Value, visibility: &Value) -> String {
    match compile(effective, visibility) {
        Ok(_) => panic!("expected a refusal"),
        Err(error) => error.code,
    }
}

#[test]
fn analyst_pack_redacts_hidden_dependencies() {
    let (effective, visibility) = analyst_documents();
    let pack = must(&effective, &visibility);
    let body = prose(&pack);
    let customer = pack.file("tables/sales.customer.md").expect("customer table");
    let order = pack.file("tables/sales.order.md").expect("order table");
    let invoice = pack.file("tables/sales.invoice.md").expect("invoice table");
    let relationships = pack.file("relationships.md").expect("relationships");
    let metrics = pack.file("metrics.md").expect("metrics");
    let glossary = pack.file("glossary.md").expect("glossary");
    let examples = pack.file("examples/verified.sql").expect("examples");
    let enum_file = pack
        .file("enums/sales.order.status.md")
        .expect("status enum");
    let readme = pack.file("README.md").expect("readme");
    let manifest: Value =
        serde_json::from_str(pack.file("manifest.json").expect("manifest")).expect("manifest json");

    assert!(readme.contains("acme"));
    assert!(readme.contains("oo_r_analyst"));
    assert!(readme.contains("project: `alpha`"));
    assert!(readme.contains("does not authenticate"));
    assert!(customer.contains("estimated rows: 100"));
    assert!(customer.contains("Row security: none"));
    assert!(customer.contains("dial string for the account"));
    assert!(customer.contains("Samples are omitted because pii is personal."));
    assert!(customer.contains("Replacement: `sales.customer.name`."));
    assert!(order.contains("Row security: rls"));
    assert!(order.contains("Estimated row count is omitted."));
    assert!(order.contains("order_to_customer"));
    assert!(invoice.contains("estimated rows: 77"));
    assert!(invoice.contains("proof-invoice-1"));
    assert!(invoice.contains("did not re-prove"));
    assert!(pack.file("tables/sales.account.md").is_ok());
    assert_eq!(
        pack.file("tables/support.ticket.md").expect_err("hidden table").code,
        "oo.builder.role-pack/v1/path.absent"
    );
    assert_eq!(
        pack.file("entities/customer.md").expect_err("hidden entity").code,
        "oo.builder.role-pack/v1/path.absent"
    );
    assert_eq!(
        pack.file("entities/ticket.md").expect_err("other role entity").code,
        "oo.builder.role-pack/v1/path.absent"
    );

    let declared = section(relationships, "order_to_customer");
    assert!(declared.contains("source-declared"));
    assert_absent(declared, "name match");
    assert_absent(declared, "UNREVIEWED");
    let holder = section(relationships, "account_holder");
    assert!(holder.contains("name match: `exact`"));
    assert!(holder.contains("type compatible: true"));
    assert_absent(holder, "0.9991");

    assert!(metrics.contains("## order_amount"));
    assert!(metrics.contains("control_total was not executed"));
    assert!(metrics.contains("Unconfirmed codes"));
    assert!(metrics.contains("sales.order.status"));
    assert!(glossary.contains("在途"));
    assert!(examples.contains("ex-order"));
    assert!(examples.contains("SELECT sum(amount) AS total FROM sales.order WHERE status = 1"));
    assert!(enum_file.contains("Meaning is unconfirmed"));
    assert!(enum_file.contains("Coverage and evidence row counts are omitted."));
    assert!(pack.file("calendar.md").expect("calendar").contains("Asia/Shanghai"));
    assert!(pack.file("calendar.md").expect("calendar").contains("本月"));

    for token in [
        "email",
        "EMAIL",
        "secret_note",
        "scratch",
        "pending_col",
        "customer_email",
        "support.ticket",
        "note_link",
        "order_guessed",
        "email_roll",
        "ex-email",
        "ex-mailcase",
        "ex-ticket",
        "工单",
        "hidden contact",
        "queued for a writer",
        "retired wording",
        "legacy alias kept for readers",
        "buyer account file",
        "legacy load caveat",
        "finance",
        "ops.wang",
        "pat.reviewer",
        "control_sum",
        "0.97",
        "0.9991",
        "1832",
        "5000",
        "4242",
        "9090",
        "seen_in_query_log",
        "password=",
        "postgres://",
        "secret://",
    ] {
        assert_absent(&body, token);
    }

    assert_eq!(manifest["protocol"], "oo.builder.role-pack-manifest/v1");
    assert_eq!(manifest["project"], "alpha");
    assert_eq!(manifest["role"], "oo_r_analyst");
    assert_eq!(manifest["policy"]["id"], "pol-alpha");
    assert_eq!(manifest["policy"]["version"], "7");
    assert_eq!(manifest["knowledge"]["version"], "3");
    assert_eq!(manifest["compiler"], "oo-builder/role-pack/v1");
    assert_eq!(manifest["authentication"], "not_claimed");
    assert_eq!(manifest["authorization"], "not_claimed");
    assert_eq!(manifest["publication"], "not_claimed");
    assert_eq!(manifest["mount"], "not_performed");
    assert_eq!(manifest["samples"], "omitted");
    assert_eq!(manifest["domains"], "unsupported");
    assert_eq!(manifest["file_splitting"], "unsupported");
    assert_eq!(manifest["search"], "unsupported");
    assert_eq!(manifest["row_security_reproven"], false);
    assert_eq!(manifest["tokens"]["status"], "unsupported");
    assert_eq!(manifest["tokens"]["estimator"], "none");
    assert_eq!(manifest["tokens"]["readme_token_budget"], 2000);
    assert_eq!(manifest["tokens"]["table_file_token_budget"], 3000);
    assert!(manifest.get("principal").is_none());
    let listed = manifest["files"].as_array().expect("manifest files");
    assert!(listed.iter().all(|file| file["path"] != "manifest.json"));
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact");
    assert!(artifact.get("principal").is_none());
    assert_eq!(artifact["protocol"], "oo.builder.role-pack/v1");
    assert!(
        artifact["files"]
            .as_array()
            .expect("files")
            .iter()
            .all(|file| !file["path"].as_str().expect("path").starts_with("domains/"))
    );
    let digest = pack.pack_digest.as_str();
    let expected_key = format!(
        "oo.builder.role-pack-cache/v1|project=alpha|principal=ada|role=oo_r_analyst|policy=pol-alpha@7|knowledge=know-alpha@3|ontology=ontology-v3|catalog=catalog-v3|compiler=oo-builder/role-pack/v1|pack={digest}"
    );
    assert_eq!(pack.cache_key, expected_key);
    assert!(!pack.artifact.contains("principal"));
    assert!(customer.ends_with('\n') && !customer.ends_with("\n\n"));
    assert!(!pack.file("manifest.json").expect("manifest").ends_with('\n'));
}

#[test]
fn support_pack_is_a_separate_project_artifact() {
    let (analyst_doc, analyst_vis) = analyst_documents();
    let (support_doc, support_vis) = support_documents();
    let analyst = must(&analyst_doc, &analyst_vis);
    let support = must(&support_doc, &support_vis);
    assert_ne!(analyst.artifact, support.artifact);
    assert_ne!(analyst.pack_digest, support.pack_digest);
    assert!(support.cache_key.contains("project=beta"));
    assert!(support.cache_key.contains("principal=bea"));
    assert!(support.cache_key.contains("role=oo_r_support"));
    let body = prose(&support);
    let ticket = support.file("entities/ticket.md").expect("ticket entity");
    assert!(ticket.contains("support.ticket"));
    assert!(ticket.contains("subject"));
    assert!(support.file("examples/verified.sql").expect("sql").contains("ex-ticket"));
    assert!(support.file("glossary.md").expect("glossary").contains("工单"));
    assert_eq!(
        support.file("entities/customer.md").expect_err("hidden entity").code,
        "oo.builder.role-pack/v1/path.absent"
    );
    assert_eq!(
        support
            .file("enums/sales.order.status.md")
            .expect_err("hidden enum")
            .code,
        "oo.builder.role-pack/v1/path.absent"
    );
    for token in [
        "sales.",
        "email",
        "在途",
        "order_amount",
        "ex-order",
        "5000",
        "4242",
        "estimated rows: 100",
        "estimated rows: 77",
        "customer_email",
        "finance",
        "ops.wang",
    ] {
        assert_absent(&body, token);
    }
}

#[test]
fn principal_changes_only_the_cache_key() {
    let (effective, mut visibility) = analyst_documents();
    let first = must(&effective, &visibility);
    visibility["principal"] = json!("ada2");
    let second = must(&effective, &visibility);
    assert_eq!(first.artifact, second.artifact);
    assert_eq!(first.pack_digest, second.pack_digest);
    assert_eq!(first.manifest_sha256, second.manifest_sha256);
    assert_ne!(first.cache_key, second.cache_key);
    assert!(second.cache_key.contains("principal=ada2"));
    assert_absent(&prose(&second), "ada2");
}

#[test]
fn repeated_compiles_match() {
    let (effective, visibility) = analyst_documents();
    let first = must(&effective, &visibility);
    let second = must(&effective, &visibility);
    assert_eq!(first.artifact, second.artifact);
    assert_eq!(first.pack_digest, second.pack_digest);
    assert_eq!(first.cache_key, second.cache_key);
    assert_eq!(first.cache_key_sha256, second.cache_key_sha256);
}

#[test]
fn bindings_and_policy_inputs_refuse() {
    let (effective, visibility) = analyst_documents();
    let prefix = "oo.builder.role-pack/v1/";

    let mut stale = visibility.clone();
    stale["policy"]["version"] = json!("8");
    assert_eq!(err_code(&effective, &stale), format!("{prefix}binding.policy"));

    let mut compiler = visibility.clone();
    compiler["issued_for_compiler"] = json!("oo-builder/role-pack/v0");
    assert_eq!(
        err_code(&effective, &compiler),
        format!("{prefix}binding.compiler")
    );

    let mut unknown = visibility.clone();
    unknown["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!("missing"));
    assert_eq!(
        err_code(&effective, &unknown),
        format!("{prefix}visibility.unknown_column")
    );

    let mut scratch = visibility.clone();
    scratch["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!("scratch"));
    assert_eq!(
        err_code(&effective, &scratch),
        format!("{prefix}classification.unknown_pii")
    );

    let mut pending = visibility.clone();
    pending["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!("pending_col"));
    assert_eq!(
        err_code(&effective, &pending),
        format!("{prefix}classification.incomplete")
    );

    let mut unproven = visibility.clone();
    unproven["datasets"][3]
        .as_object_mut()
        .expect("invoice")
        .remove("global_stats_proof");
    assert_eq!(
        err_code(&effective, &unproven),
        format!("{prefix}evidence.unproven")
    );

    let mut extra_proof = visibility.clone();
    extra_proof["datasets"][0]["global_stats_proof"] =
        json!({"ref": "proof-customer-1", "proven": true});
    assert_eq!(
        err_code(&effective, &extra_proof),
        format!("{prefix}evidence.unproven")
    );

    let mut per_role = effective.clone();
    per_role["ontology"]["compile"]["per_role"] = json!(false);
    assert_eq!(err_code(&per_role, &visibility), format!("{prefix}compile.per_role"));

    let mut raw = effective.clone();
    raw["ontology"]["compile"]["sample_values"]["default"] = json!("raw");
    assert_eq!(err_code(&raw, &visibility), format!("{prefix}sample.raw_refused"));

    let mut decisions = effective.clone();
    decisions["decisions"] = json!([]);
    assert_eq!(
        err_code(&decisions, &visibility),
        format!("{prefix}unsupported.field")
    );

    let mut samples = visibility.clone();
    samples["datasets"][0]["sample_values"] = json!("masked");
    assert_eq!(
        err_code(&effective, &samples),
        format!("{prefix}unsupported.samples")
    );

    let mut token = effective.clone();
    token["ontology"]["publish"]["include_unreviewed"] = json!(["not_a_token"]);
    assert_eq!(
        err_code(&token, &visibility),
        format!("{prefix}include_unreviewed.unsupported")
    );

    let raw_json = "{\"protocol\":\"oo.builder.effective-oos/v1\",\"protocol\":\"again\",\"leak\":\"password=hunter\"}";
    let error = compile_role_pack(raw_json, "{}").expect_err("duplicate key");
    assert_eq!(error.code, "oo.builder.effective-oos/v1/json.duplicate_key");
    assert_eq!(error.message, "the JSON document was rejected");
    assert!(!error.to_string().contains("hunter"));
    assert!(!error.to_string().contains("password"));
}

#[test]
fn budgets_refuse() {
    let (effective, visibility) = analyst_documents();
    let prefix = "oo.builder.role-pack/v1/";

    let mut file_budget = visibility.clone();
    file_budget["limits"]["max_file_bytes"] = json!(80);
    assert_eq!(
        err_code(&effective, &file_budget),
        format!("{prefix}budget.file")
    );

    let mut entries = visibility.clone();
    entries["limits"]["max_entries"] = json!(2);
    assert_eq!(err_code(&effective, &entries), format!("{prefix}budget.entries"));

    let mut invalid = visibility.clone();
    invalid["limits"]["max_entries"] = json!(0);
    assert_eq!(err_code(&effective, &invalid), format!("{prefix}limit.invalid"));
}

#[test]
fn paths_reject_traversal() {
    for path in [
        "",
        "../x",
        "/etc/passwd",
        "tables/../../README.md",
        "./README.md",
        "tables//x",
        "tables\\x",
        "a:b",
    ] {
        let error = normalize_pack_path(path).expect_err(path);
        assert_eq!(error.code, "oo.builder.role-pack/v1/path.invalid");
        assert_eq!(error.path, "/");
        assert!(!error.to_string().contains("../"));
    }
    assert_eq!(normalize_pack_path("README.md").expect("readme"), "README.md");
    assert_eq!(
        normalize_pack_path("tables/sales.customer.md").expect("table"),
        "tables/sales.customer.md"
    );
    assert_eq!(normalize_pack_path("e\u{0301}").expect("nfc"), "é");

    let (effective, visibility) = analyst_documents();
    let pack = must(&effective, &visibility);
    assert!(
        pack.file("tables/sales.customer.md")
            .expect("lookup")
            .contains("estimated rows: 100")
    );
    let crossed = pack.file("tables/../../README.md").expect_err("traversal");
    assert_eq!(crossed.code, "oo.builder.role-pack/v1/path.invalid");
    assert_eq!(crossed.path, "/");
    assert_eq!(
        pack.file("domains/sales.md").expect_err("no domain").code,
        "oo.builder.role-pack/v1/path.absent"
    );
}

#[test]
fn unreviewed_text_needs_an_explicit_token() {
    let (effective, visibility) = analyst_documents();
    let hidden = prose(&must(&effective, &visibility));
    assert_absent(&hidden, "legacy alias kept for readers");
    assert_absent(&hidden, "buyer account file");
    assert_absent(&hidden, "legacy load caveat");
    assert_absent(&hidden, "order_guessed");

    let mut shown = effective.clone();
    shown["ontology"]["publish"]["include_unreviewed"] = json!([
        "column_descriptions",
        "dataset_descriptions",
        "caveats",
        "relationships"
    ]);
    let pack = must(&shown, &visibility);
    let customer = pack.file("tables/sales.customer.md").expect("customer");
    assert!(customer.contains("UNREVIEWED: legacy alias kept for readers"));
    assert!(customer.contains("UNREVIEWED: not an approved review, and not a grant."));
    assert!(customer.contains("buyer account file"));
    assert!(customer.contains("legacy load caveat"));
    let relationships = pack.file("relationships.md").expect("relationships");
    let guessed = section(relationships, "order_guessed");
    assert!(guessed.contains("UNREVIEWED"));
    assert_absent(&prose(&pack), "email");
}

#[test]
fn hashes_match_an_independent_oracle() {
    let (effective, visibility) = analyst_documents();
    let pack = must(&effective, &visibility);
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact");
    assert_eq!(pack.artifact, canonical_json(&artifact));
    assert!(!pack.artifact.ends_with('\n'));

    let files = artifact["files"].as_array().expect("files");
    let paths: Vec<&str> = files
        .iter()
        .map(|file| file["path"].as_str().expect("path"))
        .collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
    assert_eq!(pack.entry_count, files.len() as u64);

    let mut total = 0_u64;
    let mut records = Vec::new();
    for file in files {
        let text = file["text"].as_str().expect("text");
        let bytes = text.len() as u64;
        total += bytes;
        assert_eq!(file["bytes"], bytes);
        let digest = sha256_hex(text.as_bytes());
        assert_eq!(file["sha256"], digest);
        records.push(json!({"bytes": bytes, "path": file["path"], "sha256": digest}));
    }
    assert_eq!(pack.total_bytes, total);
    assert_eq!(pack.pack_digest, sha256_hex(canonical_json(&Value::Array(records)).as_bytes()));
    assert_eq!(pack.cache_key_sha256, sha256_hex(pack.cache_key.as_bytes()));

    let manifest_text = pack.file("manifest.json").expect("manifest");
    assert_eq!(pack.manifest_sha256, sha256_hex(manifest_text.as_bytes()));
    let manifest: Value = serde_json::from_str(manifest_text).expect("manifest json");
    let listed = manifest["files"].as_array().expect("listed");
    assert_eq!(listed.len() as u64, pack.entry_count - 1);
    for item in listed {
        let path = item["path"].as_str().expect("path");
        let text = pack.file(path).expect(path);
        assert_eq!(item["bytes"], text.len() as u64);
        assert_eq!(item["sha256"], sha256_hex(text.as_bytes()));
    }
}

fn closure_documents() -> (Value, Value) {
    let mut shipment_id = none_col("shipment_id");
    shipment_id["data_type"] = json!("bigint");
    shipment_id["description"] = json!("本月称重记录");
    shipment_id["review"] = json!({"state": "approved"});

    let mut weight = none_col("weight_kg");
    weight["data_type"] = json!("numeric queue_note");
    weight["unit"] = json!("queue_note");
    weight["description"] = json!("称重后对照 \"yard\".\"slip\" 的客户备注");
    weight["review"] = json!({"state": "approved"});

    let mut handle = none_col("handle");
    handle["pii"] = json!("personal");
    handle["semantic_type"] = json!("phone");
    handle["description"] = json!("classified handle");
    handle["review"] = json!({"state": "approved", "by": "class.clerk"});
    handle["classification"]["by"] = json!("class.clerk");

    let mut note_ascii = none_col("note_ascii");
    note_ascii["description"] = json!("legacy queue_note alias");
    note_ascii["review"] = json!({"state": "approved"});

    let mut note_quoted = none_col("note_quoted");
    note_quoted["description"] = json!("see \"queue_note\" token");
    note_quoted["review"] = json!({"state": "approved"});

    let mut status = none_col("status");
    status["enum"] = json!("depot.shipment.status");

    let mut mark_visible = none_col("mark_visible");
    mark_visible["description"] = json!("é标记不应出现");
    mark_visible["review"] = json!({"state": "approved"});

    let mut tally_weight = none_col("weight_kg");
    tally_weight["unit"] = json!("kg");

    let queue_note = json!({
        "name": "queue_note",
        "label": "客户备注",
        "pii": "sensitive",
        "classification": {"state": "confirmed", "by": "class.clerk"},
        "description": "a168-hidden-body",
        "review": {"state": "approved", "by": "review.clerk"}
    });
    let mark_nfc = json!({
        "name": "mark_nfc",
        "label": "e\u{0301}标记",
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let quarter = none_col("quarter_start");

    let shipment = json!({
        "schema_version": "oos/v1",
        "dataset": "depot.shipment",
        "label": "Shipment",
        "tier": "file",
        "grain": "one row per shipment",
        "primary_key": ["shipment_id"],
        "row_count_estimate": 4242427,
        "columns": [
            shipment_id, weight, handle, note_ascii, note_quoted, status, mark_visible,
            queue_note, mark_nfc, quarter
        ]
    });
    let tally = json!({
        "schema_version": "oos/v1",
        "dataset": "depot.tally",
        "label": "Tally",
        "tier": "file",
        "grain": "one row per tally",
        "primary_key": ["tally_id"],
        "row_count_estimate": 17,
        "columns": [none_col("tally_id"), tally_weight]
    });
    let slip = json!({
        "schema_version": "oos/v1",
        "dataset": "yard.slip",
        "label": "Slip",
        "tier": "file",
        "grain": "one row per slip",
        "primary_key": ["slip_id"],
        "row_count_estimate": 8888887,
        "columns": [none_col("slip_id")]
    });

    let metric = |name: &str, description: &str, unit: &str, sql: &str| {
        json!({
            "metric": name,
            "label": name,
            "description": description,
            "owner": "clerk.one",
            "certified": true,
            "unit": unit,
            "status": "active",
            "default_anchor": "month_start",
            "sql": sql,
            "tests": [{"params": {}, "expect": {"rows": 3}}],
            "control_total": {
                "as_of": "month_start",
                "source": "depot",
                "sql": "SELECT sum(weight_kg) AS control_sum FROM depot.shipment",
                "tolerance": {"abs": "1"}
            }
        })
    };
    let mut weight_total = metric(
        "weight_total",
        "sum of visible weight",
        "kg",
        "SELECT sum(weight_kg) AS total FROM depot.shipment",
    );
    weight_total["enum_filters"] = json!([{
        "column": "depot.shipment.status",
        "enum": "depot.shipment.status",
        "classes": ["open"]
    }]);
    let quoted_visible_sql = "SELECT \"weight_kg\" AS \"净\"\"重\" FROM \"depot\".\"shipment\"\nUNION ALL\nSELECT weight_kg FROM depot.tally";
    let quoted_mix_sql = "SELECT weight_kg AS \"客户备注\" FROM depot.shipment\nUNION ALL\nSELECT weight_kg FROM \"yard\".\"slip\"";

    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "alpha",
        "ontology_version": "ont-a168",
        "catalog_version": "cat-a168",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "depotmap",
            "locale": "zh-CN",
            "include_schemas": ["depot", "yard"],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [shipment, tally, slip],
        "relationships": [
            {
                "name": "shipment_tally",
                "from": {"dataset": "depot.shipment", "columns": ["weight_kg"]},
                "to": {"dataset": "depot.tally", "columns": ["weight_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "human",
                "confidence": "0.272727",
                "review": {"state": "approved", "by": "review.clerk"},
                "evidence": {
                    "containment": "0.919191",
                    "name_match": "exact",
                    "type_compatible": true,
                    "seen_in_query_log": 515151
                }
            },
            {
                "name": "declared_link",
                "from": {"dataset": "depot.shipment", "columns": ["weight_kg"]},
                "to": {"dataset": "depot.tally", "columns": ["weight_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "declared_fk"
            },
            {
                "name": "guessed_link",
                "from": {"dataset": "depot.shipment", "columns": ["weight_kg"]},
                "to": {"dataset": "depot.tally", "columns": ["weight_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "inferred"
            }
        ],
        "metrics": [
            weight_total,
            metric("quoted_mix", "客户备注 remains visible in this sentence", "kg", quoted_mix_sql),
            metric("ascii_hidden", "names the hidden column", "kg", "SELECT queue_note FROM depot.shipment"),
            metric("quoted_visible", "本月可见重量", "kg", quoted_visible_sql),
            metric("unit_leak", "visible unit carrier", "queue_note", "SELECT sum(weight_kg) AS total FROM depot.shipment"),
            metric("prose_metric", "本月可见重量", "kg", "weight is just prose")
        ],
        "glossary": [
            {
                "term": "在册",
                "definition": "a visible shipment term",
                "maps_to": {"dataset": "depot.shipment"},
                "status": "approved"
            },
            {
                "term": "引用隐藏",
                "definition": "see \"yard\".\"slip\" and 客户备注",
                "maps_to": {"dataset": "depot.shipment"},
                "status": "approved"
            },
            {
                "term": "草稿",
                "definition": "draft wording",
                "maps_to": {"dataset": "depot.shipment"},
                "status": "draft"
            },
            {
                "term": "隐藏表",
                "definition": "hidden dataset term",
                "maps_to": {"dataset": "yard.slip"},
                "status": "approved"
            },
            {
                "term": "可见术语",
                "definition": "本月发货重量，不含隐藏对象",
                "maps_to": {"dataset": "depot.shipment"},
                "status": "approved"
            }
        ],
        "examples": [
            closure_example("ex-weight", "how much weight is visible", "SELECT sum(weight_kg) AS total FROM depot.shipment", &["dataset:depot.shipment", "column:depot.shipment.weight_kg", "metric:weight_total"]),
            closure_example("ex-quoted", "how the hidden slip is referenced", quoted_mix_sql, &["dataset:depot.shipment", "column:depot.shipment.weight_kg"]),
            closure_example("ex-ascii", "show the hidden note", "SELECT queue_note FROM depot.shipment", &["dataset:depot.shipment"]),
            closure_example("ex-token", "show the quoted spelling of the hidden note", "SELECT \"queue_note\" FROM depot.shipment", &["dataset:depot.shipment"]),
            closure_example("ex-quoted-visible", "可见的引号查询", "SELECT weight_kg AS \"重量\" FROM \"depot\".\"shipment\"", &["dataset:depot.shipment", "column:depot.shipment.weight_kg"]),
            closure_example("ex-prose", "not a query", "weight is just prose", &["dataset:depot.shipment"]),
            closure_example("ex-join", "two visible tables joined", "SELECT weight_kg FROM depot.shipment JOIN depot.tally ON weight_kg = weight_kg", &["dataset:depot.shipment", "dataset:depot.tally"])
        ],
        "enums": [{
            "schema_version": "oos/v1",
            "enum": "depot.shipment.status",
            "classes": {"open": "open shipment", "unexplained": "meaning is not assigned"},
            "values": [
                {"code": "1", "label": "open", "class": "open", "status": "approved"},
                {"code": "9", "label": "unknown", "class": "unexplained", "status": "unexplained", "evidence": {"rows": 183213}}
            ],
            "coverage": "0.131313",
            "unexplained_values": ["9"]
        }],
        "entities": [
            {
                "schema_version": "oos/v1",
                "entity": "shipment",
                "label": "Shipment",
                "primary_dataset": "depot.shipment",
                "key": "shipment_id",
                "display": "weight_kg",
                "members": [{"dataset": "depot.shipment", "key": ["shipment_id"], "name": "weight_kg"}],
                "datasets": ["depot.shipment"]
            },
            {
                "schema_version": "oos/v1",
                "entity": "slip",
                "label": "Slip",
                "primary_dataset": "yard.slip",
                "key": "slip_id",
                "display": "slip_id",
                "members": [{"dataset": "yard.slip", "key": ["slip_id"]}],
                "datasets": ["yard.slip"]
            }
        ],
        "calendar": {
            "schema_version": "oos/v1",
            "business_tz": "Asia/Shanghai",
            "fiscal_year_start_month": 4,
            "week_start": "monday",
            "anchors": ["month_start", "today", "quarter_start"],
            "phrases": {
                "queue_note": {"start": "month_start", "end": "today", "bounds": "[)"},
                "客户备注": {"start": "month_start", "end": "today", "bounds": "[)"},
                "本月": {"start": "month_start", "end": "today", "bounds": "[)"},
                "é标记": {"start": "month_start", "end": "today", "bounds": "[)"}
            }
        }
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "alpha",
        "principal": "ada",
        "role": "oo_r_clerk",
        "ontology_version": "ont-a168",
        "catalog_version": "cat-a168",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [
            {
                "dataset": "depot.shipment",
                "row_security": "rls",
                "global_stats": "denied",
                "columns": ["shipment_id", "weight_kg", "handle", "note_ascii", "note_quoted", "status", "mark_visible"]
            },
            {
                "dataset": "depot.tally",
                "row_security": "rls",
                "global_stats": "permitted",
                "global_stats_proof": {"ref": "proof-tally-1", "proven": true},
                "columns": ["tally_id", "weight_kg"]
            }
        ],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (effective, visibility)
}

fn closure_example(id: &str, question: &str, sql: &str, uses: &[&str]) -> Value {
    json!({
        "id": id,
        "question": question,
        "sql": sql,
        "uses": uses,
        "verified_by": "review.clerk",
        "verified_at": "2026-04-01"
    })
}

fn assert_rust_sha256(pack: &RolePack) {
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact json");
    assert_eq!(pack.artifact, canonical_json(&artifact));
    let files = artifact["files"].as_array().expect("files");
    let mut records = Vec::new();
    let mut total = 0_u64;
    for file in files {
        let text = file["text"].as_str().expect("text");
        let bytes = text.len() as u64;
        total += bytes;
        let digest = sha256_hex(text.as_bytes());
        assert_eq!(file["sha256"], digest);
        assert_eq!(file["bytes"], bytes);
        records.push(json!({"bytes": bytes, "path": file["path"], "sha256": digest}));
    }
    assert_eq!(pack.total_bytes, total);
    assert_eq!(pack.entry_count, files.len() as u64);
    assert_eq!(
        pack.pack_digest,
        sha256_hex(canonical_json(&Value::Array(records)).as_bytes())
    );
    assert_eq!(pack.cache_key_sha256, sha256_hex(pack.cache_key.as_bytes()));
    let manifest = pack.file("manifest.json").expect("manifest");
    assert_eq!(pack.manifest_sha256, sha256_hex(manifest.as_bytes()));
}

#[test]
fn closure_keeps_visible_text_and_omits_hidden_references() {
    let (effective, visibility) = closure_documents();
    let clerk = must(&effective, &visibility);
    let mut ada2_visibility = visibility.clone();
    ada2_visibility["principal"] = json!("ada2");
    let ada2 = must(&effective, &ada2_visibility);
    let mut audit_visibility = visibility.clone();
    audit_visibility["principal"] = json!("cy");
    audit_visibility["role"] = json!("oo_r_audit");
    audit_visibility["datasets"] = json!([{
        "dataset": "depot.tally",
        "row_security": "rls",
        "global_stats": "permitted",
        "global_stats_proof": {"ref": "proof-tally-1", "proven": true},
        "columns": ["tally_id", "weight_kg"]
    }]);
    let audit = must(&effective, &audit_visibility);
    let mut beta_effective = effective.clone();
    beta_effective["project"] = json!("beta");
    beta_effective["policy"] = json!({"id": "pol-beta", "version": "1"});
    beta_effective["knowledge"] = json!({"id": "know-beta", "version": "1"});
    let beta_visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "beta",
        "principal": "bea",
        "role": "oo_r_clerk",
        "ontology_version": "ont-a168",
        "catalog_version": "cat-a168",
        "policy": {"id": "pol-beta", "version": "1"},
        "knowledge": {"id": "know-beta", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": "yard.slip",
            "row_security": "none",
            "global_stats": "denied",
            "columns": ["slip_id"]
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    let beta = must(&beta_effective, &beta_visibility);

    assert_eq!(clerk.artifact, ada2.artifact);
    assert_eq!(clerk.pack_digest, ada2.pack_digest);
    assert_ne!(clerk.cache_key, ada2.cache_key);
    assert!(ada2.cache_key.contains("principal=ada2"));
    assert_ne!(clerk.pack_digest, audit.pack_digest);
    assert_ne!(clerk.pack_digest, beta.pack_digest);
    assert!(audit.cache_key.contains("role=oo_r_audit"));
    assert!(beta.cache_key.contains("project=beta"));
    assert_rust_sha256(&clerk);
    assert_rust_sha256(&audit);
    assert_rust_sha256(&beta);

    let body = prose(&clerk);
    let shipment = clerk.file("tables/depot.shipment.md").expect("shipment");
    let tally = clerk.file("tables/depot.tally.md").expect("tally");
    let metrics = clerk.file("metrics.md").expect("metrics");
    let examples = clerk.file("examples/verified.sql").expect("examples");
    let glossary = clerk.file("glossary.md").expect("glossary");
    let calendar = clerk.file("calendar.md").expect("calendar");
    let relationships = clerk.file("relationships.md").expect("relationships");
    assert!(shipment.contains("bigint"));
    assert!(shipment.contains("本月称重记录"));
    assert!(shipment.contains("weight_kg"));
    assert!(shipment.contains("mark_visible"));
    assert!(shipment.contains("Samples are omitted because pii is personal."));
    assert!(shipment.contains("Estimated row count is omitted."));
    assert!(tally.contains("estimated rows: 17"));
    assert!(tally.contains("unit kg"));
    assert!(tally.contains("proof-tally-1"));
    assert!(tally.contains("did not re-prove"));
    assert!(metrics.contains("## weight_total"));
    assert!(metrics.contains("## quoted_visible"));
    assert!(metrics.contains("本月可见重量"));
    assert!(metrics.contains("unit: kg"));
    assert!(metrics.contains("净\"\"重"));
    assert!(metrics.contains("\"depot\".\"shipment\""));
    assert!(metrics.contains("UNION ALL"));
    assert!(examples.contains("ex-weight"));
    assert!(examples.contains("ex-quoted-visible"));
    assert!(examples.contains("重量"));
    assert!(examples.contains("\"depot\".\"shipment\""));
    assert!(glossary.contains("在册"));
    assert!(glossary.contains("可见术语"));
    assert!(glossary.contains("本月发货重量，不含隐藏对象"));
    assert!(calendar.contains("本月"));
    assert!(calendar.contains("month_start"));
    assert!(calendar.contains("today"));
    assert!(relationships.contains("source-declared"));
    assert!(clerk.file("entities/shipment.md").is_ok());
    assert_eq!(
        clerk.file("tables/yard.slip.md").expect_err("hidden table").code,
        "oo.builder.role-pack/v1/path.absent"
    );
    assert_eq!(
        clerk.file("entities/slip.md").expect_err("hidden entity").code,
        "oo.builder.role-pack/v1/path.absent"
    );

    for token in [
        "queue_note",
        "客户备注",
        "yard",
        "slip",
        "a168-hidden-body",
        "4242427",
        "8888887",
        "quoted_mix",
        "ascii_hidden",
        "unit_leak",
        "prose_metric",
        "-- ex-quoted\n",
        "ex-ascii",
        "ex-token",
        "ex-prose",
        "ex-join",
        "引用隐藏",
        "mark_nfc",
        "é标记",
        "e\u{0301}标记",
        "quarter_start",
        "legacy queue_note alias",
        "class.clerk",
        "review.clerk",
        "0.272727",
        "515151",
        "183213",
        "control_sum",
        "guessed_link",
    ] {
        assert_absent(&body, token);
    }

    let audit_body = prose(&audit);
    let audit_calendar = audit.file("calendar.md").expect("audit calendar");
    assert!(audit_calendar.contains("本月"));
    assert!(audit.file("tables/depot.tally.md").expect("audit tally").contains("estimated rows: 17"));
    for token in ["queue_note", "客户备注", "é标记", "quarter_start", "shipment", "yard", "4242427"] {
        assert_absent(&audit_body, token);
    }
    let beta_body = prose(&beta);
    let beta_calendar = beta.file("calendar.md").expect("beta calendar");
    assert!(beta_calendar.contains("本月"));
    assert!(beta.file("tables/yard.slip.md").is_ok());
    assert!(beta.file("entities/slip.md").is_ok());
    for token in ["queue_note", "客户备注", "é标记", "e\u{0301}标记", "quarter_start", "shipment", "4242427", "8888887"] {
        assert_absent(&beta_body, token);
    }

    let manifest: Value = serde_json::from_str(clerk.file("manifest.json").expect("manifest")).expect("json");
    assert_eq!(manifest["authentication"], "not_claimed");
    assert_eq!(manifest["authorization"], "not_claimed");
    assert_eq!(manifest["publication"], "not_claimed");
    assert_eq!(manifest["samples"], "omitted");
    assert_eq!(manifest["row_security_reproven"], false);
    assert_eq!(manifest["tokens"]["status"], "unsupported");
    assert!(manifest.get("principal").is_none());
    assert!(!clerk.artifact.contains("principal"));
}

fn literal_metric(
    name: &str,
    label: &str,
    description: &str,
    sql: &str,
    control_sql: &str,
) -> Value {
    json!({
        "metric": name,
        "label": label,
        "description": description,
        "owner": "owner.a174",
        "certified": true,
        "unit": "kg",
        "status": "active",
        "default_anchor": "month_start",
        "sql": sql,
        "tests": [{"params": {}, "expect": {"rows": 1}}],
        "control_total": {
            "as_of": "month_start",
            "source": "depot",
            "sql": control_sql,
            "tolerance": {"abs": "1"}
        }
    })
}

fn literal_documents() -> (Value, Value) {
    let mut badge = none_col("badge");
    badge["pii"] = json!("personal");
    badge["classification"]["by"] = json!("class.a174");

    let tick = json!({
        "name": "tick_col",
        "label": "签'记",
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let hold = json!({
        "name": "hold_bin",
        "label": "暂存格",
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let parcel = json!({
        "schema_version": "oos/v1",
        "dataset": "depot.parcel",
        "label": "Parcel",
        "tier": "file",
        "grain": "one row per parcel",
        "primary_key": ["parcel_id"],
        "row_count_estimate": 515151,
        "columns": [none_col("parcel_id"), none_col("mass_kg"), badge, tick, hold]
    });
    let scale = json!({
        "schema_version": "oos/v1",
        "dataset": "depot.scale",
        "label": "Scale",
        "tier": "file",
        "grain": "one row per scale",
        "primary_key": ["scale_id"],
        "row_count_estimate": 19,
        "columns": [none_col("scale_id"), none_col("mass_kg")]
    });
    let legal_sql = "SELECT 'it''s visible' AS label_ok FROM depot.parcel";
    let leak_sql = "SELECT '签''记' AS mass_kg FROM depot.parcel";
    let legal_control = "SELECT sum(mass_kg) AS control_sum FROM depot.parcel";
    let ends = json!({
        "from": {"dataset": "depot.parcel", "columns": ["mass_kg"]},
        "to": {"dataset": "depot.scale", "columns": ["mass_kg"]},
        "cardinality": "many_to_one",
        "join_hint": "left"
    });
    let mut parcel_scale = ends.clone();
    parcel_scale["name"] = json!("parcel_scale");
    parcel_scale["provenance"] = json!("human");
    parcel_scale["review"] = json!({"state": "approved", "by": "review.a174"});
    let mut hold_link = ends;
    hold_link["name"] = json!("hold_bin");
    hold_link["provenance"] = json!("declared_fk");
    let entity = |name: &str| {
        json!({
            "schema_version": "oos/v1",
            "entity": name,
            "label": "Parcel",
            "primary_dataset": "depot.parcel",
            "key": "parcel_id",
            "display": "mass_kg",
            "members": [{"dataset": "depot.parcel", "key": ["parcel_id"], "name": "mass_kg"}],
            "datasets": ["depot.parcel"]
        })
    };
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "alpha",
        "ontology_version": "ont-a174",
        "catalog_version": "cat-a174",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "depotmap",
            "locale": "zh-CN",
            "include_schemas": ["depot"],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [parcel, scale],
        "relationships": [parcel_scale, hold_link],
        "metrics": [
            literal_metric(
                "string_metric",
                "可见字符串",
                "visible weight string",
                legal_sql,
                legal_control
            ),
            literal_metric("sq_leak", "单引号标签", "escaped hidden label", leak_sql, legal_control),
            literal_metric("control_leak", "对账泄漏", "legal display sql", legal_sql, leak_sql),
            literal_metric(
                "distinct_bare",
                "退化去重",
                "degenerate distinct",
                "SELECT DISTINCT FROM depot.parcel",
                legal_control
            ),
            literal_metric(
                "distinct_cols",
                "列去重",
                "distinct before a column",
                "SELECT DISTINCT mass_kg FROM depot.parcel",
                legal_control
            ),
            literal_metric(
                "open_quote",
                "未闭合",
                "see '签''记",
                "SELECT mass_kg FROM depot.parcel",
                legal_control
            ),
            literal_metric(
                "kept_mass",
                "可见重量",
                "visible mass",
                "SELECT mass_kg FROM depot.parcel",
                legal_control
            )
        ],
        "glossary": [{
            "term": "可见",
            "definition": "it's a visible note",
            "maps_to": {"dataset": "depot.parcel"},
            "status": "approved"
        }],
        "examples": [
            closure_example(
                "ex-rel-kept",
                "kept relationship question",
                "SELECT mass_kg AS rel_kept_alias FROM depot.parcel",
                &["rel:parcel_scale", "dataset:depot.parcel", "column:depot.parcel.mass_kg"]
            ),
            closure_example(
                "ex-rel-hidden",
                "hidden relationship question",
                "SELECT mass_kg AS rel_hidden_alias FROM depot.parcel",
                &["rel:hold_bin", "dataset:depot.parcel", "column:depot.parcel.mass_kg"]
            )
        ],
        "entities": [entity("parcel"), entity("hold_bin")]
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "alpha",
        "principal": "ada",
        "role": "oo_r_clerk",
        "ontology_version": "ont-a174",
        "catalog_version": "cat-a174",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [
            {
                "dataset": "depot.parcel",
                "row_security": "rls",
                "global_stats": "denied",
                "columns": ["parcel_id", "mass_kg", "badge"]
            },
            {
                "dataset": "depot.scale",
                "row_security": "rls",
                "global_stats": "permitted",
                "global_stats_proof": {"ref": "proof-scale-1", "proven": true},
                "columns": ["scale_id", "mass_kg"]
            }
        ],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (effective, visibility)
}

/// a172 fresh literal: `'签''记'` is the hidden label `签'记`.
///
/// The hash oracle is `sha2` 0.10.9, the dev-dependency already on
/// `oo-builder`. It does not call the library hash helper.
#[test]
fn escaped_sql_label_distinct_and_name_closure() {
    let (effective, visibility) = literal_documents();
    let clerk = must(&effective, &visibility);
    let mut ada2_visibility = visibility.clone();
    ada2_visibility["principal"] = json!("ada2");
    let ada2 = must(&effective, &ada2_visibility);

    assert_eq!(clerk.artifact, ada2.artifact);
    assert_eq!(clerk.pack_digest, ada2.pack_digest);
    assert_ne!(clerk.cache_key, ada2.cache_key);
    assert!(ada2.cache_key.contains("principal=ada2"));
    assert!(!clerk.artifact.contains("principal"));
    assert_rust_sha256(&clerk);
    assert_rust_sha256(&ada2);

    let body = prose(&clerk);
    let metrics = clerk.file("metrics.md").expect("metrics");
    let examples = clerk.file("examples/verified.sql").expect("examples");
    let glossary = clerk.file("glossary.md").expect("glossary");
    let relationships = clerk.file("relationships.md").expect("relationships");
    let parcel = clerk.file("tables/depot.parcel.md").expect("parcel");
    let scale = clerk.file("tables/depot.scale.md").expect("scale");
    let entity = clerk.file("entities/parcel.md").expect("parcel entity");

    assert!(metrics.contains("## string_metric"));
    assert!(metrics.contains("## kept_mass"));
    assert!(metrics.contains("SELECT 'it''s visible' AS label_ok FROM depot.parcel"));
    assert!(glossary.contains("it's a visible note"));
    assert!(relationships.contains("## parcel_scale"));
    assert!(examples.contains("ex-rel-kept"));
    assert!(examples.contains("rel_kept_alias"));
    assert!(entity.contains("# parcel"));
    assert!(parcel.contains("Samples are omitted because pii is personal."));
    assert!(parcel.contains("Estimated row count is omitted."));
    assert!(parcel.contains("did not re-prove"));
    assert!(scale.contains("estimated rows: 19"));
    assert!(scale.contains("proof-scale-1"));
    assert!(scale.contains("did not re-prove"));
    assert_eq!(
        clerk
            .file("entities/hold_bin.md")
            .expect_err("hidden entity name")
            .code,
        "oo.builder.role-pack/v1/path.absent"
    );

    for token in [
        "签'记",
        "签''记",
        "sq_leak",
        "单引号标签",
        "control_leak",
        "对账泄漏",
        "distinct_bare",
        "distinct_cols",
        "DISTINCT",
        "open_quote",
        "see '签",
        "control_sum",
        "hold_bin",
        "暂存格",
        "tick_col",
        "ex-rel-hidden",
        "rel_hidden_alias",
        "hidden relationship question",
        "source-declared",
        "515151",
        "owner.a174",
        "review.a174",
        "class.a174",
    ] {
        assert_absent(&body, token);
    }

    let manifest: Value =
        serde_json::from_str(clerk.file("manifest.json").expect("manifest")).expect("json");
    assert_eq!(manifest["authentication"], "not_claimed");
    assert_eq!(manifest["authorization"], "not_claimed");
    assert_eq!(manifest["publication"], "not_claimed");
    assert_eq!(manifest["mount"], "not_performed");
    assert_eq!(manifest["samples"], "omitted");
    assert_eq!(manifest["domains"], "unsupported");
    assert_eq!(manifest["file_splitting"], "unsupported");
    assert_eq!(manifest["search"], "unsupported");
    assert_eq!(manifest["row_security_reproven"], false);
    assert_eq!(manifest["tokens"]["status"], "unsupported");
    assert_eq!(manifest["tokens"]["estimator"], "none");
    assert!(manifest.get("principal").is_none());
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(&mut out, value);
    out
}

fn write_canonical(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_string(out, key);
                out.push(':');
                write_canonical(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}
