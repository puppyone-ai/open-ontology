-- A PostgreSQL superuser already owns the underlying data, but an explicit
-- view predicate is not bypassed by BYPASSRLS. Preserve ordinary reader fences.
CREATE OR REPLACE FUNCTION _oo.c5_overlay_visible(p_physical text,p_reader name)
RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path=pg_catalog,_oo,pg_temp AS $$
DECLARE visible boolean; gate oid;
BEGIN
  IF p_reader IN ('oo_target_owner','oo_target_admin')
    OR EXISTS(SELECT FROM pg_catalog.pg_roles WHERE rolname=p_reader AND rolsuper)
  THEN RETURN true; END IF;
  IF p_physical IS NULL OR p_reader IS NULL
    OR to_regclass('oo_publish_target.fence') IS NULL THEN RETURN false; END IF;
  -- ACL checks use current catalog permissions, not an old repeatable-read
  -- snapshot. A session born during close cannot read through a later stage.
  gate:=to_regprocedure(format('_oo.%I()','pv_'||substr(p_physical,4)));
  IF gate IS NULL OR NOT has_function_privilege(p_reader,gate,'EXECUTE') THEN RETURN false; END IF;
  EXECUTE 'SELECT EXISTS(SELECT FROM oo_publish_target.fence
    WHERE schema_name=''_oo'' AND relation_name=$1 AND role_name=$2 AND state=''open'')'
    INTO visible USING p_physical,p_reader::text;
  RETURN visible;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_overlay_visible(text,name) FROM PUBLIC,oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.c5_overlay_visible(text,name) TO PUBLIC;

