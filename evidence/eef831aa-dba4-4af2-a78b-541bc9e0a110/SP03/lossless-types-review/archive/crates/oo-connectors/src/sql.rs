//! Static server text. Call sites pass these to `sqlx::query` directly.
//! Identifiers from the catalog are bound parameters, never concatenated.

pub(crate) const APPLICATION_NAME: &str = "oo-pg-source";

pub(crate) const SESSION_GUARDS: &str = "\
SELECT pg_catalog.set_config('statement_timeout', $1, false), \
pg_catalog.set_config('lock_timeout', $2, false), \
pg_catalog.set_config('idle_in_transaction_session_timeout', $1, false), \
pg_catalog.set_config('default_transaction_read_only', 'on', false)";

pub(crate) const VERSION: &str = "\
SELECT pg_catalog.current_setting('server_version_num')::int8, \
pg_catalog.current_setting('server_version')";

/// Session identity and TLS, read before the guard `set_config` calls.
/// `$1` is the allow-list of parameter names this driver sends.
///
/// `role` is absent from `pg_settings` (it is not shown), so a server
/// `ALTER ROLE ... SET role` is read with `current_setting('role')`. The
/// default value `none` is not a finding. This statement does not `SET ROLE`.
pub(crate) const IDENTITY: &str = "\
SELECT SESSION_USER::text, CURRENT_USER::text, \
(SELECT s.ssl FROM pg_catalog.pg_stat_ssl s WHERE s.pid = pg_catalog.pg_backend_pid()), \
(SELECT s.version FROM pg_catalog.pg_stat_ssl s WHERE s.pid = pg_catalog.pg_backend_pid()), \
COALESCE((SELECT pg_catalog.string_agg(extra.name, ',' ORDER BY extra.name) FROM ( \
SELECT s.name FROM pg_catalog.pg_settings s \
WHERE s.source IN ('client', 'session', 'startup', 'user', 'database', 'database user') \
AND s.name <> ALL($1::text[]) \
UNION SELECT 'role'::text WHERE pg_catalog.current_setting('role') IS DISTINCT FROM 'none' \
) AS extra), ''), \
pg_catalog.current_database()::text";

pub(crate) const SCAN_COUNTS: &str = "\
SELECT (SELECT count(*)::int8 FROM pg_catalog.pg_namespace n WHERE n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%'), \
(SELECT count(*)::int8 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%' AND c.relkind IN ('r', 'p', 'v', 'f')), \
(SELECT count(*)::int8 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE p.prosecdef AND n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%'), \
(SELECT count(*)::int8 FROM pg_catalog.pg_operator o JOIN pg_catalog.pg_proc p ON p.oid = o.oprcode JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE p.prosecdef AND n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%'), \
(SELECT count(*)::int8 FROM pg_catalog.pg_cast ca JOIN pg_catalog.pg_proc p ON p.oid = ca.castfunc JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE p.prosecdef AND n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%')";

pub(crate) const ROLE_VISIBLE: &str = "\
SELECT count(*)::int8 FROM unnest($1::text[]) AS wanted(rolname) \
JOIN pg_catalog.pg_roles r ON r.rolname = wanted.rolname";

pub(crate) const DATABASE_CREATE: &str = "\
SELECT r.rolname::text, pg_catalog.has_database_privilege(r.rolname, pg_catalog.current_database(), 'CREATE') \
FROM unnest($1::text[]) AS wanted(rolname) \
JOIN pg_catalog.pg_roles r ON r.rolname = wanted.rolname \
WHERE pg_catalog.has_database_privilege(r.rolname, pg_catalog.current_database(), 'CREATE') IS DISTINCT FROM FALSE";

pub(crate) const SCHEMA_CREATE: &str = "\
SELECT r.rolname::text, n.nspname::text, pg_catalog.has_schema_privilege(r.rolname, n.oid, 'CREATE') \
FROM unnest($1::text[]) AS wanted(rolname) \
JOIN pg_catalog.pg_roles r ON r.rolname = wanted.rolname \
JOIN pg_catalog.pg_namespace n ON true \
WHERE n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%' \
AND pg_catalog.has_schema_privilege(r.rolname, n.oid, 'CREATE') IS DISTINCT FROM FALSE";

pub(crate) const GLOBAL_TABLE_WRITE: &str = "\
SELECT r.rolname::text, n.nspname::text, c.relname::text, c.oid::int8, (r.oid = c.relowner), \
pg_catalog.has_table_privilege(r.rolname, c.oid, 'INSERT'), \
pg_catalog.has_table_privilege(r.rolname, c.oid, 'UPDATE'), \
pg_catalog.has_table_privilege(r.rolname, c.oid, 'DELETE'), \
pg_catalog.has_table_privilege(r.rolname, c.oid, 'TRUNCATE'), \
EXISTS (SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl, pg_catalog.acldefault('r', c.relowner))) AS priv WHERE priv.privilege_type = 'INSERT' AND (priv.grantee = r.oid OR priv.grantee = 0)), \
EXISTS (SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl, pg_catalog.acldefault('r', c.relowner))) AS priv WHERE priv.privilege_type = 'UPDATE' AND (priv.grantee = r.oid OR priv.grantee = 0)), \
EXISTS (SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl, pg_catalog.acldefault('r', c.relowner))) AS priv WHERE priv.privilege_type = 'DELETE' AND (priv.grantee = r.oid OR priv.grantee = 0)), \
EXISTS (SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl, pg_catalog.acldefault('r', c.relowner))) AS priv WHERE priv.privilege_type = 'TRUNCATE' AND (priv.grantee = r.oid OR priv.grantee = 0)) \
FROM unnest($1::text[]) AS wanted(rolname) \
JOIN pg_catalog.pg_roles r ON r.rolname = wanted.rolname \
JOIN pg_catalog.pg_class c ON true \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%' \
AND c.relkind IN ('r', 'p', 'v', 'f') \
AND ((r.oid = c.relowner) OR pg_catalog.has_table_privilege(r.rolname, c.oid, 'INSERT') IS DISTINCT FROM FALSE OR pg_catalog.has_table_privilege(r.rolname, c.oid, 'UPDATE') IS DISTINCT FROM FALSE OR pg_catalog.has_table_privilege(r.rolname, c.oid, 'DELETE') IS DISTINCT FROM FALSE OR pg_catalog.has_table_privilege(r.rolname, c.oid, 'TRUNCATE') IS DISTINCT FROM FALSE)";

pub(crate) const GLOBAL_COLUMN_WRITE: &str = "\
SELECT r.rolname::text, n.nspname::text, c.relname::text, \
bool_or(pg_catalog.has_column_privilege(r.rolname, att.attrelid, att.attnum, 'INSERT') IS NULL OR pg_catalog.has_column_privilege(r.rolname, att.attrelid, att.attnum, 'UPDATE') IS NULL), \
bool_or(pg_catalog.has_column_privilege(r.rolname, att.attrelid, att.attnum, 'INSERT') IS TRUE OR pg_catalog.has_column_privilege(r.rolname, att.attrelid, att.attnum, 'UPDATE') IS TRUE), \
bool_or(pg_catalog.has_table_privilege(r.rolname, c.oid, 'INSERT') IS TRUE), \
bool_or(pg_catalog.has_table_privilege(r.rolname, c.oid, 'UPDATE') IS TRUE) \
FROM unnest($1::text[]) AS wanted(rolname) \
JOIN pg_catalog.pg_roles r ON r.rolname = wanted.rolname \
JOIN pg_catalog.pg_class c ON true \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_catalog.pg_attribute att ON att.attrelid = c.oid AND att.attnum > 0 AND NOT att.attisdropped \
WHERE n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%' \
AND c.relkind IN ('r', 'p', 'v', 'f') \
GROUP BY r.rolname, n.nspname, c.relname \
HAVING bool_or(pg_catalog.has_column_privilege(r.rolname, att.attrelid, att.attnum, 'INSERT') IS NULL OR pg_catalog.has_column_privilege(r.rolname, att.attrelid, att.attnum, 'UPDATE') IS NULL) OR bool_or(pg_catalog.has_column_privilege(r.rolname, att.attrelid, att.attnum, 'INSERT') IS TRUE OR pg_catalog.has_column_privilege(r.rolname, att.attrelid, att.attnum, 'UPDATE') IS TRUE)";

pub(crate) const DEFINER_REACH: &str = "\
SELECT n.nspname::text, p.proname::text, pg_catalog.pg_get_userbyid(p.proowner)::text, \
(EXISTS (SELECT 1 FROM unnest($1::text[]) AS wanted(rolname) WHERE pg_catalog.has_schema_privilege(wanted.rolname, n.oid, 'USAGE') AND pg_catalog.has_function_privilege(wanted.rolname, p.oid, 'EXECUTE')) OR (pg_catalog.has_schema_privilege('public', n.oid, 'USAGE') AND pg_catalog.has_function_privilege('public', p.oid, 'EXECUTE')) OR EXISTS (SELECT 1 FROM pg_catalog.pg_operator o JOIN pg_catalog.pg_namespace ons ON ons.oid = o.oprnamespace WHERE o.oprcode = p.oid AND ons.nspname <> 'pg_catalog' AND ons.nspname <> 'information_schema' AND ons.nspname <> 'pg_toast' AND ons.nspname NOT LIKE 'pg_temp_%' AND ons.nspname NOT LIKE 'pg_toast_temp_%' AND (EXISTS (SELECT 1 FROM unnest($1::text[]) AS wanted(rolname) WHERE pg_catalog.has_schema_privilege(wanted.rolname, ons.oid, 'USAGE') AND pg_catalog.has_function_privilege(wanted.rolname, p.oid, 'EXECUTE')) OR (pg_catalog.has_schema_privilege('public', ons.oid, 'USAGE') AND pg_catalog.has_function_privilege('public', p.oid, 'EXECUTE')))) OR EXISTS (SELECT 1 FROM pg_catalog.pg_cast ca JOIN pg_catalog.pg_type src ON src.oid = ca.castsource JOIN pg_catalog.pg_type dst ON dst.oid = ca.casttarget JOIN pg_catalog.pg_namespace sn ON sn.oid = src.typnamespace JOIN pg_catalog.pg_namespace dn ON dn.oid = dst.typnamespace WHERE ca.castfunc = p.oid AND (EXISTS (SELECT 1 FROM unnest($1::text[]) AS wanted(rolname) WHERE pg_catalog.has_function_privilege(wanted.rolname, p.oid, 'EXECUTE') AND (pg_catalog.has_schema_privilege(wanted.rolname, sn.oid, 'USAGE') OR pg_catalog.has_schema_privilege(wanted.rolname, dn.oid, 'USAGE'))) OR (pg_catalog.has_function_privilege('public', p.oid, 'EXECUTE') AND (pg_catalog.has_schema_privilege('public', sn.oid, 'USAGE') OR pg_catalog.has_schema_privilege('public', dn.oid, 'USAGE')))))) , \
(EXISTS (SELECT 1 FROM unnest($1::text[]) AS wanted(rolname) WHERE pg_catalog.has_schema_privilege(wanted.rolname, n.oid, 'USAGE') IS NULL OR pg_catalog.has_function_privilege(wanted.rolname, p.oid, 'EXECUTE') IS NULL) OR pg_catalog.has_schema_privilege('public', n.oid, 'USAGE') IS NULL OR pg_catalog.has_function_privilege('public', p.oid, 'EXECUTE') IS NULL OR EXISTS (SELECT 1 FROM pg_catalog.pg_operator o JOIN pg_catalog.pg_namespace ons ON ons.oid = o.oprnamespace WHERE o.oprcode = p.oid AND (EXISTS (SELECT 1 FROM unnest($1::text[]) AS wanted(rolname) WHERE pg_catalog.has_schema_privilege(wanted.rolname, ons.oid, 'USAGE') IS NULL OR pg_catalog.has_function_privilege(wanted.rolname, p.oid, 'EXECUTE') IS NULL) OR pg_catalog.has_schema_privilege('public', ons.oid, 'USAGE') IS NULL OR pg_catalog.has_function_privilege('public', p.oid, 'EXECUTE') IS NULL)) OR EXISTS (SELECT 1 FROM pg_catalog.pg_cast ca JOIN pg_catalog.pg_type src ON src.oid = ca.castsource JOIN pg_catalog.pg_type dst ON dst.oid = ca.casttarget JOIN pg_catalog.pg_namespace sn ON sn.oid = src.typnamespace JOIN pg_catalog.pg_namespace dn ON dn.oid = dst.typnamespace WHERE ca.castfunc = p.oid AND (EXISTS (SELECT 1 FROM unnest($1::text[]) AS wanted(rolname) WHERE pg_catalog.has_function_privilege(wanted.rolname, p.oid, 'EXECUTE') IS NULL OR pg_catalog.has_schema_privilege(wanted.rolname, sn.oid, 'USAGE') IS NULL OR pg_catalog.has_schema_privilege(wanted.rolname, dn.oid, 'USAGE') IS NULL) OR pg_catalog.has_function_privilege('public', p.oid, 'EXECUTE') IS NULL OR pg_catalog.has_schema_privilege('public', sn.oid, 'USAGE') IS NULL OR pg_catalog.has_schema_privilege('public', dn.oid, 'USAGE') IS NULL))) \
FROM pg_catalog.pg_proc p \
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
WHERE p.prosecdef AND n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' AND n.nspname NOT LIKE 'pg_temp_%' AND n.nspname NOT LIKE 'pg_toast_temp_%'";

pub(crate) const BEGIN: &str = "START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY";

pub(crate) const ROLLBACK: &str = "ROLLBACK";

pub(crate) const FACTS: &str = "\
SELECT CURRENT_USER::text, SESSION_USER::text, \
pg_catalog.current_database()::text, pg_catalog.pg_is_in_recovery(), \
pg_catalog.current_setting('wal_level'), \
pg_catalog.current_setting('max_slot_wal_keep_size')";

pub(crate) const ROLE: &str = "\
SELECT rolname::text, rolsuper, rolinherit, rolcreaterole, rolcreatedb, \
rolcanlogin, rolreplication, rolbypassrls \
FROM pg_catalog.pg_roles WHERE rolname = CURRENT_USER";

pub(crate) const ASSUMABLE: &str = "\
WITH RECURSIVE assumable AS ( \
SELECT r.oid AS roleid, r.rolname::text AS rolname, 0 AS depth, ARRAY[r.oid] AS path \
FROM pg_catalog.pg_roles r WHERE r.rolname = CURRENT_USER \
UNION ALL \
SELECT parent.oid, parent.rolname::text, a.depth + 1, a.path || parent.oid \
FROM assumable a \
JOIN pg_catalog.pg_auth_members m ON m.member = a.roleid \
JOIN pg_catalog.pg_roles parent ON parent.oid = m.roleid \
WHERE a.depth < 16 AND (m.set_option OR m.admin_option) \
AND NOT (parent.oid = ANY (a.path)) \
) \
SELECT roleid::int8, rolname, max(depth)::int8 FROM assumable \
GROUP BY roleid, rolname ORDER BY rolname";

pub(crate) const MEMBERSHIP_TRUNCATED: &str = "\
WITH RECURSIVE assumable AS ( \
SELECT r.oid AS roleid, 0 AS depth, ARRAY[r.oid] AS path \
FROM pg_catalog.pg_roles r WHERE r.rolname = CURRENT_USER \
UNION ALL \
SELECT parent.oid, a.depth + 1, a.path || parent.oid \
FROM assumable a \
JOIN pg_catalog.pg_auth_members m ON m.member = a.roleid \
JOIN pg_catalog.pg_roles parent ON parent.oid = m.roleid \
WHERE a.depth < 16 AND (m.set_option OR m.admin_option) \
AND NOT (parent.oid = ANY (a.path)) \
) \
SELECT EXISTS ( \
SELECT 1 FROM assumable a \
JOIN pg_catalog.pg_auth_members m ON m.member = a.roleid \
WHERE a.depth >= 16 AND (m.set_option OR m.admin_option) \
)";

pub(crate) const DIRECT_MEMBERS: &str = "\
SELECT parent.rolname::text, m.inherit_option, m.set_option, m.admin_option \
FROM pg_catalog.pg_auth_members m \
JOIN pg_catalog.pg_roles parent ON parent.oid = m.roleid \
JOIN pg_catalog.pg_roles mem ON mem.oid = m.member \
WHERE mem.rolname = CURRENT_USER ORDER BY 1";

pub(crate) const ROLE_ATTRS: &str = "\
SELECT rolname::text, rolsuper, rolbypassrls, rolcreaterole, rolcreatedb, \
rolreplication, rolinherit FROM pg_catalog.pg_roles \
WHERE rolname = ANY($1::text[]) ORDER BY 1";

pub(crate) const PREFLIGHT: &str = "\
SELECT c.relname::text, \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'SELECT') \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'pg_catalog' AND c.relname = ANY($1::text[])";

pub(crate) const TABLES: &str = "\
SELECT w.ord::int8, w.schema_name, w.table_name, n.oid::int8, c.oid::int8, \
c.relkind::text, c.relowner::int8, pg_catalog.pg_get_userbyid(c.relowner)::text, \
c.relrowsecurity, c.relforcerowsecurity, c.relreplident::text, \
pg_catalog.obj_description(c.oid, 'pg_class'), \
pg_catalog.has_schema_privilege(CURRENT_USER, n.oid, 'USAGE'), \
pg_catalog.has_schema_privilege(CURRENT_USER, n.oid, 'CREATE'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'SELECT'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'INSERT'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'UPDATE'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'DELETE'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'TRUNCATE') \
FROM unnest($1::text[], $2::text[]) WITH ORDINALITY AS w(schema_name, table_name, ord) \
LEFT JOIN pg_catalog.pg_namespace n ON n.nspname = w.schema_name \
LEFT JOIN pg_catalog.pg_class c ON c.relnamespace = n.oid AND c.relname = w.table_name \
AND c.relkind IN ('r', 'p') ORDER BY w.ord";

pub(crate) const ACL: &str = "\
SELECT c.oid::int8, priv.privilege_type, \
CASE WHEN priv.grantee = 0 THEN 'public' ELSE pg_catalog.pg_get_userbyid(priv.grantee)::text END, \
priv.is_grantable \
FROM unnest($1::int8[]) AS ids(oid) \
JOIN pg_catalog.pg_class c ON c.oid = ids.oid \
CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(c.relacl, pg_catalog.acldefault('r', c.relowner))) AS priv \
ORDER BY 1, 3, 2";

pub(crate) const PUBLICATIONS: &str =
    "SELECT pubname::text FROM pg_catalog.pg_publication ORDER BY 1";

pub(crate) const PUBLICATION_TABLES: &str = "\
SELECT p.pubname::text, n.nspname::text, c.relname::text \
FROM pg_catalog.pg_publication_rel pr \
JOIN pg_catalog.pg_publication p ON p.oid = pr.prpubid \
JOIN pg_catalog.pg_class c ON c.oid = pr.prrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
ORDER BY 1, 2, 3";

pub(crate) const SLOTS: &str = "\
SELECT slot_name::text, slot_type::text, database::text \
FROM pg_catalog.pg_replication_slots ORDER BY 1";

pub(crate) const KEYWORDS: &str = "\
SELECT word::text FROM pg_catalog.pg_get_keywords() WHERE catcode = 'R' ORDER BY 1";

pub(crate) const COLUMNS: &str = "\
SELECT c.oid::int8, a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, \
a.attnotnull, a.attcollation::int8, coll.collname::text, a.attidentity::text, \
a.attgenerated::text, pg_catalog.pg_get_expr(ad.adbin, ad.adrelid), \
pg_catalog.col_description(a.attrelid, a.attnum), t.typname::text, tn.nspname::text, \
t.typtype::text, t.typbasetype::int8, \
pg_catalog.has_column_privilege(CURRENT_USER, a.attrelid, a.attnum, 'SELECT'), \
pg_catalog.has_column_privilege(CURRENT_USER, a.attrelid, a.attnum, 'INSERT'), \
pg_catalog.has_column_privilege(CURRENT_USER, a.attrelid, a.attnum, 'UPDATE') \
FROM unnest($1::int8[]) AS ids(oid) \
JOIN pg_catalog.pg_class c ON c.oid = ids.oid \
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
JOIN pg_catalog.pg_namespace tn ON tn.oid = t.typnamespace \
LEFT JOIN pg_catalog.pg_collation coll ON coll.oid = a.attcollation AND a.attcollation <> 0 \
LEFT JOIN pg_catalog.pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
ORDER BY c.oid, a.attnum";

pub(crate) const PRIMARY_KEYS: &str = "\
SELECT c.oid::int8, ic.relname::text, i.indisunique, i.indnullsnotdistinct, \
a.attnum::int4, a.attname::text, u.ord::int8 \
FROM unnest($1::int8[]) AS ids(oid) \
JOIN pg_catalog.pg_class c ON c.oid = ids.oid \
JOIN pg_catalog.pg_index i ON i.indrelid = c.oid AND i.indisprimary \
JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
ORDER BY c.oid, u.ord";

pub(crate) const UNIQUE_KEYS: &str = "\
SELECT c.oid::int8, ic.relname::text, i.indnullsnotdistinct, \
a.attnum::int4, a.attname::text, u.ord::int8 \
FROM unnest($1::int8[]) AS ids(oid) \
JOIN pg_catalog.pg_class c ON c.oid = ids.oid \
JOIN pg_catalog.pg_index i ON i.indrelid = c.oid AND i.indisunique AND NOT i.indisprimary \
JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
ORDER BY c.oid, ic.relname, u.ord";

pub(crate) const FOREIGN_KEYS: &str = "\
SELECT src.oid::int8, con.conname::text, fn.nspname::text, fc.relname::text, \
sa.attnum::int4, sa.attname::text, da.attnum::int4, da.attname::text, u.ord::int8 \
FROM unnest($1::int8[]) AS ids(oid) \
JOIN pg_catalog.pg_class src ON src.oid = ids.oid \
JOIN pg_catalog.pg_constraint con ON con.conrelid = src.oid AND con.contype = 'f' \
JOIN pg_catalog.pg_class fc ON fc.oid = con.confrelid \
JOIN pg_catalog.pg_namespace fn ON fn.oid = fc.relnamespace \
JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS u(src_att, dst_att, ord) ON true \
JOIN pg_catalog.pg_attribute sa ON sa.attrelid = src.oid AND sa.attnum = u.src_att \
JOIN pg_catalog.pg_attribute da ON da.attrelid = fc.oid AND da.attnum = u.dst_att \
ORDER BY src.oid, con.conname, u.ord";

pub(crate) const POLICY_COUNTS: &str = "\
SELECT c.oid::int8, count(pol.oid)::int8 \
FROM unnest($1::int8[]) AS ids(oid) \
JOIN pg_catalog.pg_class c ON c.oid = ids.oid \
LEFT JOIN pg_catalog.pg_policy pol ON pol.polrelid = c.oid \
GROUP BY c.oid";

pub(crate) const POLICIES: &str = "\
SELECT c.oid::int8, pol.polname::text, pol.polcmd::text, pol.polpermissive, \
pg_catalog.pg_get_expr(pol.polqual, pol.polrelid), \
pg_catalog.pg_get_expr(pol.polwithcheck, pol.polrelid) \
FROM unnest($1::int8[]) AS ids(oid) \
JOIN pg_catalog.pg_class c ON c.oid = ids.oid \
JOIN pg_catalog.pg_policy pol ON pol.polrelid = c.oid \
ORDER BY c.oid, pol.polname";

pub(crate) const ENUM_LABELS: &str = "\
SELECT e.enumtypid::int8, e.enumlabel::text \
FROM pg_catalog.pg_enum e WHERE e.enumtypid = ANY($1::int8[]) \
ORDER BY e.enumtypid, e.enumsortorder";

pub(crate) const DOMAIN_CHECKS: &str = "\
SELECT con.contypid::int8, pg_catalog.pg_get_constraintdef(con.oid) \
FROM pg_catalog.pg_constraint con \
WHERE con.contype = 'c' AND con.contypid = ANY($1::int8[]) \
ORDER BY con.contypid, con.oid";

pub(crate) const QUOTE_IDENT: &str = "SELECT pg_catalog.quote_ident($1)";

pub(crate) const TYPE_FACTS: &str = "\
SELECT t.oid::int8, t.typname::text, n.nspname::text, t.typtype::text, t.typlen::int4, \
COALESCE(t.typelem, 0)::int8, COALESCE(t.typbasetype, 0)::int8, t.typtypmod::int4, \
COALESCE(t.typsend, 0)::int8, COALESCE(p.proname, '')::text, COALESCE(pn.nspname, '')::text \
FROM unnest($1::int8[]) AS ids(oid) \
JOIN pg_catalog.pg_type t ON t.oid = ids.oid \
JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
LEFT JOIN pg_catalog.pg_proc p ON p.oid = t.typsend \
LEFT JOIN pg_catalog.pg_namespace pn ON pn.oid = p.pronamespace \
ORDER BY t.oid";

pub(crate) const CAPTURED_AT: &str = "\
SELECT pg_catalog.to_char(pg_catalog.transaction_timestamp() AT TIME ZONE 'UTC', \
'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')";

#[cfg(test)]
const ALL: &[&str] = &[
    SESSION_GUARDS,
    VERSION,
    IDENTITY,
    SCAN_COUNTS,
    ROLE_VISIBLE,
    DATABASE_CREATE,
    SCHEMA_CREATE,
    GLOBAL_TABLE_WRITE,
    GLOBAL_COLUMN_WRITE,
    DEFINER_REACH,
    BEGIN,
    ROLLBACK,
    FACTS,
    ROLE,
    ASSUMABLE,
    MEMBERSHIP_TRUNCATED,
    DIRECT_MEMBERS,
    ROLE_ATTRS,
    PREFLIGHT,
    TABLES,
    ACL,
    PUBLICATIONS,
    PUBLICATION_TABLES,
    SLOTS,
    KEYWORDS,
    COLUMNS,
    PRIMARY_KEYS,
    UNIQUE_KEYS,
    FOREIGN_KEYS,
    POLICY_COUNTS,
    POLICIES,
    ENUM_LABELS,
    DOMAIN_CHECKS,
    QUOTE_IDENT,
    TYPE_FACTS,
    CAPTURED_AT,
];

#[cfg(test)]
fn first_verb(sql: &str) -> &str {
    sql.split_whitespace().next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_are_read_only_and_carry_no_secret_marker() {
        const ALLOWED: &[&str] = &["SELECT", "WITH", "START", "ROLLBACK"];
        for sql in ALL {
            let verb = first_verb(sql);
            assert!(ALLOWED.contains(&verb), "{verb}");
            let upper = sql.to_ascii_uppercase();
            for banned in [
                "PG_AUTHID",
                "ROLPASSWORD",
                "PROSRC",
                "COPY ",
                "PG_READ_FILE",
                "PG_WRITE_FILE",
                "LO_IMPORT",
                "DBLINK",
            ] {
                assert!(!upper.contains(banned), "{banned}");
            }
            assert!(!sql.contains("CANARY"));
            assert!(!sql.contains('\n'));
        }
    }

    #[test]
    fn identity_reads_hidden_role_guc_without_changing_it() {
        let upper = IDENTITY.to_ascii_uppercase();
        assert!(upper.contains("CURRENT_SETTING('ROLE')"));
        assert!(!upper.contains("SET ROLE"));
        assert!(!upper.contains("RESET ROLE"));
        assert!(!upper.contains("SET SESSION AUTHORIZATION"));
        assert_eq!(first_verb(IDENTITY), "SELECT");
    }
}
