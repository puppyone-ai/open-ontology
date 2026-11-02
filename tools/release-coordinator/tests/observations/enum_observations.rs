use super::*;

fn definition() -> Value {
    json!({
        "schema_version":"oos/v1","enum":"app.orders.status","owner":"ops",
        "classes":{"open":"Open","undetermined":"Not reviewed","unexplained":"Unknown"},
        "values":[
            {"code":"ok","label":"Open","class":"open","status":"approved",
                "source":"dictionary_table","review":{"state":"approved","by":"ops.reviewer"}},
            {"code":"pending","class":"undetermined","status":"undetermined"}
        ]
    })
}

fn document(enumeration: Value) -> Value {
    json!({"relationships":[],"enums":[enumeration],"metrics":[]})
}

fn detail(measured: &crate::apply::Measured) -> Value {
    serde_json::from_str(
        measured
            .detail
            .strip_prefix("independent_target_enum_observations;")
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn approval_requires_declared_review_owner_and_valid_meaning_not_just_a_label() {
    assert_eq!(
        library_enum_codes(&definition()).unwrap(),
        (
            vec!["ok".to_owned(), "pending".to_owned()],
            vec!["ok".to_owned()]
        )
    );
    for (path, value) in [
        ("/owner", Value::Null),
        ("/owner", json!(" ")),
        ("/values/0/review", Value::Null),
        ("/values/0/review/by", Value::Null),
        ("/values/0/review/by", json!(" ")),
        ("/values/0/review/state", json!("rejected")),
    ] {
        let mut enumeration = definition();
        *enumeration.pointer_mut(path).unwrap() = value;
        assert_eq!(
            library_enum_codes(&enumeration).unwrap_err(),
            ("not_run", "approved_review_owner_evidence_missing"),
            "{path}"
        );
    }
    let mut enumeration = definition();
    enumeration["values"][0]["label"] = Value::Null;
    assert_eq!(
        library_enum_codes(&enumeration).unwrap_err(),
        ("not_run", "approved_label_missing")
    );
    enumeration["values"][0]["class"] = json!("unexplained");
    assert_eq!(
        library_enum_codes(&enumeration).unwrap_err(),
        ("fail", "invalid_approved_class")
    );
}

#[test]
fn every_non_approved_state_is_excluded_from_approved_codes() {
    for (status, class) in [
        ("undetermined", "undetermined"),
        ("unexplained", "unexplained"),
        ("superseded", "open"),
        ("deprecated", "open"),
    ] {
        let mut enumeration = definition();
        enumeration["values"][0]["status"] = json!(status);
        enumeration["values"][0]["class"] = json!(class);
        assert!(
            library_enum_codes(&enumeration).unwrap().1.is_empty(),
            "{status}"
        );
    }
}

#[test]
fn optional_unreviewed_dictionary_never_fabricates_empty_data_or_passed_semantics() {
    let observation = library_unreviewed_enum_observation();
    assert_eq!(observation["status"], "warn");
    for key in ["non_null_rows", "approved_rows", "unlisted_rows", "listed_unapproved_rows"] {
        assert!(observation[key].is_null(), "{key}");
    }
    assert_eq!(observation["coverage"]["defined"], false);
    assert!(observation["coverage"]["threshold_met"].is_null());
    let gate = library_enum_gate(&json!({}), vec![observation], &json!({"user":"owned"}));
    assert_eq!(gate.status, "warn");
    assert!(!gate.blocking);
    assert_eq!(detail(&gate)["business_label_accuracy"], "not_measured");
    assert_eq!(detail(&gate)["business_owner_signed"], false);
}

#[test]
fn malformed_duplicate_and_time_scoped_definitions_cannot_be_flattened() {
    let mut enumeration = definition();
    enumeration["values"][1]["code"] = json!("ok");
    assert_eq!(
        library_enum_codes(&enumeration).unwrap_err(),
        ("fail", "duplicate_enum_code")
    );
    for bad in [json!(2), Value::Null] {
        let mut enumeration = definition();
        enumeration["values"][0]["code"] = bad;
        assert_eq!(
            library_enum_codes(&enumeration).unwrap_err(),
            ("fail", "invalid_enum_definition")
        );
    }
    for field in ["valid_from", "valid_to"] {
        let mut enumeration = definition();
        enumeration["values"][0][field] = json!("2026-10-23");
        assert_eq!(
            library_enum_codes(&enumeration).unwrap_err(),
            ("not_run", "temporal_scope_unbound")
        );
    }
}

#[test]
fn exact_row_weighted_threshold_counts_unknown_and_listed_unapproved_separately() {
    let counts = library_enum_counts(100, 3, 95, 1).unwrap();
    assert_eq!(counts["coverage"]["threshold_met"], true);
    assert_eq!(counts["listed_unapproved_rows"], 2);
    assert_eq!(counts["review_needed"], true);
    assert_eq!(counts["status"], "warn");
    assert_eq!(
        library_enum_counts(100, 3, 94, 1).unwrap()["coverage"]["threshold_met"],
        false
    );
    assert_eq!(
        library_enum_counts(100, 0, 100, 1).unwrap()["review_needed"],
        false
    );
    let max = i64::MAX;
    let cutoff = ((i128::from(max) * 95 + 99) / 100) as i64;
    assert_eq!(
        library_enum_counts(max, 0, cutoff, 1).unwrap()["coverage"]["threshold_met"],
        true
    );
    assert_eq!(
        library_enum_counts(max, 0, cutoff - 1, 1).unwrap()["coverage"]["threshold_met"],
        false
    );
}

#[test]
fn zero_rows_and_impossible_counts_are_not_successes() {
    let zero = library_enum_counts(0, 0, 0, 1).unwrap();
    assert_eq!(zero["status"], "not_run");
    assert_eq!(zero["coverage"]["defined"], false);
    assert_eq!(zero["coverage"]["threshold_met"], Value::Null);
    for counts in [
        (-1, 0, 0),
        (1, -1, 0),
        (1, 0, -1),
        (1, 2, 0),
        (2, 1, 2),
        (i64::MAX, i64::MAX, i64::MAX),
    ] {
        assert!(library_enum_counts(counts.0, counts.1, counts.2, 1).is_err());
    }
}

#[test]
fn empty_without_approved_semantics_warns_without_claiming_coverage_or_review() {
    let mut enumeration = definition();
    enumeration["owner"] = Value::Null;
    enumeration["values"] = json!([
        {"code":"pending","status":"undetermined","class":"undetermined","label":"Not approved"},
        {"code":"unknown","status":"unexplained","class":"unexplained"}
    ]);
    let (_, approved) = library_enum_codes(&enumeration).unwrap();
    assert!(approved.is_empty());
    let counts = library_enum_counts(0, 0, 0, approved.len()).unwrap();
    assert_eq!(counts["status"], "warn");
    assert_eq!(counts["reason"], "no_approved_semantics_empty_field");
    assert_eq!(counts["coverage"]["defined"], false);
    assert_eq!(counts["coverage"]["threshold_met"], Value::Null);
    assert_eq!(counts["review_needed"], true);
    let gate = library_enum_gate(&document(enumeration), vec![counts], &json!({}));
    assert_eq!((gate.status, gate.blocking), ("warn", false));
    assert_eq!(detail(&gate)["coverage_threshold_met"], Value::Null);
    assert_eq!(detail(&gate)["business_owner_signed"], false);
    assert_eq!(library_enum_counts(0, 0, 0, 1).unwrap()["status"], "not_run");
    assert!(library_enum_counts(1, 0, 1, 0).is_err());
    let mut missing_review = definition();
    missing_review["values"][0]["review"] = Value::Null;
    assert!(library_enum_codes(&missing_review).is_err());
}

#[test]
fn coverage_is_per_enum_and_never_authenticates_labels_or_owners() {
    let effective = document(definition());
    let gate = library_enum_gate(
        &effective,
        vec![
            library_enum_counts(10000, 0, 10000, 1).unwrap(),
            library_enum_counts(10, 10, 0, 1).unwrap(),
        ],
        &json!({}),
    );
    assert_eq!(gate.status, "warn");
    assert!(!gate.blocking);
    let report = detail(&gate);
    assert_eq!(report["coverage_threshold_met"], false);
    assert_eq!(report["business_label_accuracy"], "not_measured");
    assert_eq!(report["business_owner_signed"], false);
    assert_eq!(report["owner_authenticated"], false);
    assert_eq!(report["production_certification"], "not_issued");
    for field in ["label", "code", "class", "review"] {
        let mut changed = effective.clone();
        changed["enums"][0]["values"][0][field] = json!("changed");
        assert_ne!(
            report["effective_sha256"],
            crate::canon::canonical_sha(&changed)
        );
    }
}

#[test]
fn unmeasured_or_failed_enum_is_not_hidden_by_a_measured_enum() {
    for (observation, status) in [
        (library_enum_counts(0, 0, 0, 1).unwrap(), "not_run"),
        (
            json!({"status":"not_run","reason":"approved_review_owner_evidence_missing"}),
            "not_run",
        ),
        (
            json!({"status":"fail","reason":"approved_enum_target_mapping_missing"}),
            "fail",
        ),
    ] {
        let gate = library_enum_gate(
            &document(definition()),
            vec![library_enum_counts(100, 0, 100, 1).unwrap(), observation],
            &json!({}),
        );
        assert_eq!(gate.status, status);
        assert!(gate.blocking);
        assert_eq!(detail(&gate)["coverage_threshold_met"], Value::Null);
    }
    assert!(library_enum_gate(&document(definition()), vec![], &json!({})).blocking);
}

#[test]
fn measured_warning_keeps_fixture_not_run_and_never_becomes_a_production_certificate() {
    let gate = library_enum_gate(
        &document(definition()),
        vec![library_enum_counts(100, 0, 100, 1).unwrap()],
        &json!({}),
    );
    let hash = "ab".repeat(32);
    let fact = crate::decide::GateFact {
        gate_id: "G4".into(),
        source_hash: hash.clone(),
        issuer_class: "fixture_authority".into(),
        status: gate.status.into(),
        blocking: gate.blocking,
        scope: "test_scope".into(),
        report_type: "target_measurement".into(),
        authority_statement_sha256: Some(hash.clone()),
        certificate_sha256: "cd".repeat(32),
    };
    assert_eq!(
        crate::decide::fact_disposition(&fact, &hash, &hash, &["G4".into()]),
        "not_run"
    );
    assert!(!crate::decide::production_admissible(&fact));
}

#[tokio::test]
#[ignore = "requires OO_G4_TEST_CONFIG for an owned local TLS PostgreSQL database"]
async fn local_pg_enum_observer_measures_and_fails_closed_without_publication() {
    let config: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_G4_TEST_CONFIG").expect("owned fixture config")).unwrap(),
    )
    .unwrap();
    let endpoint = Endpoint {
        host: config["host"].as_str().unwrap().into(),
        port: config["port"].as_u64().unwrap() as u16,
        database: config["database"].as_str().unwrap().into(),
        pack_ceilings: None,
    };
    let user = config["user"].as_str().unwrap();
    assert!(matches!(endpoint.host.as_str(), "127.0.0.1" | "localhost"));
    assert!(endpoint.database.starts_with("g4_observer_test_"));
    assert!(user.starts_with("g4_observer_test_"));
    let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
    let ca = std::fs::read(config["ca_file"].as_str().unwrap()).unwrap();
    let conn = TargetConn {
        endpoint: &endpoint,
        user,
        password: password.trim_end(),
        ca_pem: &ca,
    };
    let session = open(&endpoint, user, conn.password, &ca).await.unwrap();
    require_tls(&session.client).await.unwrap();
    let bypass: bool = session
        .client
        .query_one(
            "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(!bypass);
    session.client.batch_execute(
        "CREATE SCHEMA g4_fixture;
         CREATE TABLE g4_fixture.orders(id integer, status text);
         INSERT INTO g4_fixture.orders SELECT n, CASE WHEN n<=95 THEN 'ok' WHEN n<=97 THEN 'pending' ELSE 'rogue' END FROM generate_series(1,100) n;
         INSERT INTO g4_fixture.orders SELECT n,NULL FROM generate_series(101,110) n;"
    ).await.unwrap();
    let logical = vec![(
        "app".into(),
        "orders".into(),
        "unused".into(),
        vec!["status".into()],
    )];
    let physical = vec![(
        "g4_fixture".into(),
        "orders".into(),
        "unused".into(),
        vec!["status".into()],
    )];
    let effective = document(definition());
    let observe = |doc: Value| {
        let conn = conn;
        let logical = &logical;
        let physical = &physical;
        async move { library_semantic_observations(&conn, &doc, logical, physical, 1).await }
    };
    let (_, gate) = observe(effective.clone()).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("warn", false));
    let report = detail(&gate);
    let measured = &report["enums"][0];
    assert_eq!(measured["non_null_rows"], 100);
    assert_eq!(measured["approved_rows"], 95);
    assert_eq!(measured["unlisted_rows"], 3);
    assert_eq!(measured["listed_unapproved_rows"], 2);
    assert_eq!(measured["coverage"]["threshold_met"], true);
    assert_eq!(
        measured["definition_sha256"],
        crate::canon::canonical_sha(&effective["enums"][0])
    );
    assert_eq!(report["observer"]["database"], endpoint.database);
    assert_eq!(report["observer"]["read_only"], true);
    println!("G4_OWNED_PG coverage=95/100 unknown=3 listed_unapproved=2 nulls_excluded=10");

    session
        .client
        .batch_execute("UPDATE g4_fixture.orders SET status='rogue' WHERE id=1")
        .await
        .unwrap();
    let (_, gate) = observe(effective.clone()).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("warn", false));
    assert_eq!(
        detail(&gate)["enums"][0]["coverage"]["threshold_met"],
        false
    );
    assert_eq!(detail(&gate)["enums"][0]["approved_rows"], 94);

    let mut invented_counts = effective.clone();
    invented_counts["enums"][0]["coverage"] = json!("1");
    invented_counts["enums"][0]["unexplained_values"] = json!([]);
    invented_counts["enums"][0]["values"][0]["evidence"] = json!({"rows":1000000});
    let (_, gate) = observe(invented_counts).await.unwrap();
    assert_eq!(detail(&gate)["enums"][0]["approved_rows"], 94);
    assert_eq!(detail(&gate)["enums"][0]["unlisted_rows"], 4);

    let mut unreviewed = effective.clone();
    unreviewed["enums"][0]["values"][0]["review"] = Value::Null;
    let (_, gate) = observe(unreviewed).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("not_run", true));
    let mut missing = effective.clone();
    missing["enums"][0]["enum"] = json!("app.orders.missing");
    let (_, gate) = observe(missing).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("fail", true));
    assert!(
        library_semantic_observations(&conn, &effective, &logical, &[], 1)
            .await
            .is_err()
    );
    let duplicate_logical = vec![logical[0].clone(), logical[0].clone()];
    let duplicate_physical = vec![physical[0].clone(), physical[0].clone()];
    assert!(
        library_semantic_observations(
            &conn,
            &effective,
            &duplicate_logical,
            &duplicate_physical,
            1
        )
        .await
        .is_err()
    );

    let mut unknown = definition();
    unknown["owner"] = Value::Null;
    unknown["values"] = json!([{"code":"ok","status":"unexplained","class":"unexplained"}]);
    let (_, gate) = observe(document(unknown)).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("warn", false));
    assert_eq!(detail(&gate)["enums"][0]["approved_rows"], 0);
    let mut unmapped_unknown = effective.clone();
    unmapped_unknown["enums"][0]["enum"] = json!("app.orders.missing");
    unmapped_unknown["enums"][0]["values"] = json!([]);
    let (_, gate) = observe(unmapped_unknown).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("not_run", true));

    let (_, gate) = observe(json!({"relationships":[],"enums":[]}))
        .await
        .unwrap();
    assert_eq!((gate.status, gate.blocking), ("warn", false));
    let mut historical = effective.clone();
    historical["enums"][0]["values"][0]["valid_from"] = json!("2020-01-01");
    let (_, gate) = observe(historical).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("not_run", true));
    assert_eq!(
        detail(&gate)["enums"][0]["reason"],
        "temporal_scope_unbound"
    );

    session
        .client
        .batch_execute(
            "ALTER TABLE g4_fixture.orders ENABLE ROW LEVEL SECURITY;
         ALTER TABLE g4_fixture.orders FORCE ROW LEVEL SECURITY;
         CREATE POLICY deny_all ON g4_fixture.orders USING(false)",
        )
        .await
        .unwrap();
    let (_, gate) = observe(effective.clone()).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("not_run", true));
    assert!(gate.detail.contains("rls_filtered"));
    session
        .client
        .batch_execute(
            "ALTER TABLE g4_fixture.orders DISABLE ROW LEVEL SECURITY;
         UPDATE g4_fixture.orders SET status=NULL",
        )
        .await
        .unwrap();
    let (_, gate) = observe(effective.clone()).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("not_run", true));
    assert_eq!(detail(&gate)["enums"][0]["non_null_rows"], 0);
    let mut no_approval = definition();
    no_approval["values"][0]["status"] = json!("undetermined");
    no_approval["values"][0]["class"] = json!("undetermined");
    let (_, gate) = observe(document(no_approval.clone())).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("warn", false));
    let observed = &detail(&gate)["enums"][0];
    assert_eq!(observed["approved_codes"], 0);
    assert_eq!(observed["non_null_rows"], 0);
    assert_eq!(observed["reason"], "no_approved_semantics_empty_field");
    assert_eq!(observed["coverage"]["defined"], false);
    assert_eq!(observed["coverage"]["threshold_met"], Value::Null);
    assert_eq!(observed["review_needed"], true);
    let mut incomplete = effective.clone();
    incomplete["enums"][0]["values"][0]["review"] = Value::Null;
    let (_, gate) = observe(incomplete).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("not_run", true));
    session
        .client
        .batch_execute("TRUNCATE g4_fixture.orders")
        .await
        .unwrap();
    let (_, gate) = observe(effective.clone()).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("not_run", true));
    no_approval["values"] = json!([]);
    let (_, gate) = observe(document(no_approval)).await.unwrap();
    assert_eq!((gate.status, gate.blocking), ("warn", false));
    assert_eq!(detail(&gate)["enums"][0]["reason"], "no_approved_semantics_empty_field");

    let mut quoted = definition();
    quoted["values"][0]["code"] = json!("x'); DROP TABLE g4_fixture.orders;--");
    session
        .client
        .execute(
            "INSERT INTO g4_fixture.orders VALUES(1,$1)",
            &[&quoted["values"][0]["code"].as_str().unwrap()],
        )
        .await
        .unwrap();
    let (_, gate) = observe(document(quoted)).await.unwrap();
    assert_eq!(detail(&gate)["enums"][0]["approved_rows"], 1);
    assert_eq!(detail(&gate)["enums"][0]["coverage"]["threshold_met"], true);
    session
        .client
        .batch_execute(
            "TRUNCATE g4_fixture.orders;
         INSERT INTO g4_fixture.orders VALUES (1,'OK'),(2,'ok '),(3,''),(4,'ok'),(5,NULL)",
        )
        .await
        .unwrap();
    let (_, gate) = observe(effective.clone()).await.unwrap();
    assert_eq!(detail(&gate)["enums"][0]["non_null_rows"], 4);
    assert_eq!(detail(&gate)["enums"][0]["approved_rows"], 1);
    assert_eq!(detail(&gate)["enums"][0]["unlisted_rows"], 3);
    session
        .client
        .batch_execute("ALTER TABLE g4_fixture.orders RENAME COLUMN status TO gone")
        .await
        .unwrap();
    assert!(observe(effective).await.is_err());
    session
        .client
        .batch_execute("DROP SCHEMA g4_fixture CASCADE")
        .await
        .unwrap();
    println!(
        "G4_OWNED_PG missing_review=blocking approved_unmapped=fail rls=blocking approved_empty=blocking no_approved_empty=warn sql_error=refused publication_ran=false"
    );
}
