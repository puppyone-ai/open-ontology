//! Fixture DDL and checkpoint rows. Schema files themselves are the actual migrations.

use crate::protocol::{hex_digest, AGENT_DB, CONTROL_DB};
use crate::sha256::{self, hex};
use crate::Fail;

pub const KEY_DDL: &str = "\
CREATE SCHEMA oo_key_fixture;
REVOKE ALL ON SCHEMA oo_key_fixture FROM PUBLIC;
CREATE TABLE oo_key_fixture.issued_key (
    entity_key text PRIMARY KEY,
    source_member text NOT NULL UNIQUE,
    issued_generation bigint NOT NULL,
    CONSTRAINT issued_key_generation CHECK (issued_generation >= 1),
    CONSTRAINT issued_key_shape CHECK (entity_key ~ '^k-[0-9]{4}$')
);
CREATE TABLE oo_key_fixture.negative_decision (
    left_key text NOT NULL REFERENCES oo_key_fixture.issued_key (entity_key),
    right_key text NOT NULL REFERENCES oo_key_fixture.issued_key (entity_key),
    decision text NOT NULL,
    operator_label text NOT NULL,
    CONSTRAINT negative_pk PRIMARY KEY (left_key, right_key),
    CONSTRAINT negative_order CHECK (left_key < right_key),
    CONSTRAINT negative_word CHECK (decision = 'negative')
);
CREATE TABLE oo_key_fixture.cut_marker (
    singleton boolean PRIMARY KEY,
    source_cut text NOT NULL,
    entry_count bigint NOT NULL,
    CONSTRAINT cut_marker_one CHECK (singleton),
    CONSTRAINT cut_marker_count CHECK (entry_count = 100000),
    CONSTRAINT cut_marker_cut CHECK (source_cut = 'cut-a356-1')
);
CREATE SEQUENCE oo_key_fixture.cut_seq AS bigint START WITH 41 INCREMENT BY 1;
REVOKE ALL ON ALL TABLES IN SCHEMA oo_key_fixture FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA oo_key_fixture FROM PUBLIC;
";

pub const SHOP_DDL: &str = "\
CREATE SCHEMA shop;
CREATE TABLE shop.party (
    party_key text PRIMARY KEY,
    display_name text,
    active boolean NOT NULL,
    phone text NOT NULL,
    national_id text NOT NULL
);
CREATE TABLE shop.account (
    account_id int8 PRIMARY KEY,
    party_key text NOT NULL,
    opened boolean NOT NULL,
    label text,
    CONSTRAINT account_party_fk FOREIGN KEY (party_key) REFERENCES shop.party (party_key)
);
CREATE TABLE shop.entry (
    entry_id int8 PRIMARY KEY,
    account_id int8 NOT NULL,
    qty int8 NOT NULL,
    amount numeric(18,4),
    memo text,
    CONSTRAINT entry_account_fk FOREIGN KEY (account_id) REFERENCES shop.account (account_id)
);
CREATE SEQUENCE shop.cut_seq AS bigint START WITH 41 INCREMENT BY 1;
COMMENT ON TABLE shop.party IS '甲注评名录';
REVOKE ALL ON SCHEMA shop FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA shop FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA shop FROM PUBLIC;
GRANT USAGE ON SCHEMA shop TO oo_a356_reader, oo_a356_writer;
GRANT SELECT ON shop.party, shop.account, shop.entry TO oo_a356_reader;
GRANT INSERT ON shop.party TO oo_a356_writer;
";

pub const ROLES: &[&str] = &[
    "CREATE ROLE oo_cdc_offset_admin NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
    "CREATE ROLE oo_cdc_offset_runtime NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
    "CREATE ROLE oo_calendar_view NOLOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
    "CREATE ROLE oo_session_runtime NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
    "CREATE ROLE oo_enum_view NOLOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
    "CREATE ROLE oo_a356_reader NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
    "CREATE ROLE oo_a356_writer NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
    "CREATE ROLE oo_a377_owner NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
    "CREATE ROLE oo_a377_operator NOLOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS",
];

pub const ASSIGN_OWNER: &str = "\
DO $oo$\n\
DECLARE r record;\n\
BEGIN\n\
  FOR r IN\n\
    SELECT n.nspname AS schema_name\n\
    FROM pg_catalog.pg_namespace n\n\
    WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'public')\n\
      AND n.nspname NOT LIKE 'pg_%'\n\
  LOOP\n\
    EXECUTE format('ALTER SCHEMA %I OWNER TO oo_a377_owner', r.schema_name);\n\
  END LOOP;\n\
  FOR r IN\n\
    SELECT n.nspname, c.relname, c.relkind\n\
    FROM pg_catalog.pg_class c\n\
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace\n\
    WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'public')\n\
      AND n.nspname NOT LIKE 'pg_%'\n\
      AND c.relkind IN ('r', 'p', 'v', 'm', 'S')\n\
  LOOP\n\
    IF r.relkind = 'S' THEN\n\
      EXECUTE format('ALTER SEQUENCE %I.%I OWNER TO oo_a377_owner', r.nspname, r.relname);\n\
    ELSIF r.relkind = 'v' THEN\n\
      EXECUTE format('ALTER VIEW %I.%I OWNER TO oo_a377_owner', r.nspname, r.relname);\n\
    ELSIF r.relkind = 'm' THEN\n\
      EXECUTE format('ALTER MATERIALIZED VIEW %I.%I OWNER TO oo_a377_owner', r.nspname, r.relname);\n\
    ELSE\n\
      EXECUTE format('ALTER TABLE %I.%I OWNER TO oo_a377_owner', r.nspname, r.relname);\n\
    END IF;\n\
  END LOOP;\n\
END\n\
$oo$;\n\
";

pub fn role_name(statement: &str) -> &str {
    statement.split_whitespace().nth(2).unwrap_or("")
}

fn digest(label: &str) -> String {
    hex(&sha256::hash(label.as_bytes()))
}

fn insert(table: &str, pairs: &[(&str, &str)]) -> String {
    let columns = pairs.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ");
    let values = pairs.iter().map(|(_, value)| *value).collect::<Vec<_>>().join(", ");
    format!("INSERT INTO {table} ({columns}) VALUES ({values});\n")
}

pub fn control_sql(cdc_hash: &str, session_hash: &str, calendar_hash: &str) -> Result<String, Fail> {
    let cdc_hash = hex_digest(cdc_hash)?;
    let session_hash = hex_digest(session_hash)?;
    let calendar_hash = hex_digest(calendar_hash)?;
    let config = digest("cdc-config-a356");
    let type_digest = digest("cdc-type-a356");
    let hook = digest("cdc-hook-a356");
    let schema = digest("cdc-schema-a356");
    let payload = digest("cdc-payload-a356");
    let receipt = digest("cdc-receipt-a356");
    let snapshot = digest("calendar-snapshot-a356");
    let closure = digest("calendar-closure-a356");
    let resolved = digest("calendar-resolved-a356");
    let record = digest("calendar-record-a356");
    let cdc_hash_lit = format!("'{cdc_hash}'");
    let config_lit = format!("'{config}'");
    let type_lit = format!("'{type_digest}'");
    let hook_lit = format!("'{hook}'");
    let schema_lit = format!("'{schema}'");
    let payload_lit = format!("'{payload}'");
    let receipt_lit = format!("'{receipt}'");
    let mut sql = String::new();
    sql.push_str(&insert(
        "oo_cdc_offset.install_manifest",
        &[("singleton", "true"), ("sql_sha256", cdc_hash_lit.as_str())],
    ));
    sql.push_str(&insert(
        "oo_cdc_offset.mirrored_receipt",
        &[
            ("source", "'pubfix'"),
            ("table_generation", "'shop_entry'"),
            ("txn_or_batch_id", "'txn-a356-1'"),
            ("config_version", "'config/v1'"),
            ("config_digest", config_lit.as_str()),
            ("type_policy_version", "'type-policy/v1'"),
            ("type_digest", type_lit.as_str()),
            ("hook_digest", hook_lit.as_str()),
            ("schema_digest", schema_lit.as_str()),
            ("payload_digest", payload_lit.as_str()),
            ("protocol_version", "'batch/v1'"),
            ("position_codec", "'batch-cursor-us/v1'"),
            ("position_lineage", "'lineage-a356'"),
            ("source_epoch", "1"),
            ("applied_pos", "100000"),
            ("partial_txn", "false"),
            ("cross_table_consistent", "true"),
            ("txn_complete", "true"),
            ("segment_index", "NULL"),
            ("segment_count", "NULL"),
            ("segment_bitmap", "''::bytea"),
            ("fence_shard", "'fence-a356'"),
            ("fence_generation", "1"),
            ("target_commit_at", "TIMESTAMPTZ '2026-04-01 00:00:00+00'"),
            ("receipt_hash", receipt_lit.as_str()),
        ],
    ));
    sql.push_str(&insert(
        "oo_cdc_offset.watermark",
        &[
            ("source", "'pubfix'"),
            ("position_codec", "'batch-cursor-us/v1'"),
            ("position_lineage", "'lineage-a356'"),
            ("source_epoch", "1"),
            ("applied_pos", "100000"),
            ("applied_txn_id", "'txn-a356-1'"),
            ("declared_next_txn", "'txn-a356-2'"),
            ("declared_next_pos", "100001"),
            ("binding_provenance", "'caller_declared_pending_real_normalizer'"),
        ],
    ));
    let session_hash_lit = format!("'{session_hash}'");
    sql.push_str(&insert(
        "oo_session_control.install_manifest",
        &[("singleton", "true"), ("sql_sha256", session_hash_lit.as_str())],
    ));
    sql.push_str(&insert(
        "oo_session_control.session_head",
        &[
            ("project_id", "'a356'"),
            ("session_id", "'sess-a356-1'"),
            ("revision", "3"),
            ("owner_generation", "1"),
            ("bound_project_id", "NULL"),
            ("bound_principal_id", "NULL"),
            ("bound_role", "NULL"),
            ("bound_claims_hash", "NULL"),
            ("bound_policy_version", "NULL"),
            ("bound_knowledge_version", "NULL"),
            ("bound_config_version", "NULL"),
            ("state_json", "'{\"state\":\"closed\"}'"),
            ("journal_len", "0"),
        ],
    ));
    let calendar_hash_lit = format!("'{calendar_hash}'");
    sql.push_str(&insert(
        "oo_calendar_control.install_manifest",
        &[("singleton", "true"), ("sql_sha256", calendar_hash_lit.as_str())],
    ));
    sql.push_str(&insert(
        "oo_calendar_control.deployment",
        &[
            ("singleton", "true"),
            ("project_id", "'a356'"),
            ("finance_approval", "false"),
            ("authenticated", "false"),
            ("ontology_published", "false"),
            ("clock_used", "false"),
        ],
    ));
    let snapshot_lit = format!("'{snapshot}'");
    let closure_lit = format!("'{closure}'");
    let resolved_lit = format!("'{resolved}'");
    let record_lit = format!("'{record}'");
    sql.push_str(&insert(
        "oo_calendar_control.snapshot",
        &[
            ("snapshot_id", snapshot_lit.as_str()),
            ("project_id", "'a356'"),
            ("calendar_version", "'cal-a356'"),
            ("config_sha256", config_lit.as_str()),
            ("as_of", "'2026-04-01T00:00:00Z'"),
            ("closure_fingerprint", closure_lit.as_str()),
            ("resolved_sha256", resolved_lit.as_str()),
            ("calendar_yaml", "'version: cal-a356\n'"),
            ("closure_date", "DATE '2026-03-31'"),
            ("closure_label", "'2026-03'"),
            ("closure_record_sha256", record_lit.as_str()),
            ("report_json", "'{\"closure\":\"2026-03\"}'"),
            ("business_tz", "'Asia/Shanghai'"),
        ],
    ));
    sql.push_str(&calendar_published(&snapshot));
    sql.push_str(&insert(
        "oo_calendar_control.active_pointer",
        &[
            ("singleton", "true"),
            ("project_id", "'a356'"),
            ("snapshot_id", snapshot_lit.as_str()),
            ("calendar_version", "'cal-a356'"),
            ("as_of", "TIMESTAMPTZ '2026-04-01 00:00:00+00'"),
        ],
    ));
    sql.push_str(KEY_DDL);
    sql.push_str(&insert(
        "oo_key_fixture.issued_key",
        &[
            ("entity_key", "'k-0001'"),
            ("source_member", "'member-a'"),
            ("issued_generation", "1"),
        ],
    ));
    sql.push_str(&insert(
        "oo_key_fixture.issued_key",
        &[
            ("entity_key", "'k-0002'"),
            ("source_member", "'member-b'"),
            ("issued_generation", "2"),
        ],
    ));
    sql.push_str(&insert(
        "oo_key_fixture.negative_decision",
        &[
            ("left_key", "'k-0001'"),
            ("right_key", "'k-0002'"),
            ("decision", "'negative'"),
            ("operator_label", "'public-fixture'"),
        ],
    ));
    sql.push_str(&insert(
        "oo_key_fixture.cut_marker",
        &[
            ("singleton", "true"),
            ("source_cut", "'cut-a356-1'"),
            ("entry_count", "100000"),
        ],
    ));
    let _ = (CONTROL_DB, AGENT_DB);
    Ok(sql)
}

fn calendar_published(snapshot: &str) -> String {
    let groups = [
        ("today", false),
        ("month_start", false),
        ("prev_month_start", false),
        ("prev_month_end", false),
        ("quarter_start", false),
        ("prev_quarter_start", false),
        ("prev_quarter_end", false),
        ("fiscal_year_start", false),
        ("prev_fiscal_year_end", false),
        ("last_closed_period_end", true),
    ];
    let mut pairs = vec![
        ("snapshot_id", format!("'{snapshot}'")),
        ("project_id", "'a356'".to_string()),
        ("business_tz", "'Asia/Shanghai'".to_string()),
        ("as_of", "TIMESTAMPTZ '2026-04-01 00:00:00+00'".to_string()),
        ("calendar_version", "'cal-a356'".to_string()),
    ];
    for (prefix, closed) in groups {
        if closed {
            pairs.push((prefix_name(prefix, "date"), "DATE '2026-03-31'".to_string()));
            pairs.push((prefix_name(prefix, "midnight_status"), "'unique'".to_string()));
            pairs.push((
                prefix_name(prefix, "midnight"),
                "TIMESTAMPTZ '2026-03-30 16:00:00+00'".to_string(),
            ));
        } else {
            pairs.push((prefix_name(prefix, "date"), "NULL".to_string()));
            pairs.push((prefix_name(prefix, "midnight_status"), "'absent'".to_string()));
            pairs.push((prefix_name(prefix, "midnight"), "NULL".to_string()));
        }
    }
    let borrowed: Vec<(&str, &str)> = pairs.iter().map(|(name, value)| (*name, value.as_str())).collect();
    insert("oo_calendar_control.published", &borrowed)
}

fn prefix_name(prefix: &'static str, suffix: &'static str) -> &'static str {
    match (prefix, suffix) {
        ("today", "date") => "today_date",
        ("today", "midnight_status") => "today_midnight_status",
        ("today", "midnight") => "today_midnight",
        ("month_start", "date") => "month_start_date",
        ("month_start", "midnight_status") => "month_start_midnight_status",
        ("month_start", "midnight") => "month_start_midnight",
        ("prev_month_start", "date") => "prev_month_start_date",
        ("prev_month_start", "midnight_status") => "prev_month_start_midnight_status",
        ("prev_month_start", "midnight") => "prev_month_start_midnight",
        ("prev_month_end", "date") => "prev_month_end_date",
        ("prev_month_end", "midnight_status") => "prev_month_end_midnight_status",
        ("prev_month_end", "midnight") => "prev_month_end_midnight",
        ("quarter_start", "date") => "quarter_start_date",
        ("quarter_start", "midnight_status") => "quarter_start_midnight_status",
        ("quarter_start", "midnight") => "quarter_start_midnight",
        ("prev_quarter_start", "date") => "prev_quarter_start_date",
        ("prev_quarter_start", "midnight_status") => "prev_quarter_start_midnight_status",
        ("prev_quarter_start", "midnight") => "prev_quarter_start_midnight",
        ("prev_quarter_end", "date") => "prev_quarter_end_date",
        ("prev_quarter_end", "midnight_status") => "prev_quarter_end_midnight_status",
        ("prev_quarter_end", "midnight") => "prev_quarter_end_midnight",
        ("fiscal_year_start", "date") => "fiscal_year_start_date",
        ("fiscal_year_start", "midnight_status") => "fiscal_year_start_midnight_status",
        ("fiscal_year_start", "midnight") => "fiscal_year_start_midnight",
        ("prev_fiscal_year_end", "date") => "prev_fiscal_year_end_date",
        ("prev_fiscal_year_end", "midnight_status") => "prev_fiscal_year_end_midnight_status",
        ("prev_fiscal_year_end", "midnight") => "prev_fiscal_year_end_midnight",
        ("last_closed_period_end", "date") => "last_closed_period_end_date",
        ("last_closed_period_end", "midnight_status") => "last_closed_period_end_midnight_status",
        ("last_closed_period_end", "midnight") => "last_closed_period_end_midnight",
        _ => "refused_column",
    }
}

pub fn enum_sql(enum_hash: &str) -> Result<String, Fail> {
    let enum_hash = hex_digest(enum_hash)?;
    let snapshot = digest("enum-snapshot-a356");
    let config = digest("enum-config-a356");
    let observations = digest("enum-observations-a356");
    let report = digest("enum-report-a356");
    let enum_hash_lit = format!("'{enum_hash}'");
    let snapshot_lit = format!("'{snapshot}'");
    let config_lit = format!("'{config}'");
    let observations_lit = format!("'{observations}'");
    let report_lit = format!("'{report}'");
    let mut sql = String::new();
    sql.push_str(&insert(
        "oo_enum_control.install_manifest",
        &[("singleton", "true"), ("sql_sha256", enum_hash_lit.as_str())],
    ));
    sql.push_str(&insert(
        "oo_enum_control.deployment",
        &[
            ("singleton", "true"),
            ("project_id", "'a356'"),
            ("finance_certified", "false"),
            ("semantic_certified", "false"),
            ("owner_authenticated", "false"),
            ("metric_certified", "false"),
            ("clock_used", "false"),
        ],
    ));
    sql.push_str(&insert(
        "oo_enum_control.snapshot",
        &[
            ("snapshot_id", snapshot_lit.as_str()),
            ("project_id", "'a356'"),
            ("enum_version", "'enum-a356'"),
            ("config_sha256", config_lit.as_str()),
            ("as_of", "DATE '2026-04-01'"),
            ("observations_sha256", observations_lit.as_str()),
            ("threshold_spelling", "'1/1'"),
            ("report_sha256", report_lit.as_str()),
            ("enum_yaml", "'id: enum-a356\n'"),
            ("observations_json", "'[]'"),
            ("report_json", "'{}'"),
            ("enum_id", "'shop.entry.status'"),
            ("observed_defined", "false"),
            ("coverage_numerator", "NULL"),
            ("coverage_denominator", "NULL"),
            ("coverage_ratio", "NULL"),
            ("threshold_decimal", "'1.0'"),
            ("threshold_numerator", "'1'"),
            ("threshold_denominator", "'1'"),
            ("threshold_defaulted", "true"),
            ("threshold_met", "NULL"),
            ("comparison", "NULL"),
            ("review_needed", "true"),
            ("warning_count", "0"),
            ("published_count", "1"),
            ("declared_coverage", "NULL"),
            ("declared_owner", "NULL"),
        ],
    ));
    let published = [
        ("snapshot_id", format!("'{snapshot}'")),
        ("line_no", "1".to_string()),
        ("row_kind", "'definition'".to_string()),
        ("enum_id", "'shop.entry.status'".to_string()),
        ("code", "'confirmed'".to_string()),
        ("description", "'Confirmed'".to_string()),
        ("class", "'business'".to_string()),
        ("valid_from", "DATE '2026-01-01'".to_string()),
        ("valid_to", "NULL".to_string()),
        ("version", "'enum-a356'".to_string()),
        ("as_of", "DATE '2026-04-01'".to_string()),
        ("project_id", "'a356'".to_string()),
        ("status", "'approved'".to_string()),
    ];
    let published_ref: Vec<(&str, &str)> = published.iter().map(|(n, v)| (*n, v.as_str())).collect();
    sql.push_str(&insert("oo_enum_control.published", &published_ref));
    let served = [
        ("line_no", "1".to_string()),
        ("snapshot_id", format!("'{snapshot}'")),
        ("enum_id", "'shop.entry.status'".to_string()),
        ("code", "'confirmed'".to_string()),
        ("description", "'Confirmed'".to_string()),
        ("class", "'business'".to_string()),
        ("valid_from", "DATE '2026-01-01'".to_string()),
        ("valid_to", "NULL".to_string()),
        ("version", "'enum-a356'".to_string()),
        ("as_of", "DATE '2026-04-01'".to_string()),
        ("project_id", "'a356'".to_string()),
        ("status", "'approved'".to_string()),
    ];
    let served_ref: Vec<(&str, &str)> = served.iter().map(|(n, v)| (*n, v.as_str())).collect();
    sql.push_str(&insert("oo_enum_control.served", &served_ref));
    sql.push_str(&insert(
        "oo_enum_control.active_pointer",
        &[
            ("singleton", "true"),
            ("project_id", "'a356'"),
            ("snapshot_id", snapshot_lit.as_str()),
            ("enum_version", "'enum-a356'"),
            ("as_of", "DATE '2026-04-01'"),
        ],
    ));
    Ok(sql)
}
