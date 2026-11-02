-- Private, durable parts for one complete C5 range. No watermark/ACK/publication.
CREATE TABLE _oo.c5_parts_capture (
 source text NOT NULL, task uuid NOT NULL,
 header jsonb NOT NULL, spec jsonb NOT NULL,
 physical text NOT NULL, inbox text NOT NULL, columns_sql text NOT NULL, key_sql text NOT NULL,
 expected_parts bigint NOT NULL CHECK(expected_parts>0),
 expected_rows bigint NOT NULL CHECK(expected_rows>=0),
 next_part bigint NOT NULL DEFAULT 0 CHECK(next_part>=0),
 copied_rows bigint NOT NULL DEFAULT 0 CHECK(copied_rows>=0),
 complete boolean NOT NULL DEFAULT false,
 PRIMARY KEY(source,task)
);
CREATE TABLE _oo.c5_parts_receipt (
 source text NOT NULL, task uuid NOT NULL, ordinal bigint NOT NULL CHECK(ordinal>=0),
 scope jsonb NOT NULL, admission_sha256 text NOT NULL CHECK(admission_sha256 ~ '^[0-9a-f]{64}$'),
 rows bigint NOT NULL CHECK(rows>=0),
 PRIMARY KEY(source,task,ordinal),
 FOREIGN KEY(source,task) REFERENCES _oo.c5_parts_capture(source,task)
);
REVOKE ALL ON _oo.c5_parts_capture,_oo.c5_parts_receipt FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_parts_live(s text,t uuid,g bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
 IF s IS NULL OR t IS NULL OR g IS NULL OR g<1 OR NOT EXISTS(
   SELECT FROM jobs.tasks j JOIN jw_native.occurrences o ON o.task_id=j.id
   JOIN jw_native.bindings b ON b.id=o.binding_id
   JOIN jw_native.source_progress p ON p.source_id=b.source_id
     AND p.target_database=b.target_database AND p.active_task_id=j.id
   WHERE j.id=t AND j.generation=g AND j.state='running' AND j.kind='native_capture'
    AND j.lease_until>clock_timestamp() AND b.source_id=s AND b.method='C5'
    AND b.target_database=current_database() AND b.project_id=j.project_id)
 THEN RAISE EXCEPTION 'c5_parts_live_claim' USING ERRCODE='OO010'; END IF;
END $fn$;

CREATE FUNCTION _oo.c5_parts_open(s text,t uuid,g bigint,p_scope jsonb,p_spec jsonb)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE old _oo.c5_parts_capture; col jsonb; idx integer; n integer:=0; k jsonb;
 defs text:=''; names text:=''; key_names text[]:='{}'; seen text[]:='{}'; typ text;
 phys text:='c5p_'||md5(s||':'||t::text); box text:='c5i_'||md5(s||':'||t::text);
 expression text; header jsonb:=p_scope-'part';
BEGIN
 PERFORM _oo.c5_parts_live(s,t,g);
 PERFORM pg_advisory_xact_lock(hashtext('oo-c5-parts'),hashtext(s||t::text));
 IF p_scope->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-c5-range-part/v1'
  OR (SELECT count(*) FROM jsonb_object_keys(p_scope))<>7
  OR NOT p_scope ?& ARRAY['protocol','range','parts_index','part_count','total_rows','part','snapshot']
  OR (SELECT count(*) FROM jsonb_object_keys(p_scope->'range'))<>12
  OR NOT (p_scope->'range') ?& ARRAY['source_engine','source_id','schema','table','schema_fingerprint',
    'chunk_topology_revision','chunk_id','lo','hi','prior_membership_revision','parent_generation','source_generation']
  OR p_scope#>>'{range,source_id}' IS DISTINCT FROM s
  OR p_scope#>>'{range,source_engine}' IS DISTINCT FROM 'mysql'
  OR coalesce(p_scope#>>'{range,schema}','')='' OR coalesce(p_scope#>>'{range,table}','')=''
  OR coalesce(p_scope#>>'{range,chunk_id}','')=''
  OR coalesce(p_scope#>>'{range,schema_fingerprint}','') !~ '^[0-9a-f]{64}$'
  OR (p_scope#>>'{range,chunk_topology_revision}')::bigint<1
  OR (p_scope#>>'{range,prior_membership_revision}')::bigint<0
  OR (p_scope#>>'{range,parent_generation}')::bigint<0
  OR (p_scope#>>'{range,source_generation}')::bigint IS DISTINCT FROM
     (p_scope#>>'{range,parent_generation}')::bigint+1
  OR (SELECT count(*) FROM jsonb_object_keys(p_scope->'snapshot'))<>2
  OR coalesce(p_scope#>>'{snapshot,kind}','')='' OR coalesce(p_scope#>>'{snapshot,value}','')=''
  OR (SELECT count(*) FROM jsonb_object_keys(p_scope->'parts_index'))<>3
  OR coalesce(p_scope#>>'{parts_index,path}','') NOT LIKE '/%'
  OR (p_scope#>>'{parts_index,bytes}')::bigint<=0
  OR coalesce(p_scope#>>'{parts_index,sha256}','') !~ '^[0-9a-f]{64}$'
  OR (SELECT count(*) FROM jsonb_object_keys(p_spec))<>2 OR NOT p_spec ?& ARRAY['columns','keys']
  OR jsonb_typeof(p_spec->'columns') IS DISTINCT FROM 'array'
  OR jsonb_array_length(p_spec->'columns')=0
  OR jsonb_typeof(p_spec->'keys') IS DISTINCT FROM 'array'
  OR jsonb_array_length(p_spec->'keys')=0
  OR (p_scope->>'part_count')::bigint<1 OR (p_scope->>'total_rows')::bigint<0
 THEN RAISE EXCEPTION 'c5_parts_header' USING ERRCODE='OO030'; END IF;
 SELECT * INTO old FROM _oo.c5_parts_capture WHERE source=s AND task=t FOR UPDATE;
 IF FOUND THEN
  IF old.header IS DISTINCT FROM header OR old.spec IS DISTINCT FROM p_spec
  THEN RAISE EXCEPTION 'c5_parts_replay_changed' USING ERRCODE='OO030'; END IF;
  RETURN old.inbox;
 END IF;
 FOR col IN SELECT value FROM jsonb_array_elements(p_spec->'columns') LOOP
  IF coalesce(col->>'name','')='' OR col->>'name'=ANY(seen)
   OR jsonb_typeof(col->'nullable') IS DISTINCT FROM 'boolean'
  THEN RAISE EXCEPTION 'c5_parts_column' USING ERRCODE='OO030'; END IF;
  seen:=array_append(seen,col->>'name');
  typ:=_oo.full_import_type_sql((col->>'oid')::bigint,(col->>'typmod')::integer);
  defs:=defs||format(',%I %s%s','c'||n,typ,CASE WHEN (col->>'nullable')::boolean THEN '' ELSE ' NOT NULL' END);
  names:=names||CASE WHEN n=0 THEN '' ELSE ',' END||format('%I','c'||n);
  n:=n+1;
 END LOOP;
 FOR k IN SELECT value FROM jsonb_array_elements(p_spec->'keys') LOOP
  idx:=(k#>>'{}')::integer;
  IF idx<0 OR idx>=n OR ('c'||idx)=ANY(key_names)
   OR p_spec#>>ARRAY['columns',idx::text,'nullable'] IS DISTINCT FROM 'false'
  THEN RAISE EXCEPTION 'c5_parts_key' USING ERRCODE='OO030'; END IF;
  key_names:=array_append(key_names,'c'||idx);
 END LOOP;
 EXECUTE format('CREATE TABLE _oo.%I (k bytea PRIMARY KEY,part bigint NOT NULL,row_no bigint NOT NULL%s,UNIQUE(part,row_no))',phys,defs);
 EXECUTE format('CREATE UNLOGGED TABLE _oo.%I (k bytea NOT NULL,row_no bigint NOT NULL%s)',box,defs);
 expression:=_oo.c5_overlay_key_sql(to_regclass(format('_oo.%I',phys)),key_names,'x');
 EXECUTE format('GRANT INSERT ON _oo.%I TO oo_pipeline_runtime',box);
 INSERT INTO _oo.c5_parts_capture(source,task,header,spec,physical,inbox,columns_sql,key_sql,expected_parts,expected_rows)
  VALUES(s,t,header,p_spec,phys,box,names,expression,(p_scope->>'part_count')::bigint,(p_scope->>'total_rows')::bigint);
 RETURN box;
END $fn$;

CREATE FUNCTION _oo.c5_parts_begin(s text,t uuid,g bigint,p_scope jsonb,p_admission text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE c _oo.c5_parts_capture; old _oo.c5_parts_receipt; part_ordinal bigint:=(p_scope#>>'{part,ordinal}')::bigint;
BEGIN
 PERFORM _oo.c5_parts_live(s,t,g);
 SELECT * INTO STRICT c FROM _oo.c5_parts_capture WHERE source=s AND task=t FOR UPDATE;
 IF p_scope-'part' IS DISTINCT FROM c.header OR p_admission IS NULL OR p_admission !~ '^[0-9a-f]{64}$'
  OR part_ordinal IS NULL OR part_ordinal<0 OR part_ordinal>=c.expected_parts
  OR coalesce(p_scope#>>'{part,arrow,sha256}','') !~ '^[0-9a-f]{64}$'
  OR (p_scope#>>'{part,row_count}')::bigint<0
 THEN RAISE EXCEPTION 'c5_parts_binding' USING ERRCODE='OO030'; END IF;
 SELECT * INTO old FROM _oo.c5_parts_receipt WHERE source=s AND task=t AND c5_parts_receipt.ordinal=part_ordinal;
 IF FOUND THEN
  IF old.scope IS DISTINCT FROM p_scope OR old.admission_sha256<>p_admission
  THEN RAISE EXCEPTION 'c5_parts_replay_changed' USING ERRCODE='OO030'; END IF;
  RETURN false;
 END IF;
 IF c.complete OR part_ordinal<>c.next_part
 THEN RAISE EXCEPTION 'c5_parts_order' USING ERRCODE='OO030'; END IF;
 EXECUTE format('TRUNCATE TABLE _oo.%I',c.inbox);
 RETURN true;
END $fn$;

CREATE FUNCTION _oo.c5_parts_finish_part(s text,t uuid,g bigint,p_scope jsonb,p_admission text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE c _oo.c5_parts_capture; n bigint; first_row bigint; last_row bigint; invalid boolean;
 part_ordinal bigint:=(p_scope#>>'{part,ordinal}')::bigint; declared bigint:=(p_scope#>>'{part,row_count}')::bigint;
BEGIN
 PERFORM _oo.c5_parts_live(s,t,g);
 SELECT * INTO STRICT c FROM _oo.c5_parts_capture WHERE source=s AND task=t FOR UPDATE;
 IF c.complete OR p_scope-'part' IS DISTINCT FROM c.header OR part_ordinal IS DISTINCT FROM c.next_part
  OR declared IS NULL OR declared<0 OR p_admission IS NULL OR p_admission !~ '^[0-9a-f]{64}$'
  OR c.next_part>=c.expected_parts OR declared>c.expected_rows-c.copied_rows
 THEN RAISE EXCEPTION 'c5_parts_binding' USING ERRCODE='OO030'; END IF;
 EXECUTE format('SELECT count(*),min(row_no),max(row_no),coalesce(bool_or(k IS DISTINCT FROM %s),false) FROM _oo.%I x',
   c.key_sql,c.inbox) INTO n,first_row,last_row,invalid;
 IF n<>declared OR invalid OR (n>0 AND (first_row<>0 OR last_row<>n-1))
 THEN RAISE EXCEPTION 'c5_parts_typed_rows' USING ERRCODE='OO030'; END IF;
 -- The two unique constraints check duplicate keys and duplicate row ordinals.
 EXECUTE format('INSERT INTO _oo.%I(k,part,row_no,%s) SELECT k,$1,row_no,%s FROM _oo.%I',
   c.physical,c.columns_sql,c.columns_sql,c.inbox) USING part_ordinal;
 INSERT INTO _oo.c5_parts_receipt VALUES(s,t,part_ordinal,p_scope,p_admission,n);
 UPDATE _oo.c5_parts_capture SET next_part=next_part+1,copied_rows=copied_rows+n WHERE source=s AND task=t;
 EXECUTE format('TRUNCATE TABLE _oo.%I',c.inbox);
END $fn$;

CREATE FUNCTION _oo.c5_parts_complete(s text,t uuid,g bigint,p_index_sha text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE c _oo.c5_parts_capture; n bigint;
BEGIN
 PERFORM _oo.c5_parts_live(s,t,g);
 SELECT * INTO STRICT c FROM _oo.c5_parts_capture WHERE source=s AND task=t FOR UPDATE;
 IF c.next_part<>c.expected_parts OR c.copied_rows<>c.expected_rows
  OR p_index_sha IS DISTINCT FROM c.header#>>'{parts_index,sha256}'
  OR (SELECT count(*) FROM _oo.c5_parts_receipt WHERE source=s AND task=t)<>c.expected_parts
 THEN RAISE EXCEPTION 'c5_parts_incomplete' USING ERRCODE='OO030'; END IF;
 EXECUTE format('SELECT count(*) FROM _oo.%I',c.physical) INTO n;
 IF n<>c.expected_rows THEN RAISE EXCEPTION 'c5_parts_rows' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.c5_parts_capture SET complete=true WHERE source=s AND task=t;
 RETURN jsonb_build_object('protocol','oo.pipeline.c5-private-range-stage/v1',
  'source_id',s,'task_id',t,'parts',c.expected_parts,'rows',n,'index_sha256',p_index_sha,
  'published',false,'acknowledged',false);
END $fn$;
REVOKE ALL ON FUNCTION _oo.c5_parts_live(text,uuid,bigint) FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.c5_parts_open(text,uuid,bigint,jsonb,jsonb),
 _oo.c5_parts_begin(text,uuid,bigint,jsonb,text),
 _oo.c5_parts_finish_part(text,uuid,bigint,jsonb,text),
 _oo.c5_parts_complete(text,uuid,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_parts_open(text,uuid,bigint,jsonb,jsonb),
 _oo.c5_parts_begin(text,uuid,bigint,jsonb,text),
 _oo.c5_parts_finish_part(text,uuid,bigint,jsonb,text),
 _oo.c5_parts_complete(text,uuid,bigint,text) TO oo_pipeline_runtime;
