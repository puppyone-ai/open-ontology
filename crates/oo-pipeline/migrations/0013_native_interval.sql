-- native-interval/v1: MySQL TIME is a signed microsecond interval.
-- Successor to 0009 and 0012. Existing migration checksums and old time
-- semantics stay intact. CREATE OR REPLACE preserves function owner and ACLs.
DO $fi$
BEGIN
  IF current_user <> 'oo_target_owner'
     OR pg_catalog.to_regprocedure('_oo.full_import_type_sql(bigint, integer)') IS NULL
     OR pg_catalog.to_regprocedure('_oo.full_import_probe_conflict(text, uuid, bigint, text, text, text, text, text, text, text, text, numeric)') IS NULL
  THEN
    RAISE EXCEPTION 'native interval requires installed full import'
      USING ERRCODE = 'OO002';
  END IF;
END;
$fi$;

CREATE OR REPLACE FUNCTION _oo.full_import_type_sql(p_oid bigint, p_typmod integer)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $fi$
DECLARE
  v_precision integer;
  v_scale integer;
  v_length integer;
BEGIN
  IF p_oid = 16 THEN RETURN 'bool'; END IF;
  IF p_oid = 21 THEN RETURN 'int2'; END IF;
  IF p_oid = 23 THEN RETURN 'int4'; END IF;
  IF p_oid = 20 THEN RETURN 'int8'; END IF;
  IF p_oid = 700 THEN RETURN 'float4'; END IF;
  IF p_oid = 701 THEN RETURN 'float8'; END IF;
  IF p_oid = 25 THEN RETURN 'text'; END IF;
  IF p_oid = 17 THEN RETURN 'bytea'; END IF;
  IF p_oid = 2950 THEN RETURN 'uuid'; END IF;
  IF p_oid = 1082 THEN RETURN 'date'; END IF;
  IF p_oid = 1083 THEN RETURN 'time without time zone'; END IF;
  IF p_oid = 1186 AND p_typmod = -1 THEN RETURN 'interval'; END IF;
  IF p_oid = 1114 THEN RETURN 'timestamp without time zone'; END IF;
  IF p_oid = 1184 THEN RETURN 'timestamp with time zone'; END IF;
  IF p_oid = 114 THEN RETURN 'json'; END IF;
  IF p_oid = 1700 THEN
    IF p_typmod IS NULL OR p_typmod < 0 THEN
      RETURN 'numeric';
    END IF;
    v_precision := ((p_typmod - 4) >> 16) & 65535;
    v_scale := (p_typmod - 4) & 65535;
    IF v_precision < 1 OR v_precision > 1000 OR v_scale > 1000 OR v_precision < v_scale THEN
      RAISE EXCEPTION 'closed numeric typmod' USING ERRCODE = '22023';
    END IF;
    RETURN format('numeric(%s,%s)', v_precision, v_scale);
  END IF;
  IF p_oid = 1043 OR p_oid = 1042 THEN
    IF p_typmod IS NULL OR p_typmod < 0 THEN
      IF p_oid = 1043 THEN
        RETURN 'character varying';
      END IF;
      RETURN 'character';
    END IF;
    v_length := p_typmod - 4;
    IF v_length < 1 OR v_length > 10485760 THEN
      RAISE EXCEPTION 'closed character typmod' USING ERRCODE = '22023';
    END IF;
    IF p_oid = 1043 THEN
      RETURN format('character varying(%s)', v_length);
    END IF;
    RETURN format('character(%s)', v_length);
  END IF;
  IF p_oid = 3802 THEN
    IF p_typmod IS NULL OR p_typmod <> -1 THEN
      RAISE EXCEPTION 'closed jsonb typmod' USING ERRCODE = '22023';
    END IF;
    RETURN 'jsonb';
  END IF;
  RAISE EXCEPTION 'closed copy type' USING ERRCODE = '22023';
END;
$fi$;

CREATE OR REPLACE FUNCTION _oo.full_import_probe_conflict(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_source text,
  p_physical text,
  p_batch_id text,
  p_payload_digest text,
  p_config_digest text,
  p_type_digest text,
  p_hook_digest text,
  p_schema_digest text,
  p_applied_pos numeric
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_attname text;
  v_oid bigint;
  v_list text := '';
  v_values text := '';
  v_literal text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  IF NOT _oo.full_import_physical_ok(p_physical) OR p_physical !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
  END IF;
  FOR v_attname, v_oid IN
    SELECT attribute.attname::text, attribute.atttypid::bigint
    FROM pg_catalog.pg_attribute AS attribute
    JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid
    JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = '_oo'
      AND relation.relname = p_physical
      AND attribute.attnum > 0
      AND NOT attribute.attisdropped
    ORDER BY attribute.attnum
  LOOP
    IF v_attname = '_oo_pos' THEN
      v_literal := '900000';
    ELSIF v_oid = 16 THEN
      v_literal := 'false';
    ELSIF v_oid IN (21, 23, 20, 700, 701, 1700) THEN
      v_literal := '0';
    ELSIF v_oid IN (25, 1043, 1042) THEN
      v_literal := pg_catalog.quote_literal('');
    ELSIF v_oid = 17 THEN
      v_literal := pg_catalog.quote_literal('') || '::bytea';
    ELSIF v_oid = 2950 THEN
      v_literal := pg_catalog.quote_literal('00000000-0000-0000-0000-000000000000') || '::uuid';
    ELSIF v_oid = 1082 THEN
      v_literal := pg_catalog.quote_literal('2000-01-01') || '::date';
    ELSIF v_oid = 1083 THEN
      v_literal := pg_catalog.quote_literal('00:00:00') || '::time';
    ELSIF v_oid = 1186 THEN
      v_literal := pg_catalog.quote_literal('0 seconds') || '::interval';
    ELSIF v_oid = 1114 THEN
      v_literal := pg_catalog.quote_literal('2000-01-01') || '::timestamp';
    ELSIF v_oid = 1184 THEN
      v_literal := pg_catalog.quote_literal('2000-01-01 00:00:00+00') || '::timestamptz';
    ELSIF v_oid = 114 THEN
      v_literal := pg_catalog.quote_literal('{}') || '::json';
    ELSE
      RAISE EXCEPTION 'closed copy type' USING ERRCODE = '22023';
    END IF;
    IF v_attname <> '_oo_pos' AND NOT _oo.full_import_physical_ok(v_attname) THEN
      RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
    END IF;
    IF v_list <> '' THEN
      v_list := v_list || ', ';
      v_values := v_values || ', ';
    END IF;
    IF v_attname = '_oo_pos' THEN
      v_list := v_list || '"_oo_pos"';
    ELSE
      v_list := v_list || format('%I', v_attname);
    END IF;
    v_values := v_values || v_literal;
  END LOOP;
  EXECUTE format('INSERT INTO _oo.%I (%s) VALUES (%s)', p_physical, v_list, v_values);
  INSERT INTO _oo.full_import_quality (
    run_id, physical_name, batch_id, kind, input_digest, accepted_rows, rejected_rows, payload_digest
  )
  SELECT run_row.run_id, p_physical, p_batch_id, 'batch', run_row.input_digest, 1, 0, p_payload_digest
  FROM _oo.full_import_table AS table_row
  JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
  WHERE table_row.physical_name = p_physical
    AND run_row.state = 'open'
  LIMIT 1;
  PERFORM _oo.commit_apply_receipt(
    p_shard, p_owner, p_generation, p_source, p_physical, p_batch_id,
    p_config_digest, p_type_digest, p_hook_digest, p_schema_digest, p_payload_digest,
    'snapshot-zero/v1', 'full-import.snapshot', 0, p_applied_pos,
    false, false, true, NULL, NULL, ''::bytea
  );
  PERFORM pg_catalog.set_config('_oo.writer', '', true);
END;
$fi$;
