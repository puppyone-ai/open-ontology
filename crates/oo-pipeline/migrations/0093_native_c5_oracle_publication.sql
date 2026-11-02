-- Complete Oracle SCN checks are not MySQL multipart/digest scan receipts.
-- Keep the original MySQL publication proof unchanged.
ALTER FUNCTION _oo.c5_bounded_publishable(text,text,text,uuid)
 RENAME TO c5_bounded_publishable_before_oracle;
REVOKE ALL ON FUNCTION _oo.c5_bounded_publishable_before_oracle(text,text,text,uuid) FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_bounded_publishable(p_source text,p_schema text,p_table text,p_run uuid)
RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 IF NOT EXISTS(SELECT FROM _oo.c5_oracle_binding
   WHERE source=p_source AND schema_name=p_schema AND table_name=p_table) THEN
  RETURN _oo.c5_bounded_publishable_before_oracle(p_source,p_schema,p_table,p_run);
 END IF;
 RETURN EXISTS(
  SELECT FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id)
  JOIN _oo.c5_table c ON c.source=r.source_id AND c.schema_name=t.schema_ident AND c.table_name=t.table_ident
  JOIN _oo.c5_oracle_binding b ON b.source=c.source AND b.schema_name=c.schema_name AND b.table_name=c.table_name
  JOIN _oo.full_import_run baseline ON baseline.run_id=b.baseline_run AND baseline.source_id=b.source
  JOIN _oo.c5_block_receipt receipt ON receipt.run_id=r.run_id AND receipt.source=r.source_id
   AND receipt.schema_name=t.schema_ident AND receipt.table_name=t.table_ident
  WHERE r.run_id=p_run AND r.source_id=p_source AND t.schema_ident=p_schema AND t.table_ident=p_table
   AND r.state IN ('awaiting_publish','published') AND t.exhausted AND t.canonical_digest IS NOT NULL
   AND c.source_generation=r.source_generation AND c.input_digest=r.input_digest
   AND r.config_digest=baseline.config_digest AND r.type_digest=baseline.type_digest
   AND r.schema_digest=baseline.schema_digest AND baseline.state='published'
   AND b.schema_name=_oo.c5_oracle_ident(b.source_schema) AND b.table_name=_oo.c5_oracle_ident(b.source_table)
   AND receipt.source_generation=r.source_generation AND receipt.target_generation=r.target_generation
   AND receipt.scope->>'source_engine'='oracle' AND receipt.scope->>'arrow_sha256'=r.input_digest
   AND receipt.scope->>'schema_fingerprint'=c.schema_fingerprint
   AND receipt.scope->'oracle_snapshot'=jsonb_build_object('identity',b.identity,'scn',b.snapshot_scn::text,
     'manifest_sha256',b.manifest_sha256,'source_schema',b.source_schema,'source_table',b.source_table)
   AND _oo.c5_overlay_is_valid(t.physical_name)
   AND EXISTS(SELECT FROM _oo.c5_chunk ch WHERE ch.source=c.source AND ch.schema_name=c.schema_name
     AND ch.table_name=c.table_name AND ch.retired_at IS NULL)
   AND NOT EXISTS(SELECT FROM _oo.c5_chunk ch LEFT JOIN _oo.c5_oracle_checked checked
     USING(source,schema_name,table_name,chunk_id)
     WHERE ch.source=c.source AND ch.schema_name=c.schema_name AND ch.table_name=c.table_name AND ch.retired_at IS NULL
      AND (ch.topology_revision<>c.topology_revision OR checked.snapshot_scn IS DISTINCT FROM b.snapshot_scn
       OR checked.manifest_sha256 IS DISTINCT FROM b.manifest_sha256))
   AND EXISTS(SELECT FROM _oo.source_watermark w WHERE w.source=r.source_id
     AND w.source_generation=r.source_generation AND w.target_generation=r.target_generation
     AND w.position_codec='snapshot-zero/v1' AND w.position_lineage='oracle.snapshot'
     AND w.consistency_grade='S0' AND NOT w.cross_table_consistent)
 );
END $$;
REVOKE ALL ON FUNCTION _oo.c5_bounded_publishable(text,text,text,uuid) FROM PUBLIC,oo_pipeline_runtime;
