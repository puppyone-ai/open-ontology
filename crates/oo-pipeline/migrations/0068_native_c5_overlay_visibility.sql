-- native-c5-overlay-visibility/v1
-- Run as oo_target_owner. Existing ungated overlays are deliberately not upgraded.
ALTER TABLE _oo.c5_overlay
  ADD COLUMN visibility_gate boolean NOT NULL DEFAULT false,
  ADD COLUMN visibility_gate_oid oid,
  ADD CHECK (visibility_gate = (visibility_gate_oid IS NOT NULL));

-- The argument is evaluated by the view's invoker, BEFORE SECURITY DEFINER.
-- This exposes only a boolean, never fence contents or underlying table access.
CREATE FUNCTION _oo.c5_overlay_visible(p_physical text,p_reader name)
RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path=pg_catalog,_oo,pg_temp AS $$
DECLARE visible boolean; gate oid;
BEGIN
  IF p_reader IN ('oo_target_owner','oo_target_admin') THEN RETURN true; END IF;
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

ALTER FUNCTION _oo.c5_overlay_finish(text,uuid,bigint,uuid)
  RENAME TO c5_overlay_finish_before_visibility;
REVOKE ALL ON FUNCTION _oo.c5_overlay_finish_before_visibility(text,uuid,bigint,uuid)
  FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_overlay_finish(p_shard text,p_owner uuid,p_fence bigint,p_run uuid)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo,pg_temp AS $$
DECLARE n bigint; o _oo.c5_overlay; definition text; gate_name text;
BEGIN
  -- The original implementation locks the run/registry and checks the open
  -- state, bound owner/generation and prepared_xid, including on replay.
  n:=_oo.c5_overlay_finish_before_visibility(p_shard,p_owner,p_fence,p_run);
  SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run FOR UPDATE;
  IF NOT o.visibility_gate THEN
    gate_name:='pv_'||substr(o.physical_name,4);
    -- This routine is only an ACL latch; EXECUTE never grants data or mutation.
    EXECUTE format('CREATE FUNCTION _oo.%I() RETURNS boolean LANGUAGE sql IMMUTABLE
      SET search_path=pg_catalog AS %L',gate_name,'SELECT false');
    EXECUTE format('REVOKE ALL ON FUNCTION _oo.%I() FROM PUBLIC,oo_pipeline_runtime',gate_name);
    definition:=regexp_replace(o.view_definition,';[[:space:]]*$','');
    EXECUTE format('CREATE OR REPLACE VIEW _oo.%I WITH (security_barrier=true) AS
      SELECT gated.* FROM (%s) gated
      WHERE _oo.c5_overlay_visible(%L::text,CURRENT_USER)',
      o.physical_name,definition,o.physical_name);
    UPDATE _oo.c5_overlay SET visibility_gate=true,
      visibility_gate_oid=to_regprocedure(format('_oo.%I()',gate_name)),
      view_definition=pg_get_viewdef(o.view_oid,false),
      view_sha256=encode(sha256(convert_to(pg_get_viewdef(o.view_oid,false),'UTF8')),'hex')
      WHERE physical_name=o.physical_name;
  END IF;
  RETURN n;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_overlay_finish(text,uuid,bigint,uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_overlay_finish(text,uuid,bigint,uuid) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_overlay_is_valid(text) RENAME TO c5_overlay_is_valid_before_visibility;
REVOKE ALL ON FUNCTION _oo.c5_overlay_is_valid_before_visibility(text) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_overlay_is_valid(p_physical text)
RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo,pg_temp AS $$
  SELECT EXISTS(SELECT FROM _oo.c5_overlay o JOIN pg_class v ON v.oid=o.view_oid
    JOIN pg_proc g ON g.oid=o.visibility_gate_oid
    WHERE o.physical_name=p_physical AND o.visibility_gate
      AND g.oid=to_regprocedure(format('_oo.%I()','pv_'||substr(p_physical,4)))
      AND g.proowner='oo_target_owner'::regrole AND g.prorettype='boolean'::regtype
      AND g.pronargs=0 AND NOT g.prosecdef AND g.prosrc='SELECT false'
      AND NOT g.proretset AND g.prokind='f' AND g.provolatile='i'
      AND g.proconfig=ARRAY['search_path=pg_catalog']
      AND g.prolang=(SELECT oid FROM pg_language WHERE lanname='sql')
      AND v.reloptions=ARRAY['security_barrier=true']
      AND EXISTS(SELECT FROM pg_rewrite r JOIN pg_depend d
        ON d.classid='pg_rewrite'::regclass AND d.objid=r.oid
        WHERE r.ev_class=v.oid AND r.rulename='_RETURN'
          AND d.refclassid='pg_proc'::regclass
          AND d.refobjid='_oo.c5_overlay_visible(text,name)'::regprocedure))
    AND _oo.c5_overlay_is_valid_before_visibility(p_physical);
$$;
REVOKE ALL ON FUNCTION _oo.c5_overlay_is_valid(text) FROM PUBLIC,oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.c5_overlay_is_valid(text) TO PUBLIC;

-- The library's failure callback must close views too; its previous heap-only
-- ALTER TABLE would abort the entire fail-closed transaction.
CREATE OR REPLACE FUNCTION _oo.native_library_fail_closed(p_library text,p_sha text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo,pg_temp AS $body$
DECLARE publication _oo.native_library_publication%ROWTYPE; f record; columns text; kind "char";
BEGIN
  PERFORM 1 FROM _oo.native_library_head WHERE library_id=p_library AND receipt_sha256=p_sha FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'current library pointer required' USING ERRCODE='OO030'; END IF;
  UPDATE _oo.native_library_head SET state='closed' WHERE library_id=p_library;
  SELECT * INTO publication FROM _oo.native_library_publication WHERE receipt_sha256=p_sha;
  IF to_regclass('oo_publish_target.fence') IS NOT NULL THEN
    FOR f IN EXECUTE 'SELECT f.* FROM oo_publish_target.fence f WHERE EXISTS
      (SELECT FROM jsonb_array_elements($1) s CROSS JOIN LATERAL jsonb_array_elements(s->''receipt''->''tables'') t
       WHERE t->>''physical_name''=f.relation_name AND f.schema_name=''_oo'')
      ORDER BY f.schema_name,f.relation_name,f.role_name FOR UPDATE' USING publication.shards LOOP
      SELECT relkind INTO kind FROM pg_class WHERE oid=to_regclass(format('%I.%I',f.schema_name,f.relation_name));
      IF kind='v' THEN
        IF NOT _oo.c5_overlay_is_valid(f.relation_name) THEN
          RAISE EXCEPTION 'registered gated C5 view required' USING ERRCODE='OO030';
        END IF;
        -- Match the heap DDL's exclusion of active/prepared readers.
        EXECUTE format('LOCK TABLE %I.%I IN ACCESS EXCLUSIVE MODE',f.schema_name,f.relation_name);
        EXECUTE format('REVOKE ALL ON FUNCTION _oo.%I() FROM PUBLIC,%I',
          'pv_'||substr(f.relation_name,4),f.role_name);
      ELSIF kind='r' THEN
        EXECUTE format('ALTER TABLE %I.%I ENABLE ROW LEVEL SECURITY',f.schema_name,f.relation_name);
        EXECUTE format('ALTER TABLE %I.%I FORCE ROW LEVEL SECURITY',f.schema_name,f.relation_name);
        EXECUTE format('DROP POLICY IF EXISTS oo_fence ON %I.%I',f.schema_name,f.relation_name);
        EXECUTE format('CREATE POLICY oo_fence ON %I.%I FOR SELECT TO %I USING(false)',f.schema_name,f.relation_name,f.role_name);
      ELSE
        RAISE EXCEPTION 'publication relation required' USING ERRCODE='OO030';
      END IF;
      SELECT string_agg(quote_ident(attname),',') INTO columns FROM pg_attribute
        WHERE attrelid=to_regclass(format('%I.%I',f.schema_name,f.relation_name)) AND attnum>0 AND NOT attisdropped;
      EXECUTE format('REVOKE ALL ON TABLE %I.%I FROM PUBLIC,%I',f.schema_name,f.relation_name,f.role_name);
      IF columns IS NOT NULL THEN
        EXECUTE format('REVOKE ALL (%s) ON TABLE %I.%I FROM PUBLIC,%I',columns,f.schema_name,f.relation_name,f.role_name);
      END IF;
      -- Keep prior columns so the next open still records permanent removals.
      EXECUTE 'UPDATE oo_publish_target.fence SET state=''closed'',opened_pack_digest=NULL,
        opened_version=NULL,candidate_id=NULL,epoch=NULL WHERE role_name=$1 AND schema_name=$2 AND relation_name=$3'
        USING f.role_name,f.schema_name,f.relation_name;
    END LOOP;
  END IF;
END
$body$;
REVOKE ALL ON FUNCTION _oo.native_library_fail_closed(text,text) FROM PUBLIC,oo_pipeline_runtime;
