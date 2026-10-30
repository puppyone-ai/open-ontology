SELECT overlay_fixture.ok(NOT _oo.c5_overlay_is_valid(ctx->>'physical'),'legacy rejected')
  FROM overlay_fixture.saved WHERE label='legacy';
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'gated',overlay_fixture.open_table('visibility','orders');
UPDATE overlay_fixture.saved SET ctx=overlay_fixture.prepare(ctx,
  overlay_fixture.scope(ctx,'0','[["1","updated"]]')) WHERE label='gated';
SELECT overlay_fixture.insert_rows(ctx,'[["1","updated"]]') FROM overlay_fixture.saved WHERE label='gated';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT _oo.c5_overlay_finish(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
  (ctx->>'run')::uuid) FROM overlay_fixture.saved WHERE label='gated';
SELECT view_sha256 AS first_hash FROM _oo.c5_overlay WHERE physical_name=
  (SELECT ctx->>'physical' FROM overlay_fixture.saved WHERE label='gated') \gset
SELECT overlay_fixture.complete(ctx) FROM overlay_fixture.saved WHERE label='gated';
SELECT overlay_fixture.ok(view_sha256=:'first_hash','finish replay does not rewrap or change hash')
  FROM _oo.c5_overlay WHERE physical_name=(SELECT ctx->>'physical' FROM overlay_fixture.saved WHERE label='gated');
COMMIT;
CREATE ROLE oo_target_admin LOGIN NOSUPERUSER NOBYPASSRLS;
GRANT oo_target_owner,pg_signal_backend,pg_read_all_stats TO oo_target_admin;
GRANT CREATE ON DATABASE visibility_fixture TO oo_target_admin;
CREATE ROLE oo_r_visibility NOLOGIN NOINHERIT NOSUPERUSER NOBYPASSRLS;
CREATE ROLE oo_visibility_login LOGIN NOINHERIT NOSUPERUSER NOBYPASSRLS;
GRANT CONNECT ON DATABASE visibility_fixture TO oo_target_admin,oo_visibility_login;
GRANT oo_r_visibility TO oo_visibility_login;
GRANT USAGE ON SCHEMA _oo TO oo_r_visibility,oo_visibility_login;
GRANT USAGE ON SCHEMA overlay_fixture TO oo_target_owner,oo_target_admin,oo_r_visibility;
GRANT SELECT ON overlay_fixture.saved TO oo_target_owner,oo_target_admin;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA overlay_fixture TO oo_target_owner,oo_target_admin;
SELECT ctx->>'physical' AS view_name,ctx->'pcs'->>0 AS key_col
  FROM overlay_fixture.saved WHERE label='gated' \gset
GRANT SELECT (:"key_col") ON _oo.:"view_name" TO oo_r_visibility;
SET ROLE oo_r_visibility;
SELECT overlay_fixture.ok(count(*)=0,'absent coordinator fence fails closed') FROM _oo.:"view_name";
RESET ROLE;
SET ROLE oo_target_owner;
SELECT overlay_fixture.ok(overlay_fixture.read_rows(ctx)='[["1","updated"],["3","keep"],["4","keep4"]]'::jsonb,
  'owner Loader oracle remains readable') FROM overlay_fixture.saved WHERE label='gated';
RESET ROLE;
CREATE VIEW _oo.pt_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff AS SELECT 1 AS forged;
ALTER VIEW _oo.pt_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff OWNER TO oo_target_owner;
