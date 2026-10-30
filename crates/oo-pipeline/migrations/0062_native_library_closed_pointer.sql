CREATE OR REPLACE FUNCTION _oo.native_library_fail_closed(p_library text,p_sha text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE publication _oo.native_library_publication%ROWTYPE; f record; columns text;
BEGIN
 PERFORM 1 FROM _oo.native_library_head WHERE library_id=p_library AND receipt_sha256=p_sha FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'current library pointer required' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.native_library_head SET state='closed' WHERE library_id=p_library;
 SELECT * INTO publication FROM _oo.native_library_publication WHERE receipt_sha256=p_sha;
 IF to_regclass('oo_publish_target.fence') IS NOT NULL THEN
  FOR f IN EXECUTE 'SELECT f.* FROM oo_publish_target.fence f WHERE EXISTS (SELECT 1 FROM jsonb_array_elements($1) s CROSS JOIN LATERAL jsonb_array_elements(s->''receipt''->''tables'') t WHERE t->>''physical_name''=f.relation_name AND f.schema_name=''_oo'') ORDER BY f.schema_name,f.relation_name,f.role_name FOR UPDATE' USING publication.shards LOOP
   SELECT string_agg(quote_ident(attname),',') INTO columns FROM pg_attribute
    WHERE attrelid=to_regclass(format('%I.%I',f.schema_name,f.relation_name)) AND attnum>0 AND NOT attisdropped;
   EXECUTE format('REVOKE ALL ON TABLE %I.%I FROM %I',f.schema_name,f.relation_name,f.role_name);
   IF columns IS NOT NULL THEN EXECUTE format('REVOKE SELECT (%s) ON TABLE %I.%I FROM %I',columns,f.schema_name,f.relation_name,f.role_name); END IF;
   EXECUTE format('ALTER TABLE %I.%I ENABLE ROW LEVEL SECURITY',f.schema_name,f.relation_name);
   EXECUTE format('ALTER TABLE %I.%I FORCE ROW LEVEL SECURITY',f.schema_name,f.relation_name);
   EXECUTE format('DROP POLICY IF EXISTS oo_fence ON %I.%I',f.schema_name,f.relation_name);
   EXECUTE format('CREATE POLICY oo_fence ON %I.%I FOR SELECT TO %I USING(false)',f.schema_name,f.relation_name,f.role_name);
   EXECUTE 'UPDATE oo_publish_target.fence SET state=''closed'',opened_columns=''{}'',opened_pack_digest=NULL,opened_version=NULL,candidate_id=NULL,epoch=NULL WHERE role_name=$1 AND schema_name=$2 AND relation_name=$3' USING f.role_name,f.schema_name,f.relation_name;
  END LOOP;
 END IF;
END
$body$;
REVOKE ALL ON FUNCTION _oo.native_library_fail_closed(text,text) FROM PUBLIC,oo_pipeline_runtime;
