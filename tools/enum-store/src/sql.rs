//! Fixed SQL. Identifiers are literals. Caller values are parameters.
//! Runtime role grants are built in `role` after the name grammar accepts them.

pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_enum_control_v1.sql");

pub const SCHEMA: &str = "oo_enum_control";
pub const DICT_SCHEMA: &str = "oo_dict";
pub const VIEW: &str = "enums";
pub const INSTALL_LOCK: i64 = 881_275_293;
pub const DICT_LOCK: i64 = 881_275_280;
pub const ACCEPTED_MIGRATION_SHA256: &str = "b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd";
pub const SIBLING_MIGRATION_SHA256: &str = "ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520";
pub const SIBLING_ADMIN: &str = "oo_calendar_admin";
pub const SIBLING_VIEW: &str = "oo_calendar_view";
pub const SIBLING_SCHEMA: &str = "oo_calendar_control";
pub const SIBLING_RELATION: &str = "calendar";
pub const DICT_CREATE: &str = "CREATE SCHEMA oo_dict;\n";
pub const DICT_REVOKE_PUBLIC: &str = "REVOKE ALL ON SCHEMA oo_dict FROM PUBLIC;\n";
pub const DICT_GRANT_CREATE: &str = "GRANT USAGE, CREATE ON SCHEMA oo_dict TO oo_enum_view;\n";
pub const DICT_REVOKE_CREATE: &str = "REVOKE CREATE ON SCHEMA oo_dict FROM oo_enum_view;\n";
pub const ADOPT_OPEN: &str = "dict_create_open";
pub const ADOPT_CLOSE: &str = "dict_create_close";
pub const ADOPT_OPEN_BODY: &str = "GRANT USAGE, CREATE ON SCHEMA oo_dict TO oo_calendar_view";
pub const ADOPT_CLOSE_BODY: &str = "REVOKE CREATE ON SCHEMA oo_dict FROM oo_calendar_view";
pub const OUR_OPEN_REG: &str = "oo_enum_control.dict_create_open()";
pub const OUR_CLOSE_REG: &str = "oo_enum_control.dict_create_close()";
pub const SIBLING_OPEN_REG: &str = "oo_calendar_control.dict_create_open()";
pub const SIBLING_CLOSE_REG: &str = "oo_calendar_control.dict_create_close()";
pub const CALL_ADOPT_OPEN: &str = "SELECT oo_calendar_control.dict_create_open();\n";
pub const CALL_ADOPT_CLOSE: &str = "SELECT oo_calendar_control.dict_create_close();\n";
pub const CREATE_ADOPT_FUNCTIONS: &str = "\
CREATE FUNCTION oo_enum_control.dict_create_open() RETURNS void \
LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog \
AS $body$GRANT USAGE, CREATE ON SCHEMA oo_dict TO oo_calendar_view$body$; \
CREATE FUNCTION oo_enum_control.dict_create_close() RETURNS void \
LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog \
AS $body$REVOKE CREATE ON SCHEMA oo_dict FROM oo_calendar_view$body$; \
REVOKE ALL ON FUNCTION oo_enum_control.dict_create_open() FROM PUBLIC; \
REVOKE ALL ON FUNCTION oo_enum_control.dict_create_close() FROM PUBLIC";
pub const GRANT_ADOPT_EXECUTE: &str = "\
GRANT USAGE ON SCHEMA oo_enum_control TO oo_calendar_admin; \
GRANT SELECT ON TABLE oo_enum_control.install_manifest TO oo_calendar_admin; \
GRANT SELECT ON TABLE oo_enum_control.runtime_allow TO oo_calendar_admin; \
GRANT EXECUTE ON FUNCTION oo_enum_control.dict_create_open() TO oo_calendar_admin; \
GRANT EXECUTE ON FUNCTION oo_enum_control.dict_create_close() TO oo_calendar_admin";
pub const RUNTIME_GRANT_BODY: &str = "\
BEGIN IF runtime_role IS NULL OR runtime_role !~ '^[a-z_][a-z0-9_]{0,62}$' THEN RAISE EXCEPTION 'runtime role' USING ERRCODE = '42501'; END IF; PERFORM 1 FROM oo_calendar_control.runtime_allow WHERE role_name = runtime_role; IF NOT FOUND THEN RAISE EXCEPTION 'runtime role' USING ERRCODE = '42501'; END IF; EXECUTE format('GRANT USAGE ON SCHEMA oo_dict TO %I', runtime_role); END";
pub const SIBLING_RUNTIME_GRANT_BODY: &str = "\
BEGIN IF runtime_role IS NULL OR runtime_role !~ '^[a-z_][a-z0-9_]{0,62}$' THEN RAISE EXCEPTION 'runtime role' USING ERRCODE = '42501'; END IF; PERFORM 1 FROM oo_enum_control.runtime_allow WHERE role_name = runtime_role; IF NOT FOUND THEN RAISE EXCEPTION 'runtime role' USING ERRCODE = '42501'; END IF; EXECUTE format('GRANT USAGE ON SCHEMA oo_dict TO %I', runtime_role); END";
pub const CREATE_RUNTIME_GRANT_PREFIX: &str = "\
CREATE FUNCTION oo_enum_control.dict_grant_runtime(runtime_role text) RETURNS void \
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $body$";
pub const CREATE_RUNTIME_GRANT_SUFFIX: &str = "\
$body$; REVOKE ALL ON FUNCTION oo_enum_control.dict_grant_runtime(text) FROM PUBLIC; \
GRANT EXECUTE ON FUNCTION oo_enum_control.dict_grant_runtime(text) TO oo_calendar_admin";
pub const CALL_GRANT_RUNTIME: &str = "SELECT oo_calendar_control.dict_grant_runtime($1)";
pub const OUR_RUNTIME_REG: &str = "oo_enum_control.dict_grant_runtime(text)";
pub const SIBLING_RUNTIME_REG: &str = "oo_calendar_control.dict_grant_runtime(text)";
pub const RUNTIME_GRANT_FACT: &str = "\
SELECT pg_catalog.pg_get_userbyid(p.proowner), p.prosecdef, p.prosrc, p.proconfig, \
       l.lanname = 'plpgsql', \
       (SELECT count(*) FROM pg_catalog.aclexplode(COALESCE(p.proacl, pg_catalog.acldefault('f', p.proowner))) a WHERE a.grantee = 0) \
FROM pg_catalog.pg_proc p \
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
JOIN pg_catalog.pg_language l ON l.oid = p.prolang \
WHERE n.nspname = $1 AND p.proname = 'dict_grant_runtime' \
  AND pg_catalog.pg_get_function_identity_arguments(p.oid) = 'runtime_role text'";

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
WHERE n.nspname = 'oo_dict' AND c.relname = 'enums'";

pub const INSERT_MANIFEST: &str = "\
INSERT INTO oo_enum_control.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)";
pub const READ_MANIFEST: &str = "\
SELECT sql_sha256 FROM oo_enum_control.install_manifest WHERE singleton = TRUE";
pub const INSERT_DEPLOYMENT: &str = "\
INSERT INTO oo_enum_control.deployment ( \
  singleton, project_id, finance_certified, semantic_certified, owner_authenticated, metric_certified, clock_used \
) VALUES (TRUE, $1, FALSE, FALSE, FALSE, FALSE, FALSE)";
pub const READ_DEPLOYMENT: &str = "\
SELECT project_id, finance_certified, semantic_certified, owner_authenticated, metric_certified, clock_used \
FROM oo_enum_control.deployment WHERE singleton = TRUE";
pub const INSERT_ALLOW: &str = "INSERT INTO oo_enum_control.runtime_allow (role_name) VALUES ($1)";
pub const READ_ALLOW: &str = "SELECT role_name FROM oo_enum_control.runtime_allow ORDER BY role_name";
pub const SIBLING_READ_ALLOW: &str = "SELECT role_name FROM oo_calendar_control.runtime_allow";
pub const ALLOW_SELECT: &str = "\
SELECT pg_catalog.has_table_privilege($1, 'oo_enum_control.runtime_allow', 'SELECT')";
pub const SIBLING_ALLOW_SELECT: &str = "\
SELECT pg_catalog.has_table_privilege($1, 'oo_calendar_control.runtime_allow', 'SELECT')";
pub const INSERT_POINTER: &str = "\
INSERT INTO oo_enum_control.active_pointer ( \
  singleton, project_id, snapshot_id, enum_version, as_of \
) VALUES (TRUE, $1, NULL, NULL, NULL)";
pub const READ_POINTER: &str = "\
SELECT project_id, snapshot_id, enum_version, to_char(as_of, 'YYYY-MM-DD') \
FROM oo_enum_control.active_pointer WHERE singleton = TRUE FOR UPDATE";

pub const ROLE_ATTR: &str = "\
SELECT rolsuper, rolbypassrls, rolcanlogin, rolcreatedb, rolcreaterole \
FROM pg_catalog.pg_roles WHERE rolname = $1";
pub const RUNTIME_MEMBER: &str = "\
SELECT 1 FROM pg_catalog.pg_auth_members m \
JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid \
JOIN pg_catalog.pg_roles member ON member.oid = m.member \
WHERE (granted.rolname = 'oo_enum_view' AND member.rolname = $1) \
   OR (granted.rolname = $1 AND member.rolname = 'oo_enum_view')";
pub const ADMIN_MEMBER: &str = "\
SELECT 1 FROM pg_catalog.pg_auth_members m \
JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid \
JOIN pg_catalog.pg_roles member ON member.oid = m.member \
WHERE granted.rolname = 'oo_enum_view' AND member.rolname = 'oo_enum_admin'";

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
WHERE n.nspname = 'oo_dict' AND c.relname = 'enums'";
pub const VIEW_DEF: &str = "SELECT pg_catalog.pg_get_viewdef('oo_dict.enums'::regclass, true)";
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
  pg_catalog.has_schema_privilege('oo_enum_view', 'oo_enum_control', 'USAGE'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.served', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.served', 'INSERT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.served', 'UPDATE'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.published', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.snapshot', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.notice', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.install_manifest', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.deployment', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.runtime_allow', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_enum_control.active_pointer', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_enum_view', 'oo_dict.enums', 'SELECT'), \
  pg_catalog.has_schema_privilege('oo_enum_view', 'oo_dict', 'USAGE'), \
  pg_catalog.has_schema_privilege('oo_enum_view', 'oo_dict', 'CREATE')";
pub const CONTROL_GRANTS: &str = "\
SELECT \
  pg_catalog.has_schema_privilege($1, 'oo_enum_control', 'USAGE'), \
  pg_catalog.has_table_privilege($1, 'oo_enum_control.snapshot', 'SELECT'), \
  pg_catalog.has_table_privilege($1, 'oo_enum_control.published', 'SELECT'), \
  pg_catalog.has_table_privilege($1, 'oo_enum_control.notice', 'SELECT'), \
  pg_catalog.has_table_privilege($1, 'oo_enum_control.served', 'SELECT'), \
  pg_catalog.has_table_privilege($1, 'oo_enum_control.deployment', 'SELECT'), \
  pg_catalog.has_table_privilege($1, 'oo_enum_control.install_manifest', 'SELECT')";

pub const INSERT_SNAPSHOT: &str = "\
INSERT INTO oo_enum_control.snapshot ( \
  snapshot_id, project_id, enum_version, config_sha256, as_of, observations_sha256, \
  threshold_spelling, report_sha256, enum_yaml, observations_json, report_json, enum_id, \
  observed_defined, coverage_numerator, coverage_denominator, coverage_ratio, \
  threshold_decimal, threshold_numerator, threshold_denominator, threshold_defaulted, \
  threshold_met, comparison, review_needed, warning_count, published_count, \
  declared_coverage, declared_owner \
) VALUES ( \
  $1, $2, $3, $4, $5::text::date, $6, \
  $7, $8, $9, $10, $11, $12, \
  $13, $14, $15, $16, \
  $17, $18, $19, $20, \
  $21, $22, $23, $24, $25, \
  $26, $27 \
)";
pub const READ_SNAPSHOT: &str = "\
SELECT project_id, enum_version, config_sha256, to_char(as_of, 'YYYY-MM-DD'), \
       observations_sha256, threshold_spelling, report_sha256, enum_yaml, observations_json, \
       report_json, enum_id, observed_defined, coverage_numerator, coverage_denominator, \
       coverage_ratio, threshold_decimal, threshold_numerator, threshold_denominator, \
       threshold_defaulted, threshold_met, comparison, review_needed, warning_count, \
       published_count, declared_coverage, declared_owner \
FROM oo_enum_control.snapshot WHERE snapshot_id = $1 FOR UPDATE";
pub const READ_IDENTITY: &str = "\
SELECT snapshot_id FROM oo_enum_control.snapshot \
WHERE project_id = $1 AND enum_version = $2 AND config_sha256 = $3 \
  AND as_of = $4::text::date AND observations_sha256 = $5 AND threshold_spelling = $6";
pub const INSERT_NOTICE: &str = "\
INSERT INTO oo_enum_control.notice ( \
  snapshot_id, notice_no, code, kind, reason, count_text, source_index \
) VALUES ($1, $2, $3, $4, $5, $6, $7)";
pub const READ_NOTICES: &str = "\
SELECT notice_no, code, kind, reason, count_text, source_index \
FROM oo_enum_control.notice WHERE snapshot_id = $1 ORDER BY notice_no";
pub const INSERT_PUBLISHED: &str = "\
INSERT INTO oo_enum_control.published ( \
  snapshot_id, line_no, row_kind, enum_id, code, description, class, \
  valid_from, valid_to, version, as_of, project_id, status \
) VALUES ( \
  $1, $2, $3, $4, $5, $6, $7, \
  $8::text::date, $9::text::date, $10, $11::text::date, $12, $13 \
)";
pub const READ_PUBLISHED: &str = "\
SELECT line_no, row_kind, enum_id, code, description, class, \
       to_char(valid_from, 'YYYY-MM-DD'), to_char(valid_to, 'YYYY-MM-DD'), \
       version, to_char(as_of, 'YYYY-MM-DD'), project_id, status \
FROM oo_enum_control.published WHERE snapshot_id = $1 ORDER BY line_no";
pub const CLEAR_SERVED: &str = "DELETE FROM oo_enum_control.served";
pub const INSERT_SERVED: &str = "\
INSERT INTO oo_enum_control.served ( \
  line_no, snapshot_id, enum_id, code, description, class, \
  valid_from, valid_to, version, as_of, project_id, status \
) \
SELECT line_no, snapshot_id, enum_id, code, description, class, \
       valid_from, valid_to, version, as_of, project_id, status \
FROM oo_enum_control.published WHERE snapshot_id = $1";
pub const COUNT_SERVED: &str = "SELECT count(*) FROM oo_enum_control.served";
pub const COUNT_PUBLISHED: &str = "SELECT count(*) FROM oo_enum_control.published WHERE snapshot_id = $1";
pub const ACTIVATE: &str = "\
UPDATE oo_enum_control.active_pointer \
SET snapshot_id = $1, enum_version = $2, as_of = $3::text::date \
WHERE singleton AND project_id = $4 AND snapshot_id IS NOT DISTINCT FROM $5::text";
pub const READ_VIEW: &str = "\
SELECT enum_id, code, description, class, \
       to_char(valid_from, 'YYYY-MM-DD'), to_char(valid_to, 'YYYY-MM-DD'), \
       version, to_char(as_of, 'YYYY-MM-DD'), project_id, status \
FROM oo_dict.enums \
ORDER BY code, valid_from NULLS FIRST, valid_to NULLS LAST, status, description NULLS FIRST";

pub const EXPECTED_RELATIONS: [&str; 8] = [
    "active_pointer",
    "deployment",
    "install_manifest",
    "notice",
    "published",
    "runtime_allow",
    "served",
    "snapshot",
];

pub const EXPECTED_MANIFEST_COLUMNS: [(&str, &str); 2] = [("singleton", "boolean"), ("sql_sha256", "text")];
pub const EXPECTED_DEPLOYMENT_COLUMNS: [(&str, &str); 7] = [
    ("singleton", "boolean"),
    ("project_id", "text"),
    ("finance_certified", "boolean"),
    ("semantic_certified", "boolean"),
    ("owner_authenticated", "boolean"),
    ("metric_certified", "boolean"),
    ("clock_used", "boolean"),
];
pub const EXPECTED_ALLOW_COLUMNS: [(&str, &str); 1] = [("role_name", "text")];
pub const EXPECTED_POINTER_COLUMNS: [(&str, &str); 5] = [
    ("singleton", "boolean"),
    ("project_id", "text"),
    ("snapshot_id", "text"),
    ("enum_version", "text"),
    ("as_of", "date"),
];
pub const EXPECTED_SNAPSHOT_COLUMNS: [(&str, &str); 27] = [
    ("snapshot_id", "text"),
    ("project_id", "text"),
    ("enum_version", "text"),
    ("config_sha256", "text"),
    ("as_of", "date"),
    ("observations_sha256", "text"),
    ("threshold_spelling", "text"),
    ("report_sha256", "text"),
    ("enum_yaml", "text"),
    ("observations_json", "text"),
    ("report_json", "text"),
    ("enum_id", "text"),
    ("observed_defined", "boolean"),
    ("coverage_numerator", "text"),
    ("coverage_denominator", "text"),
    ("coverage_ratio", "text"),
    ("threshold_decimal", "text"),
    ("threshold_numerator", "text"),
    ("threshold_denominator", "text"),
    ("threshold_defaulted", "boolean"),
    ("threshold_met", "boolean"),
    ("comparison", "text"),
    ("review_needed", "boolean"),
    ("warning_count", "integer"),
    ("published_count", "integer"),
    ("declared_coverage", "text"),
    ("declared_owner", "text"),
];
pub const EXPECTED_NOTICE_COLUMNS: [(&str, &str); 7] = [
    ("snapshot_id", "text"),
    ("notice_no", "integer"),
    ("code", "text"),
    ("kind", "text"),
    ("reason", "text"),
    ("count_text", "text"),
    ("source_index", "integer"),
];
pub const EXPECTED_PUBLISHED_COLUMNS: [(&str, &str); 13] = [
    ("snapshot_id", "text"),
    ("line_no", "integer"),
    ("row_kind", "text"),
    ("enum_id", "text"),
    ("code", "text"),
    ("description", "text"),
    ("class", "text"),
    ("valid_from", "date"),
    ("valid_to", "date"),
    ("version", "text"),
    ("as_of", "date"),
    ("project_id", "text"),
    ("status", "text"),
];
pub const EXPECTED_SERVED_COLUMNS: [(&str, &str); 12] = [
    ("line_no", "integer"),
    ("snapshot_id", "text"),
    ("enum_id", "text"),
    ("code", "text"),
    ("description", "text"),
    ("class", "text"),
    ("valid_from", "date"),
    ("valid_to", "date"),
    ("version", "text"),
    ("as_of", "date"),
    ("project_id", "text"),
    ("status", "text"),
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
pub const EXPECTED_SNAPSHOT_CONSTRAINTS: [&str; 14] = [
    "snapshot_as_of",
    "snapshot_coverage",
    "snapshot_declared",
    "snapshot_enum_id",
    "snapshot_hashes",
    "snapshot_id_hex",
    "snapshot_identity_uq",
    "snapshot_json_bound",
    "snapshot_obs_bound",
    "snapshot_pk",
    "snapshot_project",
    "snapshot_threshold",
    "snapshot_version",
    "snapshot_yaml_bound",
];
pub const EXPECTED_NOTICE_CONSTRAINTS: [&str; 7] = [
    "notice_code",
    "notice_count",
    "notice_kind",
    "notice_no",
    "notice_pk",
    "notice_reason",
    "notice_snapshot_fk",
];
pub const EXPECTED_PUBLISHED_CONSTRAINTS: [&str; 10] = [
    "published_code",
    "published_enum_id",
    "published_kind",
    "published_line",
    "published_pk",
    "published_project",
    "published_snapshot_fk",
    "published_status",
    "published_unknown_shape",
    "published_version",
];
pub const EXPECTED_SERVED_CONSTRAINTS: [&str; 8] = [
    "served_code",
    "served_enum_id",
    "served_pk",
    "served_project",
    "served_published_fk",
    "served_snapshot",
    "served_status",
    "served_version",
];

pub const EXPECTED_VIEW_GRANTS: [bool; 14] = [
    true, true, false, false, false, false, false, false, false, false, false, true, true, false,
];
pub const EXPECTED_RUNTIME_CONTROL: [bool; 7] = [false, false, false, false, false, false, false];

pub const SIBLING_MANIFEST: &str = "\
SELECT sql_sha256 FROM oo_calendar_control.install_manifest WHERE singleton = TRUE";
pub const FUNCTION_FACT: &str = "\
SELECT pg_catalog.pg_get_userbyid(p.proowner), p.prosecdef, p.prosrc, p.proconfig \
FROM pg_catalog.pg_proc p \
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
WHERE n.nspname = $1 AND p.proname = $2 \
  AND pg_catalog.pg_get_function_identity_arguments(p.oid) = ''";
pub const FUNCTION_PUBLIC: &str = "\
SELECT count(*) FROM pg_catalog.pg_proc p \
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(p.proacl, pg_catalog.acldefault('f', p.proowner))) a \
WHERE n.nspname = $1 AND p.proname = $2 \
  AND pg_catalog.pg_get_function_identity_arguments(p.oid) = '' \
  AND a.grantee = 0";
pub const DICT_NONOWNER_GRANTS: &str = "\
SELECT r.rolname, a.privilege_type FROM pg_catalog.pg_namespace n \
CROSS JOIN LATERAL pg_catalog.aclexplode(n.nspacl) a \
JOIN pg_catalog.pg_roles r ON r.oid = a.grantee \
WHERE n.nspname = 'oo_dict' \
  AND r.rolname <> 'oo_calendar_view' \
  AND r.rolname <> 'oo_enum_view' \
  AND r.oid <> n.nspowner";
pub const SIBLING_VIEW_FACT: &str = "\
SELECT pg_catalog.pg_get_userbyid(c.relowner), COALESCE(c.reloptions::text, ''), c.relkind::text \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'";
pub const SIBLING_VIEW_DEF: &str = "\
SELECT pg_catalog.pg_get_viewdef(c.oid, true) \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'";
pub const SIBLING_COLUMNS_SQL: &str = "\
SELECT a.attname FROM pg_catalog.pg_attribute a \
JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar' AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY a.attnum";
pub const SIBLING_MEMBER: &str = "\
SELECT 1 FROM pg_catalog.pg_auth_members m \
JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid \
JOIN pg_catalog.pg_roles member ON member.oid = m.member \
WHERE granted.rolname = 'oo_calendar_view' AND member.rolname = 'oo_calendar_admin'";
pub const SIBLING_REVERSE_MEMBER: &str = "\
SELECT 1 FROM pg_catalog.pg_auth_members m \
JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid \
JOIN pg_catalog.pg_roles member ON member.oid = m.member \
WHERE granted.rolname = 'oo_calendar_admin' AND member.rolname = 'oo_calendar_view'";
pub const SCHEMA_USAGE: &str = "SELECT pg_catalog.has_schema_privilege($1, $2, 'USAGE')";
pub const FUNCTION_EXECUTE: &str = "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')";
pub const SIBLING_VIEW_PRIV: &str = "\
SELECT \
  pg_catalog.has_schema_privilege('oo_calendar_view', 'oo_dict', 'USAGE'), \
  pg_catalog.has_schema_privilege('oo_calendar_view', 'oo_dict', 'CREATE')";
pub const SIBLING_COLUMN_NAMES: [&str; 34] = [
    "project_id",
    "business_tz",
    "as_of",
    "calendar_version",
    "today_date",
    "today_midnight_status",
    "today_midnight",
    "month_start_date",
    "month_start_midnight_status",
    "month_start_midnight",
    "prev_month_start_date",
    "prev_month_start_midnight_status",
    "prev_month_start_midnight",
    "prev_month_end_date",
    "prev_month_end_midnight_status",
    "prev_month_end_midnight",
    "quarter_start_date",
    "quarter_start_midnight_status",
    "quarter_start_midnight",
    "prev_quarter_start_date",
    "prev_quarter_start_midnight_status",
    "prev_quarter_start_midnight",
    "prev_quarter_end_date",
    "prev_quarter_end_midnight_status",
    "prev_quarter_end_midnight",
    "fiscal_year_start_date",
    "fiscal_year_start_midnight_status",
    "fiscal_year_start_midnight",
    "prev_fiscal_year_end_date",
    "prev_fiscal_year_end_midnight_status",
    "prev_fiscal_year_end_midnight",
    "last_closed_period_end_date",
    "last_closed_period_end_midnight_status",
    "last_closed_period_end_midnight",
];
pub const SIBLING_VIEW_NEEDLES: [&str; 2] = ["oo_calendar_control.published", "oo_calendar_control.active_pointer"];
pub const SIBLING_VIEW_FORBIDDEN: [&str; 11] = [
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

pub const VIEW_FORBIDDEN: [&str; 14] = [
    "now(",
    "current_date",
    "current_timestamp",
    "clock_timestamp",
    "date_trunc",
    "current_setting",
    "statement_timestamp",
    "transaction_timestamp",
    "timeofday",
    "enum_yaml",
    "report_json",
    "row_kind",
    "config_sha256",
    "observations_json",
];

#[cfg(test)]
mod tests {
    use super::{ACCEPTED_MIGRATION_SHA256, MIGRATION_SQL, SIBLING_VIEW_DEF};

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
        assert!(!lower.contains("current_setting"));
        assert!(lower.contains("security_invoker = false"));
        assert!(lower.contains("security_barrier = true"));
        assert!(!MIGRATION_SQL.contains("GRANT ALL"));
        assert!(!MIGRATION_SQL.contains("GRANT INSERT"));
        assert!(!MIGRATION_SQL.contains("GRANT UPDATE"));
        assert!(!MIGRATION_SQL.contains("oo_r_"));
        let source = include_str!("sql.rs");
        let format_bang = ["format", "!"].concat();
        assert!(!source.contains(&format_bang));
        assert_eq!(ACCEPTED_MIGRATION_SHA256.len(), 64);
        assert!(source.contains("r.oid <> n.nspowner"));
        assert!(!SIBLING_VIEW_DEF.contains("::regclass"));
        assert!(SIBLING_VIEW_DEF.contains("pg_get_viewdef(c.oid"));
    }
}
