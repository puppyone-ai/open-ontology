//! Independent review fixture for the frozen column-ACL set.
//!
//! Runs only when `OO_A229_LIVE=1`. Product sources are not edited.
//! A public-schema event trigger is the one post-grant fault. Mapping and
//! type failures are recorded only when that trigger sees zero `GRANT` events,
//! so they are not described as post-grant rollback.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use oo_gateway::policy::{
    compile_column_acl_set, install_column_acl_set, COLUMN_ACL_SET_MAX_COLUMNS,
    COLUMN_ACL_SET_MAX_TABLES,
};
use oo_gateway::{
    compile_column_acl, connect_require_tls, install_column_acl, ClassStatus, ColumnAclPolicy,
    ColumnSpec, InstallEffect, LogicalTable, PiiClass, PolicyError, RowFilter, TlsEndpoint,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const HOST: &str = "oo-a229-pg";
const DATABASE: &str = "oo_a229";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a229_login";
const OTHER_LOGIN: &str = "oo_a229_other";
const SECRET_DIR: &str = "/run/oo-a229/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const LIVE_DIR: &str = "/evidence/live";
const OWNER: &str = "a2290000-0000-4000-8000-000000000229";
const SHARD: &str = "shard_a229";
const SOURCE: &str = "depot";
const CONFIG: &str = "a229a229a229a229a229a229a229a229a229a229a229a229a229a229a229a229";
const OTHER_DIGEST: &str = "b229b229b229b229b229b229b229b229b229b229b229b229b229b229b229b229";
const TYPE_DIGEST: &str = "c229c229c229c229c229c229c229c229c229c229c229c229c229c229c229c229";
const INPUT_DIGEST: &str = "d229d229d229d229d229d229d229d229d229d229d229d229d229d229d229d229";
const SCHEMA_DIGEST: &str = "e229e229e229e229e229e229e229e229e229e229e229e229e229e229e229e229";
const NAMESPACE: &str = "depot";
const COUNTER: &str = "counter";
const SLIP: &str = "slip";
const STALE_GENERATION: i64 = 4;
const CURRENT_GENERATION: i64 = 7;
const LATER_GENERATION: i64 = 11;

const HOOK_SQL: &str = "\
CREATE SEQUENCE IF NOT EXISTS public.a229_grant_seen;
CREATE SEQUENCE IF NOT EXISTS public.a229_second_hit;
CREATE TABLE IF NOT EXISTS public.a229_grant_fault (
  id integer PRIMARY KEY,
  enabled boolean NOT NULL,
  fail_at integer NOT NULL,
  second_table text NOT NULL
);
INSERT INTO public.a229_grant_fault (id, enabled, fail_at, second_table)
VALUES (1, false, 0, '')
ON CONFLICT (id) DO UPDATE
  SET enabled = false, fail_at = 0, second_table = '';
CREATE OR REPLACE FUNCTION public.a229_on_grant()
RETURNS event_trigger
LANGUAGE plpgsql
AS $fn$
DECLARE
  enabled boolean;
  fail_at integer;
  seen bigint;
  target text := '';
  identity text := '';
  relname text;
  named text;
  query_text text := '';
  hit boolean := false;
  command_row record;
BEGIN
  seen := nextval('public.a229_grant_seen');
  SELECT fault.enabled, fault.fail_at, fault.second_table
    INTO enabled, fail_at, target
  FROM public.a229_grant_fault AS fault
  WHERE fault.id = 1;
  IF COALESCE(enabled, false) AND seen >= fail_at THEN
    query_text := COALESCE(current_query(), '');
    FOR command_row IN
      SELECT command.classid, command.objid, command.objsubid,
             command.command_tag, command.object_type,
             command.schema_name, command.object_identity
      FROM pg_catalog.pg_event_trigger_ddl_commands() AS command
    LOOP
      relname := NULL;
      named := NULL;
      IF command_row.objid IS NOT NULL AND command_row.objid <> 0::oid THEN
        SELECT relation.relname::text INTO relname
        FROM pg_catalog.pg_class AS relation
        WHERE relation.oid = command_row.objid;
        BEGIN
          SELECT identified.identity INTO named
          FROM pg_catalog.pg_identify_object(
            command_row.classid, command_row.objid, command_row.objsubid
          ) AS identified;
        EXCEPTION WHEN OTHERS THEN
          named := NULL;
        END;
      END IF;
      identity := COALESCE(command_row.command_tag, '')
        || ' type=' || COALESCE(command_row.object_type, '')
        || ' schema=' || COALESCE(command_row.schema_name, '')
        || ' identity=' || COALESCE(command_row.object_identity, '')
        || ' rel=' || COALESCE(relname, '')
        || ' named=' || COALESCE(named, '')
        || ' obj=' || COALESCE(command_row.objid::text, '')
        || ' sub=' || COALESCE(command_row.objsubid::text, '');
      IF COALESCE(target, '') <> '' AND relname = target THEN
        hit := true;
      END IF;
    END LOOP;
    IF COALESCE(target, '') <> '' AND strpos(query_text, target) > 0 THEN
      hit := true;
    END IF;
    IF hit THEN
      PERFORM nextval('public.a229_second_hit');
    END IF;
    RAISE EXCEPTION 'a229 post-grant fault grant % hit % detail % query %',
      seen, hit, identity, left(query_text, 400)
      USING ERRCODE = 'P0001';
  END IF;
END;
$fn$;
DROP EVENT TRIGGER IF EXISTS a229_grant_fault;
CREATE EVENT TRIGGER a229_grant_fault
  ON ddl_command_end
  WHEN TAG IN ('GRANT')
  EXECUTE FUNCTION public.a229_on_grant();
";

#[tokio::test]
async fn independent_two_table_column_acl_set() {
    if env::var("OO_A229_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    assert_eq!(COLUMN_ACL_SET_MAX_COLUMNS, 512);
    assert_eq!(COLUMN_ACL_SET_MAX_TABLES, 32);

    let admin_pass = read_hex("admin.pass");
    let login_pass = read_hex("login.pass");
    let other_pass = read_hex("other.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let mut oracle = Oracle::new(&[&admin_pass, &login_pass, &other_pass]);

    reset_database(&admin_pass, &ca).await;
    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, LOGIN, &login_pass).await;
    create_login(&admin, OTHER_LOGIN, &other_pass).await;
    let shadow = prepare_shadow(&mut admin).await;
    register_other_digest(&admin).await;
    let digest_a = max_generation(&admin, CONFIG).await;
    let digest_b = max_generation(&admin, OTHER_DIGEST).await;
    assert_eq!(digest_a, Some(CURRENT_GENERATION));
    assert_eq!(digest_b, Some(LATER_GENERATION));
    oracle.fact("digest_a_max", "7");
    oracle.fact("digest_b_max", "11");

    exec_sql(&admin, "install grant hook", HOOK_SQL).await;
    let roles_before = role_count(&admin, "oo_r_").await;
    reset_events(&admin).await;
    prove_pure(&mut admin, &mut oracle).await;
    assert_eq!(grant_events(&admin).await, 0);
    assert_eq!(role_count(&admin, "oo_r_").await, roles_before);
    oracle.fact("pure_grant_events", "0");

    prove_pre_grant(&mut admin, &shadow, &mut oracle).await;
    prove_legacy(&mut admin, &shadow, &mut oracle).await;

    assert!(reltuples_unknown(&admin, &shadow.counter.table).await);
    assert!(reltuples_unknown(&admin, &shadow.slip.table).await);
    oracle.fact("reltuples_before", "unknown");
    assert_eq!(publication_count(&admin).await, 0);
    oracle.fact("publication", "0");

    reset_events(&admin).await;
    let scribe_in = scribe_set();
    let installed = install_column_acl_set(&mut admin, &scribe_in)
        .await
        .expect("install scribe");
    let installed_events = grant_events(&admin).await;
    assert!(
        installed_events >= 2,
        "hook saw {installed_events} grants on a new two-table install"
    );
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, "oo_r_scribe");
    assert_eq!(installed.grantor, ADMIN);
    assert_eq!(installed.tables.len(), 2);
    assert_eq!(installed.tables[0].physical_table, shadow.counter.table);
    assert_eq!(installed.tables[1].physical_table, shadow.slip.table);
    assert_eq!(
        installed.tables[0].physical_columns,
        vec![
            shadow.counter.counter_id.clone(),
            shadow.counter.label.clone(),
            shadow.counter.token.clone(),
        ]
    );
    assert!(!installed.tables[0]
        .physical_columns
        .contains(&shadow.counter.ratio));
    oracle.fact("scribe_effect", "Installed");
    oracle.fact("grantor", &installed.grantor);
    oracle.fact(
        "installed_grant_events",
        &installed_events.to_string(),
    );

    let glance = install_column_acl_set(&mut admin, &glance_set())
        .await
        .expect("install glance");
    assert_eq!(glance.effect, InstallEffect::Installed);
    assert_eq!(glance.role, "oo_r_glance");
    oracle.fact("glance_effect", "Installed");

    let replay = install_column_acl_set(&mut admin, &scribe_in)
        .await
        .expect("replay scribe");
    assert_eq!(replay.effect, InstallEffect::Replay);
    assert_eq!(replay.tables, installed.tables);
    oracle.fact("scribe_replay", "Replay");

    let counter_only = install_column_acl(&mut admin, &scribe_in[0])
        .await
        .expect("old api replays one table");
    assert_eq!(counter_only.effect, InstallEffect::Replay);
    assert_eq!(
        select_columns(&admin, "oo_r_scribe", &shadow.slip.table).await,
        BTreeSet::from([
            shadow.slip.slip_id.clone(),
            shadow.slip.counter_id.clone(),
            shadow.slip.body.clone(),
        ])
    );
    oracle.fact("old_api_one_table_replay", "Replay");
    oracle.fact("slip_grants_after_old_replay", "unchanged");

    let scribe_grants = grant_text(&admin, "oo_r_scribe").await;
    let glance_grants = grant_text(&admin, "oo_r_glance").await;
    refuse_mutations(&mut admin, &shadow, &scribe_grants, &glance_grants, &mut oracle).await;
    assert_catalog(&admin, &shadow, &mut oracle).await;

    let admin_pid = backend_pid(&admin).await;
    prove_post_grant_fault(&mut admin, &shadow, installed_events, &scribe_grants, &mut oracle)
        .await;
    assert_eq!(backend_pid(&admin).await, admin_pid);
    oracle.fact("same_backend_after_fault", "true");
    drop_hook(&admin).await;

    assert!(reltuples_unknown(&admin, &shadow.counter.table).await);
    analyze(&admin, &shadow.counter.table).await;
    analyze(&admin, &shadow.slip.table).await;
    assert!(!reltuples_unknown(&admin, &shadow.counter.table).await);
    assert!(!reltuples_unknown(&admin, &shadow.slip.table).await);
    oracle.fact("reltuples_after", "known");
    assert_eq!(publication_count(&admin).await, 0);

    let mapping_before = mapping_select_count(&admin, LOGIN).await;
    assert_eq!(mapping_before, 0);
    grant_mapping_read(&admin).await;
    assert_eq!(mapping_select_count(&admin, LOGIN).await, 1);
    assert_eq!(table_privilege_count(&admin, "oo_r_scribe").await, 0);
    oracle.fact("loader_mapping_grant", "fixture_login_not_installer");

    let bans = oracle.bans.clone();
    query_public_api(&bans, &mut oracle);

    let mut reader = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    let reader_pid = backend_pid(&reader).await;
    direct_denials(&mut reader, &shadow).await;
    assert_eq!(backend_pid(&reader).await, reader_pid);
    oracle.fact("hidden_write", "42501");
    oracle.fact("raw_star", "42501");
    oracle.fact("reader_pid_stable", "true");

    let rls: bool = admin
        .query_one(
            "SELECT COALESCE(bool_or(relation.relrowsecurity), false) \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname IN ($1, $2)",
            &[&shadow.counter.table, &shadow.slip.table],
        )
        .await
        .expect("rls")
        .get(0);
    assert!(!rls);
    oracle.fact("authentication", "not_claimed");
    oracle.fact("rls", "open");
    oracle.fact("audit_wal", "open");
    oracle.fact("nonsuperuser_grantor", "not_proven");
    oracle.flush();
}

async fn prove_pure(admin: &mut Client, oracle: &mut Oracle) {
        assert_eq!(
            install_column_acl_set(admin, &[]).await,
            Err(PolicyError::EmptyPolicySet)
        );
        let counter = acl(
            "scribe",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            COUNTER,
            &["counter_id"],
            false,
        );
        assert_eq!(
            install_column_acl_set(admin, &[counter.clone(), counter.clone()]).await,
            Err(PolicyError::InvalidIdentifier { field: "table" })
        );
        let other = acl(
            "scribe",
            LOGIN,
            OTHER_DIGEST,
            LATER_GENERATION,
            SLIP,
            &["slip_id"],
            false,
        );
        assert_eq!(
            install_column_acl_set(admin, &[counter.clone(), other]).await,
            Err(PolicyError::MixedIdentity)
        );
        let stale = acl(
            "scribe",
            LOGIN,
            CONFIG,
            STALE_GENERATION,
            SLIP,
            &["slip_id"],
            false,
        );
        assert_eq!(
            install_column_acl_set(admin, &[counter.clone(), stale]).await,
            Err(PolicyError::MixedIdentity)
        );
        let filtered = acl(
            "filtered",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            SLIP,
            &["slip_id"],
            true,
        );
        assert_eq!(
            install_column_acl_set(admin, &[counter.clone(), filtered]).await,
            Err(PolicyError::RowFilterRefused)
        );
        let mut hidden = counter.clone();
        hidden.table.columns = vec![column("_oo_pos")];
        assert!(matches!(
            install_column_acl_set(admin, &[hidden.clone()]).await,
            Err(PolicyError::MetadataColumn { .. })
        ));
        assert!(compile_column_acl(&hidden).is_ok());
        let mut unknown = counter.clone();
        unknown.role = "unknown_pii".to_string();
        unknown.table.columns[0].pii = PiiClass::Unknown;
        assert!(matches!(
            install_column_acl_set(admin, &[unknown]).await,
            Err(PolicyError::ClassificationRefused { .. })
        ));
        let mut wide = Vec::with_capacity(33);
        for index in 0..33 {
            wide.push(acl(
                "bounds",
                LOGIN,
                CONFIG,
                CURRENT_GENERATION,
                &format!("t{index:02}"),
                &["counter_id"],
                false,
            ));
        }
        assert_eq!(
            install_column_acl_set(admin, &wide).await,
            Err(PolicyError::LimitExceeded)
        );
        assert!(compile_column_acl_set(&[counter]).is_ok());
        assert!(!role_present(admin, "oo_r_unknown_pii").await);
        assert!(!role_present(admin, "oo_r_bounds").await);
        oracle.fact(
            "pure_refusals",
            "empty_duplicate_mixed_filter_metadata_unknown_bounds",
        );
}

async fn prove_pre_grant(admin: &mut Client, shadow: &Shadow, oracle: &mut Oracle) {
    reset_events(admin).await;
    let counter = acl(
        "bad_map",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        COUNTER,
        &["counter_id", "label", "token"],
        false,
    );
    let missing = acl(
        "bad_map",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        "missing_table",
        &["slip_id"],
        false,
    );
    assert_eq!(
        install_column_acl_set(admin, &[counter, missing]).await,
        Err(PolicyError::MappingAbsent)
    );
    assert!(!role_present(admin, "oo_r_bad_map").await);
    assert_eq!(privilege_count(admin, "oo_r_bad_map").await, 0);
    assert_eq!(grant_events(admin).await, 0);
    oracle.fact("bad_second_mapping", "MappingAbsent");
    oracle.fact("bad_second_role", "absent");
    oracle.fact("bad_second_grants", "0");
    oracle.fact("bad_second_grant_events", "0");
    oracle.fact("bad_second_timing", "pre_create_role");

    reset_events(admin).await;
    let slip = acl(
        "bad_type",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        SLIP,
        &["slip_id"],
        false,
    );
    let typed = acl(
        "bad_type",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        COUNTER,
        &["counter_id", "ratio"],
        false,
    );
    assert_eq!(
        install_column_acl_set(admin, &[slip, typed]).await,
        Err(PolicyError::TypeRefused {
            column: "ratio".to_string(),
        })
    );
    assert!(!role_present(admin, "oo_r_bad_type").await);
    assert_eq!(privilege_count(admin, "oo_r_bad_type").await, 0);
    assert_eq!(grant_events(admin).await, 0);
    assert!(!shadow.counter.ratio.is_empty(), "float column was not mapped");
    oracle.fact("bad_type", "TypeRefused");
    oracle.fact("bad_type_role", "absent");
    oracle.fact("bad_type_grants", "0");
    oracle.fact("bad_type_grant_events", "0");
    oracle.fact("bad_type_timing", "pre_create_role");

    reset_events(admin).await;
    assert_eq!(
        install_column_acl_set(admin, &orphan_set()).await,
        Err(PolicyError::LoginAbsent)
    );
    assert!(!role_present(admin, "oo_r_orphan").await);
    assert_eq!(grant_events(admin).await, 0);
    oracle.fact("login_absent", "LoginAbsent");
    oracle.fact("login_absent_grant_events", "0");

    reset_events(admin).await;
    assert_eq!(
        install_column_acl_set(admin, &stale_set(STALE_GENERATION, "stale_gen")).await,
        Err(PolicyError::StaleGeneration {
            policy: STALE_GENERATION,
            current: Some(CURRENT_GENERATION),
        })
    );
    assert_eq!(
        install_column_acl_set(admin, &stale_set(LATER_GENERATION, "later_gen")).await,
        Err(PolicyError::StaleGeneration {
            policy: LATER_GENERATION,
            current: Some(CURRENT_GENERATION),
        })
    );
    assert_eq!(grant_events(admin).await, 0);
    assert!(!role_present(admin, "oo_r_stale_gen").await);
    assert!(!role_present(admin, "oo_r_later_gen").await);
    oracle.fact("stale_cross_digest", "StaleGeneration");
    oracle.fact("stale_grant_events", "0");
}

async fn prove_legacy(admin: &mut Client, shadow: &Shadow, oracle: &mut Oracle) {
    let legacy_counter = acl(
        "legacy",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        COUNTER,
        &["counter_id"],
        false,
    );
    let legacy_slip = acl(
        "legacy",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        SLIP,
        &["slip_id"],
        false,
    );
    let first = install_column_acl(admin, &legacy_counter)
        .await
        .expect("legacy counter");
    assert_eq!(first.effect, InstallEffect::Installed);
    assert_eq!(
        first.physical_columns,
        vec![shadow.counter.counter_id.clone()]
    );
    let again = install_column_acl(admin, &legacy_counter)
        .await
        .expect("legacy replay");
    assert_eq!(again.effect, InstallEffect::Replay);
    assert_eq!(
        install_column_acl(admin, &legacy_slip).await,
        Err(PolicyError::GrantMismatch)
    );
    assert!(select_columns(admin, "oo_r_legacy", &shadow.slip.table)
        .await
        .is_empty());
    let before = grant_text(admin, "oo_r_legacy").await;
    assert_eq!(
        install_column_acl_set(admin, &[legacy_counter, legacy_slip]).await,
        Err(PolicyError::GrantMismatch)
    );
    assert_eq!(grant_text(admin, "oo_r_legacy").await, before);
    assert_eq!(
        select_columns(admin, "oo_r_legacy", &shadow.counter.table).await,
        BTreeSet::from([shadow.counter.counter_id.clone()])
    );
    oracle.fact("legacy_second_table", "GrantMismatch");
    oracle.fact("legacy_set_widen", "GrantMismatch");
    oracle.fact("legacy_counter_columns", "1");
}

async fn refuse_mutations(
    admin: &mut Client,
    shadow: &Shadow,
    scribe_grants: &str,
    glance_grants: &str,
    oracle: &mut Oracle,
) {
    let partial = vec![acl(
        "scribe",
        LOGIN,
        CONFIG,
        CURRENT_GENERATION,
        COUNTER,
        &["counter_id", "label", "token"],
        false,
    )];
    assert_eq!(
        install_column_acl_set(admin, &partial).await,
        Err(PolicyError::GrantMismatch)
    );
    oracle.fact("partial_refuse", "GrantMismatch");

    let mut changed = scribe_set();
    changed[0].table.columns.pop();
    assert_eq!(
        install_column_acl_set(admin, &changed).await,
        Err(PolicyError::GrantMismatch)
    );
    oracle.fact("changed_refuse", "GrantMismatch");

    let mut other_login = scribe_set();
    for policy in &mut other_login {
        policy.login = OTHER_LOGIN.to_string();
    }
    assert_eq!(
        install_column_acl_set(admin, &other_login).await,
        Err(PolicyError::MembershipMismatch)
    );
    oracle.fact("membership_refuse", "MembershipMismatch");
    assert_eq!(grant_text(admin, "oo_r_scribe").await, scribe_grants);
    assert_eq!(grant_text(admin, "oo_r_glance").await, glance_grants);

    let grant_sql = format!(
        "GRANT SELECT ({column}) ON TABLE \"_oo\".{table} TO \"oo_r_scribe\"",
        column = quote_physical(&shadow.counter.ratio, "pc_"),
        table = quote_physical(&shadow.counter.table, "pt_"),
    );
    exec_sql(admin, "extra column grant", &grant_sql).await;
    assert!(select_columns(admin, "oo_r_scribe", &shadow.counter.table)
        .await
        .contains(&shadow.counter.ratio));
    assert_eq!(
        install_column_acl_set(admin, &scribe_set()).await,
        Err(PolicyError::GrantMismatch)
    );
    assert!(
        select_columns(admin, "oo_r_scribe", &shadow.counter.table)
            .await
            .contains(&shadow.counter.ratio),
        "installer revoked the unexpected grant"
    );
    oracle.fact("unexpected_grant", "left_in_place");
    let revoke_sql = format!(
        "REVOKE SELECT ({column}) ON TABLE \"_oo\".{table} FROM \"oo_r_scribe\"",
        column = quote_physical(&shadow.counter.ratio, "pc_"),
        table = quote_physical(&shadow.counter.table, "pt_"),
    );
    exec_sql(admin, "review revoke after assert", &revoke_sql).await;
    assert!(!select_columns(admin, "oo_r_scribe", &shadow.counter.table)
        .await
        .contains(&shadow.counter.ratio));
    assert_eq!(grant_text(admin, "oo_r_scribe").await, scribe_grants);
    oracle.fact("grants_after_refuse", "unchanged");
}

async fn assert_catalog(admin: &Client, shadow: &Shadow, oracle: &mut Oracle) {
    for role in ["oo_r_scribe", "oo_r_glance"] {
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
             WHERE granted.rolname = 'oo_r_scribe'",
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
        select_columns(admin, "oo_r_glance", &shadow.counter.table).await,
        BTreeSet::from([
            shadow.counter.counter_id.clone(),
            shadow.counter.label.clone(),
        ])
    );
    assert_eq!(table_privilege_count(admin, "oo_r_scribe").await, 0);
    assert_eq!(table_privilege_count(admin, "oo_r_glance").await, 0);
    oracle.fact("table_privileges", "0");
    let allowed: Option<bool> = admin
        .query_one(
            "SELECT pg_catalog.has_column_privilege( \
                ($1::text)::name, relation.oid, '_oo_pos'::text, 'SELECT') \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $2",
            &[&"oo_r_scribe", &shadow.counter.table],
        )
        .await
        .expect("pos privilege")
        .get(0);
    assert_ne!(allowed, Some(true));
    oracle.fact("metadata_select", "false");
    let schema = admin
        .query_one(
            "SELECT pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'USAGE'), \
                    pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'CREATE')",
            &[&"oo_r_scribe"],
        )
        .await
        .expect("schema privilege");
    let usage: bool = schema.get(0);
    let create: bool = schema.get(1);
    assert!(usage);
    assert!(!create);
    oracle.fact("schema_usage", "usage_without_create");
}

async fn prove_post_grant_fault(
    admin: &mut Client,
    shadow: &Shadow,
    installed_events: i64,
    scribe_grants: &str,
    oracle: &mut Oracle,
) {
    let fail_at = i32::try_from(installed_events).expect("grant count fits");
    reset_events(admin).await;
    exec_sql(
        admin,
        "reset second-table marker",
        "SELECT setval('public.a229_second_hit', 1, false)",
    )
    .await;
    admin
        .execute(
            "UPDATE public.a229_grant_fault \
             SET enabled = true, fail_at = $1, second_table = $2 WHERE id = 1",
            &[&fail_at, &shadow.slip.table],
        )
        .await
        .expect("enable fault");
    let fault = fault_set();
    let error = install_column_acl_set(admin, &fault)
        .await
        .expect_err("post-grant fault");
    match error {
        PolicyError::Database { sqlstate, message } => {
            assert_eq!(sqlstate, "P0001", "{message}");
            assert_eq!(message, "db error", "{message}");
            oracle.fact("post_grant_fault", "raised");
            oracle.fact("post_grant_sqlstate", &sqlstate);
            oracle.fact("post_grant_product_message", "db_error");
        }
        other => panic!("expected the hook's database error, got {other:?}"),
    }
    admin
        .execute(
            "UPDATE public.a229_grant_fault SET enabled = false WHERE id = 1",
            &[],
        )
        .await
        .expect("disable fault");
    let seen = grant_events(admin).await;
    let hits = second_hits(admin).await;
    assert_eq!(seen, installed_events);
    assert!(seen >= 2, "fault did not follow an earlier grant");
    if hits == 1 {
        oracle.fact("post_grant_target", "second_table");
        oracle.fact("post_grant_second_hit", "1");
    } else {
        assert_eq!(hits, 0, "unexpected second-table marker {hits}");
        oracle.fact("post_grant_target", "unresolved");
        oracle.fact("post_grant_second_hit", "0");
    }
    assert!(!role_present(admin, "oo_r_fault").await);
    assert_eq!(privilege_count(admin, "oo_r_fault").await, 0);
    assert_eq!(grant_text(admin, "oo_r_scribe").await, scribe_grants);
    let probe: i32 = admin
        .query_one("SELECT 1", &[])
        .await
        .expect("admin session after rollback")
        .get(0);
    assert_eq!(probe, 1);
    oracle.fact("post_grant_events", &seen.to_string());
    oracle.fact("post_grant_events_match", "true");
    oracle.fact("post_grant_role", "absent");
    oracle.fact("post_grant_grants", "0");
    oracle.fact("post_grant_timing", "after_earlier_grants");
    oracle.fact("post_grant_not_pregrant", "true");
}

fn query_public_api(bans: &[String], oracle: &mut Oracle) {
    fs::create_dir_all(LIVE_DIR).expect("live dir");
    let scribe_sql = [
        "SELECT counter_id, label, token FROM depot.counter ORDER BY counter_id",
        "SELECT count(*) FROM depot.counter",
        "SELECT * FROM depot.counter ORDER BY counter_id",
        "SELECT slip_id, counter_id, body FROM depot.slip ORDER BY slip_id",
        "SELECT count(*) FROM depot.slip",
        "SELECT * FROM depot.slip ORDER BY slip_id",
    ];
    let scribe = run_query(
        "scribe",
        &scribe_sql,
        &scribe_json(),
        "scribe-audit.jsonl",
        bans,
    );
    assert_eq!(scribe.exit, 0, "scribe stderr={}", scribe.stderr);
    let turns = parse_turns(&scribe.stdout);
    assert_eq!(turns.len(), 6);
    assert_row(
        &turns[0],
        &[
            &["1", "北店", "vault-token"],
            &["2", "南店", "other-token"],
        ],
    );
    assert_count(&turns[1]);
    assert_row(
        &turns[2],
        &[
            &["1", "北店", "vault-token"],
            &["2", "南店", "other-token"],
        ],
    );
    assert_row(
        &turns[3],
        &[&["20", "1", "slip-a"], &["21", "2", "slip-b"]],
    );
    assert_count(&turns[4]);
    assert_row(
        &turns[5],
        &[&["20", "1", "slip-a"], &["21", "2", "slip-b"]],
    );
    oracle.fact("query_scribe_exit", "0");

    let glance_sql = [
        "SELECT counter_id, label FROM depot.counter ORDER BY counter_id",
        "SELECT count(*) FROM depot.counter",
        "SELECT * FROM depot.counter ORDER BY counter_id",
        "SELECT slip_id, counter_id FROM depot.slip ORDER BY slip_id",
        "SELECT count(*) FROM depot.slip",
        "SELECT * FROM depot.slip ORDER BY slip_id",
    ];
    let glance = run_query(
        "glance",
        &glance_sql,
        &glance_json(),
        "glance-audit.jsonl",
        bans,
    );
    assert_eq!(glance.exit, 0, "glance stderr={}", glance.stderr);
    let glance_text = String::from_utf8(glance.stdout.clone()).expect("glance utf8");
    for hidden in ["vault-token", "other-token", "slip-a", "slip-b"] {
        assert!(
            !glance_text.contains(hidden),
            "glance leaked {hidden}"
        );
    }
    let parsed = parse_turns(&glance.stdout);
    assert_row(&parsed[0], &[&["1", "北店"], &["2", "南店"]]);
    assert_count(&parsed[1]);
    assert_row(&parsed[2], &[&["1", "北店"], &["2", "南店"]]);
    assert_row(&parsed[3], &[&["20", "1"], &["21", "2"]]);
    assert_count(&parsed[4]);
    assert_row(&parsed[5], &[&["20", "1"], &["21", "2"]]);
    oracle.fact("query_glance_exit", "0");
    oracle.fact("glance_hidden", "absent");

    let guard = run_query(
        "glance",
        &["SELECT token FROM depot.counter"],
        &glance_json(),
        "glance-guard-audit.jsonl",
        bans,
    );
    assert_eq!(guard.exit, 6, "guard stderr={}", guard.stderr);
    assert!(guard.stderr.contains("query-pg: guard"));
    assert!(!Path::new(LIVE_DIR).join("glance-guard-audit.jsonl").exists());
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
            body.push_str(
                &serde_json::to_string(&json!({
                    "version": "oo.gateway.query-pg-turn/v1",
                    "sql": sql
                }))
                .expect("turn"),
            );
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
    if let Some(path) = option_env!("CARGO_BIN_EXE_query_pg") {
        return PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_query-pg") {
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
    let mut turns = Vec::new();
    let mut rows = Vec::new();
    for line in text.split('\n').filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_str(line).expect("stdout json");
        match value["record"].as_str() {
            Some("row") => {
                let cells = value["cells"].as_array().expect("cells");
                rows.push(cells.iter().map(cell_text).collect());
            }
            Some("summary") => {
                turns.push(Turn {
                    rows: std::mem::take(&mut rows),
                    status: value["status"].as_str().expect("status").to_string(),
                    returned: value["returned_rows"].as_u64().expect("returned"),
                });
                assert!(value["end_audit_ok"].as_bool().expect("audit flag"));
                assert!(!value["truncated"].as_bool().expect("truncated"));
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

fn scribe_json() -> Vec<Value> {
    vec![
        policy_json("scribe", COUNTER, &["counter_id", "label", "token"]),
        policy_json("scribe", SLIP, &["slip_id", "counter_id", "body"]),
    ]
}

fn glance_json() -> Vec<Value> {
    vec![
        policy_json("glance", COUNTER, &["counter_id", "label"]),
        policy_json("glance", SLIP, &["slip_id", "counter_id"]),
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

async fn direct_denials(client: &mut Client, shadow: &Shadow) {
    let counter = quote_physical(&shadow.counter.table, "pt_");
    let slip = quote_physical(&shadow.slip.table, "pt_");
    let counter_id = quote_physical(&shadow.counter.counter_id, "pc_");
    let token = quote_physical(&shadow.counter.token, "pc_");
    let ratio = quote_physical(&shadow.counter.ratio, "pc_");
    let body = quote_physical(&shadow.slip.body, "pc_");
    let allowed = query_as_role(
        client,
        "oo_r_scribe",
        &format!("SELECT {counter_id}::int4 FROM \"_oo\".{counter} ORDER BY {counter_id}::int4 LIMIT 1"),
    )
    .await;
    assert_eq!(allowed.get::<_, i32>(0), 1);
    expect_42501(
        client,
        Some("oo_r_glance"),
        &format!("SELECT {token}::text FROM \"_oo\".{counter}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_scribe"),
        &format!("SELECT \"_oo_pos\" FROM \"_oo\".{counter}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_scribe"),
        &format!("SELECT * FROM \"_oo\".{counter}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_glance"),
        &format!("SELECT * FROM \"_oo\".{slip}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_scribe"),
        &format!("SELECT {ratio}::float4 FROM \"_oo\".{counter}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_glance"),
        &format!("SELECT {body}::text FROM \"_oo\".{slip}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_scribe"),
        &format!("INSERT INTO \"_oo\".{counter} (\"_oo_pos\") VALUES (9)"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_scribe"),
        &format!("UPDATE \"_oo\".{slip} SET {body} = 'rewritten'"),
    )
    .await;
    expect_42501(
        client,
        None,
        &format!("SELECT {counter_id}::int4 FROM \"_oo\".{counter}"),
    )
    .await;
}

async fn reset_database(admin_pass: &str, ca: &[u8]) {
    let maintenance = connect_db("postgres", ADMIN, admin_pass, ca).await;
    exec_sql(
        &maintenance,
        "terminate database sessions",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    exec_sql(
        &maintenance,
        "drop database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    exec_sql(
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
                OR member_role.rolname IN ('oo_a229_login', 'oo_a229_other')\n\
           LOOP\n\
             EXECUTE format('REVOKE %I FROM %I', granted_name, member_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a229_login') THEN\n\
             DROP ROLE oo_a229_login;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a229_other') THEN\n\
             DROP ROLE oo_a229_other;\n\
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
    exec_sql(
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
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202629)",
    )
    .await;
    tx
}

async fn exec_sql(client: &Client, name: &str, sql: &str) {
    client.batch_execute(sql).await.unwrap_or_else(|err| {
        panic!("{name} failed: {}", db_detail(&err));
    });
}

fn db_detail(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.to_string())
        .unwrap_or_else(|| err.to_string())
}

async fn exec_named(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql).await.unwrap_or_else(|err| {
        panic!("{name} failed: {}", db_detail(&err));
    });
}

async fn create_login(client: &Client, name: &str, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(name
        .bytes()
        .all(|byte| byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()));
    client
        .batch_execute(&format!(
            "CREATE ROLE \"{name}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{name}\""
        ))
        .await
        .expect("create login");
}

struct CounterMap {
    table: String,
    counter_id: String,
    label: String,
    token: String,
    ratio: String,
}

struct SlipMap {
    table: String,
    slip_id: String,
    counter_id: String,
    body: String,
}

struct Shadow {
    counter: CounterMap,
    slip: SlipMap,
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
        register(
            client,
            &map_key(CONFIG, generation, "table", "t", COUNTER, ""),
        )
        .await;
        register(client, &map_key(CONFIG, generation, "table", "t", SLIP, "")).await;
        for column in ["counter_id", "label", "token", "ratio"] {
            register(
                client,
                &map_key(CONFIG, generation, "column", "c", COUNTER, column),
            )
            .await;
        }
        for column in ["slip_id", "counter_id", "body"] {
            register(
                client,
                &map_key(CONFIG, generation, "column", "c", SLIP, column),
            )
            .await;
        }
    }
    let counter_table = lookup(client, "table", COUNTER, "").await;
    let slip_table = lookup(client, "table", SLIP, "").await;
    let counter_cols = vec![
        (
            "counter_id".to_string(),
            lookup(client, "column", COUNTER, "counter_id").await,
        ),
        (
            "label".to_string(),
            lookup(client, "column", COUNTER, "label").await,
        ),
        (
            "token".to_string(),
            lookup(client, "column", COUNTER, "token").await,
        ),
        (
            "ratio".to_string(),
            lookup(client, "column", COUNTER, "ratio").await,
        ),
    ];
    let slip_cols = vec![
        (
            "slip_id".to_string(),
            lookup(client, "column", SLIP, "slip_id").await,
        ),
        (
            "counter_id".to_string(),
            lookup(client, "column", SLIP, "counter_id").await,
        ),
        (
            "body".to_string(),
            lookup(client, "column", SLIP, "body").await,
        ),
    ];
    let run_id = open_run(client, target, &counter_table, &slip_table).await;
    prepare_one(
        client,
        &PrepareRequest {
            target,
            run_id: &run_id,
            index: 0,
            table: COUNTER,
            idents: &["counter_id", "label", "token", "ratio"],
            oids: &[23, 25, 25, 700],
        },
    )
    .await;
    prepare_one(
        client,
        &PrepareRequest {
            target,
            run_id: &run_id,
            index: 1,
            table: SLIP,
            idents: &["slip_id", "counter_id", "body"],
            oids: &[23, 23, 25],
        },
    )
    .await;
    insert_rows(client, &counter_table, &counter_cols, &slip_table, &slip_cols).await;
    Shadow {
        counter: CounterMap {
            table: counter_table,
            counter_id: counter_cols[0].1.clone(),
            label: counter_cols[1].1.clone(),
            token: counter_cols[2].1.clone(),
            ratio: counter_cols[3].1.clone(),
        },
        slip: SlipMap {
            table: slip_table,
            slip_id: slip_cols[0].1.clone(),
            counter_id: slip_cols[1].1.clone(),
            body: slip_cols[2].1.clone(),
        },
    }
}

async fn open_run(client: &Client, target: i64, counter: &str, slip: &str) -> String {
    let schemas = vec![NAMESPACE.to_string(), NAMESPACE.to_string()];
    let tables = vec![COUNTER.to_string(), SLIP.to_string()];
    let physical = vec![counter.to_string(), slip.to_string()];
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

struct PrepareRequest<'a> {
    target: i64,
    run_id: &'a str,
    index: i32,
    table: &'a str,
    idents: &'a [&'a str],
    oids: &'a [i64],
}

async fn prepare_one(client: &Client, request: &PrepareRequest<'_>) {
    let run_owned = request.run_id.to_string();
    let names: Vec<String> = request.idents.iter().map(|name| (*name).to_string()).collect();
    let oid_values = request.oids.to_vec();
    let typmods = vec![-1_i32; request.idents.len()];
    let required = vec![true; request.idents.len()];
    let namespace = NAMESPACE.to_string();
    let table_name = request.table.to_string();
    let params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &request.target,
        &run_owned,
        &request.index,
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
    counter_table: &str,
    counter_cols: &[(String, String)],
    slip_table: &str,
    slip_cols: &[(String, String)],
) {
    let counter_sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {counter_id}, {label}, {token}, {ratio}) \
         VALUES (($1::text)::numeric, $2, $3, $4, ($5::text)::real)",
        table = quote_physical(counter_table, "pt_"),
        counter_id = quote_physical(&counter_cols[0].1, "pc_"),
        label = quote_physical(&counter_cols[1].1, "pc_"),
        token = quote_physical(&counter_cols[2].1, "pc_"),
        ratio = quote_physical(&counter_cols[3].1, "pc_"),
    );
    let slip_sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {slip_id}, {counter_id}, {body}) \
         VALUES (($1::text)::numeric, $2, $3, $4)",
        table = quote_physical(slip_table, "pt_"),
        slip_id = quote_physical(&slip_cols[0].1, "pc_"),
        counter_id = quote_physical(&slip_cols[1].1, "pc_"),
        body = quote_physical(&slip_cols[2].1, "pc_"),
    );
    let tx = client.transaction().await.expect("insert begin");
    let first: [&(dyn ToSql + Sync); 5] = [&"1", &1_i32, &"北店", &"vault-token", &"1.25"];
    let second: [&(dyn ToSql + Sync); 5] = [&"2", &2_i32, &"南店", &"other-token", &"2.25"];
    tx.execute(&counter_sql, &first).await.expect("insert counter 1");
    tx.execute(&counter_sql, &second)
        .await
        .expect("insert counter 2");
    let slip_a: [&(dyn ToSql + Sync); 4] = [&"1", &20_i32, &1_i32, &"slip-a"];
    let slip_b: [&(dyn ToSql + Sync); 4] = [&"2", &21_i32, &2_i32, &"slip-b"];
    tx.execute(&slip_sql, &slip_a).await.expect("insert slip 20");
    tx.execute(&slip_sql, &slip_b).await.expect("insert slip 21");
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

fn scribe_set() -> Vec<ColumnAclPolicy> {
    vec![
        acl(
            "scribe",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            COUNTER,
            &["counter_id", "label", "token"],
            false,
        ),
        acl(
            "scribe",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            SLIP,
            &["slip_id", "counter_id", "body"],
            false,
        ),
    ]
}

fn glance_set() -> Vec<ColumnAclPolicy> {
    vec![
        acl(
            "glance",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            COUNTER,
            &["counter_id", "label"],
            false,
        ),
        acl(
            "glance",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            SLIP,
            &["slip_id", "counter_id"],
            false,
        ),
    ]
}

fn fault_set() -> Vec<ColumnAclPolicy> {
    vec![
        acl(
            "fault",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            COUNTER,
            &["counter_id"],
            false,
        ),
        acl(
            "fault",
            LOGIN,
            CONFIG,
            CURRENT_GENERATION,
            SLIP,
            &["slip_id"],
            false,
        ),
    ]
}

fn orphan_set() -> Vec<ColumnAclPolicy> {
    vec![
        acl(
            "orphan",
            "oo_a229_missing",
            CONFIG,
            CURRENT_GENERATION,
            COUNTER,
            &["counter_id"],
            false,
        ),
        acl(
            "orphan",
            "oo_a229_missing",
            CONFIG,
            CURRENT_GENERATION,
            SLIP,
            &["slip_id"],
            false,
        ),
    ]
}

fn stale_set(generation: i64, role: &str) -> Vec<ColumnAclPolicy> {
    vec![
        acl(
            role,
            LOGIN,
            CONFIG,
            generation,
            COUNTER,
            &["counter_id"],
            false,
        ),
        acl(role, LOGIN, CONFIG, generation, SLIP, &["slip_id"], false),
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
            column: "counter_id".to_string(),
            claim: "counter".to_string(),
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
               AND privilege_type = 'SELECT'",
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

async fn mapping_select_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.table_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = 'physical_mappings' \
               AND privilege_type = 'SELECT'",
            &[&role],
        )
        .await
        .expect("mapping select")
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

async fn reset_events(client: &Client) {
    exec_sql(
        client,
        "reset grant counter",
        "SELECT setval('public.a229_grant_seen', 1, false)",
    )
    .await;
}

async fn grant_events(client: &Client) -> i64 {
    client
        .query_one(
            "SELECT CASE WHEN is_called THEN last_value ELSE 0 END FROM public.a229_grant_seen",
            &[],
        )
        .await
        .expect("grant events")
        .get(0)
}

async fn second_hits(client: &Client) -> i64 {
    client
        .query_one(
            "SELECT CASE WHEN is_called THEN last_value ELSE 0 END FROM public.a229_second_hit",
            &[],
        )
        .await
        .expect("second table marker")
        .get(0)
}

async fn drop_hook(client: &Client) {
    exec_sql(
        client,
        "drop grant hook",
        "DROP EVENT TRIGGER IF EXISTS a229_grant_fault; \
         DROP FUNCTION IF EXISTS public.a229_on_grant();",
    )
    .await;
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length");
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
        !role.is_empty()
            && role.bytes().all(|byte| {
                byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()
            })
    );
    format!("\"{role}\"")
}
