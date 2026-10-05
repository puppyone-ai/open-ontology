//! One role, two mapped tables, one column-ACL transaction.
//!
//! Runs only when `OO_A218_LIVE=1`. The fixture applies frozen target scripts
//! `0000`, `0001`, `0003`, `0004`, and `0005` itself. It does not link
//! `oo-pipeline` and does not apply `0002`. Passwords stay in files.
//! `query-pg` does not install grants. The installer does not `ANALYZE`.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use std::collections::BTreeSet;

use oo_gateway::policy::{
    compile_column_acl_set, install_column_acl_set, COLUMN_ACL_SET_MAX_COLUMNS,
};
use oo_gateway::{
    compile_column_acl, connect_require_tls, install_column_acl, ClassStatus, ColumnAclPolicy,
    ColumnSpec, InstallEffect, LogicalTable, PiiClass, PolicyError, RowFilter, TlsEndpoint,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const HOST: &str = "oo-a218-pg";
const DATABASE: &str = "oo_a218";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a218_login";
const OTHER_LOGIN: &str = "oo_a218_other";
const OTHER_PASS: &str = "b218b218b218b218";
const SECRET_DIR: &str = "/run/oo-a218/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const LIVE_DIR: &str = "/evidence/live";
const OWNER: &str = "a2180000-0000-4000-8000-000000000218";
const SHARD: &str = "shard_a218";
const SOURCE: &str = "trade";
const CONFIG: &str = "a218a218a218a218a218a218a218a218a218a218a218a218a218a218a218a218";
const OTHER_DIGEST: &str = "f218f218f218f218f218f218f218f218f218f218f218f218f218f218f218f218";
const TYPE_DIGEST: &str = "b218b218b218b218b218b218b218b218b218b218b218b218b218b218b218b218";
const INPUT_DIGEST: &str = "c218c218c218c218c218c218c218c218c218c218c218c218c218c218c218c218";
const SCHEMA_DIGEST: &str = "d218d218d218d218d218d218d218d218d218d218d218d218d218d218d218d218";
const NAMESPACE: &str = "trade";
const PARTY: &str = "party";
const LEDGER: &str = "ledger";
const STALE_GENERATION: i64 = 3;
const CURRENT_GENERATION: i64 = 7;
const LATER_GENERATION: i64 = 9;

#[tokio::test]
async fn two_tables_one_role_column_acl_set() {
    if env::var("OO_A218_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    assert_eq!(COLUMN_ACL_SET_MAX_COLUMNS, 512);

    let admin_pass = read_hex("admin.pass");
    let login_pass = read_hex("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let mut oracle = Oracle::new(&[&admin_pass, &login_pass, OTHER_PASS]);

    reset_database(&admin_pass, &ca).await;
    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, LOGIN, &login_pass).await;
    create_login(&admin, OTHER_LOGIN, OTHER_PASS).await;
    let shadow = prepare_shadow(&mut admin).await;
    oracle.fact("physical_party", &shadow.party.table);
    oracle.fact("physical_ledger", &shadow.ledger.table);
    register_other_digest(&admin).await;

    let digest_a = max_generation(&admin, CONFIG).await;
    let digest_b = max_generation(&admin, OTHER_DIGEST).await;
    assert_eq!(digest_a, Some(CURRENT_GENERATION));
    assert_eq!(digest_b, Some(LATER_GENERATION));
    oracle.fact("digest_a_max", "7");
    oracle.fact("digest_b_max", "9");

    let before = role_count(&admin, "oo_r_").await;
    refuse_before_sql(&mut admin, &mut oracle).await;
    assert_eq!(role_count(&admin, "oo_r_").await, before);
    oracle.fact("refusal_roles", "unchanged");

    assert_eq!(
        install_column_acl_set(&mut admin, &orphan_set()).await,
        Err(PolicyError::LoginAbsent)
    );
    assert!(!role_present(&admin, "oo_r_orphan").await);

    assert_eq!(
        install_column_acl_set(&mut admin, &stale_set(STALE_GENERATION, "stale_gen")).await,
        Err(PolicyError::StaleGeneration {
            policy: STALE_GENERATION,
            current: Some(CURRENT_GENERATION),
        })
    );
    assert_eq!(
        install_column_acl_set(&mut admin, &stale_set(LATER_GENERATION, "later_gen")).await,
        Err(PolicyError::StaleGeneration {
            policy: LATER_GENERATION,
            current: Some(CURRENT_GENERATION),
        })
    );
    assert!(!role_present(&admin, "oo_r_stale_gen").await);
    assert!(!role_present(&admin, "oo_r_later_gen").await);
    oracle.fact("stale_cross_digest", "StaleGeneration");

    prove_legacy_split(&mut admin, &shadow, &mut oracle).await;
    prove_bad_second_table(&mut admin, &mut oracle).await;

    assert!(reltuples_unknown(&admin, &shadow.party.table).await);
    assert!(reltuples_unknown(&admin, &shadow.ledger.table).await);
    oracle.fact("reltuples_before", "unknown");
    let publications = publication_count(&admin).await;
    assert_eq!(publications, 0);
    oracle.fact("publication", "0");

    let clerk = clerk_set();
    let installed = install_column_acl_set(&mut admin, &clerk)
        .await
        .expect("install clerk");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, "oo_r_clerk");
    assert_eq!(installed.grantor, ADMIN);
    assert_eq!(installed.tables.len(), 2);
    assert_eq!(installed.tables[0].physical_table, shadow.party.table);
    assert_eq!(installed.tables[1].physical_table, shadow.ledger.table);
    assert_eq!(
        installed.tables[0].physical_columns,
        vec![
            shadow.party.party_id.clone(),
            shadow.party.display_name.clone(),
            shadow.party.note.clone(),
        ]
    );
    assert_eq!(
        installed.tables[1].physical_columns,
        vec![
            shadow.ledger.entry_id.clone(),
            shadow.ledger.party_id.clone(),
            shadow.ledger.memo.clone(),
        ]
    );
    assert!(!installed.tables[0]
        .physical_columns
        .contains(&shadow.party.ratio));
    oracle.fact("clerk_effect", "Installed");
    oracle.fact("grantor", &installed.grantor);

    let peek = install_column_acl_set(&mut admin, &peek_set())
        .await
        .expect("install peek");
    assert_eq!(peek.effect, InstallEffect::Installed);
    assert_eq!(peek.role, "oo_r_peek");
    oracle.fact("peek_effect", "Installed");

    let replay = install_column_acl_set(&mut admin, &clerk)
        .await
        .expect("replay clerk");
    assert_eq!(replay.effect, InstallEffect::Replay);
    assert_eq!(replay.tables, installed.tables);
    oracle.fact("clerk_replay", "Replay");

    let party_only = install_column_acl(&mut admin, &clerk[0])
        .await
        .expect("old api replays one table of the set");
    assert_eq!(party_only.effect, InstallEffect::Replay);
    assert_eq!(party_only.physical_columns, installed.tables[0].physical_columns);
    assert_eq!(
        select_columns(&admin, "oo_r_clerk", &shadow.ledger.table).await,
        BTreeSet::from([
            shadow.ledger.entry_id.clone(),
            shadow.ledger.party_id.clone(),
            shadow.ledger.memo.clone(),
        ])
    );
    oracle.fact("old_api_one_table_replay", "Replay");
    oracle.fact("ledger_grants_after_old_replay", "unchanged");

    let clerk_grants = grant_text(&admin, "oo_r_clerk").await;
    let peek_grants = grant_text(&admin, "oo_r_peek").await;
    refuse_mutations(&mut admin, &clerk_grants, &peek_grants, &mut oracle).await;
    assert_catalog(&admin, &shadow, &mut oracle).await;

    assert!(reltuples_unknown(&admin, &shadow.party.table).await);
    assert!(reltuples_unknown(&admin, &shadow.ledger.table).await);
    analyze(&admin, &shadow.party.table).await;
    analyze(&admin, &shadow.ledger.table).await;
    assert!(!reltuples_unknown(&admin, &shadow.party.table).await);
    assert!(!reltuples_unknown(&admin, &shadow.ledger.table).await);
    oracle.fact("reltuples_after", "known");
    assert_eq!(publication_count(&admin).await, 0);
    // query-pg loads mappings as the login, before SET ROLE. The column role
    // does not grant that catalog read. This is the same fixture grant the
    // reviewed query-pg live test uses.
    grant_mapping_read(&admin).await;
    oracle.fact("loader_mapping_grant", "login_usage_select_execute");

    let bans = oracle.bans.clone();
    query_public_api(&bans, &mut oracle);

    let mut reader = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    let pid = backend_pid(&reader).await;
    direct_denials(&mut reader, &shadow, pid, &mut oracle).await;
    assert_eq!(publication_count(&admin).await, 0);
    let rls: bool = admin
        .query_one(
            "SELECT COALESCE(bool_or(relation.relrowsecurity), false) \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname IN ($1, $2)",
            &[&shadow.party.table, &shadow.ledger.table],
        )
        .await
        .expect("rls")
        .get(0);
    assert!(!rls);
    oracle.fact("authentication", "not_claimed");
    oracle.fact("rls", "open");
    oracle.fact("same_backend_after_rollback", "true");
    oracle.flush();
}

async fn refuse_before_sql(admin: &mut Client, oracle: &mut Oracle) {
    assert_eq!(
        install_column_acl_set(admin, &[]).await,
        Err(PolicyError::EmptyPolicySet)
    );
    let party = acl("clerk", LOGIN, CONFIG, CURRENT_GENERATION, PARTY, &["party_id"], false);
    assert_eq!(
        install_column_acl_set(admin, &[party.clone(), party.clone()]).await,
        Err(PolicyError::InvalidIdentifier { field: "table" })
    );
    let mut other_digest = acl(
        "clerk",
        LOGIN,
        OTHER_DIGEST,
        LATER_GENERATION,
        LEDGER,
        &["entry_id"],
        false,
    );
    other_digest.generation = LATER_GENERATION;
    assert_eq!(
        install_column_acl_set(admin, &[party.clone(), other_digest]).await,
        Err(PolicyError::MixedIdentity)
    );
    let mut other_generation = acl(
        "clerk",
        LOGIN,
        CONFIG,
        STALE_GENERATION,
        LEDGER,
        &["entry_id"],
        false,
    );
    other_generation.generation = STALE_GENERATION;
    assert_eq!(
        install_column_acl_set(admin, &[party.clone(), other_generation]).await,
        Err(PolicyError::MixedIdentity)
    );
    let filtered = acl(
        "filtered",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        LEDGER,
        &["entry_id"],
        true,
    );
    assert_eq!(
        install_column_acl_set(admin, &[party.clone(), filtered]).await,
        Err(PolicyError::RowFilterRefused)
    );
    let mut hidden = party.clone();
    hidden.table.columns = vec![column("_oo_pos")];
    assert!(matches!(
        install_column_acl_set(admin, &[hidden.clone()]).await,
        Err(PolicyError::MetadataColumn { .. })
    ));
    assert!(compile_column_acl(&hidden).is_ok());
    assert!(compile_column_acl_set(&[party]).is_ok());
    oracle.fact("pure_refusals", "empty_duplicate_mixed_filter_metadata");
}

async fn prove_legacy_split(admin: &mut Client, shadow: &Shadow, oracle: &mut Oracle) {
    let legacy_party = acl(
        "legacy",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        PARTY,
        &["party_id"],
        false,
    );
    let legacy_ledger = acl(
        "legacy",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        LEDGER,
        &["entry_id"],
        false,
    );
    let first = install_column_acl(admin, &legacy_party)
        .await
        .expect("legacy party");
    assert_eq!(first.effect, InstallEffect::Installed);
    assert_eq!(first.physical_columns, vec![shadow.party.party_id.clone()]);
    let again = install_column_acl(admin, &legacy_party)
        .await
        .expect("legacy replay");
    assert_eq!(again.effect, InstallEffect::Replay);
    assert_eq!(
        install_column_acl(admin, &legacy_ledger).await,
        Err(PolicyError::GrantMismatch)
    );
    assert!(select_columns(admin, "oo_r_legacy", &shadow.ledger.table)
        .await
        .is_empty());
    let before = grant_text(admin, "oo_r_legacy").await;
    assert_eq!(
        install_column_acl_set(admin, &[legacy_party, legacy_ledger]).await,
        Err(PolicyError::GrantMismatch)
    );
    assert_eq!(grant_text(admin, "oo_r_legacy").await, before);
    assert_eq!(
        select_columns(admin, "oo_r_legacy", &shadow.party.table).await,
        BTreeSet::from([shadow.party.party_id.clone()])
    );
    oracle.fact("legacy_second_table", "GrantMismatch");
    oracle.fact("legacy_set_widen", "GrantMismatch");
    oracle.fact("legacy_party_columns", "1");
}

async fn prove_bad_second_table(admin: &mut Client, oracle: &mut Oracle) {
    let party = acl(
        "bad_map",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        PARTY,
        &["party_id", "display_name", "note"],
        false,
    );
    let missing = acl(
        "bad_map",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        "missing_table",
        &["entry_id"],
        false,
    );
    assert_eq!(
        install_column_acl_set(admin, &[party, missing]).await,
        Err(PolicyError::MappingAbsent)
    );
    assert!(!role_present(admin, "oo_r_bad_map").await);
    assert_eq!(privilege_count(admin, "oo_r_bad_map").await, 0);
    oracle.fact("bad_second_mapping", "MappingAbsent");
    oracle.fact("bad_second_role", "absent");

    let typed = acl(
        "bad_type",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        PARTY,
        &["party_id", "ratio"],
        false,
    );
    let ledger = acl(
        "bad_type",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        LEDGER,
        &["entry_id"],
        false,
    );
    assert_eq!(
        install_column_acl_set(admin, &[typed, ledger]).await,
        Err(PolicyError::TypeRefused {
            column: "ratio".to_string(),
        })
    );
    assert!(!role_present(admin, "oo_r_bad_type").await);
    assert_eq!(privilege_count(admin, "oo_r_bad_type").await, 0);
    oracle.fact("bad_type", "TypeRefused");
    oracle.fact("bad_type_role", "absent");
}

async fn refuse_mutations(
    admin: &mut Client,
    clerk_grants: &str,
    peek_grants: &str,
    oracle: &mut Oracle,
) {
    let partial = vec![acl(
        "clerk",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        PARTY,
        &["party_id", "display_name", "note"],
        false,
    )];
    assert_eq!(
        install_column_acl_set(admin, &partial).await,
        Err(PolicyError::GrantMismatch)
    );
    oracle.fact("partial_refuse", "GrantMismatch");

    let mut changed = clerk_set();
    changed[0].table.columns.pop();
    assert_eq!(
        install_column_acl_set(admin, &changed).await,
        Err(PolicyError::GrantMismatch)
    );
    oracle.fact("changed_refuse", "GrantMismatch");

    let mut extra = clerk_set();
    extra.push(acl(
        "clerk",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        "missing_table",
        &["entry_id"],
        false,
    ));
    assert_eq!(
        install_column_acl_set(admin, &extra).await,
        Err(PolicyError::MappingAbsent)
    );

    let mut typed = clerk_set();
    typed[0]
        .table
        .columns
        .push(column("ratio"));
    assert_eq!(
        install_column_acl_set(admin, &typed).await,
        Err(PolicyError::TypeRefused {
            column: "ratio".to_string(),
        })
    );

    let mut other_login = clerk_set();
    for policy in &mut other_login {
        policy.login = OTHER_LOGIN.to_string();
    }
    assert_eq!(
        install_column_acl_set(admin, &other_login).await,
        Err(PolicyError::MembershipMismatch)
    );
    oracle.fact("membership_refuse", "MembershipMismatch");

    assert_eq!(grant_text(admin, "oo_r_clerk").await, clerk_grants);
    assert_eq!(grant_text(admin, "oo_r_peek").await, peek_grants);
    let members: i64 = admin
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             WHERE granted.rolname = 'oo_r_clerk'",
            &[],
        )
        .await
        .expect("member count")
        .get(0);
    assert_eq!(members, 1);
    oracle.fact("grants_after_refuse", "unchanged");
}

async fn assert_catalog(admin: &Client, shadow: &Shadow, oracle: &mut Oracle) {
    for role in ["oo_r_clerk", "oo_r_peek"] {
        let row = admin
            .query_one(
                "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, \
                        rolbypassrls, rolreplication \
                 FROM pg_catalog.pg_roles WHERE rolname = $1",
                &[&role],
            )
            .await
            .expect("role flags");
        let flags = [
            row.get::<_, bool>(0),
            row.get::<_, bool>(1),
            row.get::<_, bool>(2),
            row.get::<_, bool>(3),
            row.get::<_, bool>(4),
            row.get::<_, bool>(5),
            row.get::<_, bool>(6),
        ];
        assert_eq!(flags, [false; 7], "{role}");
    }
    let member = admin
        .query_one(
            "SELECT member_role.rolname, inherit_option, set_option, admin_option, \
                    grantor_role.rolname \
             FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
             JOIN pg_catalog.pg_roles AS grantor_role ON grantor_role.oid = membership.grantor \
             WHERE granted.rolname = 'oo_r_clerk'",
            &[],
        )
        .await
        .expect("membership");
    let member_name: String = member.get(0);
    let inherit: bool = member.get(1);
    let set_option: bool = member.get(2);
    let admin_option: bool = member.get(3);
    let grantor: String = member.get(4);
    assert_eq!(member_name, LOGIN);
    assert!(!inherit);
    assert!(set_option);
    assert!(!admin_option);
    assert_eq!(grantor, ADMIN);
    oracle.fact("membership", "inherit=false set=true admin=false");
    oracle.fact("membership_grantor", &grantor);
    oracle.fact("role_shape", "nologin");

    assert_eq!(
        select_columns(admin, "oo_r_clerk", &shadow.party.table).await,
        BTreeSet::from([
            shadow.party.party_id.clone(),
            shadow.party.display_name.clone(),
            shadow.party.note.clone(),
        ])
    );
    assert_eq!(
        select_columns(admin, "oo_r_peek", &shadow.party.table).await,
        BTreeSet::from([
            shadow.party.party_id.clone(),
            shadow.party.display_name.clone(),
        ])
    );
    assert_eq!(
        select_columns(admin, "oo_r_peek", &shadow.ledger.table).await,
        BTreeSet::from([
            shadow.ledger.entry_id.clone(),
            shadow.ledger.party_id.clone(),
        ])
    );
    for role in ["oo_r_clerk", "oo_r_peek", "oo_r_legacy", LOGIN] {
        assert_eq!(table_privilege_count(admin, role).await, 0, "{role}");
        assert_eq!(non_select_count(admin, role).await, 0, "{role}");
    }
    oracle.fact("table_privileges", "0");
    for table in [&shadow.party.table, &shadow.ledger.table] {
        assert_eq!(public_column_privileges(admin, table).await, 0, "{table}");
        let hidden: i64 = admin
            .query_one(
                "SELECT count(*)::int8 FROM information_schema.column_privileges \
                 WHERE table_schema = '_oo' AND table_name = $1 \
                   AND column_name LIKE '\\_oo%' ESCAPE '\\' \
                   AND grantee IN ('oo_r_clerk', 'oo_r_peek', 'oo_r_legacy', \
                                   'oo_a218_login', 'oo_a218_other', 'PUBLIC')",
                &[&table.as_str()],
            )
            .await
            .expect("metadata grants")
            .get(0);
        assert_eq!(hidden, 0, "{table}");
    }
    oracle.fact("public_column_privileges", "0");
    let pos_allowed: Option<bool> = admin
        .query_one(
            "SELECT pg_catalog.has_column_privilege( \
                ($1::text)::name, relation.oid, '_oo_pos'::text, 'SELECT') \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $2",
            &[&"oo_r_clerk", &shadow.party.table],
        )
        .await
        .expect("pos privilege")
        .get(0);
    assert_ne!(pos_allowed, Some(true));
    oracle.fact("metadata_select", "false");
    let schema = admin
        .query_one(
            "SELECT pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'USAGE'), \
                    pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'CREATE')",
            &[&"oo_r_clerk"],
        )
        .await
        .expect("schema privilege");
    let usage: bool = schema.get(0);
    let create: bool = schema.get(1);
    assert!(usage);
    assert!(!create);
    oracle.fact("schema_usage", "usage_without_create");
}

fn query_public_api(bans: &[String], oracle: &mut Oracle) {
    fs::create_dir_all(LIVE_DIR).expect("live dir");
    let clerk_turns = [
        "SELECT party_id, display_name, note FROM trade.party ORDER BY party_id",
        "SELECT count(*) FROM trade.party",
        "SELECT * FROM trade.party ORDER BY party_id",
        "SELECT entry_id, party_id, memo FROM trade.ledger ORDER BY entry_id",
        "SELECT count(*) FROM trade.ledger",
        "SELECT * FROM trade.ledger ORDER BY entry_id",
    ];
    let clerk = run_query(
        "clerk",
        &clerk_turns,
        &clerk_policies(),
        "clerk-audit.jsonl",
        bans,
    );
    let clerk_stderr = clerk.stderr.clone();
    assert_eq!(clerk.exit, 0, "clerk stderr={clerk_stderr}");
    let clerk_turns = parse_turns(&clerk.stdout);
    assert_eq!(clerk_turns.len(), 6);
    assert_row(
        &clerk_turns[0],
        &[
            &["1", "甲公司", "secret-note"],
            &["2", "乙部", "other-note"],
        ],
    );
    assert_count(&clerk_turns[1]);
    assert_row(
        &clerk_turns[2],
        &[
            &["1", "甲公司", "secret-note"],
            &["2", "乙部", "other-note"],
        ],
    );
    assert_row(
        &clerk_turns[3],
        &[&["10", "1", "memo-a"], &["11", "2", "memo-b"]],
    );
    assert_count(&clerk_turns[4]);
    assert_row(
        &clerk_turns[5],
        &[&["10", "1", "memo-a"], &["11", "2", "memo-b"]],
    );
    oracle.fact("query_clerk_exit", "0");
    oracle.fact("clerk_rows", "present");

    let peek_turns = [
        "SELECT party_id, display_name FROM trade.party ORDER BY party_id",
        "SELECT count(*) FROM trade.party",
        "SELECT * FROM trade.party ORDER BY party_id",
        "SELECT entry_id, party_id FROM trade.ledger ORDER BY entry_id",
        "SELECT count(*) FROM trade.ledger",
        "SELECT * FROM trade.ledger ORDER BY entry_id",
    ];
    let peek = run_query(
        "peek",
        &peek_turns,
        &peek_policies(),
        "peek-audit.jsonl",
        bans,
    );
    let peek_stderr = peek.stderr.clone();
    assert_eq!(peek.exit, 0, "peek stderr={peek_stderr}");
    let peek_text = String::from_utf8(peek.stdout.clone()).expect("peek utf8");
    for hidden in ["secret-note", "other-note", "memo-a", "memo-b"] {
        assert!(!peek_text.contains(hidden), "peek leaked {hidden}");
    }
    let parsed = parse_turns(&peek.stdout);
    assert_row(&parsed[0], &[&["1", "甲公司"], &["2", "乙部"]]);
    assert_count(&parsed[1]);
    assert_row(&parsed[2], &[&["1", "甲公司"], &["2", "乙部"]]);
    assert_row(&parsed[3], &[&["10", "1"], &["11", "2"]]);
    assert_count(&parsed[4]);
    assert_row(&parsed[5], &[&["10", "1"], &["11", "2"]]);
    oracle.fact("query_peek_exit", "0");
    oracle.fact("peek_hidden", "absent");

    let guard_audit = Path::new(LIVE_DIR).join("peek-guard-audit.jsonl");
    if guard_audit.exists() {
        fs::remove_file(&guard_audit).expect("remove stale guard audit");
    }
    let guard = run_query(
        "peek",
        &["SELECT note FROM trade.party"],
        &peek_policies(),
        "peek-guard-audit.jsonl",
        bans,
    );
    let guard_stderr = guard.stderr.clone();
    assert_eq!(guard.exit, 6, "guard stderr={guard_stderr}");
    assert!(guard.stderr.contains("query-pg: guard"));
    assert!(!guard_audit.exists(), "guard wrote an audit file");
    oracle.fact("query_guard_exit", "6");
    oracle.fact("guard_audit", "absent");
}

struct QueryOut {
    exit: i32,
    stdout: Vec<u8>,
    stderr: String,
}

fn run_query(
    role: &str,
    statements: &[&str],
    policies: &[Value],
    audit_name: &str,
    bans: &[String],
) -> QueryOut {
    let audit = Path::new(LIVE_DIR).join(audit_name);
    if audit.exists() {
        fs::remove_file(&audit).expect("remove stale audit");
    }
    let request_path = Path::new(LIVE_DIR).join(format!("{role}-{audit_name}-request.json"));
    let source = if statements.len() == 1 && audit_name.contains("guard") {
        "request"
    } else {
        "stdin"
    };
    let mut request = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {
            "host": HOST,
            "port": 5432,
            "database": DATABASE,
            "user": LOGIN
        },
        "password_file": Path::new(SECRET_DIR).join("login.pass"),
        "ca_file": Path::new(SECRET_DIR).join("ca.crt"),
        "audit_file": &audit,
        "identity": {
            "project": SOURCE,
            "role": format!("oo_r_{role}"),
            "policy_id": CONFIG
        },
        "policies": policies,
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "refuse"
        },
        "statement": {
            "source": source,
            "max_rows": 20,
            "max_result_bytes": 65536,
            "wall_clock_ms": 30000
        }
    });
    let stdin = if source == "request" {
        request["statement"]["sql"] = json!(statements[0]);
        None
    } else {
        let mut body = String::new();
        for sql in statements {
            body.push_str(&serde_json::to_string(&json!({
                "version": "oo.gateway.query-pg-turn/v1",
                "sql": sql
            })).expect("turn"));
            body.push('\n');
        }
        Some(body)
    };
    let bytes = serde_json::to_vec_pretty(&request).expect("request");
    assert_quiet(&bytes, bans, "request");
    fs::write(&request_path, &bytes).expect("request file");
    let output = spawn_query(&request_path, stdin.as_deref());
    assert_quiet(&output.stdout, bans, "stdout");
    assert_quiet(&output.stderr, bans, "stderr");
    let stdout_path = Path::new(LIVE_DIR).join(format!("{role}-{audit_name}-stdout.txt"));
    let stderr_path = Path::new(LIVE_DIR).join(format!("{role}-{audit_name}-stderr.txt"));
    fs::write(&stdout_path, &output.stdout).expect("stdout file");
    fs::write(&stderr_path, &output.stderr).expect("stderr file");
    QueryOut {
        exit: output.status.code().unwrap_or(127),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn spawn_query(request: &Path, stdin: Option<&str>) -> Output {
    let mut command = Command::new(query_pg_exe());
    command.arg("--request").arg(request);
    command.env_remove("PGPASSWORD");
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn query-pg");
    if let Some(body) = stdin {
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(body.as_bytes())
            .expect("write stdin");
    }
    child.wait_with_output().expect("wait query-pg")
}

fn query_pg_exe() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_query-pg") {
        return PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_query_pg") {
        return PathBuf::from(path);
    }
    let mut path = env::current_exe().expect("current_exe");
    path.pop();
    path.pop();
    path.push("query-pg");
    path
}

struct Turn {
    rows: Vec<Vec<String>>,
    status: String,
    returned: u64,
}

fn parse_turns(stdout: &[u8]) -> Vec<Turn> {
    let text = std::str::from_utf8(stdout).expect("stdout utf8");
    let mut turns: Vec<Turn> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    for line in text.split('\n').filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_str(line).expect("stdout json");
        match value["record"].as_str() {
            Some("row") => {
                let cells = value["cells"].as_array().expect("cells");
                rows.push(cells.iter().map(cell_text).collect());
            }
            Some("summary") => {
                let returned = value["returned_rows"].as_u64().expect("returned");
                let status = value["status"].as_str().expect("status").to_string();
                assert!(value["end_audit_ok"].as_bool().expect("audit flag"));
                assert!(!value["truncated"].as_bool().expect("truncated"));
                turns.push(Turn {
                    rows: std::mem::take(&mut rows),
                    status,
                    returned,
                });
            }
            other => panic!("unexpected record {other:?}"),
        }
    }
    assert!(rows.is_empty(), "rows without a summary");
    turns
}

fn cell_text(cell: &Value) -> String {
    match cell["kind"].as_str() {
        Some("int" | "text") => cell["value"].as_str().expect("cell value").to_string(),
        other => panic!("unexpected cell {other:?} {cell}"),
    }
}

fn assert_row(turn: &Turn, expected: &[&[&str]]) {
    assert_eq!(turn.status, "succeeded");
    assert_eq!(turn.returned, expected.len() as u64);
    assert_eq!(turn.rows.len(), expected.len());
    for (row, want) in turn.rows.iter().zip(expected.iter()) {
        assert_eq!(row, want);
    }
}

fn assert_count(turn: &Turn) {
    assert_eq!(turn.status, "succeeded");
    assert_eq!(turn.returned, 1);
    assert_eq!(turn.rows, vec![vec!["2".to_string()]]);
}

fn clerk_policies() -> Vec<Value> {
    vec![
        policy_json("clerk", PARTY, &["party_id", "display_name", "note"]),
        policy_json("clerk", LEDGER, &["entry_id", "party_id", "memo"]),
    ]
}

fn peek_policies() -> Vec<Value> {
    vec![
        policy_json("peek", PARTY, &["party_id", "display_name"]),
        policy_json("peek", LEDGER, &["entry_id", "party_id"]),
    ]
}

fn policy_json(role: &str, table: &str, columns: &[&str]) -> Value {
    json!({
        "project": SOURCE,
        "config_digest": CONFIG,
        "generation": CURRENT_GENERATION,
        "role": role,
        "login": LOGIN,
        "table": {
            "namespace": NAMESPACE,
            "name": table,
            "columns": columns.iter().copied().map(|name| json!({
                "name": name,
                "status": "confirmed",
                "semantic_type": "identifier",
                "pii": {"class": "none"}
            })).collect::<Vec<_>>()
        }
    })
}

async fn direct_denials(client: &mut Client, shadow: &Shadow, pid: i32, oracle: &mut Oracle) {
    let party = quote_physical(&shadow.party.table, "pt_");
    let ledger = quote_physical(&shadow.ledger.table, "pt_");
    let party_id = quote_physical(&shadow.party.party_id, "pc_");
    let note = quote_physical(&shadow.party.note, "pc_");
    let ratio = quote_physical(&shadow.party.ratio, "pc_");
    let memo = quote_physical(&shadow.ledger.memo, "pc_");
    let allowed = query_as_role(
        client,
        "oo_r_clerk",
        &format!("SELECT {party_id}::int4 FROM \"_oo\".{party} ORDER BY {party_id}::int4 LIMIT 1"),
    )
    .await;
    assert_eq!(allowed.get::<_, i32>(0), 1);
    expect_42501(
        client,
        Some("oo_r_peek"),
        &format!("SELECT {note}::text FROM \"_oo\".{party}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_clerk"),
        &format!("SELECT \"_oo_pos\" FROM \"_oo\".{party}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_clerk"),
        &format!("SELECT * FROM \"_oo\".{party}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_peek"),
        &format!("SELECT * FROM \"_oo\".{ledger}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_clerk"),
        &format!("SELECT {ratio}::float4 FROM \"_oo\".{party}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_peek"),
        &format!("SELECT {memo}::text FROM \"_oo\".{ledger}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_clerk"),
        &format!("INSERT INTO \"_oo\".{party} (\"_oo_pos\") VALUES (9)"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_clerk"),
        &format!("UPDATE \"_oo\".{ledger} SET {memo} = 'rewritten'"),
    )
    .await;
    expect_42501(
        client,
        None,
        &format!("SELECT {party_id}::int4 FROM \"_oo\".{party}"),
    )
    .await;
    assert_eq!(backend_pid(client).await, pid);
    oracle.fact("hidden_write", "42501");
    oracle.fact("raw_star", "42501");
    oracle.fact("reader_pid", &pid.to_string());
}

async fn reset_database(admin_pass: &str, ca: &[u8]) {
    let maintenance = connect_db("postgres", ADMIN, admin_pass, ca).await;
    expect_sql(
        &maintenance,
        "terminate database sessions",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    expect_sql(
        &maintenance,
        "drop database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    expect_sql(
        &maintenance,
        "drop fixture roles",
        "DO $$\n\
         DECLARE granted_name text;\n\
         DECLARE member_name text;\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           FOR granted_name, member_name IN\n\
             SELECT granted.rolname, member_role.rolname\n\
             FROM pg_catalog.pg_auth_members AS membership\n\
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid\n\
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member\n\
             WHERE granted.rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
                OR member_role.rolname IN ('oo_a218_login', 'oo_a218_other')\n\
           LOOP\n\
             EXECUTE format('REVOKE %I FROM %I', granted_name, member_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a218_login') THEN\n\
             DROP ROLE oo_a218_login;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a218_other') THEN\n\
             DROP ROLE oo_a218_other;\n\
           END IF;\n\
           FOR role_name IN\n\
             SELECT rolname FROM pg_catalog.pg_roles\n\
             WHERE rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
           LOOP\n\
             EXECUTE format('DROP ROLE %I', role_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN\n\
             DROP ROLE oo_pipeline_runtime;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') THEN\n\
             DROP ROLE oo_target_owner;\n\
           END IF;\n\
         END\n\
         $$",
    )
    .await;
    expect_sql(
        &maintenance,
        "create database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_named(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_named(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_named(
        &tx,
        "row-lock",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_named(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_named(
        &tx,
        "markers-target",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            (
                "target-fence-receipts/3",
                &preflight.1,
                "0003_target_segment_preflight_expand.sql",
            ),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");

    let tx = begin_locked(client).await;
    exec_named(
        &tx,
        "row-lock-import",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_named(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_named(
        &tx,
        "marker-import",
        &marker_sql(&[(
            "full-import/v1",
            &full_import.1,
            "0005_full_import_expand.sql",
        )]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {name}: {err}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} is not utf-8"));
    (text, sha256_hex(&bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        assert!(closed_token(version), "version token");
        assert!(closed_token(filename), "filename token");
        assert_eq!(checksum.len(), 64, "checksum length");
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
    }
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}")
}

fn closed_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'))
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_named(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_named(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_named(
        &tx,
        "advisory-lock",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202618)",
    )
    .await;
    tx
}

async fn expect_sql(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| {
            let detail = db_detail(&err);
            panic!("{name} failed: {detail}");
        });
}

fn db_detail(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.to_string())
        .unwrap_or_else(|| err.to_string())
}

async fn exec_named(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| {
            let detail = db_detail(&err);
            panic!("{name} failed: {detail}");
        });
}

async fn create_login(client: &Client, name: &str, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(name.bytes().all(|byte| byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()));
    client
        .batch_execute(&format!(
            "CREATE ROLE \"{name}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{name}\""
        ))
        .await
        .expect("create login");
}

struct PartyMap {
    table: String,
    party_id: String,
    display_name: String,
    note: String,
    ratio: String,
}

struct LedgerMap {
    table: String,
    entry_id: String,
    party_id: String,
    memo: String,
}

struct Shadow {
    party: PartyMap,
    ledger: LedgerMap,
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .expect("adopt")
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .expect("bind");
    for generation in [STALE_GENERATION, CURRENT_GENERATION] {
        register(client, &map_key(CONFIG, generation, "namespace", "n", "", "")).await;
        register(client, &map_key(CONFIG, generation, "table", "t", PARTY, "")).await;
        register(client, &map_key(CONFIG, generation, "table", "t", LEDGER, "")).await;
        for column in ["party_id", "display_name", "note", "ratio"] {
            register(client, &map_key(CONFIG, generation, "column", "c", PARTY, column)).await;
        }
        for column in ["entry_id", "party_id", "memo"] {
            register(client, &map_key(CONFIG, generation, "column", "c", LEDGER, column)).await;
        }
    }
    let party_table = lookup(client, "table", PARTY, "").await;
    let ledger_table = lookup(client, "table", LEDGER, "").await;
    let party_cols = vec![
        ("party_id".to_string(), lookup(client, "column", PARTY, "party_id").await),
        ("display_name".to_string(), lookup(client, "column", PARTY, "display_name").await),
        ("note".to_string(), lookup(client, "column", PARTY, "note").await),
        ("ratio".to_string(), lookup(client, "column", PARTY, "ratio").await),
    ];
    let ledger_cols = vec![
        ("entry_id".to_string(), lookup(client, "column", LEDGER, "entry_id").await),
        ("party_id".to_string(), lookup(client, "column", LEDGER, "party_id").await),
        ("memo".to_string(), lookup(client, "column", LEDGER, "memo").await),
    ];
    let run_id = open_run(client, target, &party_table, &ledger_table).await;
    prepare_one(
        client,
        target,
        &run_id,
        0,
        PARTY,
        &["party_id", "display_name", "note", "ratio"],
        &[23, 25, 25, 700],
    )
    .await;
    prepare_one(
        client,
        target,
        &run_id,
        1,
        LEDGER,
        &["entry_id", "party_id", "memo"],
        &[23, 23, 25],
    )
    .await;
    insert_rows(client, &party_table, &party_cols, &ledger_table, &ledger_cols).await;
    Shadow {
        party: PartyMap {
            table: party_table,
            party_id: party_cols[0].1.clone(),
            display_name: party_cols[1].1.clone(),
            note: party_cols[2].1.clone(),
            ratio: party_cols[3].1.clone(),
        },
        ledger: LedgerMap {
            table: ledger_table,
            entry_id: ledger_cols[0].1.clone(),
            party_id: ledger_cols[1].1.clone(),
            memo: ledger_cols[2].1.clone(),
        },
    }
}

async fn open_run(client: &Client, target: i64, party: &str, ledger: &str) -> String {
    let schemas = vec![NAMESPACE.to_string(), NAMESPACE.to_string()];
    let tables = vec![PARTY.to_string(), LEDGER.to_string()];
    let physical = vec![party.to_string(), ledger.to_string()];
    let params: [&(dyn ToSql + Sync); 12] = [
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &CURRENT_GENERATION,
        &CONFIG,
        &TYPE_DIGEST,
        &INPUT_DIGEST,
        &SCHEMA_DIGEST,
        &schemas,
        &tables,
        &physical,
    ];
    client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &params,
        )
        .await
        .expect("open run")
        .get(0)
}

async fn prepare_one(
    client: &Client,
    target: i64,
    run_id: &str,
    index: i32,
    table: &str,
    idents: &[&str],
    oids: &[i64],
) {
    let run_owned = run_id.to_string();
    let names: Vec<String> = idents.iter().map(|name| (*name).to_string()).collect();
    let oid_values = oids.to_vec();
    let typmods = vec![-1_i32; idents.len()];
    let required = vec![true; idents.len()];
    let namespace = NAMESPACE.to_string();
    let table_name = table.to_string();
    let params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &target,
        &run_owned,
        &index,
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &namespace,
        &table_name,
        &names,
        &oid_values,
        &typmods,
        &required,
    ];
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &params,
        )
        .await
        .expect("prepare shadow")
        .get(0);
    assert!(prepared.starts_with("pt_"), "{prepared}");
}

async fn insert_rows(
    client: &mut Client,
    party_table: &str,
    party_cols: &[(String, String)],
    ledger_table: &str,
    ledger_cols: &[(String, String)],
) {
    let party_sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {party_id}, {display_name}, {note}, {ratio}) \
         VALUES (($1::text)::numeric, $2, $3, $4, ($5::text)::real)",
        table = quote_physical(party_table, "pt_"),
        party_id = quote_physical(&party_cols[0].1, "pc_"),
        display_name = quote_physical(&party_cols[1].1, "pc_"),
        note = quote_physical(&party_cols[2].1, "pc_"),
        ratio = quote_physical(&party_cols[3].1, "pc_"),
    );
    let ledger_sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {entry_id}, {party_id}, {memo}) \
         VALUES (($1::text)::numeric, $2, $3, $4)",
        table = quote_physical(ledger_table, "pt_"),
        entry_id = quote_physical(&ledger_cols[0].1, "pc_"),
        party_id = quote_physical(&ledger_cols[1].1, "pc_"),
        memo = quote_physical(&ledger_cols[2].1, "pc_"),
    );
    let tx = client.transaction().await.expect("insert begin");
    let first: [&(dyn ToSql + Sync); 5] = [&"1", &1_i32, &"甲公司", &"secret-note", &"1.5"];
    let second: [&(dyn ToSql + Sync); 5] = [&"2", &2_i32, &"乙部", &"other-note", &"2.5"];
    tx.execute(&party_sql, &first).await.expect("insert party 1");
    tx.execute(&party_sql, &second).await.expect("insert party 2");
    let entry_a: [&(dyn ToSql + Sync); 4] = [&"10", &10_i32, &1_i32, &"memo-a"];
    let entry_b: [&(dyn ToSql + Sync); 4] = [&"11", &11_i32, &2_i32, &"memo-b"];
    tx.execute(&ledger_sql, &entry_a).await.expect("insert ledger 10");
    tx.execute(&ledger_sql, &entry_b).await.expect("insert ledger 11");
    tx.commit().await.expect("insert commit");
}

async fn register_other_digest(client: &Client) {
    register(
        client,
        &map_key(OTHER_DIGEST, LATER_GENERATION, "namespace", "n", "", ""),
    )
    .await;
}

struct MapKey<'a> {
    digest: &'a str,
    generation: i64,
    level: &'a str,
    letter: &'a str,
    table: &'a str,
    column: &'a str,
}

fn map_key<'a>(
    digest: &'a str,
    generation: i64,
    level: &'a str,
    letter: &'a str,
    table: &'a str,
    column: &'a str,
) -> MapKey<'a> {
    MapKey {
        digest,
        generation,
        level,
        letter,
        table,
        column,
    }
}

async fn register(client: &Client, key: &MapKey<'_>) {
    let namespace = NAMESPACE;
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &key.digest,
        &key.generation,
        &key.letter,
        &namespace,
        &key.table,
        &key.column,
    ];
    let digest_bytes: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &key.digest,
        &key.generation,
        &key.level,
        &namespace,
        &key.table,
        &key.column,
        &digest_bytes,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("register");
}

async fn lookup(client: &Client, level: &str, table: &str, column: &str) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &level,
        &NAMESPACE,
        &table,
        &column,
    ];
    let name: String = client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .expect("lookup")
        .get(0);
    let prefix = if level == "table" { "pt_" } else { "pc_" };
    let _ = quote_physical(&name, prefix);
    name
}

fn clerk_set() -> Vec<ColumnAclPolicy> {
    vec![
        acl(
            "clerk",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            PARTY,
            &["party_id", "display_name", "note"],
            false,
        ),
        acl(
            "clerk",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            LEDGER,
            &["entry_id", "party_id", "memo"],
            false,
        ),
    ]
}

fn peek_set() -> Vec<ColumnAclPolicy> {
    vec![
        acl(
            "peek",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            PARTY,
            &["party_id", "display_name"],
            false,
        ),
        acl(
            "peek",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            LEDGER,
            &["entry_id", "party_id"],
            false,
        ),
    ]
}

fn orphan_set() -> Vec<ColumnAclPolicy> {
    vec![
        acl(
            "orphan",
            "oo_a218_missing",
            CONFIG,
            CURRENT_GENERATION,
            PARTY,
            &["party_id"],
            false,
        ),
        acl(
            "orphan",
            "oo_a218_missing",
            CONFIG,
            CURRENT_GENERATION,
            LEDGER,
            &["entry_id"],
            false,
        ),
    ]
}

fn stale_set(generation: i64, role: &str) -> Vec<ColumnAclPolicy> {
    vec![
        acl(role, LOGIN, CONFIG, generation, PARTY, &["party_id"], false),
        acl(role, LOGIN, CONFIG, generation, LEDGER, &["entry_id"], false),
    ]
}

fn acl(
    role: &str,
    login: &str,
    digest: &str,
    generation: i64,
    table: &str,
    columns: &[&str],
    row_filter: bool,
) -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: digest.to_string(),
        generation,
        role: role.to_string(),
        login: login.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: table.to_string(),
            columns: columns.iter().copied().map(column).collect(),
        },
        row_filter: row_filter.then(|| RowFilter::Claims {
            column: "party_id".to_string(),
            claim: "party".to_string(),
        }),
    }
}

fn column(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "identifier".to_string(),
        pii: PiiClass::None,
    }
}

async fn max_generation(client: &Client, digest: &str) -> Option<i64> {
    client
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2",
            &[&SOURCE, &digest],
        )
        .await
        .expect("max generation")
        .get(0)
}

async fn role_count(client: &Client, prefix: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_roles WHERE left(rolname, length($1)) = $1",
            &[&prefix],
        )
        .await
        .expect("role count")
        .get(0)
}

async fn role_present(client: &Client, name: &str) -> bool {
    client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)",
            &[&name],
        )
        .await
        .expect("role present")
        .get(0)
}

async fn privilege_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .expect("privilege count")
        .get(0)
}

async fn select_columns(client: &Client, role: &str, table: &str) -> BTreeSet<String> {
    client
        .query(
            "SELECT column_name::text FROM information_schema.column_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = $2 \
               AND privilege_type = 'SELECT' \
             ORDER BY column_name",
            &[&role, &table],
        )
        .await
        .expect("select columns")
        .into_iter()
        .map(|row| row.get(0))
        .collect()
}

async fn grant_text(client: &Client, role: &str) -> String {
    let rows = client
        .query(
            "SELECT table_name::text || '.' || column_name::text || '.' || privilege_type::text \
             FROM information_schema.column_privileges \
             WHERE grantee = $1 \
             ORDER BY table_name, column_name, privilege_type",
            &[&role],
        )
        .await
        .expect("grant text");
    rows.into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>()
        .join(",")
}

async fn table_privilege_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.table_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .expect("table privileges")
        .get(0)
}

async fn non_select_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges \
             WHERE grantee = $1 AND privilege_type <> 'SELECT'",
            &[&role],
        )
        .await
        .expect("non select")
        .get(0)
}

async fn public_column_privileges(client: &Client, table: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges \
             WHERE grantee = 'PUBLIC' AND table_schema = '_oo' AND table_name = $1",
            &[&table],
        )
        .await
        .expect("public columns")
        .get(0)
}

async fn publication_count(client: &Client) -> i64 {
    client
        .query_one("SELECT count(*)::int8 FROM pg_catalog.pg_publication", &[])
        .await
        .expect("publications")
        .get(0)
}

async fn reltuples_unknown(client: &Client, table: &str) -> bool {
    client
        .query_one(
            "SELECT relation.reltuples < 0 \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1",
            &[&table],
        )
        .await
        .expect("reltuples")
        .get(0)
}

async fn grant_mapping_read(client: &Client) {
    client
        .batch_execute(&format!(
            "GRANT USAGE ON SCHEMA \"_oo\" TO \"{LOGIN}\";\n\
             GRANT SELECT ON TABLE \"_oo\".physical_mappings TO \"{LOGIN}\";\n\
             GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO \"{LOGIN}\""
        ))
        .await
        .expect("grant mapping read");
}

async fn analyze(client: &Client, table: &str) {
    let quoted = quote_physical(table, "pt_");
    client
        .batch_execute(&format!("ANALYZE \"_oo\".{quoted}"))
        .await
        .expect("analyze");
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length {name}");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "physical hex"
    );
    format!("\"{name}\"")
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::new();
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(err) => {
                last = err.to_string();
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
}

fn read_hex(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    let value = raw.trim();
    let hex = (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    assert!(hex, "{name} is not lowercase hex");
    value.to_string()
}

struct Oracle {
    lines: Vec<String>,
    bans: Vec<String>,
}

impl Oracle {
    fn new(secrets: &[&str]) -> Self {
        Self {
            lines: Vec::new(),
            bans: secrets.iter().map(|secret| (*secret).to_string()).collect(),
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        assert_quiet(value.as_bytes(), &self.bans, key);
        self.lines.push(format!("{key}={value}"));
        self.flush();
    }

    fn flush(&self) {
        let path = Path::new(ORACLE_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(path, self.lines.join("\n") + "\n").expect("oracle write");
    }
}

fn assert_quiet(bytes: &[u8], bans: &[String], label: &str) {
    let text = String::from_utf8_lossy(bytes);
    for ban in bans {
        assert!(!text.contains(ban.as_str()), "{label} contained a secret");
    }
}

async fn query_as_role(client: &mut Client, role: &str, sql: &str) -> tokio_postgres::Row {
    let tx = client.transaction().await.expect("begin read");
    let quoted = quote_role(role);
    tx.batch_execute(&format!("SET LOCAL ROLE {quoted}"))
        .await
        .expect("set role");
    let row = tx.query_one(sql, &[]).await.expect("selected read");
    tx.rollback().await.expect("rollback read");
    row
}

async fn expect_42501(client: &mut Client, role: Option<&str>, sql: &str) {
    let tx = client.transaction().await.expect("begin denial");
    if let Some(role) = role {
        let quoted = quote_role(role);
        tx.batch_execute(&format!("SET LOCAL ROLE {quoted}"))
            .await
            .expect("set role");
    }
    let err = tx.query_one(sql, &[]).await.expect_err("expected denial");
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    assert_eq!(state, "42501", "{err}");
    tx.rollback().await.expect("rollback denial");
}

async fn backend_pid(client: &Client) -> i32 {
    client
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .expect("pid")
        .get(0)
}

fn quote_role(role: &str) -> String {
    assert!(
        role.bytes().all(|byte| {
            byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()
        }) && !role.is_empty()
    );
    format!("\"{role}\"")
}
