-- Run only after the fixture runner created visibility_fixture in the owned PG17.
SELECT ctx->>'physical' AS view_name,ctx->'pcs'->>0 AS key_col,ctx->'pcs'->>1 AS val_col
  FROM overlay_fixture.saved WHERE label='gated' \gset
SELECT ctx->>'physical' AS root_name FROM overlay_fixture.saved WHERE label='base' \gset
SELECT 'pv_'||substr(:'view_name',4) AS gate_name \gset
SET ROLE oo_target_admin;
CREATE SCHEMA oo_publish_target;
CREATE TABLE oo_publish_target.fence(
  schema_name text,relation_name text,role_name text,state text);
GRANT USAGE ON SCHEMA oo_publish_target TO oo_target_owner;
GRANT SELECT ON oo_publish_target.fence TO oo_target_owner;
INSERT INTO oo_publish_target.fence VALUES('_oo',:'view_name','oo_r_visibility','granted');
ALTER TABLE _oo.:"root_name" ENABLE ROW LEVEL SECURITY;
ALTER TABLE _oo.:"root_name" FORCE ROW LEVEL SECURITY;
CREATE POLICY oo_fence ON _oo.:"root_name" FOR SELECT TO oo_r_visibility USING(false);
CREATE POLICY oo_internal_read ON _oo.:"root_name" FOR SELECT TO oo_target_owner USING(true);
GRANT SELECT (:"key_col") ON _oo.:"view_name" TO oo_r_visibility;
SET ROLE oo_r_visibility;
SELECT overlay_fixture.ok(count(*)=0,'real current_user does not become view owner (staged)')
  FROM _oo.:"view_name";
SELECT overlay_fixture.ok(NOT has_column_privilege(current_user,format('_oo.%I',:'view_name'),:'val_col','SELECT'),
  'hidden column denied');
SELECT overlay_fixture.ok(NOT has_table_privilege(current_user,format('_oo.%I',:'root_name'),'SELECT'),
  'no raw root grant');
RESET ROLE;
SET ROLE oo_target_admin;
UPDATE oo_publish_target.fence SET state='open';
GRANT EXECUTE ON FUNCTION _oo.:"gate_name"() TO oo_r_visibility;
SET ROLE oo_r_visibility;
SELECT overlay_fixture.ok(current_user='oo_r_visibility','actual invoker');
SELECT overlay_fixture.ok(count(*)=3,'open gate reads delta plus FORCE RLS root') FROM _oo.:"view_name";
SELECT overlay_fixture.ok(NOT _oo.c5_overlay_visible(:'view_name','oo_visibility_login'),'no membership fallback');
RESET ROLE;
SET ROLE oo_target_admin;
UPDATE oo_publish_target.fence SET state='closed';
REVOKE ALL ON FUNCTION _oo.:"gate_name"() FROM oo_r_visibility;
SET ROLE oo_r_visibility;
SELECT overlay_fixture.ok(count(*)=0,'closed gate denies rows even before ACL revocation') FROM _oo.:"view_name";
RESET ROLE;
SET ROLE oo_target_admin;
REVOKE ALL ON _oo.:"view_name" FROM oo_r_visibility;
REVOKE ALL (:"key_col",:"val_col","_oo_pos") ON _oo.:"view_name" FROM oo_r_visibility;
SET ROLE oo_r_visibility;
SELECT overlay_fixture.ok(NOT has_column_privilege(current_user,format('_oo.%I',:'view_name'),:'key_col','SELECT'),
  'column grants actually revoked');
RESET ROLE;
SELECT overlay_fixture.ok(NOT _oo.c5_overlay_is_valid('pt_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'),
  'arbitrary view rejected');
SELECT overlay_fixture.ok(NOT _oo.c5_overlay_is_valid(ctx->>'physical'),'old ungated view rejected')
  FROM overlay_fixture.saved WHERE label='legacy';
SELECT overlay_fixture.ok(_oo.c5_overlay_is_valid(:'view_name'),'gated registry still valid');
SELECT overlay_fixture.ok(NOT rolbypassrls,'owner has no BYPASSRLS') FROM pg_roles WHERE rolname='oo_target_owner';
SELECT 'PASS owned-container SQL gate and invoker checks' AS result;
