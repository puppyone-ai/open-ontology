-- Terminal PG capture -> bounded typed server-side export -> ordinary full import.
-- No legacy group/typed ACK is synthesized; the original worker ACK is the authority.
CREATE TABLE _oo.pg_current_publication (
 source text PRIMARY KEY REFERENCES _oo.pg_current_run(source),
 task uuid NOT NULL UNIQUE, request jsonb NOT NULL, export jsonb NOT NULL,
 receipt jsonb NOT NULL, receipt_sha256 text NOT NULL CHECK(receipt_sha256 ~ '^[0-9a-f]{64}$'),
 published_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON _oo.pg_current_publication FROM PUBLIC,oo_pipeline_runtime;
CREATE TRIGGER pg_current_publication_immutable
 BEFORE UPDATE OR DELETE OR TRUNCATE ON _oo.pg_current_publication
 FOR EACH STATEMENT EXECUTE FUNCTION _oo.native_publication_guard_receipt();

CREATE FUNCTION _oo.pg_current_publication_proof(s text,t uuid,g bigint,b uuid,d text,l text,shard text,owner uuid)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE r _oo.pg_current_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
 base _oo.native_c1_baseline_run%ROWTYPE; entries jsonb; scopes jsonb; c jsonb; spec jsonb;
BEGIN
 IF s IS NULL OR t IS NULL OR g IS NULL OR g<1 OR b IS NULL OR d IS NULL
  OR l IS NULL OR shard IS NULL OR owner IS NULL THEN
  RAISE EXCEPTION 'PG publication closed identity' USING ERRCODE='OO030'; END IF;
 SELECT * INTO STRICT r FROM _oo.pg_current_run WHERE source=s AND task=t;
 SELECT * INTO STRICT i FROM _oo.source_instance WHERE source=s AND source_epoch=1;
 SELECT * INTO STRICT base FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
 IF r.bundle_sha IS DISTINCT FROM d OR r.sealed_at IS NULL
  OR r.result->>'protocol' IS DISTINCT FROM 'oo.pipeline.pg-current-result/v1'
  OR r.result->>'load_state' IS DISTINCT FROM 'staged'
  OR r.result->>'source_id' IS DISTINCT FROM s OR r.result->>'task' IS DISTINCT FROM t::text
  OR r.result->>'bundle_sha256' IS DISTINCT FROM d
  OR r.result->'source_identity' IS DISTINCT FROM r.source_identity
  OR r.result->>'end_lsn' IS DISTINCT FROM l OR r.boundary->>'end_lsn' IS DISTINCT FROM l
  OR (r.result->>'applied_pos')::numeric IS DISTINCT FROM (l::pg_lsn-'0/0'::pg_lsn)
  OR r.result->>'source_generation' IS DISTINCT FROM '1'
  OR i.engine<>'postgresql' OR i.shard_id<>shard OR i.owner_token<>owner
  OR i.lease_id<>t OR i.lease_generation<>g OR i.config_digest<>d
  OR i.measured_id IS DISTINCT FROM 'postgresql.'||(r.source_identity->>'system_identifier')||'.'||
      (r.source_identity->>'database_oid')||'.'||(r.source_identity->>'slot')
  OR base.engine<>'postgresql' OR base.source_epoch<>1 OR base.lease_generation<>g
  OR base.config_digest<>d OR base.schema_digest<>i.schema_digest OR base.measured_id<>i.measured_id
  OR base.l0 IS DISTINCT FROM r.boundary->>'original_slot_lsn'
  OR base.state<>'awaiting_publish' OR base.sealed_at IS NULL
  OR base.l_end IS DISTINCT FROM (r.result->>'applied_pos')::numeric OR base.applied_pos<>base.l_end
  OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format
      WHERE source=s AND task_id=t AND sealed AND format='chunked-typed/v2')
 THEN RAISE EXCEPTION 'PG publication staged identity differs' USING ERRCODE='OO030'; END IF;
 IF NOT EXISTS(
  SELECT 1 FROM jw_native.bindings binding
  JOIN jw_native.occurrences o ON o.binding_id=binding.id AND o.pin=binding.pin
  JOIN jobs.tasks j ON j.id=o.task_id
  JOIN jw_native.pending_acks a ON a.task_id=j.id AND a.binding_id=binding.id AND a.pin=o.pin
  JOIN jw_native.watermarks w ON w.task_id=j.id AND w.binding_id=binding.id AND w.pin=o.pin
  JOIN jw_native.source_progress p ON p.source_id=s AND p.target_database=current_database()
  WHERE binding.id=b AND binding.source_id=s AND NOT binding.enabled
   AND binding.target_database=current_database()
   AND j.id=t AND j.state='succeeded' AND j.generation=g
   AND a.generation=g AND a.acknowledged_at IS NOT NULL
   AND a.ack_request->>'protocol'='oo.jobs.pg-slot-ack/v1'
   AND a.ack_request->>'task'=t::text AND a.ack_request->>'source_id'=s
   AND a.ack_request#>>'{bundle,sha256}'=d AND a.ack_request->>'end_lsn'=l
   AND a.ack_request->'source_identity'=r.source_identity
   AND a.ack_request->>'applied_pos'=r.result->>'applied_pos'
   AND w.generation=g AND w.source_generation=1 AND w.applied_pos=r.result->>'applied_pos'
   AND w.load_state='staged' AND w.result-'replay'=r.result-'replay'
   AND o.result-'replay'=r.result-'replay'
   AND p.task_id=t AND p.binding_id=b AND p.source_generation=1
   AND p.active_task_id IS NULL AND p.confirmed_at IS NOT NULL)
 THEN RAISE EXCEPTION 'PG publication terminal task fence ACK differs' USING ERRCODE='OO030'; END IF;
 SELECT jsonb_agg(scope ORDER BY ordinal) INTO scopes FROM _oo.pg_current_tx WHERE source=s;
 IF scopes IS DISTINCT FROM r.boundary->'transactions'
  OR (SELECT count(*) FROM _oo.pg_current_tx WHERE source=s) IS DISTINCT FROM (r.result->>'transaction_count')::bigint
  OR (SELECT max(end_lsn) FROM _oo.pg_current_tx WHERE source=s) IS DISTINCT FROM base.l_end
  OR EXISTS(SELECT 1 FROM _oo.pg_current_tx WHERE source=s AND
      (end_lsn IS DISTINCT FROM ((scope->>'end_lsn')::pg_lsn-'0/0'::pg_lsn)
       OR applied_events IS DISTINCT FROM (scope->>'inserts')::bigint+(scope->>'updates')::bigint+(scope->>'deletes')::bigint))
 THEN RAISE EXCEPTION 'PG publication complete transactions differ' USING ERRCODE='OO030'; END IF;
 SELECT jsonb_agg(jsonb_build_object('name',x.name,'physical',x.physical,'spec',x.spec) ORDER BY x.name)
  INTO entries FROM _oo.c1_typed_table x WHERE x.source=s AND x.task=t;
 IF entries IS DISTINCT FROM r.result->'entries'
  OR jsonb_array_length(entries) IS DISTINCT FROM jsonb_array_length(r.tables)
  OR jsonb_array_length(entries)<1
  OR EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t AND NOT snapshot_complete)
 THEN RAISE EXCEPTION 'PG publication complete tables differ' USING ERRCODE='OO030'; END IF;
 IF i.schema_digest<>encode(sha256(convert_to(_oo.native_library_canonical(r.tables),'UTF8')),'hex') THEN
  RAISE EXCEPTION 'PG publication catalog digest differs' USING ERRCODE='OO030'; END IF;
 FOR c IN SELECT value FROM jsonb_array_elements(r.tables) LOOP
  SELECT x.spec INTO STRICT spec FROM _oo.c1_typed_table x
   WHERE x.source=s AND x.task=t AND x.name=(c->>'schema')||'.'||(c->>'table');
  IF spec->'columns' IS DISTINCT FROM (SELECT jsonb_agg(value-'arrow_type' ORDER BY ord)
      FROM jsonb_array_elements(c->'columns') WITH ORDINALITY x(value,ord))
   OR jsonb_array_length(spec->'keys')<>1
   OR spec->'columns'->(spec->'keys'->>0)::integer->>'name' IS DISTINCT FROM c->>'primary_key'
  THEN RAISE EXCEPTION 'PG publication catalog differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 RETURN jsonb_build_object('stage',r.result,'catalog',r.tables,'instance',to_jsonb(i));
EXCEPTION WHEN no_data_found OR too_many_rows OR invalid_text_representation THEN
 RAISE EXCEPTION 'PG publication durable proof missing or ambiguous' USING ERRCODE='OO030';
END $f$;

CREATE FUNCTION _oo.pg_current_publication_shard(s text)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE p _oo.pg_current_publication%ROWTYPE; w _oo.source_watermark%ROWTYPE;
 r _oo.full_import_run%ROWTYPE; tab _oo.full_import_table%ROWTYPE; e jsonb; proof jsonb;
 n bigint; accepted bigint; rejected bigint;
BEGIN
 SELECT * INTO STRICT p FROM _oo.pg_current_publication WHERE source=s;
 proof:=_oo.pg_current_publication_proof(s,p.task,(p.request->>'task_generation')::bigint,
  (p.request->>'binding_id')::uuid,p.request->>'bundle_sha256',p.request->>'end_lsn',
  p.request->>'shard',(p.request->>'owner')::uuid);
 SELECT * INTO STRICT w FROM _oo.source_watermark WHERE source=s;
 n:=jsonb_array_length(p.receipt->'tables');
 IF p.receipt_sha256<>encode(sha256(convert_to(_oo.native_library_canonical(p.receipt),'UTF8')),'hex')
  OR p.receipt->>'source_id' IS DISTINCT FROM s OR p.receipt->>'position_codec' IS DISTINCT FROM 'pg-commit-lsn/v1'
  OR p.receipt->'pg_current' IS DISTINCT FROM p.request
  OR w.state<>'live' OR w.consistency<>'transactional' OR w.consistency_grade<>'T' OR NOT w.cross_table_consistent
  OR w.source_generation<>1 OR w.target_generation IS DISTINCT FROM (p.receipt->>'target_generation')::bigint
  OR w.applied_pos IS DISTINCT FROM (proof#>>'{stage,applied_pos}')::numeric
  OR w.position_codec<>'pg-commit-lsn/v1' OR w.position_lineage IS DISTINCT FROM proof#>>'{instance,measured_id}'
  OR w.source_epoch IS NOT NULL
  OR n IS DISTINCT FROM jsonb_array_length(proof#>'{stage,entries}')
  OR (SELECT count(*) FROM _oo.table_freshness WHERE source=s)<>n
  OR (SELECT count(*) FROM _oo.native_publication_receipt WHERE source_id=s)<>n
 THEN RAISE EXCEPTION 'PG publication current cut differs' USING ERRCODE='OO030'; END IF;
 FOR e IN SELECT value FROM jsonb_array_elements(p.receipt->'tables') LOOP
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=(e->>'run_id')::uuid;
  SELECT * INTO STRICT tab FROM _oo.full_import_table WHERE run_id=r.run_id;
  IF r.source_id<>s OR r.config_digest<>p.request->>'bundle_sha256' OR r.state<>'published'
   OR r.source_generation<>1 OR r.target_generation<>w.target_generation
   OR r.owner_token::text IS DISTINCT FROM p.request->>'owner' OR r.shard_id<>p.request->>'shard'
   OR tab.schema_ident IS DISTINCT FROM e->>'schema' OR tab.table_ident IS DISTINCT FROM e->>'table'
   OR tab.physical_name IS DISTINCT FROM e->>'physical_name' OR NOT tab.exhausted
   OR tab.applied_pos<>w.applied_pos OR tab.applied_rows IS DISTINCT FROM (e->>'accepted_rows')::bigint
   OR tab.canonical_digest IS DISTINCT FROM e->>'canonical_digest'
   OR NOT EXISTS(SELECT 1 FROM _oo.native_publication_receipt x WHERE x.run_id=r.run_id
      AND x.source_id=s AND x.source_generation=1 AND x.target_generation=w.target_generation
      AND x.receipt_sha256=p.receipt_sha256 AND x.table_vector=p.receipt->'tables'
      AND x.position_codec=w.position_codec AND x.applied_pos=w.applied_pos
      AND x.consistency='transactional' AND x.consistency_grade='T' AND x.cross_table_consistent)
   OR NOT EXISTS(SELECT 1 FROM _oo.table_freshness f WHERE f.source=s AND f.state='live'
      AND f.schema_name=tab.schema_ident AND f.table_name=tab.table_ident AND f.physical_name=tab.physical_name
      AND f.source_generation=1 AND f.target_generation=w.target_generation AND f.last_pos=w.applied_pos)
   OR NOT EXISTS(SELECT 1 FROM _oo.table_quality q WHERE q.schema_name=tab.schema_ident AND q.table_name=tab.table_ident
      AND q.rejects_open=0 AND NOT q.partial_txn_in_progress AND q.position_state='ok'
      AND q.budget_state='ok' AND q.capture_method='c1_cdc' AND q.delete_detection='native')
  THEN RAISE EXCEPTION 'PG publication target vector differs' USING ERRCODE='OO030'; END IF;
  SELECT sum(accepted_rows),sum(rejected_rows) INTO accepted,rejected
   FROM _oo.full_import_quality WHERE run_id=r.run_id AND kind='batch';
  IF accepted IS DISTINCT FROM tab.applied_rows OR rejected IS DISTINCT FROM 0::bigint
   OR EXISTS(SELECT 1 FROM _oo.full_import_quality WHERE run_id=r.run_id AND kind='reject')
  THEN RAISE EXCEPTION 'PG publication quality differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 RETURN jsonb_build_object('source_id',s,'anchor_run',p.receipt->'run_id',
  'receipt_sha256',p.receipt_sha256,'receipt',p.receipt);
EXCEPTION WHEN no_data_found OR too_many_rows OR invalid_text_representation THEN
 RAISE EXCEPTION 'PG publication durable vector missing or ambiguous' USING ERRCODE='OO030';
END $f$;

CREATE FUNCTION _oo.pg_current_export_publish(s text,t uuid,g bigint,b uuid,d text,l text,shard text,owner uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,_oo AS $f$
DECLARE proof jsonb; request jsonb; saved _oo.pg_current_publication%ROWTYPE;
 stage _oo.c1_typed_table%ROWTYPE; c jsonb; col jsonb; row_in record;
 fence bigint; pos numeric; lineage text; run uuid; physical text; mapped text;
 ns text; tn text; names text[]; oids bigint[]; mods integer[]; required boolean[];
 columns_sql text; values_sql text; key_sql text; idx integer; key_idx integer;
 after_key bytea; last_key bytea; page_rows bigint; page_bytes bigint; copied bigint; total bigint;
 hashes text; payload text; batch text; canonical text; schema_sha text; types_sha text; input_sha text;
 pages jsonb; exports jsonb:='[]'; tables jsonb:='[]'; runs jsonb:='[]'; receipt jsonb; sha text;
BEGIN
 -- The authenticated operator must already have control row-lock privileges.
 -- Do not grant jobs writes to the target owner just to acquire these locks.
 PERFORM 1 FROM jw_native.bindings WHERE source_id=s AND target_database=current_database() ORDER BY id FOR SHARE;
 PERFORM 1 FROM jw_native.source_progress WHERE source_id=s AND target_database=current_database() FOR UPDATE;
 PERFORM _oo.native_c1_baseline_lock(s,t);
 PERFORM 1 FROM _oo.pg_current_run WHERE source=s FOR SHARE;
 proof:=_oo.pg_current_publication_proof(s,t,g,b,d,l,shard,owner);
 request:=jsonb_build_object('protocol','oo.pg-current.publication-binding/v1','source_id',s,'task_id',t,
  'task_generation',g,'binding_id',b,'bundle_sha256',d,'end_lsn',l,'shard',shard,'owner',owner);
 SELECT * INTO saved FROM _oo.pg_current_publication WHERE source=s;
 IF FOUND THEN
  IF saved.request IS DISTINCT FROM request THEN
   RAISE EXCEPTION 'PG publication replay conflict' USING ERRCODE='OO030'; END IF;
  RETURN jsonb_build_object('outcome','replay','shard',_oo.pg_current_publication_shard(s));
 END IF;
 IF EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=s)
  OR EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=s)
  OR EXISTS(SELECT 1 FROM _oo.table_freshness f JOIN jsonb_array_elements(proof->'catalog') x
       ON f.schema_name=x->>'schema' AND f.table_name=x->>'table')
 THEN RAISE EXCEPTION 'PG publication prior or foreign target' USING ERRCODE='OO030'; END IF;
 pos:=(proof#>>'{stage,applied_pos}')::numeric;
 lineage:=proof#>>'{instance,measured_id}';
 types_sha:=encode(sha256(convert_to(_oo.native_library_canonical(proof->'catalog'),'UTF8')),'hex');
 input_sha:=encode(sha256(convert_to(_oo.native_library_canonical(request),'UTF8')),'hex');
 fence:=_oo.adopt_target_generation(shard,owner);
 PERFORM _oo.full_import_bind_session(shard,owner,fence);
 SET CONSTRAINTS ALL IMMEDIATE;
 FOR c IN SELECT value FROM jsonb_array_elements(proof->'catalog') ORDER BY value->>'schema',value->>'table' LOOP
  ns:=c->>'schema';tn:=c->>'table';
  IF ns !~ '^[a-z_][a-z0-9_]{0,62}$' OR tn !~ '^[a-z_][a-z0-9_]{0,62}$' THEN
   RAISE EXCEPTION 'PG publication bounded identifier' USING ERRCODE='OO030'; END IF;
  SELECT * INTO STRICT stage FROM _oo.c1_typed_table WHERE source=s AND task=t AND name=ns||'.'||tn;
  EXECUTE format('LOCK TABLE _oo.%I IN SHARE MODE',stage.physical);
  names:='{}';oids:='{}';mods:='{}';required:='{}';columns_sql:='';values_sql:='';key_sql:='';idx:=0;
  key_idx:=(stage.spec->'keys'->>0)::integer;
  PERFORM _oo.register_physical_mapping(s,d,1,'namespace',ns,'','',_oo.pm_sha256(_oo.pm_payload(s,d,1,'n',ns,'','')));
  PERFORM _oo.register_physical_mapping(s,d,1,'table',ns,tn,'',_oo.pm_sha256(_oo.pm_payload(s,d,1,'t',ns,tn,'')));
  physical:=_oo.lookup_physical_mapping(s,d,1,'table',ns,tn,'');
  FOR col IN SELECT value FROM jsonb_array_elements(stage.spec->'columns') LOOP
   IF col->>'name' !~ '^[a-z_][a-z0-9_]{0,62}$' OR (col->>'oid')::bigint NOT IN (20,25)
    OR (col->>'typmod')::integer<>-1
    OR (idx=key_idx AND (col->>'oid'<>'20' OR col->>'nullable'<>'false'))
    OR NOT EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass(format('_oo.%I',stage.physical))
      AND attname='c'||idx AND atttypid=(col->>'oid')::oid AND atttypmod=-1
      AND attnotnull=NOT (col->>'nullable')::boolean AND NOT attisdropped)
   THEN RAISE EXCEPTION 'PG publication bounded typed schema' USING ERRCODE='OO030'; END IF;
   names:=array_append(names,col->>'name');oids:=array_append(oids,(col->>'oid')::bigint);
   mods:=array_append(mods,-1);required:=array_append(required,NOT (col->>'nullable')::boolean);
   PERFORM _oo.register_physical_mapping(s,d,1,'column',ns,tn,col->>'name',
    _oo.pm_sha256(_oo.pm_payload(s,d,1,'c',ns,tn,col->>'name')));
   mapped:=_oo.lookup_physical_mapping(s,d,1,'column',ns,tn,col->>'name');
   columns_sql:=columns_sql||','||quote_ident(mapped);values_sql:=values_sql||format(',c%s',idx);
   IF idx=key_idx THEN key_sql:=quote_ident(mapped); END IF;
   idx:=idx+1;
  END LOOP;
  schema_sha:=encode(sha256(convert_to(_oo.native_library_canonical(c),'UTF8')),'hex');
  run:=_oo.full_import_open_run(shard,owner,fence,s,1,d,types_sha,input_sha,schema_sha,ARRAY[ns],ARRAY[tn],ARRAY[physical]);
  IF _oo.full_import_prepare_shadow(shard,owner,fence,run,0,s,d,1,ns,tn,names,oids,mods,required)<>physical THEN
   RAISE EXCEPTION 'PG publication mapping' USING ERRCODE='OO030'; END IF;
  EXECUTE format('ALTER TABLE _oo.%I ADD PRIMARY KEY (%s)',physical,key_sql);
  after_key:=''::bytea;total:=0;pages:='[]';
  LOOP
   last_key:=after_key;page_rows:=0;page_bytes:=0;hashes:='';
   FOR row_in IN EXECUTE format(
    'SELECT k,pos,octet_length(to_jsonb(x)::text) AS bytes,encode(sha256(convert_to(to_jsonb(x)::text,''UTF8'')),''hex'') AS sha FROM (SELECT * FROM _oo.%I WHERE k>$1 ORDER BY k LIMIT 2048) x',
    stage.physical) USING after_key LOOP
    IF row_in.bytes>4194304 OR row_in.pos<0 OR row_in.pos>pos THEN
     RAISE EXCEPTION 'PG publication row budget or position' USING ERRCODE='OO030'; END IF;
    EXIT WHEN page_bytes+row_in.bytes>4194304;
    page_rows:=page_rows+1;page_bytes:=page_bytes+row_in.bytes;last_key:=row_in.k;hashes:=hashes||row_in.sha;
   END LOOP;
   EXIT WHEN page_rows=0;
   EXECUTE format('INSERT INTO _oo.%I(_oo_pos%s) SELECT pos%s FROM _oo.%I WHERE k>$1 AND k<=$2 ORDER BY k',
    physical,columns_sql,values_sql,stage.physical) USING after_key,last_key;
   GET DIAGNOSTICS copied=ROW_COUNT;
   IF copied<>page_rows THEN RAISE EXCEPTION 'PG publication typed page coverage' USING ERRCODE='OO030'; END IF;
   payload:=encode(sha256(convert_to(hashes,'UTF8')),'hex');
   pages:=pages||jsonb_build_array(jsonb_build_object('index',jsonb_array_length(pages)+1,
    'after_key',encode(after_key,'hex'),'last_key',encode(last_key,'hex'),'rows',copied,'bytes',page_bytes,'sha256',payload));
   total:=total+copied;after_key:=last_key;
  END LOOP;
  canonical:=encode(sha256(convert_to(_oo.native_library_canonical(
   jsonb_build_object('protocol','oo.pg-current.typed-export/v1','schema_digest',schema_sha,'rows',total,'pages',pages)),'UTF8')),'hex');
  batch:='pg-export:'||t::text||':'||jsonb_array_length(tables);
  IF _oo.full_import_write_batch(shard,owner,fence,run,0,s,physical,batch,canonical,d,types_sha,
    encode(sha256(convert_to('full-import/v1 hook none','UTF8')),'hex'),schema_sha,input_sha,pos,total,0,
    '{}'::text[],'{}'::bigint[],'{}'::integer[],'{}'::bigint[],'{}'::text[])<>'inserted' THEN
   RAISE EXCEPTION 'PG publication batch replay' USING ERRCODE='OO030'; END IF;
  PERFORM _oo.full_import_note_table(shard,owner,fence,run,0,total,canonical,s);
  PERFORM _oo.native_update_seal_quality(shard,owner,fence,run,s,1,ns,tn,physical,batch,canonical,pos,
   'pg-commit-lsn/v1',lineage,'c1_cdc','native','T',true,'{}'::text[],total,'never',0,'full');
  PERFORM _oo.native_update_mirror(s,batch,physical);
  IF _oo.full_import_seal_run(shard,owner,fence,run,s,1,d,types_sha,input_sha,schema_sha,ARRAY[ns],ARRAY[tn])<>'awaiting_publish' THEN
   RAISE EXCEPTION 'PG publication seal' USING ERRCODE='OO030'; END IF;
  tables:=tables||jsonb_build_array(jsonb_build_object('schema',ns,'table',tn,'run_id',run,
   'physical_name',physical,'accepted_rows',total,'canonical_digest',canonical,'source_generation',1,
   'target_generation',fence,'applied_pos',_oo.native_pos_text(pos),'rejects','[]'::jsonb));
  runs:=runs||jsonb_build_array(run);
  exports:=exports||jsonb_build_array(jsonb_build_object('schema',ns,'table',tn,'spec',stage.spec,
   'rows',total,'schema_digest',schema_sha,'canonical_digest',canonical,'pages',pages));
 END LOOP;
 receipt:=jsonb_build_object('protocol','oo.pipeline.native-publication-receipt/v1',
  'run_id',runs->0,'run_ids',runs,'source_id',s,'source_generation',1,'target_generation',fence,
  'position_codec','pg-commit-lsn/v1','position_lineage',lineage,'applied_pos',_oo.native_pos_text(pos),
  'consistency','transactional','consistency_grade','T','cross_table_consistent',true,'tables',tables,'pg_current',request);
 sha:=encode(sha256(convert_to(_oo.native_library_canonical(receipt),'UTF8')),'hex');
 -- This is a new PG export proof, not a fabricated native_transaction/group receipt.
 INSERT INTO _oo.pg_current_publication(source,task,request,export,receipt,receipt_sha256)
  VALUES(s,t,request,exports,receipt,sha);
 FOR c IN SELECT value FROM jsonb_array_elements(runs) LOOP
  INSERT INTO _oo.native_publication_receipt(run_id,receipt_sha256,source_id,source_generation,target_generation,
   position_codec,applied_pos,consistency,consistency_grade,cross_table_consistent,table_vector,publication_xid)
  VALUES((c#>>'{}')::uuid,sha,s,1,fence,'pg-commit-lsn/v1',pos,'transactional','T',true,tables,pg_current_xact_id());
  UPDATE _oo.full_import_run SET state='published' WHERE run_id=(c#>>'{}')::uuid AND state='awaiting_publish';
 END LOOP;
 UPDATE _oo.table_freshness SET state='live',applied_at=clock_timestamp()
  WHERE source=s AND source_generation=1 AND target_generation=fence;
 UPDATE _oo.source_watermark SET state='live',applied_at=clock_timestamp() WHERE source=s;
 RETURN jsonb_build_object('outcome','published','shard',_oo.pg_current_publication_shard(s));
END $f$;

-- Only the PG branch is new; other engines retain the exact installed functions.
ALTER FUNCTION _oo.native_library_c1_shard(text) RENAME TO native_library_c1_shard_0117;
CREATE FUNCTION _oo.native_library_c1_shard(s text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
BEGIN
 IF EXISTS(SELECT 1 FROM _oo.pg_current_run WHERE source=s) THEN
  RETURN _oo.pg_current_publication_shard(s);
 END IF;
 RETURN _oo.native_library_c1_shard_0117(s);
END $f$;
ALTER FUNCTION _oo.publish_native_c1_collection(uuid,text,jsonb) RENAME TO publish_native_c1_collection_0117;
CREATE FUNCTION _oo.publish_native_c1_collection(anchor uuid,sha text,receipt jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE expected jsonb;
BEGIN
 IF receipt->>'position_codec'='pg-commit-lsn/v1' THEN
  expected:=_oo.pg_current_publication_shard(receipt->>'source_id');
  IF expected->>'anchor_run' IS DISTINCT FROM anchor::text OR expected->>'receipt_sha256' IS DISTINCT FROM sha
   OR expected->'receipt' IS DISTINCT FROM receipt THEN
   RAISE EXCEPTION 'PG publication exact original shard required' USING ERRCODE='OO030'; END IF;
  RETURN jsonb_build_object('outcome','replay','run_id',anchor,'receipt_sha256',sha);
 END IF;
 RETURN _oo.publish_native_c1_collection_0117(anchor,sha,receipt);
END $f$;

REVOKE ALL ON FUNCTION
 _oo.pg_current_publication_proof(text,uuid,bigint,uuid,text,text,text,uuid),
 _oo.pg_current_publication_shard(text),
 _oo.pg_current_export_publish(text,uuid,bigint,uuid,text,text,text,uuid),
 _oo.native_library_c1_shard(text),_oo.publish_native_c1_collection(uuid,text,jsonb)
 FROM PUBLIC,oo_pipeline_runtime;
DO $grant$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.pg_current_export_publish(text,uuid,bigint,uuid,text,text,text,uuid),
   _oo.native_library_c1_shard(text),_oo.publish_native_c1_collection(uuid,text,jsonb) TO oo_target_admin;
 END IF;
END $grant$;
