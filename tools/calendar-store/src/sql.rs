//! Fixed SQL. Identifiers are literals. Caller values are parameters.
//! Runtime role grants are built in `role` after the name grammar accepts them.

pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_calendar_control_v1.sql");

pub const SCHEMA: &str = "oo_calendar_control";
pub const DICT_SCHEMA: &str = "oo_dict";
pub const VIEW: &str = "calendar";
pub const INSTALL_LOCK: i64 = 881_275_289;

pub const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog; \
SET LOCAL lock_timeout = '10s'; \
SET LOCAL statement_timeout = '15s'; \
SET LOCAL synchronous_commit = on";

pub const LOCK_INSTALL: &str = "SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)";
pub const CURRENT_USER: &str = "SELECT current_user";
pub const TLS_FACT: &str = "\
SELECT ssl, version FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()";

pub const SCHEMA_EXISTS: &str = "SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1";
pub const DICT_RELATION: &str = "\
SELECT 1 FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'";

pub const INSERT_MANIFEST: &str = "\
INSERT INTO oo_calendar_control.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)";
pub const READ_MANIFEST: &str = "\
SELECT sql_sha256 FROM oo_calendar_control.install_manifest WHERE singleton = TRUE";
pub const INSERT_DEPLOYMENT: &str = "\
INSERT INTO oo_calendar_control.deployment ( \
  singleton, project_id, finance_approval, authenticated, ontology_published, clock_used \
) VALUES (TRUE, $1, FALSE, FALSE, FALSE, FALSE)";
pub const READ_DEPLOYMENT: &str = "\
SELECT project_id, finance_approval, authenticated, ontology_published, clock_used \
FROM oo_calendar_control.deployment WHERE singleton = TRUE";
pub const INSERT_ALLOW: &str = "INSERT INTO oo_calendar_control.runtime_allow (role_name) VALUES ($1)";
pub const READ_ALLOW: &str = "SELECT role_name FROM oo_calendar_control.runtime_allow ORDER BY role_name";
pub const INSERT_POINTER: &str = "\
INSERT INTO oo_calendar_control.active_pointer ( \
  singleton, project_id, snapshot_id, calendar_version, as_of \
) VALUES (TRUE, $1, NULL, NULL, NULL)";
pub const READ_POINTER: &str = "\
SELECT project_id, snapshot_id, calendar_version, as_of::text \
FROM oo_calendar_control.active_pointer WHERE singleton = TRUE FOR UPDATE";

pub const ROLE_ATTR: &str = "\
SELECT rolsuper, rolbypassrls, rolcanlogin, rolcreatedb, rolcreaterole \
FROM pg_catalog.pg_roles WHERE rolname = $1";
pub const RUNTIME_MEMBER: &str = "\
SELECT 1 FROM pg_catalog.pg_auth_members m \
JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid \
JOIN pg_catalog.pg_roles member ON member.oid = m.member \
WHERE (granted.rolname = 'oo_calendar_view' AND member.rolname = $1) \
   OR (granted.rolname = $1 AND member.rolname = 'oo_calendar_view')";
pub const ADMIN_MEMBER: &str = "\
SELECT 1 FROM pg_catalog.pg_auth_members m \
JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid \
JOIN pg_catalog.pg_roles member ON member.oid = m.member \
WHERE granted.rolname = 'oo_calendar_view' AND member.rolname = 'oo_calendar_admin'";

pub const RELATIONS: &str = "\
SELECT c.relname FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relkind = 'r' ORDER BY c.relname";
pub const COLUMNS: &str = "\
SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod) \
FROM pg_catalog.pg_attribute a \
JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY a.attnum";
pub const CONSTRAINTS: &str = "\
SELECT c.conname FROM pg_catalog.pg_constraint c \
JOIN pg_catalog.pg_class r ON r.oid = c.conrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = r.relnamespace \
WHERE n.nspname = $1 AND r.relname = $2 AND c.contype IN ('c', 'p', 'u', 'f') \
ORDER BY c.conname";
pub const SCHEMA_OWNER: &str = "\
SELECT pg_catalog.pg_get_userbyid(n.nspowner) FROM pg_catalog.pg_namespace n WHERE n.nspname = $1";
pub const VIEW_FACT: &str = "\
SELECT pg_catalog.pg_get_userbyid(c.relowner), COALESCE(c.reloptions::text, ''), c.relkind::text \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'";
pub const VIEW_DEF: &str = "SELECT pg_catalog.pg_get_viewdef('oo_dict.calendar'::regclass, true)";
pub const DICT_OBJECTS: &str = "\
SELECT c.relname, c.relkind::text FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'oo_dict' AND c.relkind IN ('r', 'v', 'm', 'p', 'f') \
ORDER BY c.relname";
pub const PUBLIC_SCHEMA: &str = "\
SELECT count(*) FROM pg_catalog.pg_namespace n \
CROSS JOIN LATERAL pg_catalog.aclexplode(n.nspacl) a \
WHERE n.nspname = $1 AND a.grantee = 0";
pub const PUBLIC_RELATION: &str = "\
SELECT c.relacl IS NOT NULL AND NOT EXISTS ( \
  SELECT 1 FROM pg_catalog.aclexplode(c.relacl) a WHERE a.grantee = 0 \
) FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2";
pub const VIEW_GRANTS: &str = "\
SELECT \
  pg_catalog.has_schema_privilege('oo_calendar_view', 'oo_calendar_control', 'USAGE'), \
  pg_catalog.has_table_privilege('oo_calendar_view', 'oo_calendar_control.published', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_calendar_view', 'oo_calendar_control.published', 'INSERT'), \
  pg_catalog.has_table_privilege('oo_calendar_view', 'oo_calendar_control.snapshot', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_calendar_view', 'oo_calendar_control.install_manifest', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_calendar_view', 'oo_calendar_control.deployment', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_calendar_view', 'oo_calendar_control.runtime_allow', 'SELECT'), \
  pg_catalog.has_column_privilege('oo_calendar_view', 'oo_calendar_control.active_pointer', 'snapshot_id', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_calendar_view', 'oo_calendar_control.active_pointer', 'UPDATE'), \
  pg_catalog.has_table_privilege('oo_calendar_view', 'oo_dict.calendar', 'SELECT'), \
  pg_catalog.has_schema_privilege('oo_calendar_view', 'oo_dict', 'USAGE'), \
  pg_catalog.has_schema_privilege('oo_calendar_view', 'oo_dict', 'CREATE')";
pub const CONTROL_GRANTS: &str = "\
SELECT \
  pg_catalog.has_schema_privilege($1, 'oo_calendar_control', 'USAGE'), \
  pg_catalog.has_table_privilege($1, 'oo_calendar_control.snapshot', 'SELECT'), \
  pg_catalog.has_table_privilege($1, 'oo_calendar_control.published', 'SELECT'), \
  pg_catalog.has_table_privilege($1, 'oo_calendar_control.deployment', 'SELECT'), \
  pg_catalog.has_table_privilege($1, 'oo_calendar_control.install_manifest', 'SELECT')";

pub const INSERT_SNAPSHOT: &str = "\
INSERT INTO oo_calendar_control.snapshot ( \
  snapshot_id, project_id, calendar_version, config_sha256, as_of, closure_fingerprint, \
  resolved_sha256, calendar_yaml, closure_date, closure_label, closure_record_sha256, \
  report_json, business_tz \
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";
pub const READ_SNAPSHOT: &str = "\
SELECT project_id, calendar_version, config_sha256, as_of, closure_fingerprint, \
       resolved_sha256, calendar_yaml, closure_date, closure_label, closure_record_sha256, \
       report_json, business_tz \
FROM oo_calendar_control.snapshot WHERE snapshot_id = $1 FOR UPDATE";
pub const INSERT_PUBLISHED: &str = "\
INSERT INTO oo_calendar_control.published ( \
  snapshot_id, project_id, business_tz, as_of, calendar_version, \
  today_date, today_midnight_status, today_midnight, \
  month_start_date, month_start_midnight_status, month_start_midnight, \
  prev_month_start_date, prev_month_start_midnight_status, prev_month_start_midnight, \
  prev_month_end_date, prev_month_end_midnight_status, prev_month_end_midnight, \
  quarter_start_date, quarter_start_midnight_status, quarter_start_midnight, \
  prev_quarter_start_date, prev_quarter_start_midnight_status, prev_quarter_start_midnight, \
  prev_quarter_end_date, prev_quarter_end_midnight_status, prev_quarter_end_midnight, \
  fiscal_year_start_date, fiscal_year_start_midnight_status, fiscal_year_start_midnight, \
  prev_fiscal_year_end_date, prev_fiscal_year_end_midnight_status, prev_fiscal_year_end_midnight, \
  last_closed_period_end_date, last_closed_period_end_midnight_status, last_closed_period_end_midnight \
) VALUES ( \
  $1, $2, $3, $4::text::timestamptz, $5, \
  $6::text::date, $7, $8::text::timestamptz, \
  $9::text::date, $10, $11::text::timestamptz, \
  $12::text::date, $13, $14::text::timestamptz, \
  $15::text::date, $16, $17::text::timestamptz, \
  $18::text::date, $19, $20::text::timestamptz, \
  $21::text::date, $22, $23::text::timestamptz, \
  $24::text::date, $25, $26::text::timestamptz, \
  $27::text::date, $28, $29::text::timestamptz, \
  $30::text::date, $31, $32::text::timestamptz, \
  $33::text::date, $34, $35::text::timestamptz \
)";
pub const ACTIVATE: &str = "\
UPDATE oo_calendar_control.active_pointer \
SET snapshot_id = $1, calendar_version = $2, as_of = $3::text::timestamptz \
WHERE singleton AND project_id = $4 AND snapshot_id IS NOT DISTINCT FROM $5::text";
pub const READ_VIEW: &str = "\
SELECT project_id, business_tz, \
       to_char(as_of AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       calendar_version, \
       today_date::text, today_midnight_status, \
       to_char(today_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       month_start_date::text, month_start_midnight_status, \
       to_char(month_start_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       prev_month_start_date::text, prev_month_start_midnight_status, \
       to_char(prev_month_start_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       prev_month_end_date::text, prev_month_end_midnight_status, \
       to_char(prev_month_end_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       quarter_start_date::text, quarter_start_midnight_status, \
       to_char(quarter_start_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       prev_quarter_start_date::text, prev_quarter_start_midnight_status, \
       to_char(prev_quarter_start_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       prev_quarter_end_date::text, prev_quarter_end_midnight_status, \
       to_char(prev_quarter_end_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       fiscal_year_start_date::text, fiscal_year_start_midnight_status, \
       to_char(fiscal_year_start_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       prev_fiscal_year_end_date::text, prev_fiscal_year_end_midnight_status, \
       to_char(prev_fiscal_year_end_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
       last_closed_period_end_date::text, last_closed_period_end_midnight_status, \
       to_char(last_closed_period_end_midnight AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
FROM oo_dict.calendar";

pub const EXPECTED_RELATIONS: [&str; 6] = [
    "active_pointer",
    "deployment",
    "install_manifest",
    "published",
    "runtime_allow",
    "snapshot",
];

pub const EXPECTED_MANIFEST_COLUMNS: [(&str, &str); 2] = [("singleton", "boolean"), ("sql_sha256", "text")];
pub const EXPECTED_DEPLOYMENT_COLUMNS: [(&str, &str); 6] = [
    ("singleton", "boolean"),
    ("project_id", "text"),
    ("finance_approval", "boolean"),
    ("authenticated", "boolean"),
    ("ontology_published", "boolean"),
    ("clock_used", "boolean"),
];
pub const EXPECTED_ALLOW_COLUMNS: [(&str, &str); 1] = [("role_name", "text")];
pub const EXPECTED_POINTER_COLUMNS: [(&str, &str); 5] = [
    ("singleton", "boolean"),
    ("project_id", "text"),
    ("snapshot_id", "text"),
    ("calendar_version", "text"),
    ("as_of", "timestamp with time zone"),
];
pub const EXPECTED_SNAPSHOT_COLUMNS: [(&str, &str); 13] = [
    ("snapshot_id", "text"),
    ("project_id", "text"),
    ("calendar_version", "text"),
    ("config_sha256", "text"),
    ("as_of", "text"),
    ("closure_fingerprint", "text"),
    ("resolved_sha256", "text"),
    ("calendar_yaml", "text"),
    ("closure_date", "text"),
    ("closure_label", "text"),
    ("closure_record_sha256", "text"),
    ("report_json", "text"),
    ("business_tz", "text"),
];

pub const EXPECTED_MANIFEST_CONSTRAINTS: [&str; 3] = [
    "install_manifest_one",
    "install_manifest_pk",
    "install_manifest_sha",
];
pub const EXPECTED_DEPLOYMENT_CONSTRAINTS: [&str; 4] = [
    "deployment_flags",
    "deployment_one",
    "deployment_pk",
    "deployment_project",
];
pub const EXPECTED_ALLOW_CONSTRAINTS: [&str; 2] = ["runtime_allow_name", "runtime_allow_pk"];
pub const EXPECTED_POINTER_CONSTRAINTS: [&str; 6] = [
    "active_pointer_one",
    "active_pointer_pair",
    "active_pointer_pk",
    "active_pointer_project",
    "active_pointer_snapshot",
    "active_pointer_version",
];
pub const EXPECTED_PUBLISHED_COLUMNS: [(&str, &str); 35] = [
    ("snapshot_id", "text"),
    ("project_id", "text"),
    ("business_tz", "text"),
    ("as_of", "timestamp with time zone"),
    ("calendar_version", "text"),
    ("today_date", "date"),
    ("today_midnight_status", "text"),
    ("today_midnight", "timestamp with time zone"),
    ("month_start_date", "date"),
    ("month_start_midnight_status", "text"),
    ("month_start_midnight", "timestamp with time zone"),
    ("prev_month_start_date", "date"),
    ("prev_month_start_midnight_status", "text"),
    ("prev_month_start_midnight", "timestamp with time zone"),
    ("prev_month_end_date", "date"),
    ("prev_month_end_midnight_status", "text"),
    ("prev_month_end_midnight", "timestamp with time zone"),
    ("quarter_start_date", "date"),
    ("quarter_start_midnight_status", "text"),
    ("quarter_start_midnight", "timestamp with time zone"),
    ("prev_quarter_start_date", "date"),
    ("prev_quarter_start_midnight_status", "text"),
    ("prev_quarter_start_midnight", "timestamp with time zone"),
    ("prev_quarter_end_date", "date"),
    ("prev_quarter_end_midnight_status", "text"),
    ("prev_quarter_end_midnight", "timestamp with time zone"),
    ("fiscal_year_start_date", "date"),
    ("fiscal_year_start_midnight_status", "text"),
    ("fiscal_year_start_midnight", "timestamp with time zone"),
    ("prev_fiscal_year_end_date", "date"),
    ("prev_fiscal_year_end_midnight_status", "text"),
    ("prev_fiscal_year_end_midnight", "timestamp with time zone"),
    ("last_closed_period_end_date", "date"),
    ("last_closed_period_end_midnight_status", "text"),
    ("last_closed_period_end_midnight", "timestamp with time zone"),
];

pub const EXPECTED_PUBLISHED_CONSTRAINTS: [&str; 25] = [
    "published_fiscal_year_start_shape",
    "published_fiscal_year_start_status",
    "published_last_closed_period_end_shape",
    "published_last_closed_period_end_status",
    "published_month_start_shape",
    "published_month_start_status",
    "published_pk",
    "published_prev_fiscal_year_end_shape",
    "published_prev_fiscal_year_end_status",
    "published_prev_month_end_shape",
    "published_prev_month_end_status",
    "published_prev_month_start_shape",
    "published_prev_month_start_status",
    "published_prev_quarter_end_shape",
    "published_prev_quarter_end_status",
    "published_prev_quarter_start_shape",
    "published_prev_quarter_start_status",
    "published_project",
    "published_quarter_start_shape",
    "published_quarter_start_status",
    "published_snapshot_fk",
    "published_today_shape",
    "published_today_status",
    "published_tz",
    "published_version",
];

pub const EXPECTED_SNAPSHOT_CONSTRAINTS: [&str; 11] = [
    "snapshot_as_of",
    "snapshot_closure",
    "snapshot_hashes",
    "snapshot_id_hex",
    "snapshot_identity_uq",
    "snapshot_json_bound",
    "snapshot_pk",
    "snapshot_project",
    "snapshot_tz",
    "snapshot_version",
    "snapshot_yaml_bound",
];

pub const EXPECTED_VIEW_GRANTS: [bool; 12] = [
    true, true, false, false, false, false, false, true, false, true, true, false,
];
pub const EXPECTED_RUNTIME_CONTROL: [bool; 5] = [false, false, false, false, false];

pub const VIEW_FORBIDDEN: [&str; 11] = [
    "now(",
    "current_date",
    "current_timestamp",
    "clock_timestamp",
    "date_trunc",
    "current_setting",
    "calendar_yaml",
    "closure_label",
    "report_json",
    "config_sha256",
    "record_sha256",
];

#[cfg(test)]
mod tests {
    use super::MIGRATION_SQL;

    #[test]
    fn migration_does_not_autofix_or_read_a_clock() {
        let lower = MIGRATION_SQL.to_ascii_lowercase();
        assert!(!lower.contains("if not exists"));
        assert!(!lower.contains("create or replace"));
        assert!(!lower.contains("drop "));
        assert!(!lower.contains("now("));
        assert!(!lower.contains("current_date"));
        assert!(!lower.contains("current_timestamp"));
        assert!(!lower.contains("clock_timestamp"));
        assert!(!lower.contains("date_trunc"));
        assert!(lower.contains("security_invoker = false"));
        assert!(lower.contains("security_barrier = true"));
        assert!(!MIGRATION_SQL.contains("GRANT ALL"));
        assert!(!MIGRATION_SQL.contains("GRANT INSERT"));
        assert!(!MIGRATION_SQL.contains("GRANT UPDATE"));
        assert!(!MIGRATION_SQL.contains("oo_r_"));
        let source = include_str!("sql.rs");
        let format_bang = ["format", "!"].concat();
        assert!(!source.contains(&format_bang));
    }
}
