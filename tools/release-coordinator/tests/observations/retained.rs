//! D15 contract units. Database refusals: tests/retained_cycle_live.py (owned PostgreSQL).
use super::*;

const REVIEWED_FIXTURE: &str = include_str!("../fixtures/retained_reviewed.json");

fn cut(run: &str) -> Value {
    let physical = format!("pt_{}", "a".repeat(60));
    let receipt = json!({"protocol":RECEIPT_PROTOCOL,"source_id":"owned","run_id":run,"run_ids":[run],
        "source_generation":3,"target_generation":3,"position_codec":"snapshot-zero/v1",
        "position_lineage":"oo-load.retained","applied_pos":"0","consistency":"retained_generation",
        "consistency_grade":"B","cross_table_consistent":false,"tables":[{"schema":"app","table":"items",
        "storage_schema":"app","storage_table":"items","run_id":run,"physical_name":physical,
        "accepted_rows":1,"recipe_sha256":"b".repeat(64),"source_generation":3,"target_generation":3,
        "applied_pos":"0","capture_method":"c5_diff","delete_detection":"reconcile","consistency_grade":"B"}]});
    let manifest = json!({"protocol":"oo.library.membership/v1","library_id":"owned",
        "sources":[{"source_id":"owned","schema":"app","tables":["items"]}]});
    json!({"library":{"protocol":crate::native_library::PROTOCOL,"library_id":"owned",
        "manifest_sha256":crate::canon::canonical_sha(&manifest),"manifest":manifest,"expected_tables":1,
        "shards":["owned"]},"shards":[{"source_id":"owned","anchor_run":run,
        "receipt_sha256":crate::canon::canonical_sha(&receipt),"receipt":receipt}]})
}

fn rehash(cut: &mut Value) {
    let sha = crate::canon::canonical_sha(&cut["shards"][0]["receipt"]);
    cut["shards"][0]["receipt_sha256"] = json!(sha);
}

fn vector(data_type: &str) -> Value {
    json!([{"dataset":"app.items","source_id":"owned","generation":3,"recipe_sha256":"b".repeat(64),
        "storage_relation":format!("g_{}", "c".repeat(40)),"physical_name":format!("pt_{}", "a".repeat(60)),
        "expected_columns":[{"name":"amount","data_type":data_type}],"source_committed_at":null}])
}

#[test]
fn oo_load_cut_with_non_uuid_generation_is_the_retained_contract() {
    let value = cut("cycle-1");
    validate_cut(&value, "cycle-1").unwrap();
    assert!(is_retained(&value));
    // The legacy ACK/import publisher keeps refusing the same document.
    let context = json!({"datasets":[{"dataset":"app.items","source":"owned","generation":3}],
        "publication":{"protocol":crate::native_library::PROTOCOL,"run_id":"cycle-1",
            "receipt_sha256":crate::canon::canonical_sha(&value),"receipt":value}});
    assert!(crate::native_publication::from_context(&context).is_err());
    assert!(
        crate::native_library::validate(&context["publication"]["receipt"], "cycle-1").is_err()
    );
}

#[test]
fn retained_cut_shape_is_closed() {
    assert!(validate_cut(&cut("cycle-1"), "cycle-2").is_err());
    assert!(validate_cut(&cut("../run"), "../run").is_err());
    for (path, bad) in [
        (
            "/protocol",
            json!("oo.pipeline.native-publication-receipt/v1"),
        ),
        ("/consistency", json!("per_table")),
        ("/consistency_grade", json!("S0")),
        ("/cross_table_consistent", json!(true)),
        ("/applied_pos", json!("1")),
        ("/position_codec", json!("mysql-gtid/v1")),
        ("/run_ids", json!(["cycle-1", "cycle-0"])),
        ("/target_generation", json!(4)),
        ("/tables/0/physical_name", json!("pt_short")),
        ("/tables/0/recipe_sha256", json!("not-a-digest")),
        ("/tables/0/storage_table", json!("other")),
        ("/tables/0/run_id", json!("cycle-0")),
        ("/tables/0/source_generation", json!(2)),
        ("/tables/0/capture_method", json!("c1_log")),
    ] {
        let mut value = cut("cycle-1");
        *value["shards"][0]["receipt"].pointer_mut(path).unwrap() = bad;
        rehash(&mut value);
        assert!(validate_cut(&value, "cycle-1").is_err(), "{path}");
    }
    let mut extra = cut("cycle-1");
    extra["shards"][0]["receipt"]["tables"][0]["canonical_digest"] = json!("a".repeat(64));
    rehash(&mut extra);
    assert!(validate_cut(&extra, "cycle-1").is_err());
    let mut stale = cut("cycle-1");
    stale["shards"][0]["receipt"]["tables"][0]["accepted_rows"] = json!(2);
    assert!(
        validate_cut(&stale, "cycle-1").is_err(),
        "receipt hash binds the shard"
    );
    let mut two = cut("cycle-1");
    let shard = two["shards"][0].clone();
    two["shards"].as_array_mut().unwrap().push(shard);
    assert!(validate_cut(&two, "cycle-1").is_err());
}

#[test]
fn owner_self_review_is_recompiled_not_trusted() {
    let fixture: Value = serde_json::from_str(REVIEWED_FIXTURE).unwrap();
    let review = reviewed(&fixture).unwrap();
    assert_eq!(review.visibility["role"], "oo_r_owner");
    assert!(review.decided.contains("app.items.amount"));
    let cases: [(&str, fn(&mut Value), &str); 6] = [
        (
            "mode",
            |v| v["review_decision"]["mode"] = json!("independent"),
            "review_mode_self_required",
        ),
        (
            "status",
            |v| v["status"] = json!("rejected"),
            "review_mode_self_required",
        ),
        (
            "command",
            |v| v["command"]["review"]["mode"] = json!("independent"),
            "review_mode_self_required",
        ),
        (
            "g14",
            |v| v["review_decision"]["decisions"]["g14"]["action"] = json!("reject"),
            "review_mode_self_required",
        ),
        (
            "item",
            |v| {
                let items = v["review_decision"]["decisions"]["items"]
                    .as_object_mut()
                    .unwrap();
                let key = items.keys().next().unwrap().clone();
                items[&key] = json!({"action":"reject","reason":"owner"});
            },
            "review_rejected_item",
        ),
        (
            "pack",
            |v| v["compiled"]["producer"]["pack_digest"] = json!("0".repeat(64)),
            "review_compiled_mismatch",
        ),
    ];
    for (name, mutate, code) in cases {
        let mut bad = fixture.clone();
        mutate(&mut bad);
        assert_eq!(reviewed(&bad).err(), Some(code), "{name}");
    }
    let mut edited = fixture.clone();
    edited["compiled"]["effective"]["datasets"][0]["columns"][0]["pii"] = json!("none");
    assert_eq!(reviewed(&edited).err(), Some("review_compiled_mismatch"));
}

#[test]
fn reviewed_visibility_must_cover_the_cut_with_exact_types() {
    let review = reviewed(&serde_json::from_str(REVIEWED_FIXTURE).unwrap()).unwrap();
    let bound = bind(&review, &vector("bigint")).unwrap();
    assert_eq!(bound[0].logical_columns, ["amount"]);
    assert_eq!(
        bind(&review, &vector("text")),
        Err("retained_column_binding")
    );
    let mut wider = vector("bigint");
    let mut other = wider[0].clone();
    other["dataset"] = json!("app.other");
    wider.as_array_mut().unwrap().push(other);
    assert_eq!(bind(&review, &wider), Err("retained_visibility_coverage"));
    let mut renamed = vector("bigint");
    renamed[0]["expected_columns"][0]["name"] = json!("total");
    assert_eq!(bind(&review, &renamed), Err("retained_column_binding"));
    let mut undecided = review;
    undecided.decided.clear();
    assert_eq!(
        bind(&undecided, &vector("bigint")),
        Err("retained_classification_decision")
    );
}

#[test]
fn gateway_document_is_hash_bound_to_the_reviewed_pack() {
    let fixture: Value = serde_json::from_str(REVIEWED_FIXTURE).unwrap();
    let review = reviewed(&fixture).unwrap();
    let value = cut("cycle-1");
    let publication = json!({"protocol":crate::native_library::PROTOCOL,"run_id":"cycle-1",
        "receipt_sha256":crate::canon::canonical_sha(&value),"receipt":value});
    let document = gateway_document(
        &review,
        &publication,
        &vector("bigint"),
        &"c".repeat(64),
        "owned",
        "oo_login",
    )
    .unwrap();
    let envelope = &document["envelope"];
    let producer = &envelope["producer"];
    assert_eq!(envelope["output_sha256"], json!(gateway_sha(producer)));
    assert_eq!(
        producer["review_sha256"],
        json!(gateway_sha(&envelope["review"]))
    );
    assert_eq!(
        producer["effective_sha256"],
        json!(gateway_sha(&document["effective"]))
    );
    assert_eq!(
        producer["visibility_sha256"],
        json!(gateway_sha(&document["visibility"]))
    );
    assert_eq!(
        producer["reviewed_registry_sha256"],
        json!(gateway_sha(&fixture))
    );
    assert!(producer.get("community_review_required").is_none());
    let column = &envelope["review"]["columns"]["app.items.amount"];
    assert_eq!(column["reviewer"], "owner");
    assert_eq!(column["explicit_allow"], true);
    let pack = oo_builder::compile_role_pack_with_ceilings(
        &document["effective"].to_string(),
        &document["visibility"].to_string(),
        &serde_json::from_value(document["ceilings"].clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(producer["pack_digest"], json!(pack.pack_digest));
    assert_eq!(
        producer["artifact"],
        serde_json::from_str::<Value>(&pack.artifact).unwrap()
    );
    assert_eq!(document["bindings"][0]["generation"], 3);
}

#[test]
fn identifiers_are_closed_before_any_connection() {
    for bad in ["", "a b", "x\"y", &"a".repeat(129)] {
        assert!(!run_token(bad));
    }
    assert!(run_token("cycle-1.retry_2"));
    assert!(ident("Upper").is_err() && ident("oo_r_owner").is_ok());
}

#[tokio::test]
async fn cli_request_is_closed_before_reading_config() {
    let args = ["publish-retained", "--config", "/nonexistent"].map(String::from);
    let missing = json!({"protocol":"oo.release-coordinator/publish-retained/v1","library_id":"owned",
        "generation":"cycle-1","login":"oo_login"});
    let exit = crate::dispatch(&args, missing.to_string().as_bytes()).await;
    assert_eq!(
        (exit.code, exit.stderr),
        (1, b"release-coordinator: malformed field\n".to_vec())
    );
    let mut waiver = missing.clone();
    waiver["reviewed_registry"] = json!({});
    waiver["force"] = json!(true);
    let exit = crate::dispatch(&args, waiver.to_string().as_bytes()).await;
    assert_eq!(
        (exit.code, exit.stderr),
        (1, b"release-coordinator: malformed waiver\n".to_vec())
    );
    let mut complete = missing;
    complete["reviewed_registry"] = json!({});
    let exit = crate::dispatch(&args, complete.to_string().as_bytes()).await;
    assert_eq!(
        (exit.code, exit.stderr),
        (1, b"release-coordinator: malformed config\n".to_vec())
    );
}

const COMPOSITE_FIXTURE: &str = include_str!("../fixtures/retained_reviewed_composite.json");

fn lines_vector(columns: &[&str]) -> Value {
    let expected: Vec<_> = columns
        .iter()
        .map(|name| json!({"name":name,"data_type":"bigint"}))
        .collect();
    json!([{"dataset":"app.lines","source_id":"owned","generation":3,"recipe_sha256":"b".repeat(64),
        "storage_relation":format!("g_{}", "c".repeat(40)),"physical_name":format!("pt_{}", "a".repeat(60)),
        "expected_columns":expected,"source_committed_at":null}])
}

#[test]
fn composite_identity_review_is_recompiled_bound_and_documented() {
    let fixture: Value = serde_json::from_str(COMPOSITE_FIXTURE).unwrap();
    let review = reviewed(&fixture).unwrap();
    let entity = &review.effective["entities"][0];
    assert_eq!(entity["key"], json!(["order_id", "line_no"]));
    let bound = bind(&review, &lines_vector(&["order_id", "line_no"])).unwrap();
    assert_eq!(bound[0].logical_columns, ["order_id", "line_no"]);
    assert_eq!(
        bind(&review, &lines_vector(&["order_id"])),
        Err("retained_column_binding")
    );
    let value = cut("cycle-1");
    let publication = json!({"protocol":crate::native_library::PROTOCOL,"run_id":"cycle-1",
        "receipt_sha256":crate::canon::canonical_sha(&value),"receipt":value});
    let document = gateway_document(
        &review,
        &publication,
        &lines_vector(&["order_id", "line_no"]),
        &"c".repeat(64),
        "owned",
        "oo_login",
    )
    .unwrap();
    assert_eq!(document["effective"]["entities"][0]["key"], entity["key"]);
    let pack = oo_builder::compile_role_pack_with_ceilings(
        &document["effective"].to_string(),
        &document["visibility"].to_string(),
        &serde_json::from_value(document["ceilings"].clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        document["envelope"]["producer"]["pack_digest"],
        json!(pack.pack_digest)
    );
    assert!(pack.artifact.contains("key: `order_id`, `line_no`"));
}

#[test]
fn composite_identity_cannot_be_reordered_or_left_unopened() {
    let fixture: Value = serde_json::from_str(COMPOSITE_FIXTURE).unwrap();
    for key in [
        json!(["line_no", "order_id"]),
        json!(["order_id"]),
        json!(null),
    ] {
        let mut bad = fixture.clone();
        let selection = bad["command"]["review"]["entities"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        selection["key"] = key.clone();
        assert_eq!(
            reviewed(&bad).err(),
            Some("review_compile_refused"),
            "{key}"
        );
    }
    // A fence that does not open every identity column is refused before any database work.
    let mut review = reviewed(&fixture).unwrap();
    review.visibility["datasets"][0]["columns"] = json!(["order_id"]);
    assert_eq!(
        bind(&review, &lines_vector(&["order_id"])),
        Err("retained_entity_key_binding")
    );
    let scalar = reviewed(&serde_json::from_str(REVIEWED_FIXTURE).unwrap()).unwrap();
    assert_eq!(scalar.effective["entities"][0]["key"], json!("amount"));
}
