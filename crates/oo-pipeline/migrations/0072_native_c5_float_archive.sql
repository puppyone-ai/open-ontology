-- native-c5-float-archive/v1
-- Admit the qualified lossless finite-f64 codec; keep all archive scope guards.
CREATE OR REPLACE FUNCTION _oo.c5_seed_multipart_digest(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,
 p_chunk text,p_generation bigint,p_arrow text,p_projection text,p_digest jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE authority jsonb; h _oo.c5_multipart_hydration; c _oo.c5_chunk; receipt jsonb;
BEGIN
 authority:=_oo.c5_multipart_hydration_manifest(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_chunk,p_generation);
 SELECT * INTO STRICT h FROM _oo.c5_multipart_hydration WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
 SELECT * INTO STRICT c FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk FOR UPDATE;
 IF c.baseline_membership_pending OR c.membership_revision<>1 OR p_arrow IS DISTINCT FROM h.arrow_sha256
  OR p_arrow IS DISTINCT FROM authority->>'arrow_sha256'
  OR p_projection IS DISTINCT FROM authority->'baseline'->>'projection_sha256'
  OR jsonb_typeof(p_digest) IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'C5 archive digest scope differs' USING ERRCODE='OO030'; END IF;
 IF jsonb_array_length(p_digest)<>3 OR p_digest->0 IS DISTINCT FROM to_jsonb(h.row_count)
  OR coalesce(p_digest->>1,'') !~ '^[0-9a-f]{16}$' OR coalesce(p_digest->>2,'') !~ '^[0-9a-f]{16}$'
  OR EXISTS(SELECT FROM jsonb_array_elements(authority->'columns') col
   WHERE (col->>'target_oid')::bigint NOT IN (16,17,20,21,23,25,701,1042,1043,1700,1082,1114,1184)
    OR ((col->>'target_oid')::bigint=1700 AND ((col->>'target_typmod')::integer<4
     OR (((col->>'target_typmod')::integer-4)>>16) NOT BETWEEN 1 AND 38
     OR (((col->>'target_typmod')::integer-4)&65535) NOT BETWEEN 0 AND 30)))
 THEN RAISE EXCEPTION 'C5 archive digest type differs' USING ERRCODE='OO030'; END IF;
 receipt:=jsonb_build_object('chunk_id',c.chunk_id,'lo',c.lo,'hi',c.hi,'membership_revision',c.membership_revision,
  'arrow_sha256',c.arrow_sha256,'row_count',c.row_count,'digest',p_digest,'digest_codec','oo.c5.arrow-sha256-xor/v1',
  'projection_sha256',p_projection);
 IF (h.seed_digest IS NOT NULL AND h.seed_digest IS DISTINCT FROM p_digest)
  OR (c.digest_receipt IS NOT NULL AND c.digest_receipt IS DISTINCT FROM receipt)
 THEN RAISE EXCEPTION 'C5 archive digest replay differs' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.c5_multipart_hydration SET seed_digest=p_digest
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
 UPDATE _oo.c5_chunk SET digest_receipt=receipt,
  previous_digest_hi=('x'||(p_digest->>1))::bit(64),previous_digest_lo=('x'||(p_digest->>2))::bit(64),
  current_digest_hi=('x'||(p_digest->>1))::bit(64),current_digest_lo=('x'||(p_digest->>2))::bit(64)
 WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_seed_multipart_digest(text,uuid,bigint,text,text,text,text,bigint,text,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_seed_multipart_digest(text,uuid,bigint,text,text,text,text,bigint,text,text,jsonb) TO oo_pipeline_runtime;

