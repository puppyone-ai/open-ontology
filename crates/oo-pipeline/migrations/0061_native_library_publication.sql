CREATE TABLE _oo.native_library_limits (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 max_shards integer NOT NULL DEFAULT 64 CHECK(max_shards>0),
 max_tables integer NOT NULL DEFAULT 4096 CHECK(max_tables>0)
);
INSERT INTO _oo.native_library_limits DEFAULT VALUES;
CREATE TABLE _oo.native_library_publication (
 receipt_sha256 text PRIMARY KEY CHECK(receipt_sha256 ~ '^[0-9a-f]{64}$'),
 library_id text NOT NULL,
 library jsonb NOT NULL,
 shards jsonb NOT NULL,
 publication_xid xid8 NOT NULL,
 published_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TRIGGER native_library_publication_guard BEFORE UPDATE OR DELETE OR TRUNCATE
 ON _oo.native_library_publication FOR EACH STATEMENT EXECUTE FUNCTION _oo.native_publication_guard_receipt();
CREATE TABLE _oo.native_library_head (
 library_id text PRIMARY KEY,
 manifest_sha256 text NOT NULL,
 receipt_sha256 text NOT NULL REFERENCES _oo.native_library_publication,
 state text NOT NULL CHECK(state IN ('pending','ready','closed'))
);
CREATE TABLE _oo.native_library_member (
 source_id text PRIMARY KEY,
 library_id text NOT NULL REFERENCES _oo.native_library_head
);
REVOKE ALL ON _oo.native_library_limits,_oo.native_library_publication,_oo.native_library_head,_oo.native_library_member FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.native_library_canonical(p_value jsonb) RETURNS text
LANGUAGE plpgsql IMMUTABLE SET search_path=pg_catalog,_oo AS $body$
DECLARE result text;
BEGIN
 CASE jsonb_typeof(p_value)
 WHEN 'object' THEN
  SELECT '{'||coalesce(string_agg(to_jsonb(key)::text||':'||_oo.native_library_canonical(value),',' ORDER BY key COLLATE "C"),'')||'}'
   INTO result FROM jsonb_each(p_value);
 WHEN 'array' THEN
  SELECT '['||coalesce(string_agg(_oo.native_library_canonical(value),',' ORDER BY ordinal),'')||']'
   INTO result FROM jsonb_array_elements(p_value) WITH ORDINALITY a(value,ordinal);
 ELSE result:=p_value::text;
 END CASE;
 RETURN result;
END
$body$;
REVOKE ALL ON FUNCTION _oo.native_library_canonical(jsonb) FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.configure_native_library_limits(p_shards integer,p_tables integer) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
BEGIN
 IF p_shards IS NULL OR p_tables IS NULL OR p_shards<1 OR p_tables<1 THEN
  RAISE EXCEPTION 'positive library installation ceilings required' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.native_library_limits SET max_shards=p_shards,max_tables=p_tables;
END
$body$;

CREATE FUNCTION _oo.publish_native_library(p_library jsonb,p_sha text,p_shards jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE limits _oo.native_library_limits%ROWTYPE; head _oo.native_library_head%ROWTYPE;
 shard jsonb; member jsonb; existing _oo.native_library_publication%ROWTYPE;
 library_name text; n integer; total integer; source_name text; actual jsonb; expected jsonb;
BEGIN
 IF jsonb_typeof(p_library) IS DISTINCT FROM 'object'
  OR p_library->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-library-publication/v1'
  OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_library) key)
   IS DISTINCT FROM ARRAY['expected_tables','library_id','manifest','manifest_sha256','protocol','shards']::text[]
  OR jsonb_typeof(p_library->'library_id') IS DISTINCT FROM 'string'
  OR length(p_library->>'library_id') NOT BETWEEN 1 AND 128
  OR p_library->>'manifest_sha256' IS NULL OR p_library->>'manifest_sha256' !~ '^[0-9a-f]{64}$'
  OR p_sha IS NULL OR p_sha !~ '^[0-9a-f]{64}$'
  OR jsonb_typeof(p_library->'shards') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_library->'manifest') IS DISTINCT FROM 'object'
  OR jsonb_typeof(p_library->'manifest'->'sources') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_library->'expected_tables') IS DISTINCT FROM 'number'
  OR (p_library->>'expected_tables') !~ '^[1-9][0-9]*$'
  OR jsonb_typeof(p_shards) IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'closed library publication receipt required' USING ERRCODE='OO030'; END IF;
 library_name:=p_library->>'library_id';
 IF p_library->'manifest'->>'library_id' IS DISTINCT FROM library_name
  OR encode(sha256(convert_to(_oo.native_library_canonical(p_library->'manifest'),'UTF8')),'hex')<>p_library->>'manifest_sha256'
  OR encode(sha256(convert_to(_oo.native_library_canonical(jsonb_build_object('library',p_library,'shards',p_shards)),'UTF8')),'hex')<>p_sha
 THEN RAISE EXCEPTION 'library digest differs' USING ERRCODE='OO030'; END IF;
 PERFORM pg_advisory_xact_lock(6404202661);
 SELECT * INTO limits FROM _oo.native_library_limits FOR SHARE;
 n:=jsonb_array_length(p_shards);
 IF n<1 OR n>limits.max_shards OR (p_library->>'expected_tables')::numeric>limits.max_tables
  OR jsonb_array_length(p_library->'shards')<>n
  OR jsonb_array_length(p_library->'manifest'->'sources')<>n
  OR (SELECT count(DISTINCT value->>'source_id') FROM jsonb_array_elements(p_shards))<>n
  OR (SELECT count(DISTINCT value) FROM jsonb_array_elements(p_library->'shards'))<>n
  OR (SELECT count(DISTINCT value->>'source_id') FROM jsonb_array_elements(p_library->'manifest'->'sources'))<>n
 THEN RAISE EXCEPTION 'complete library membership required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO head FROM _oo.native_library_head WHERE library_id=library_name FOR UPDATE;
 IF FOUND AND head.manifest_sha256<>p_library->>'manifest_sha256' THEN
  RAISE EXCEPTION 'registered library membership is immutable' USING ERRCODE='OO030'; END IF;
 FOR shard IN SELECT value FROM jsonb_array_elements(p_shards) ORDER BY value->>'source_id' COLLATE "C" LOOP
  IF jsonb_typeof(shard) IS DISTINCT FROM 'object'
   OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(shard) key)
    IS DISTINCT FROM ARRAY['anchor_run','receipt','receipt_sha256','source_id']::text[]
   OR shard->>'source_id' IS NULL OR shard->>'source_id'=''
   OR shard->>'anchor_run' IS DISTINCT FROM shard->'receipt'->>'run_id'
   OR shard->>'source_id' IS DISTINCT FROM shard->'receipt'->>'source_id'
   OR shard->'receipt'->>'collection_mode' IS DISTINCT FROM 'per_table'
   OR jsonb_typeof(shard->'receipt'->'tables') IS DISTINCT FROM 'array'
   OR shard->>'receipt_sha256' IS NULL
   OR encode(sha256(convert_to(_oo.native_library_canonical(shard->'receipt'),'UTF8')),'hex')<>shard->>'receipt_sha256'
   OR NOT p_library->'shards' @> jsonb_build_array(shard->>'source_id')
  THEN RAISE EXCEPTION 'closed library shard receipt required' USING ERRCODE='OO030'; END IF;
  source_name:=shard->>'source_id';
  SELECT value INTO member FROM jsonb_array_elements(p_library->'manifest'->'sources') WHERE value->>'source_id'=source_name;
  IF member IS NULL OR jsonb_typeof(member->'tables') IS DISTINCT FROM 'array'
   OR EXISTS(SELECT 1 FROM _oo.native_library_member WHERE source_id=source_name AND library_id<>library_name)
  THEN RAISE EXCEPTION 'library source membership differs' USING ERRCODE='OO030'; END IF;
  SELECT jsonb_agg(jsonb_build_array(value->>'schema',value->>'table') ORDER BY value->>'schema',value->>'table')
   INTO actual FROM jsonb_array_elements(shard->'receipt'->'tables');
  SELECT jsonb_agg(jsonb_build_array(member->>'schema',value#>>'{}') ORDER BY member->>'schema',value#>>'{}')
   INTO expected FROM jsonb_array_elements(member->'tables');
  IF actual IS DISTINCT FROM expected THEN
   RAISE EXCEPTION 'library manifest table coverage differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 SELECT sum(jsonb_array_length(value->'receipt'->'tables')) INTO total FROM jsonb_array_elements(p_shards);
 IF total IS DISTINCT FROM (p_library->>'expected_tables')::integer OR total>limits.max_tables THEN
  RAISE EXCEPTION 'library table ceiling or coverage differs' USING ERRCODE='OO030'; END IF;
 PERFORM 1 FROM _oo.source_watermark WHERE source IN (SELECT value->>'source_id' FROM jsonb_array_elements(p_shards)) ORDER BY source FOR UPDATE;
 PERFORM 1 FROM _oo.table_freshness WHERE source IN (SELECT value->>'source_id' FROM jsonb_array_elements(p_shards)) ORDER BY source,schema_name,table_name FOR UPDATE;
 SELECT * INTO existing FROM _oo.native_library_publication WHERE receipt_sha256=p_sha;
 IF FOUND AND (existing.library<>p_library OR existing.shards<>p_shards OR head.receipt_sha256 IS DISTINCT FROM p_sha) THEN
  RAISE EXCEPTION 'library replay conflicts or is superseded' USING ERRCODE='OO030'; END IF;
 FOR shard IN SELECT value FROM jsonb_array_elements(p_shards) ORDER BY value->>'source_id' COLLATE "C" LOOP
  PERFORM _oo.publish_native_per_table_collection((shard->>'anchor_run')::uuid,shard->>'receipt_sha256',shard->'receipt');
 END LOOP;
 IF existing.receipt_sha256 IS NOT NULL THEN
  RETURN jsonb_build_object('outcome','replay','library_id',library_name,'receipt_sha256',p_sha,'tables',total,'state',head.state);
 END IF;
 INSERT INTO _oo.native_library_publication(receipt_sha256,library_id,library,shards,publication_xid)
 VALUES(p_sha,library_name,p_library,p_shards,pg_current_xact_id());
 INSERT INTO _oo.native_library_head VALUES(library_name,p_library->>'manifest_sha256',p_sha,'pending')
 ON CONFLICT(library_id) DO UPDATE SET receipt_sha256=EXCLUDED.receipt_sha256,state='pending';
 INSERT INTO _oo.native_library_member SELECT value->>'source_id',library_name FROM jsonb_array_elements(p_shards)
 ON CONFLICT(source_id) DO NOTHING;
 RETURN jsonb_build_object('outcome','published','library_id',library_name,'receipt_sha256',p_sha,'tables',total,'state','pending','ready',false);
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
 RAISE EXCEPTION 'closed library scalar required' USING ERRCODE='OO030';
END
$body$;

CREATE VIEW _oo.native_library_source_status AS
 SELECT c.source_id,c.receipt_sha256,h.library_id,h.receipt_sha256 AS library_receipt_sha256,
  'superseded_by_library'::text AS status
 FROM _oo.native_library_head h JOIN _oo.native_library_publication p ON p.receipt_sha256=h.receipt_sha256
 CROSS JOIN LATERAL jsonb_array_elements(p.shards) shard
 JOIN _oo.native_per_table_publication c ON c.receipt_sha256=shard->>'receipt_sha256' AND c.source_id=shard->>'source_id';
REVOKE ALL ON _oo.native_library_source_status FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.native_library_fail_closed(p_library text,p_sha text) RETURNS void
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
   EXECUTE 'UPDATE oo_publish_target.fence SET state=''closed'' WHERE role_name=$1 AND schema_name=$2 AND relation_name=$3' USING f.role_name,f.schema_name,f.relation_name;
  END LOOP;
 END IF;
END
$body$;
REVOKE ALL ON FUNCTION _oo.publish_native_library(jsonb,text,jsonb),_oo.configure_native_library_limits(integer,integer),_oo.native_library_fail_closed(text,text) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$
BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.publish_native_library(jsonb,text,jsonb),_oo.configure_native_library_limits(integer,integer),_oo.native_library_fail_closed(text,text) TO oo_target_admin;
 END IF;
END
$grant$;

CREATE FUNCTION _oo.native_library_source_ready(p_source text,p_receipt text) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
 SELECT NOT EXISTS(SELECT 1 FROM _oo.native_library_member WHERE source_id=p_source)
 OR EXISTS(
  SELECT 1 FROM _oo.native_library_member m JOIN _oo.native_library_head h USING(library_id)
  JOIN _oo.native_library_publication p ON p.receipt_sha256=h.receipt_sha256
  WHERE m.source_id=p_source AND h.state='ready'
   AND EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s WHERE s->>'source_id'=p_source AND s->>'receipt_sha256'=p_receipt)
   AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s LEFT JOIN _oo.source_watermark w ON w.source=s->>'source_id'
    WHERE w.source IS NULL OR w.state<>'live' OR w.source_generation IS DISTINCT FROM (s->'receipt'->>'source_generation')::bigint
     OR w.target_generation IS DISTINCT FROM (s->'receipt'->>'target_generation')::bigint)
   AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s CROSS JOIN LATERAL jsonb_array_elements(s->'receipt'->'tables') t
    LEFT JOIN _oo.table_freshness f ON f.source=s->>'source_id' AND f.schema_name=t->>'schema' AND f.table_name=t->>'table'
    WHERE f.source IS NULL OR f.state<>'live' OR f.source_generation IS DISTINCT FROM (t->>'source_generation')::bigint
     OR f.target_generation IS DISTINCT FROM (t->>'target_generation')::bigint OR f.physical_name IS DISTINCT FROM t->>'physical_name')
 )
$body$;

CREATE FUNCTION _oo.native_library_mark_ready(p_library text,p_sha text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE publication _oo.native_library_publication%ROWTYPE; shard jsonb; missing boolean;
BEGIN
 PERFORM 1 FROM _oo.native_library_head WHERE library_id=p_library AND receipt_sha256=p_sha FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'current library pointer required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO publication FROM _oo.native_library_publication WHERE receipt_sha256=p_sha;
 FOR shard IN SELECT value FROM jsonb_array_elements(publication.shards) ORDER BY value->>'source_id' LOOP
  PERFORM _oo.publish_native_per_table_collection((shard->>'anchor_run')::uuid,shard->>'receipt_sha256',shard->'receipt');
 END LOOP;
 EXECUTE 'SELECT EXISTS(SELECT 1 FROM jsonb_array_elements($1) s CROSS JOIN LATERAL jsonb_array_elements(s->''receipt''->''tables'') t WHERE NOT EXISTS(SELECT 1 FROM oo_publish_target.fence f WHERE f.schema_name=''_oo'' AND f.relation_name=t->>''physical_name'' AND f.state=''open''))' INTO missing USING publication.shards;
 IF missing THEN RAISE EXCEPTION 'all library fences must be open' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.native_library_head SET state='ready' WHERE library_id=p_library;
END
$body$;
REVOKE ALL ON FUNCTION _oo.native_library_source_ready(text,text),_oo.native_library_mark_ready(text,text) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$
BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.native_library_source_ready(text,text),_oo.native_library_mark_ready(text,text) TO oo_target_admin;
 END IF;
END
$grant$;
