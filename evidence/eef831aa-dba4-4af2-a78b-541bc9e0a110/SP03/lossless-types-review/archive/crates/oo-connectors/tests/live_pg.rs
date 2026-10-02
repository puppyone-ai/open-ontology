//! Live PostgreSQL 17.10 checks. `OO_PG_FIXTURE` is required; there is no skip.

use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use oo_connectors::{
    EMPTY_SCHEMA_HASH, PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSession, PgSourceConfig,
    PgTlsConfig, SecretRef, SelectedTable, SourceCatalog, SourceErrorKind, SourceId, SourceTable,
    TableSelection,
};
use serde::Deserialize;
use zeroize::Zeroizing;

const CANARY: &str = "CANARY-COMMENT-SECRET-a060";
const CANARY_SHA: &str = "99a042fad752401594c711cdc674f22470412394789d6e485622b0de7f264d50";
const READER_SCHEMA_HASH: &str = "b0dbcffe78f8818af5c72f37c2db048c7058ccfaf8eb1eef4b17b83ac37d9692";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    empty_database: String,
    reader_user: String,
    inherit_user: String,
    setonly_user: String,
    definer_user: String,
    side_user: String,
    hiddenfn_user: String,
    schemacreate_user: String,
    dbcreate_user: String,
    dual_user: String,
    inherit_only_user: String,
    transitive_user: String,
    noset_user: String,
    column_user: String,
    super_user: String,
    bypass_user: String,
    createrole_user: String,
    createdb_user: String,
    oponly_user: String,
    castonly_user: String,
    guc_user: String,
    password_dir: String,
    ca_file: String,
    wrong_ca_file: String,
    wrong_host: String,
    log_file: String,
}

#[tokio::test]
async fn live_source_catalog() {
    if let Ok(kind) = std::env::var("OO_A066_PROBE") {
        run_env_probe(&kind).await;
        return;
    }
    let path = std::env::var("OO_PG_FIXTURE").expect("OO_PG_FIXTURE is required");
    let pointer: Pointer =
        serde_json::from_str(&fs::read_to_string(&path).expect("pointer")).expect("pointer json");
    let reader_pass = read_secret(&pointer.password_dir, "reader.pass");
    let inherit_pass = read_secret(&pointer.password_dir, "inherit.pass");
    let setonly_pass = read_secret(&pointer.password_dir, "setonly.pass");
    let definer_pass = read_secret(&pointer.password_dir, "definer.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let wrong_ca = fs::read(&pointer.wrong_ca_file).expect("wrong ca");
    let selection = full_selection();

    let reader_dial = Dial {
        source_id: "pg_source_lab",
        secret: "secret://pg/source-lab/reader",
        user: &pointer.reader_user,
        password: &reader_pass,
        host: &pointer.host,
        port: pointer.port,
        database: &pointer.database,
        ca: &ca,
    };
    let reader = reader_dial
        .open(selection.clone())
        .await
        .expect("reader connect");
    assert_closed(
        "connector",
        &format!("{reader:?}"),
        &reader_pass,
        &pointer.host,
    );
    let mut reader = reader;
    let report = reader.inspect_access().await.expect("reader inspect");
    assert!(
        report.registration_allowed,
        "reader blocked: {:?}",
        report
            .block_reasons
            .iter()
            .map(|r| r.code.as_str())
            .collect::<Vec<_>>()
    );
    assert!(report.block_reasons.is_empty());
    assert_eq!(report.version.admission, "tested");
    assert_eq!(report.version.major, 17);
    assert_eq!(report.version.minor, 10);
    assert!(report.version.server_version.starts_with("17.10"));
    assert_eq!(report.in_recovery.value, Some(false));
    assert_eq!(report.write.state, "read_only");
    assert!(report.write.findings.is_empty());
    assert!(report.security_definers.is_empty());
    assert!(!report.cdc.implemented);
    assert_eq!(report.tls.mode, "verify-full");
    assert_eq!(report.tls.trust, "caller-ca-only");
    assert!(report.tls.hostname_verification);
    assert!(
        report.tls.negotiated_version.starts_with("TLSv1."),
        "{}",
        report.tls.negotiated_version
    );
    assert_eq!(report.tls.server_observation, "observed");
    assert!(report.capabilities.iter().all(|item| !item.supported));
    assert!(
        report
            .dba_checklist
            .iter()
            .any(|item| item.id == "not_a_standby")
    );
    assert!(
        report
            .dba_checklist
            .iter()
            .any(|item| item.statement.contains("<由 DBA 设置>"))
    );
    assert!(
        report
            .dba_checklist
            .iter()
            .any(|item| item.statement.contains("CREATE PUBLICATION"))
    );
    assert_closed(
        "report",
        &format!("{report:?}"),
        &reader_pass,
        &pointer.host,
    );

    let catalog = reader.catalog().await.expect("reader catalog");
    assert_eq!(catalog.schema_hash, READER_SCHEMA_HASH);
    assert_eq!(catalog.tables.len(), 7);
    assert!(
        catalog
            .tables
            .iter()
            .all(|table| table.name != "side_write")
    );
    assert_catalog_truth(&catalog);
    assert_closed(
        "catalog",
        &format!("{catalog:?}"),
        &reader_pass,
        &pointer.host,
    );
    let again = reader.catalog().await.expect("second catalog");
    assert_eq!(catalog.schema_hash, again.schema_hash);
    assert_ne!(catalog.captured_at, "");

    let base = reader_dial.catalog_one("app", "item_base").await;
    let next = reader_dial.catalog_one("app", "item_next").await;
    eprintln!(
        "item_base_hash={} item_next_hash={}",
        base.schema_hash, next.schema_hash
    );
    assert_eq!(base.tables.len(), 1);
    assert_eq!(next.tables.len(), 1);
    assert_eq!(
        base.tables[0].columns.len() + 1,
        next.tables[0].columns.len()
    );
    assert_ne!(
        base.schema_hash, next.schema_hash,
        "item_base={} item_next={}",
        base.schema_hash, next.schema_hash
    );
    let base_again = reader_dial.catalog_one("app", "item_base").await;
    assert_eq!(base.schema_hash, base_again.schema_hash);

    let empty_scope = reader_dial
        .open(TableSelection::new(Vec::new()).unwrap())
        .await
        .unwrap();
    let mut empty_scope = empty_scope;
    let empty_report = empty_scope.inspect_access().await.unwrap();
    assert!(empty_report.registration_allowed);
    let empty_catalog = empty_scope.catalog().await.unwrap();
    assert_eq!(empty_catalog.schema_hash, EMPTY_SCHEMA_HASH);
    assert!(empty_catalog.tables.is_empty());

    let empty_db = Dial {
        source_id: "pg_source_empty",
        secret: "secret://pg/source-empty/reader",
        database: &pointer.empty_database,
        ..reader_dial
    }
    .open(TableSelection::new(Vec::new()).unwrap())
    .await
    .unwrap();
    let mut empty_db = empty_db;
    let empty_db_report = empty_db.inspect_access().await.unwrap();
    assert!(
        empty_db_report.registration_allowed,
        "{:?}",
        empty_db_report.block_reasons
    );
    assert_eq!(
        empty_db_report.principal.value.unwrap().database,
        pointer.empty_database
    );
    assert_eq!(
        empty_db.catalog().await.unwrap().schema_hash,
        EMPTY_SCHEMA_HASH
    );
    drop(empty_db);
    drop(empty_scope);

    let reconnected = reader_dial
        .open(TableSelection::new(vec![SelectedTable::new("app", "item_base").unwrap()]).unwrap())
        .await
        .unwrap();
    let mut reconnected = reconnected;
    assert_eq!(
        reconnected.catalog().await.unwrap().schema_hash,
        base.schema_hash
    );

    let wrong_ca = Dial {
        ca: &wrong_ca,
        ..reader_dial
    }
    .open(selection.clone())
    .await
    .expect_err("wrong ca");
    assert_eq!(
        wrong_ca.kind(),
        SourceErrorKind::Tls,
        "{:?}",
        wrong_ca.fact()
    );
    assert_closed(
        "wrong-ca",
        &format!("{wrong_ca} {wrong_ca:?}"),
        &reader_pass,
        &pointer.host,
    );

    let wrong_host = Dial {
        host: &pointer.wrong_host,
        ..reader_dial
    }
    .open(selection.clone())
    .await
    .expect_err("wrong host");
    assert_eq!(
        wrong_host.kind(),
        SourceErrorKind::Tls,
        "{:?}",
        wrong_host.fact()
    );
    assert_closed(
        "wrong-host",
        &format!("{wrong_host} {wrong_host:?}"),
        &reader_pass,
        &pointer.wrong_host,
    );

    let wrong_password = Zeroizing::new("wrong-password-value".to_string());
    let bad_password = Dial {
        password: &wrong_password,
        ..reader_dial
    }
    .open(selection.clone())
    .await
    .expect_err("wrong password");
    assert_eq!(bad_password.kind(), SourceErrorKind::Auth);
    assert_eq!(bad_password.sqlstate(), Some("28P01"));
    assert_closed(
        "wrong-password",
        &format!("{bad_password} {bad_password:?}"),
        &wrong_password,
        &pointer.host,
    );

    let inherit = Dial {
        source_id: "pg_source_inherit",
        secret: "secret://pg/source-lab/inherit",
        user: &pointer.inherit_user,
        password: &inherit_pass,
        ..reader_dial
    }
    .open(selection.clone())
    .await
    .unwrap();
    let mut inherit = inherit;
    let inherit_report = inherit.inspect_access().await.unwrap();
    assert!(!inherit_report.registration_allowed);
    assert!(
        inherit_report
            .block_reasons
            .iter()
            .any(|item| item.code == "write_capable")
    );
    assert!(
        inherit_report
            .write
            .findings
            .iter()
            .any(|item| item.path == "inherited" || item.path == "set_role")
    );
    assert!(
        inherit_report
            .memberships
            .iter()
            .any(|item| item.role == "oo_src_writer" && item.inherit_option && item.set_option)
    );

    let setonly = Dial {
        source_id: "pg_source_setonly",
        secret: "secret://pg/source-lab/setonly",
        user: &pointer.setonly_user,
        password: &setonly_pass,
        ..reader_dial
    }
    .open(selection.clone())
    .await
    .unwrap();
    let mut setonly = setonly;
    let set_report = setonly.inspect_access().await.unwrap();
    assert!(!set_report.registration_allowed);
    assert!(
        set_report
            .write
            .findings
            .iter()
            .any(|item| item.path == "set_role")
    );
    assert!(
        !set_report
            .write
            .findings
            .iter()
            .any(|item| item.role == pointer.setonly_user && item.path == "direct")
    );
    assert!(
        set_report
            .memberships
            .iter()
            .any(|item| item.role == "oo_src_writer" && !item.inherit_option && item.set_option)
    );

    let definer = Dial {
        source_id: "pg_source_definer",
        secret: "secret://pg/source-lab/definer",
        user: &pointer.definer_user,
        password: &definer_pass,
        ..reader_dial
    }
    .open(selection.clone())
    .await
    .unwrap();
    let mut definer = definer;
    let definer_report = definer.inspect_access().await.unwrap();
    assert!(!definer_report.registration_allowed);
    assert!(
        definer_report
            .block_reasons
            .iter()
            .any(|item| item.code == "security_definer")
    );
    let names: Vec<&str> = definer_report
        .security_definers
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert!(names.contains(&"dangerous_insert"));
    assert!(names.contains(&"public_definer"));
    assert_closed(
        "definer",
        &format!("{definer_report:?}"),
        &definer_pass,
        &pointer.host,
    );

    let mut missing = reader_dial
        .open(
            TableSelection::new(vec![SelectedTable::new("app", "missing_table").unwrap()]).unwrap(),
        )
        .await
        .unwrap();
    let missing_err = missing.catalog().await.expect_err("missing table");
    assert_eq!(missing_err.fact(), Some("selected_table_missing"));
    assert_closed(
        "missing",
        &format!("{missing_err} {missing_err:?}"),
        &reader_pass,
        &pointer.host,
    );

    let side_pass = read_secret(&pointer.password_dir, "side.pass");
    let hidden_pass = read_secret(&pointer.password_dir, "hiddenfn.pass");
    let schema_pass = read_secret(&pointer.password_dir, "schemacreate.pass");
    let db_pass = read_secret(&pointer.password_dir, "dbcreate.pass");
    let dual_pass = read_secret(&pointer.password_dir, "dual.pass");
    let inherit_only_pass = read_secret(&pointer.password_dir, "inherit-only.pass");
    let transitive_pass = read_secret(&pointer.password_dir, "transitive.pass");
    let noset_pass = read_secret(&pointer.password_dir, "noset.pass");
    let column_pass = read_secret(&pointer.password_dir, "column.pass");
    let super_pass = read_secret(&pointer.password_dir, "login-super.pass");
    let bypass_pass = read_secret(&pointer.password_dir, "bypass.pass");
    let createrole_pass = read_secret(&pointer.password_dir, "createrole.pass");
    let createdb_pass = read_secret(&pointer.password_dir, "createdb.pass");
    let oponly_pass = read_secret(&pointer.password_dir, "oponly.pass");
    let castonly_pass = read_secret(&pointer.password_dir, "castonly.pass");

    let side = report_as(&reader_dial, &pointer.side_user, &side_pass, &selection).await;
    assert!(!side.registration_allowed);
    assert!(
        side.write
            .findings
            .iter()
            .any(|item| item.table == "side_write"
                && item.privilege == "INSERT"
                && item.path == "direct")
    );

    let hidden = report_as(
        &reader_dial,
        &pointer.hiddenfn_user,
        &hidden_pass,
        &selection,
    )
    .await;
    assert!(!hidden.registration_allowed);
    assert!(
        hidden
            .block_reasons
            .iter()
            .any(|item| item.code == "security_definer")
    );
    assert!(
        hidden
            .security_definers
            .iter()
            .any(|item| item.schema == "hidden" && item.name == "boom")
    );
    assert_ne!(hidden.write.state, "read_only");

    let schema_create = report_as(
        &reader_dial,
        &pointer.schemacreate_user,
        &schema_pass,
        &selection,
    )
    .await;
    assert!(!schema_create.registration_allowed);
    assert!(
        schema_create
            .write
            .findings
            .iter()
            .any(|item| item.schema == "side"
                && item.privilege == "CREATE"
                && item.path == "schema_create")
    );

    let db_create = report_as(&reader_dial, &pointer.dbcreate_user, &db_pass, &selection).await;
    assert!(!db_create.registration_allowed);
    assert!(
        db_create
            .write
            .findings
            .iter()
            .any(|item| item.privilege == "CREATE" && item.path == "database")
    );
    assert!(
        !db_create
            .block_reasons
            .iter()
            .any(|item| item.code == "privileged_role")
    );
    assert_eq!(
        db_create.role.value.as_ref().map(|item| item.create_db),
        Some(false)
    );

    let dual = report_as(&reader_dial, &pointer.dual_user, &dual_pass, &selection).await;
    assert!(!dual.registration_allowed);
    assert_eq!(dual.write.state, "write_capable");
    assert_eq!(
        dual.principal
            .value
            .as_ref()
            .map(|item| item.current_user.as_str()),
        Some(pointer.dual_user.as_str())
    );
    assert!(
        dual.write
            .findings
            .iter()
            .any(|item| item.path == "inherited" || item.path == "set_role")
    );

    let inherit_only = report_as(
        &reader_dial,
        &pointer.inherit_only_user,
        &inherit_only_pass,
        &selection,
    )
    .await;
    assert!(!inherit_only.registration_allowed);
    assert!(
        inherit_only
            .write
            .findings
            .iter()
            .any(|item| item.path == "inherited")
    );
    assert!(
        inherit_only
            .write
            .findings
            .iter()
            .all(|item| item.path != "set_role")
    );

    let transitive = report_as(
        &reader_dial,
        &pointer.transitive_user,
        &transitive_pass,
        &selection,
    )
    .await;
    assert!(!transitive.registration_allowed);
    assert!(
        transitive
            .write
            .findings
            .iter()
            .any(|item| item.path == "inherited")
    );

    let noset = report_as(&reader_dial, &pointer.noset_user, &noset_pass, &selection).await;
    assert!(noset.registration_allowed, "{:?}", noset.block_reasons);
    assert_eq!(noset.write.state, "read_only");
    let mut noset_empty = Dial {
        user: &pointer.noset_user,
        password: &noset_pass,
        source_id: "pg_source_noset_empty",
        secret: "secret://pg/source-lab/noset",
        ..reader_dial
    }
    .open(TableSelection::new(Vec::new()).unwrap())
    .await
    .unwrap();
    let noset_empty_report = noset_empty.inspect_access().await.unwrap();
    assert!(
        noset_empty_report.registration_allowed,
        "{:?}",
        noset_empty_report.block_reasons
    );
    assert_eq!(
        noset_empty.catalog().await.unwrap().schema_hash,
        EMPTY_SCHEMA_HASH
    );

    let column = report_as(&reader_dial, &pointer.column_user, &column_pass, &selection).await;
    assert!(!column.registration_allowed);
    assert!(
        column
            .write
            .findings
            .iter()
            .any(|item| item.privilege == "COLUMN_WRITE" && item.path == "column")
    );

    for (user, password) in [
        (pointer.super_user.as_str(), &super_pass),
        (pointer.bypass_user.as_str(), &bypass_pass),
        (pointer.createrole_user.as_str(), &createrole_pass),
        (pointer.createdb_user.as_str(), &createdb_pass),
    ] {
        let report = report_as(&reader_dial, user, password, &selection).await;
        assert!(!report.registration_allowed, "{user}");
        assert!(
            report
                .block_reasons
                .iter()
                .any(|item| item.code == "privileged_role"),
            "{user}"
        );
        assert_ne!(report.write.state, "read_only");
    }

    let oponly = report_as(&reader_dial, &pointer.oponly_user, &oponly_pass, &selection).await;
    assert!(!oponly.registration_allowed);
    assert!(
        oponly
            .security_definers
            .iter()
            .any(|item| item.name == "boom_op")
    );

    let castonly = report_as(
        &reader_dial,
        &pointer.castonly_user,
        &castonly_pass,
        &selection,
    )
    .await;
    assert!(!castonly.registration_allowed);
    assert!(
        castonly
            .security_definers
            .iter()
            .any(|item| item.name == "boom_cast")
    );

    let guc_pass = read_secret(&pointer.password_dir, "guc.pass");
    let mut guc = Dial {
        source_id: "pg_source_guc",
        secret: "secret://pg/source-lab/guc",
        user: &pointer.guc_user,
        password: &guc_pass,
        ..reader_dial
    }
    .open(selection.clone())
    .await
    .expect("guc connect");
    let guc_report = guc.inspect_access().await.expect("guc inspect");
    let guc_principal = guc_report.principal.value.expect("guc principal");
    assert!(
        !guc_report.registration_allowed,
        "{:?}",
        guc_report.block_reasons
    );
    assert_eq!(guc_principal.session_user, pointer.guc_user);
    assert_eq!(guc_principal.current_user, pointer.side_user);
    assert!(
        guc_report
            .block_reasons
            .iter()
            .any(|item| item.code == "role_mismatch" && item.fact == "session_user"),
        "{:?}",
        guc_report.block_reasons
    );
    assert!(
        guc_report.block_reasons.iter().any(|item| {
            item.code == "startup_option" && item.fact == "startup_options"
        }),
        "{:?}",
        guc_report.block_reasons
    );
    assert_eq!(guc_report.tls.mode, "verify-full");
    assert_eq!(guc_report.tls.trust, "caller-ca-only");
    assert!(guc_report.tls.hostname_verification);
    assert_eq!(guc_report.tls.negotiated_version, "unknown");
    assert_eq!(guc_report.tls.server_observation, "unknown");
    assert_eq!(guc_report.write.state, "unknown");
    assert!(guc_report.security_definers.is_empty());
    assert!(guc_report.memberships.is_empty());
    match guc.catalog().await {
        Ok(_) => panic!("guc catalog was admitted"),
        Err(err) => {
            assert_eq!(err.kind(), SourceErrorKind::Config);
            assert_eq!(err.fact(), Some("session_user_mismatch"));
        }
    }
    drop(guc);

    let parent_options = std::env::var_os("PGOPTIONS");
    spawn_env_probe("options", "-c role=oo_src_reader");
    spawn_env_probe("version", "-c server_version=13.14");
    assert_eq!(std::env::var_os("PGOPTIONS"), parent_options);

    drop(reader);
    drop(inherit);
    drop(setonly);
    drop(definer);
    drop(missing);
    drop(reconnected);
    drop(noset_empty);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let secrets = [
        reader_pass.as_str(),
        inherit_pass.as_str(),
        setonly_pass.as_str(),
        definer_pass.as_str(),
        side_pass.as_str(),
        hidden_pass.as_str(),
        schema_pass.as_str(),
        db_pass.as_str(),
        dual_pass.as_str(),
        inherit_only_pass.as_str(),
        transitive_pass.as_str(),
        noset_pass.as_str(),
        column_pass.as_str(),
        super_pass.as_str(),
        bypass_pass.as_str(),
        createrole_pass.as_str(),
        createdb_pass.as_str(),
        oponly_pass.as_str(),
        castonly_pass.as_str(),
        guc_pass.as_str(),
        "forged-secret",
    ];
    audit_log(&pointer.log_file, &secrets);
}

fn assert_catalog_truth(catalog: &SourceCatalog) {
    let customer = find(catalog, "app", "customer");
    assert_eq!(
        customer
            .primary_key
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["id"]
    );
    assert!(customer.rls_enabled);
    assert!(
        customer
            .comment
            .as_ref()
            .is_some_and(|item| item.sha256 == CANARY_SHA && item.byte_len == CANARY.len())
    );
    let hidden = customer
        .columns
        .iter()
        .find(|c| c.name == "hidden_pii")
        .unwrap();
    assert_eq!(
        hidden.comment.as_ref().map(|item| item.sha256.as_str()),
        Some(CANARY_SHA)
    );
    assert_eq!(hidden.current_privileges.select, Some(false));
    let label = customer.columns.iter().find(|c| c.name == "label").unwrap();
    assert_eq!(label.generated, "s");
    assert_eq!(label.current_privileges.select, Some(true));
    assert!(!customer.type_support_converted());

    let line = find(catalog, "app", "order_line");
    assert_eq!(
        line.primary_key
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["order_id", "line_no"]
    );
    assert_eq!(line.foreign_keys.len(), 1);
    assert_eq!(line.foreign_keys[0].columns[0].ref_table, "customer");
    assert!(
        line.columns
            .iter()
            .any(|c| c.name == "sku" && c.default_expr.is_some())
    );

    let reading = find(catalog, "app", "reading");
    assert!(reading.primary_key.is_empty());
    assert!(reading.primary_key_nulls_not_distinct.is_none());
    let id = reading.columns.iter().find(|c| c.name == "id").unwrap();
    assert_eq!(id.identity, "d");
    let amount = reading.columns.iter().find(|c| c.name == "amount").unwrap();
    assert_eq!(amount.type_name, "numeric");
    assert_eq!(amount.typmod, -1);
    assert_eq!(
        format!("{:?}", amount.type_support.reason),
        "UnboundedOrWideNumeric"
    );
    let precise = reading
        .columns
        .iter()
        .find(|c| c.name == "precise")
        .unwrap();
    assert_eq!(precise.typmod, 5242894);
    assert_eq!(precise.numeric_precision, Some(80));
    assert_eq!(precise.numeric_scale, Some(10));
    assert_eq!(
        format!("{:?}", precise.type_support.reason),
        "UnboundedOrWideNumeric"
    );
    let status = reading.columns.iter().find(|c| c.name == "status").unwrap();
    assert_eq!(status.typtype, "e");
    assert_eq!(
        status.enum_labels,
        vec!["open".to_string(), "closed".to_string()]
    );
    assert_eq!(
        format!("{:?}", status.type_support.reason),
        "EnumNotConverted"
    );
    let code = reading.columns.iter().find(|c| c.name == "code").unwrap();
    assert_eq!(code.typtype, "d");
    assert_eq!(code.base_type_oid, Some(25));
    assert_eq!(
        format!("{:?}", code.type_support.reason),
        "DomainNotConverted"
    );
    assert!(!code.domain_checks.is_empty());
    let payload = reading
        .columns
        .iter()
        .find(|c| c.name == "payload")
        .unwrap();
    assert_eq!(payload.type_oid, 3802);
    let blob = reading.columns.iter().find(|c| c.name == "blob").unwrap();
    assert_eq!(blob.type_oid, 17);
    assert!(reading.columns.iter().all(|c| !c.type_support.converted));

    let weird = find(catalog, "Weird Name", "T-1");
    assert!(weird.schema_ident.starts_with("q_"));
    assert!(weird.columns.iter().any(|c| c.name == "Col A"));
    assert!(weird.columns.iter().any(|c| c.name == "wei\"rd"));

    let orders = find(catalog, "客户", "订单");
    assert!(orders.name_ident.starts_with("q_"));
    let money = orders.columns.iter().find(|c| c.name == "金额").unwrap();
    assert!(money.name_ident.starts_with("q_"));
    assert_eq!(money.numeric_precision, Some(10));
    assert_eq!(
        format!("{:?}", money.type_support.reason),
        "KnownBuiltinNotConverted"
    );

    let base = find(catalog, "app", "item_base");
    let next = find(catalog, "app", "item_next");
    assert_eq!(base.columns.len() + 1, next.columns.len());
    assert_eq!(catalog.type_conversion, "unsupported");
}

trait ConvertedProbe {
    fn type_support_converted(&self) -> bool;
}

impl ConvertedProbe for SourceTable {
    fn type_support_converted(&self) -> bool {
        self.columns
            .iter()
            .any(|column| column.type_support.converted)
    }
}

fn find<'a>(catalog: &'a SourceCatalog, schema: &str, name: &str) -> &'a SourceTable {
    catalog
        .tables
        .iter()
        .find(|table| table.schema == schema && table.name == name)
        .unwrap_or_else(|| panic!("missing table {schema}.{name}"))
}

#[derive(Clone, Copy)]
struct Dial<'a> {
    source_id: &'a str,
    secret: &'a str,
    user: &'a str,
    password: &'a Zeroizing<String>,
    host: &'a str,
    port: u16,
    database: &'a str,
    ca: &'a [u8],
}

impl Dial<'_> {
    async fn open(
        &self,
        selection: TableSelection,
    ) -> Result<PgSession, oo_connectors::SourceError> {
        let config = PgSourceConfig::try_new(
            SourceId::new(self.source_id).unwrap(),
            PgEndpoint::new(self.host, self.port, self.database).unwrap(),
            PgCredentials::new(self.user, self.password.as_str()).unwrap(),
            SecretRef::new(self.secret).unwrap(),
            PgTlsConfig::from_ca_pem(self.ca).unwrap(),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(5),
                1_000,
                1024 * 1024,
            )
            .unwrap(),
            selection,
        )
        .unwrap();
        PgConnector::new(config).connect().await
    }

    async fn catalog_one(&self, schema: &str, name: &str) -> SourceCatalog {
        let selection =
            TableSelection::new(vec![SelectedTable::new(schema, name).unwrap()]).unwrap();
        let mut session = self.open(selection).await.expect("one-table connect");
        session.catalog().await.expect("one-table catalog")
    }
}

fn full_selection() -> TableSelection {
    TableSelection::new(vec![
        SelectedTable::new("app", "customer").unwrap(),
        SelectedTable::new("app", "order_line").unwrap(),
        SelectedTable::new("app", "reading").unwrap(),
        SelectedTable::new("Weird Name", "T-1").unwrap(),
        SelectedTable::new("客户", "订单").unwrap(),
        SelectedTable::new("app", "item_base").unwrap(),
        SelectedTable::new("app", "item_next").unwrap(),
    ])
    .unwrap()
}

fn read_secret(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("secret file");
    Zeroizing::new(text.trim().to_string())
}

fn assert_closed(label: &str, haystack: &str, password: &str, host: &str) {
    for needle in [
        password,
        host,
        CANARY,
        "BEGIN CERTIFICATE",
        "PRIVATE KEY",
        "wrong-password-value",
    ] {
        if !needle.is_empty() && haystack.contains(needle) {
            panic!("{label} leaked a closed string");
        }
    }
}

async fn report_as(
    base: &Dial<'_>,
    user: &str,
    password: &Zeroizing<String>,
    selection: &TableSelection,
) -> oo_connectors::AccessReport {
    let dial = Dial {
        user,
        password,
        source_id: "pg_source_repro",
        secret: "secret://pg/source-lab/repro",
        ..*base
    };
    let mut session = dial
        .open(selection.clone())
        .await
        .unwrap_or_else(|err| panic!("{user} connect {err:?}"));
    session
        .inspect_access()
        .await
        .unwrap_or_else(|err| panic!("{user} inspect {err:?}"))
}

fn spawn_env_probe(kind: &str, pgoptions: &str) {
    let exe = std::env::current_exe().expect("test exe");
    let output = std::process::Command::new(exe)
        .args(["--exact", "live_source_catalog"])
        .env("OO_A066_PROBE", kind)
        .env("PGOPTIONS", pgoptions)
        .env("PGSSLMODE", "disable")
        .env("PGSSLCERT", "/tmp/oo-a066-not-a-cert")
        .env("PGSSLKEY", "/tmp/oo-a066-not-a-key")
        .env("PGUSER", "forged-user")
        .env("PGPASSWORD", "forged-secret")
        .env("PGDATABASE", "forged-db")
        .env("PGAPPNAME", "forged-app")
        .env("PGSSLROOTCERT", "/tmp/oo-a066-not-a-ca")
        .env("PGHOST", "127.0.0.1")
        .env("PGPORT", "1")
        .output()
        .expect("probe process");
    assert!(
        output.status.success(),
        "probe {kind} failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn run_env_probe(kind: &str) {
    match kind {
        "options" => {
            assert_eq!(
                std::env::var("PGOPTIONS").as_deref(),
                Ok("-c role=oo_src_reader")
            );
            assert_eq!(std::env::var("PGSSLMODE").as_deref(), Ok("disable"));
            assert_eq!(
                std::env::var("PGSSLCERT").as_deref(),
                Ok("/tmp/oo-a066-not-a-cert")
            );
            assert_eq!(
                std::env::var("PGSSLKEY").as_deref(),
                Ok("/tmp/oo-a066-not-a-key")
            );
            assert_eq!(std::env::var("PGUSER").as_deref(), Ok("forged-user"));
            assert_eq!(std::env::var("PGDATABASE").as_deref(), Ok("forged-db"));
            assert_eq!(std::env::var("PGAPPNAME").as_deref(), Ok("forged-app"));
        }
        "version" => {
            assert_eq!(
                std::env::var("PGOPTIONS").as_deref(),
                Ok("-c server_version=13.14")
            );
        }
        other => panic!("unknown probe {other}"),
    }
    let path = std::env::var("OO_PG_FIXTURE").expect("OO_PG_FIXTURE");
    let pointer: Pointer =
        serde_json::from_str(&fs::read_to_string(path).expect("pointer")).expect("pointer json");
    let dual_pass = read_secret(&pointer.password_dir, "dual.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let options = sqlx::postgres::PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(&pointer.dual_user)
        .password(dual_pass.as_str())
        .database(&pointer.database)
        .application_name("oo-pg-source")
        .extra_float_digits(2i8)
        .ssl_mode(sqlx::postgres::PgSslMode::VerifyFull);
    assert!(options.get_options().is_none());
    assert_eq!(options.get_username(), pointer.dual_user);
    assert_eq!(options.get_database(), Some(pointer.database.as_str()));
    assert_eq!(options.get_host(), pointer.host.as_str());
    assert_eq!(options.get_port(), pointer.port);
    assert_eq!(options.get_application_name(), Some("oo-pg-source"));
    assert!(matches!(
        options.get_ssl_mode(),
        sqlx::postgres::PgSslMode::VerifyFull
    ));
    let dial = Dial {
        source_id: "pg_source_probe",
        secret: "secret://pg/source-lab/probe",
        user: &pointer.dual_user,
        password: &dual_pass,
        host: &pointer.host,
        port: pointer.port,
        database: &pointer.database,
        ca: &ca,
    };
    let mut session = dial.open(full_selection()).await.expect("probe connect");
    let report = session.inspect_access().await.expect("probe inspect");
    let principal = report.principal.value.expect("principal");
    assert_eq!(principal.current_user, pointer.dual_user);
    assert_eq!(principal.session_user, pointer.dual_user);
    assert!(!report.registration_allowed);
    assert_eq!(report.write.state, "write_capable");
    assert_eq!(report.version.server_version_num, 170010);
    assert!(report.version.server_version.starts_with("17.10"));
    assert_eq!(report.version.admission, "tested");
    assert_eq!(report.tls.trust, "caller-ca-only");
    assert!(report.tls.negotiated_version.starts_with("TLSv1."));
    assert_eq!(report.tls.server_observation, "observed");
    session.catalog().await.expect("probe catalog");
}

fn audit_log(path: &str, secrets: &[&str]) {
    let raw = fs::read_to_string(path).expect("postgres log");
    for secret in secrets.iter().copied().chain([CANARY]) {
        if !secret.is_empty() && raw.contains(secret) {
            panic!("postgres log contains a closed string");
        }
    }
    assert!(
        !raw.contains("app=forged-app"),
        "application_name came from the environment"
    );
    assert!(
        !raw.contains("user=forged-user"),
        "user came from the environment"
    );
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen = 0_usize;
    for line in raw.lines() {
        if !line.contains("app=oo-pg-source") {
            continue;
        }
        let Some(sql) = statement_sql(line) else {
            continue;
        };
        let verb = sql
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if verb.is_empty() {
            continue;
        }
        seen += 1;
        *counts.entry(verb.clone()).or_default() += 1;
        let allowed = matches!(
            verb.as_str(),
            "BEGIN"
                | "START"
                | "SET"
                | "SHOW"
                | "SELECT"
                | "WITH"
                | "COMMIT"
                | "ROLLBACK"
                | "PREPARE"
                | "DEALLOCATE"
                | "EXECUTE"
        );
        assert!(allowed, "disallowed verb {verb}");
        if let Some(word) = write_word(&strip_sql_literals(sql)) {
            panic!("connector statement contains {word}");
        }
    }
    assert!(seen > 0, "connector statements were not logged");
    assert!(counts.get("SELECT").copied().unwrap_or(0) > 0);
    assert!(
        counts.get("ROLLBACK").copied().unwrap_or(0) > 0
            || counts.get("COMMIT").copied().unwrap_or(0) > 0
    );
    let summary = counts
        .iter()
        .map(|(verb, count)| format!("{verb}={count}"))
        .collect::<Vec<_>>()
        .join(",");
    eprintln!("sql_audit {summary}");
}

fn strip_sql_literals(sql: &str) -> String {
    let mut out = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            while let Some(inner) = chars.next() {
                if inner == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        continue;
                    }
                    break;
                }
            }
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

fn write_word(sql: &str) -> Option<&'static str> {
    const WORDS: &[&str] = &[
        "INSERT",
        "UPDATE",
        "DELETE",
        "TRUNCATE",
        "MERGE",
        "COPY",
        "CREATE",
        "ALTER",
        "DROP",
        "GRANT",
        "REVOKE",
        "COMMENT",
        "VACUUM",
        "CLUSTER",
        "REINDEX",
        "CALL",
        "DO",
        "NEXTVAL",
        "LO_IMPORT",
        "LO_EXPORT",
        "LO_CREATE",
        "DBLINK",
        "PG_WRITE_FILE",
        "PG_READ_FILE",
        "PG_READ_BINARY_FILE",
    ];
    for token in sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        let upper = token.to_ascii_uppercase();
        if let Some(word) = WORDS.iter().copied().find(|word| *word == upper) {
            return Some(word);
        }
    }
    None
}

fn statement_sql(line: &str) -> Option<&str> {
    if let Some(rest) = line.split_once("statement:") {
        return Some(rest.1.trim());
    }
    if let Some(rest) = line.split_once("execute ") {
        return rest.1.split_once(':').map(|(_, sql)| sql.trim());
    }
    None
}
