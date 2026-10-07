mod common;

use std::path::PathBuf;

use cdc_apply_pg::{
    apply, binding_from, config_preimage, convert_cell, digest_text, plan_tables, type_preimage,
    ColumnDecl, Endpoint, KeyDecl, PgType, Request, Segment, Stored, APPLY_LIMIT, CONTRACT,
};
use cdc_spool::Cell;

fn endpoint(password: &str) -> Endpoint {
    Endpoint {
        host: "invalid.invalid".to_owned(),
        port: 1,
        database: "oo_a301".to_owned(),
        user: "oo_a301_runtime".to_owned(),
        password_file: PathBuf::from(password),
        ca_file: PathBuf::from("/no/such-ca"),
        server_name: "invalid.invalid".to_owned(),
    }
}

fn base_request(spool: PathBuf, password: &str) -> Request {
    Request {
        contract: CONTRACT.to_owned(),
        spool_dir: spool,
        endpoint: endpoint(password),
        shard: "billing".to_owned(),
        owner: "11111111-1111-4111-8111-111111111111".to_owned(),
        generation: 1,
        table_generation: "g1".to_owned(),
        run_generation: 1,
        lineage: common::LINEAGE.to_owned(),
        source: "billing".to_owned(),
        epoch: 1,
        control_lease_valid: true,
        segment: None,
        keys: vec![KeyDecl {
            table: "public.orders".to_owned(),
            ordinal: 0,
            name: "id".to_owned(),
            pg_type: "int8".to_owned(),
        }],
        columns: vec![ColumnDecl {
            table: "public.orders".to_owned(),
            ordinal: 0,
            name: "qty".to_owned(),
            pg_type: "int8".to_owned(),
            nullable: true,
        }],
    }
}

#[test]
fn sha256_empty_and_int8_bounds_stay_distinct() {
    assert_eq!(
        digest_text(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    let min = Cell::Int {
        ordinal: 0,
        value: "-9223372036854775808".to_owned(),
    };
    let max = Cell::Int {
        ordinal: 0,
        value: "9223372036854775807".to_owned(),
    };
    assert_eq!(
        convert_cell(&min, PgType::Int8).expect("min"),
        Stored::Value("-9223372036854775808".to_owned())
    );
    assert_eq!(
        convert_cell(&max, PgType::Int8).expect("max"),
        Stored::Value("9223372036854775807".to_owned())
    );
    assert_eq!(
        convert_cell(&Cell::Null { ordinal: 0 }, PgType::Int8).expect("null"),
        Stored::Null
    );
    assert_eq!(
        convert_cell(&Cell::Unavailable { ordinal: 0 }, PgType::Int8).expect("missing"),
        Stored::Unavailable
    );
    assert_eq!(
        convert_cell(
            &Cell::Text {
                ordinal: 0,
                value: "a\0b".to_owned(),
            },
            PgType::Text
        ),
        Err("conversion_rejected")
    );
    assert_eq!(
        convert_cell(
            &Cell::Decimal {
                ordinal: 0,
                value: "1".to_owned(),
            },
            PgType::Int8
        ),
        Err("type_mismatch")
    );
    assert_eq!(
        convert_cell(
            &Cell::Int {
                ordinal: 0,
                value: "40000".to_owned(),
            },
            PgType::Int2
        ),
        Err("out_of_range")
    );
    assert_eq!(
        convert_cell(
            &Cell::Bytes {
                ordinal: 0,
                value: "aa".to_owned(),
            },
            PgType::Text
        ),
        Err("type_mismatch")
    );
}

#[test]
fn config_preimage_names_the_caller_declared_policy() {
    let request = base_request(PathBuf::from("/none"), "/no/such-pass");
    let tables = plan_tables(&request, &["public.orders".to_owned()]).expect("plan");
    let text = config_preimage(&binding_from(&request, &tables));
    assert!(text.starts_with("cdc-apply-config/v1\n"));
    assert!(text.contains("provenance=caller_declared\n"));
    assert!(text.contains("position_codec=pg-commit-lsn/v1\n"));
    let typed = type_preimage(&tables);
    assert!(typed.contains("zero_date=null\n"));
    assert!(typed.contains("tinyint1=smallint\n"));
    assert!(typed.contains("strip_nul=true\n"));
    assert_eq!(APPLY_LIMIT, 200_000);
}

#[tokio::test]
async fn segment_returns_before_a_missing_password_file() {
    let mut request = base_request(PathBuf::from("/no/such-spool"), "/no/such-pass");
    request.segment = Some(Segment { index: 0, count: 2 });
    let output = apply(&request).await;
    assert_eq!(output.fault.as_deref(), Some("needs_segments"));
    assert!(!output.target_applied);
    assert!(!output.db_cross_table_consistent);
    assert!(!output.source_cross_table_consistent);
    assert!(!output.ack_allowed);
    assert!(!output.control_lease_consulted);
    assert_eq!(output.commit_position_provenance, "caller_declared");
}

#[tokio::test]
async fn unsupported_type_returns_before_a_missing_password_file() {
    let dir = common::work_dir("unsupported");
    let spool = dir.join("spool");
    let row = common::data_line(
        "wide0",
        1,
        1,
        "public.orders",
        "1",
        "upsert",
        r#"[{"type":"int","ordinal":0,"value":"1"}]"#,
        &[],
    );
    common::seal(
        &spool,
        "wide0",
        &["public.orders"],
        &[row],
        &[("public.orders", 1)],
        common::LINEAGE,
        common::LINEAGE,
        common::LINEAGE,
    );
    let mut request = base_request(spool, "/no/such-pass");
    request.columns[0].pg_type = "numeric".to_owned();
    let output = apply(&request).await;
    assert_eq!(output.fault.as_deref(), Some("unsupported_type"));
    assert!(!output.target_applied);
}

#[test]
fn product_source_does_not_grant_or_take_the_advisory_lock() {
    let files = [
        include_str!("../src/apply.rs"),
        include_str!("../src/main.rs"),
        include_str!("../src/tls.rs"),
        include_str!("../src/model.rs"),
    ];
    let advisory = ["pg_advisory_", "lock"].concat();
    for source in files {
        assert!(!source.contains("GRANT "));
        assert!(!source.contains(&advisory));
        assert!(!source.contains("XACK"));
        assert!(!source.contains("set_config("));
        assert!(!source.contains("SslMode::Disable"));
    }
    assert!(include_str!("../src/apply.rs").contains("SslMode::Require"));
    assert!(include_str!("../src/main.rs").contains("password"));
}
