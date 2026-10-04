-- physical-mapping/v1 phase expand
-- Run as oo_target_owner after 0001_target_fence_expand.sql.
-- 0003_target_segment_preflight_expand.sql already occupies migration 0003,
-- so this binding is 0004. It does not modify 0000, 0001, 0002, or 0003.
-- Logical names are pgident/v1 strings. This file stores their PostgreSQL
-- identifiers. It does not write user data, shadows, or receipts.

CREATE FUNCTION _oo.pm_rotr(p_value bigint, p_bits integer)
RETURNS bigint
LANGUAGE sql
IMMUTABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
  SELECT (
    ((p_value & 4294967295) >> p_bits)
    | (((p_value & 4294967295) << (32 - p_bits)) & 4294967295)
  ) & 4294967295;
$pm$;

CREATE FUNCTION _oo.pm_sha256(p_message bytea)
RETURNS bytea
LANGUAGE plpgsql
IMMUTABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  k bigint[] := ARRAY[
    1116352408, 1899447441, 3049323471, 3921009573, 961987163, 1508970993, 2453635748, 2870763221,
    3624381080, 310598401, 607225278, 1426881987, 1925078388, 2162078206, 2614888103, 3248222580,
    3835390401, 4022224774, 264347078, 604807628, 770255983, 1249150122, 1555081692, 1996064986,
    2554220882, 2821834349, 2952996808, 3210313671, 3336571891, 3584528711, 113926993, 338241895,
    666307205, 773529912, 1294757372, 1396182291, 1695183700, 1986661051, 2177026350, 2456956037,
    2730485921, 2820302411, 3259730800, 3345764771, 3516065817, 3600352804, 4094571909, 275423344,
    430227734, 506948616, 659060556, 883997877, 958139571, 1322822218, 1537002063, 1747873779,
    1955562222, 2024104815, 2227730452, 2361852424, 2428436474, 2756734187, 3204031479, 3329325298
  ];
  h bigint[] := ARRAY[
    1779033703,
    3144134277,
    1013904242,
    2773480762,
    1359893119,
    2600822924,
    528734635,
    1541459225
  ];
  ml integer;
  bitlen bigint;
  pad_zeros integer;
  padded bytea;
  len_bytes bytea;
  blocks integer;
  block_index integer;
  v_offset integer;
  i integer;
  w bigint[];
  word bigint;
  a bigint;
  b bigint;
  c bigint;
  d bigint;
  e bigint;
  f bigint;
  g bigint;
  hh bigint;
  s0 bigint;
  s1 bigint;
  ch bigint;
  maj bigint;
  t1 bigint;
  t2 bigint;
  out_bytes bytea;
BEGIN
  ml := octet_length(p_message);
  bitlen := ml::bigint * 8;
  pad_zeros := 55 - (ml % 64);
  IF pad_zeros < 0 THEN
    pad_zeros := pad_zeros + 64;
  END IF;
  padded := p_message || decode('80', 'hex');
  IF pad_zeros > 0 THEN
    padded := padded || decode(repeat('00', pad_zeros), 'hex');
  END IF;
  len_bytes := decode('0000000000000000', 'hex');
  FOR i IN 0..7 LOOP
    len_bytes := set_byte(len_bytes, i, ((bitlen >> (56 - i * 8)) & 255)::integer);
  END LOOP;
  padded := padded || len_bytes;
  IF octet_length(padded) % 64 <> 0 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  blocks := octet_length(padded) / 64;
  FOR block_index IN 0..blocks - 1 LOOP
    w := array_fill(0::bigint, ARRAY[64]);
    FOR i IN 0..15 LOOP
      v_offset := block_index * 64 + i * 4;
      word := (get_byte(padded, v_offset)::bigint << 24)
        | (get_byte(padded, v_offset + 1)::bigint << 16)
        | (get_byte(padded, v_offset + 2)::bigint << 8)
        | get_byte(padded, v_offset + 3)::bigint;
      w[i + 1] := word;
    END LOOP;
    FOR i IN 16..63 LOOP
      s0 := (
        _oo.pm_rotr(w[i - 15 + 1], 7)
        # _oo.pm_rotr(w[i - 15 + 1], 18)
        # ((w[i - 15 + 1] >> 3) & 4294967295)
      ) & 4294967295;
      s1 := (
        _oo.pm_rotr(w[i - 2 + 1], 17)
        # _oo.pm_rotr(w[i - 2 + 1], 19)
        # ((w[i - 2 + 1] >> 10) & 4294967295)
      ) & 4294967295;
      w[i + 1] := (w[i - 16 + 1] + s0 + w[i - 7 + 1] + s1) & 4294967295;
    END LOOP;
    a := h[1];
    b := h[2];
    c := h[3];
    d := h[4];
    e := h[5];
    f := h[6];
    g := h[7];
    hh := h[8];
    FOR i IN 0..63 LOOP
      s1 := (_oo.pm_rotr(e, 6) # _oo.pm_rotr(e, 11) # _oo.pm_rotr(e, 25)) & 4294967295;
      ch := ((e & f) # ((4294967295 # e) & g)) & 4294967295;
      t1 := (hh + s1 + ch + k[i + 1] + w[i + 1]) & 4294967295;
      s0 := (_oo.pm_rotr(a, 2) # _oo.pm_rotr(a, 13) # _oo.pm_rotr(a, 22)) & 4294967295;
      maj := ((a & b) # (a & c) # (b & c)) & 4294967295;
      t2 := (s0 + maj) & 4294967295;
      hh := g;
      g := f;
      f := e;
      e := (d + t1) & 4294967295;
      d := c;
      c := b;
      b := a;
      a := (t1 + t2) & 4294967295;
    END LOOP;
    h[1] := (h[1] + a) & 4294967295;
    h[2] := (h[2] + b) & 4294967295;
    h[3] := (h[3] + c) & 4294967295;
    h[4] := (h[4] + d) & 4294967295;
    h[5] := (h[5] + e) & 4294967295;
    h[6] := (h[6] + f) & 4294967295;
    h[7] := (h[7] + g) & 4294967295;
    h[8] := (h[8] + hh) & 4294967295;
  END LOOP;
  out_bytes := decode('', 'hex');
  FOR i IN 1..8 LOOP
    out_bytes := out_bytes || decode(lpad(to_hex(h[i]), 8, '0'), 'hex');
  END LOOP;
  RETURN out_bytes;
END;
$pm$;

CREATE FUNCTION _oo.pm_source_ok(p_name text)
RETURNS boolean
LANGUAGE plpgsql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  v_bytes bytea;
  v_len integer;
  v_index integer;
  v_byte integer;
BEGIN
  v_bytes := convert_to(p_name, 'UTF8');
  v_len := octet_length(v_bytes);
  IF v_len < 1 OR v_len > 63 THEN
    RETURN false;
  END IF;
  v_byte := get_byte(v_bytes, 0);
  IF v_byte < 97 OR v_byte > 122 THEN
    RETURN false;
  END IF;
  FOR v_index IN 1..v_len - 1 LOOP
    v_byte := get_byte(v_bytes, v_index);
    IF NOT (v_byte = 95 OR (v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 122)) THEN
      RETURN false;
    END IF;
  END LOOP;
  RETURN true;
END;
$pm$;

CREATE FUNCTION _oo.pm_config_ok(p_name text)
RETURNS boolean
LANGUAGE plpgsql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  v_bytes bytea;
  v_len integer;
  v_index integer;
  v_byte integer;
BEGIN
  v_bytes := convert_to(p_name, 'UTF8');
  v_len := octet_length(v_bytes);
  IF v_len <> 64 THEN
    RETURN false;
  END IF;
  FOR v_index IN 0..63 LOOP
    v_byte := get_byte(v_bytes, v_index);
    IF NOT ((v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 102)) THEN
      RETURN false;
    END IF;
  END LOOP;
  RETURN true;
END;
$pm$;

CREATE FUNCTION _oo.pm_lower_hex(p_text text, p_len integer)
RETURNS boolean
LANGUAGE plpgsql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  v_bytes bytea;
  v_len integer;
  v_index integer;
  v_byte integer;
BEGIN
  IF p_len < 0 THEN
    RETURN false;
  END IF;
  v_bytes := convert_to(p_text, 'UTF8');
  v_len := octet_length(v_bytes);
  IF v_len <> p_len THEN
    RETURN false;
  END IF;
  FOR v_index IN 0..v_len - 1 LOOP
    v_byte := get_byte(v_bytes, v_index);
    IF NOT ((v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 102)) THEN
      RETURN false;
    END IF;
  END LOOP;
  RETURN true;
END;
$pm$;

CREATE FUNCTION _oo.pm_logical_ok(p_name text)
RETURNS boolean
LANGUAGE plpgsql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  v_bytes bytea;
  v_len integer;
  v_index integer;
  v_byte integer;
  v_reserved text[] := ARRAY[
-- pm-reserved-begin
    'all',
    'analyse',
    'analyze',
    'and',
    'any',
    'array',
    'as',
    'asc',
    'asymmetric',
    'both',
    'case',
    'cast',
    'check',
    'collate',
    'column',
    'constraint',
    'create',
    'current_catalog',
    'current_date',
    'current_role',
    'current_schema',
    'current_time',
    'current_timestamp',
    'current_user',
    'default',
    'deferrable',
    'desc',
    'distinct',
    'do',
    'else',
    'end',
    'except',
    'false',
    'fetch',
    'for',
    'foreign',
    'from',
    'grant',
    'group',
    'having',
    'in',
    'initially',
    'intersect',
    'into',
    'lateral',
    'leading',
    'limit',
    'localtime',
    'localtimestamp',
    'not',
    'null',
    'offset',
    'on',
    'only',
    'or',
    'order',
    'placing',
    'primary',
    'references',
    'returning',
    'select',
    'session_user',
    'some',
    'symmetric',
    'system_user',
    'table',
    'then',
    'to',
    'trailing',
    'true',
    'union',
    'unique',
    'user',
    'using',
    'variadic',
    'when',
    'where',
    'window',
    'with'
-- pm-reserved-end
  ];
  v_reserved_hit boolean;
BEGIN
  v_bytes := convert_to(p_name, 'UTF8');
  v_len := octet_length(v_bytes);
  IF v_len < 1 OR v_len > 4096 THEN
    RETURN false;
  END IF;
  IF v_len <= 63
     AND NOT (
       v_len >= 2
       AND get_byte(v_bytes, 0) = 113
       AND get_byte(v_bytes, 1) = 95
     )
  THEN
    v_byte := get_byte(v_bytes, 0);
    IF NOT (v_byte = 95 OR (v_byte BETWEEN 97 AND 122)) THEN
      RETURN false;
    END IF;
    FOR v_index IN 1..v_len - 1 LOOP
      v_byte := get_byte(v_bytes, v_index);
      IF NOT (v_byte = 95 OR (v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 122)) THEN
        RETURN false;
      END IF;
    END LOOP;
    SELECT EXISTS (
      SELECT 1
      FROM unnest(v_reserved) AS word
      WHERE convert_to(word, 'UTF8') = v_bytes
    )
      INTO v_reserved_hit;
    RETURN NOT v_reserved_hit;
  END IF;
  IF v_len < 4 OR v_len = 63 OR ((v_len - 2) % 2) <> 0 THEN
    RETURN false;
  END IF;
  IF get_byte(v_bytes, 0) <> 113 OR get_byte(v_bytes, 1) <> 95 THEN
    RETURN false;
  END IF;
  FOR v_index IN 2..v_len - 1 LOOP
    v_byte := get_byte(v_bytes, v_index);
    IF NOT ((v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 102)) THEN
      RETURN false;
    END IF;
  END LOOP;
  RETURN true;
END;
$pm$;

CREATE FUNCTION _oo.pm_payload(
  p_source text,
  p_config_digest text,
  p_run_generation bigint,
  p_level text,
  p_logical_namespace text,
  p_logical_table text,
  p_logical_column text
)
RETURNS bytea
LANGUAGE sql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
  SELECT convert_to('physical-mapping/v1', 'UTF8') || decode('00', 'hex')
    || convert_to(p_source, 'UTF8') || decode('00', 'hex')
    || convert_to(p_config_digest, 'UTF8') || decode('00', 'hex')
    || convert_to(p_run_generation::text, 'UTF8') || decode('00', 'hex')
    || convert_to(p_level, 'UTF8')
    || convert_to(p_logical_namespace, 'UTF8') || decode('00', 'hex')
    || convert_to(p_logical_table, 'UTF8') || decode('00', 'hex')
    || convert_to(p_logical_column, 'UTF8') || decode('00', 'hex');
$pm$;

CREATE FUNCTION _oo.pm_physical(p_level text, p_digest bytea)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
  SELECT CASE
    WHEN octet_length(p_digest) = 32 AND p_level IN ('n', 't', 'c') THEN
      CASE p_level
        WHEN 'n' THEN 'pn_'
        WHEN 't' THEN 'pt_'
        ELSE 'pc_'
      END || substring(encode(p_digest, 'hex') FROM 1 FOR 60)
    ELSE NULL
  END;
$pm$;

CREATE TABLE _oo.physical_mappings (
  source text NOT NULL,
  config_digest text NOT NULL,
  run_generation bigint NOT NULL,
  level text NOT NULL,
  logical_namespace text NOT NULL,
  logical_table text NOT NULL,
  logical_column text NOT NULL,
  identity_digest bytea NOT NULL,
  physical_name text NOT NULL,
  scope_namespace_physical text NOT NULL,
  scope_table_physical text,
  algorithm text NOT NULL,
  registered_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT physical_mappings_pkey PRIMARY KEY (
    source, config_digest, run_generation, level, logical_namespace, logical_table, logical_column
  ),
  CONSTRAINT physical_mappings_identity_digest_unique UNIQUE (identity_digest),
  CONSTRAINT physical_mappings_physical_name_unique UNIQUE (physical_name),
  CONSTRAINT physical_mappings_table_scope_unique UNIQUE (scope_namespace_physical, physical_name),
  CONSTRAINT physical_mappings_column_scope_unique UNIQUE NULLS NOT DISTINCT (
    scope_namespace_physical, scope_table_physical, physical_name
  ),
  CONSTRAINT physical_mappings_generation_check CHECK (run_generation >= 1),
  CONSTRAINT physical_mappings_level_check CHECK (level IN ('n', 't', 'c')),
  CONSTRAINT physical_mappings_algorithm_check CHECK (algorithm = 'physical-mapping/v1'),
  CONSTRAINT physical_mappings_digest_len_check CHECK (octet_length(identity_digest) = 32),
  CONSTRAINT physical_mappings_source_check CHECK (_oo.pm_source_ok(source)),
  CONSTRAINT physical_mappings_config_check CHECK (_oo.pm_config_ok(config_digest)),
  CONSTRAINT physical_mappings_namespace_check CHECK (_oo.pm_logical_ok(logical_namespace)),
  CONSTRAINT physical_mappings_table_name_check CHECK (
    (level = 'n' AND logical_table = '')
    OR (level IN ('t', 'c') AND _oo.pm_logical_ok(logical_table))
  ),
  CONSTRAINT physical_mappings_column_name_check CHECK (
    (level IN ('n', 't') AND logical_column = '')
    OR (level = 'c' AND _oo.pm_logical_ok(logical_column))
  ),
  CONSTRAINT physical_mappings_physical_check CHECK (
    octet_length(physical_name) = 63
    AND physical_name = (
      CASE level
        WHEN 'n' THEN 'pn_'
        WHEN 't' THEN 'pt_'
        WHEN 'c' THEN 'pc_'
      END || substring(encode(identity_digest, 'hex') FROM 1 FOR 60)
    )
  ),
  CONSTRAINT physical_mappings_scope_check CHECK (
    (
      level = 'n'
      AND logical_table = ''
      AND logical_column = ''
      AND scope_namespace_physical = physical_name
      AND scope_table_physical IS NULL
    )
    OR (
      level = 't'
      AND logical_column = ''
      AND scope_table_physical IS NOT NULL
      AND scope_table_physical = physical_name
      AND octet_length(scope_namespace_physical) = 63
      AND substring(scope_namespace_physical FROM 1 FOR 3) = 'pn_'
      AND coalesce(_oo.pm_lower_hex(substring(scope_namespace_physical FROM 4), 60), false)
    )
    OR (
      level = 'c'
      AND scope_table_physical IS NOT NULL
      AND octet_length(scope_namespace_physical) = 63
      AND substring(scope_namespace_physical FROM 1 FOR 3) = 'pn_'
      AND coalesce(_oo.pm_lower_hex(substring(scope_namespace_physical FROM 4), 60), false)
      AND octet_length(scope_table_physical) = 63
      AND substring(scope_table_physical FROM 1 FOR 3) = 'pt_'
      AND coalesce(_oo.pm_lower_hex(substring(scope_table_physical FROM 4), 60), false)
    )
  )
);

COMMENT ON TABLE _oo.physical_mappings IS
  'physical-mapping/v1. One pgident/v1 logical name in a source, config digest, and run generation binds to one stored PostgreSQL identifier of 63 bytes. Lookup reads the stored name.';

CREATE FUNCTION _oo.reject_physical_mapping_write()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
BEGIN
  IF TG_OP <> 'INSERT' THEN
    RAISE EXCEPTION 'direct write to physical mapping is closed' USING ERRCODE = 'OO014';
  END IF;
  IF current_setting('_oo.physical_mapping_writer', true) IS DISTINCT FROM 'register' THEN
    RAISE EXCEPTION 'direct write to physical mapping is closed' USING ERRCODE = 'OO014';
  END IF;
  RETURN NEW;
END;
$pm$;

CREATE TRIGGER physical_mappings_reject_write
  BEFORE INSERT OR UPDATE OR DELETE ON _oo.physical_mappings
  FOR EACH ROW
  EXECUTE FUNCTION _oo.reject_physical_mapping_write();

CREATE TRIGGER physical_mappings_reject_truncate
  BEFORE TRUNCATE ON _oo.physical_mappings
  FOR EACH STATEMENT
  EXECUTE FUNCTION _oo.reject_physical_mapping_write();

CREATE FUNCTION _oo.register_physical_mapping(
  p_source text,
  p_config_digest text,
  p_run_generation bigint,
  p_level text,
  p_logical_namespace text,
  p_logical_table text,
  p_logical_column text,
  p_identity_digest bytea
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $pm$
DECLARE
  v_level text;
  v_digest bytea;
  v_physical text;
  v_scope_ns text;
  v_scope_table text;
  v_lock bigint;
  v_existing _oo.physical_mappings%ROWTYPE;
  v_found text;
BEGIN
  IF current_setting('server_encoding') <> 'UTF8' THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_source IS NULL OR NOT _oo.pm_source_ok(p_source) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_config_digest IS NULL OR NOT _oo.pm_config_ok(p_config_digest) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_run_generation IS NULL OR p_run_generation < 1 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_level IS NULL THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF convert_to(p_level, 'UTF8') = convert_to('namespace', 'UTF8') THEN
    v_level := 'n';
  ELSIF convert_to(p_level, 'UTF8') = convert_to('table', 'UTF8') THEN
    v_level := 't';
  ELSIF convert_to(p_level, 'UTF8') = convert_to('column', 'UTF8') THEN
    v_level := 'c';
  ELSE
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_logical_namespace IS NULL OR NOT _oo.pm_logical_ok(p_logical_namespace) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF v_level = 'n' THEN
    IF p_logical_table IS NULL
       OR octet_length(convert_to(p_logical_table, 'UTF8')) <> 0
       OR p_logical_column IS NULL
       OR octet_length(convert_to(p_logical_column, 'UTF8')) <> 0
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  ELSIF v_level = 't' THEN
    IF p_logical_table IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_table)
       OR p_logical_column IS NULL
       OR octet_length(convert_to(p_logical_column, 'UTF8')) <> 0
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  ELSE
    IF p_logical_table IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_table)
       OR p_logical_column IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_column)
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  END IF;
  IF p_identity_digest IS NULL OR octet_length(p_identity_digest) <> 32 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  v_digest := _oo.pm_sha256(_oo.pm_payload(
    p_source,
    p_config_digest,
    p_run_generation,
    v_level,
    p_logical_namespace,
    p_logical_table,
    p_logical_column
  ));
  IF p_identity_digest <> v_digest THEN
    RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
  END IF;
  v_physical := _oo.pm_physical(v_level, v_digest);
  IF v_physical IS NULL OR octet_length(v_physical) <> 63 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF v_level = 'n' THEN
    v_scope_ns := v_physical;
    v_scope_table := NULL;
  ELSE
    v_scope_ns := _oo.pm_physical('n', _oo.pm_sha256(_oo.pm_payload(
      p_source, p_config_digest, p_run_generation, 'n', p_logical_namespace, '', ''
    )));
    IF v_level = 't' THEN
      v_scope_table := v_physical;
    ELSE
      v_scope_table := _oo.pm_physical('t', _oo.pm_sha256(_oo.pm_payload(
        p_source,
        p_config_digest,
        p_run_generation,
        't',
        p_logical_namespace,
        p_logical_table,
        ''
      )));
    END IF;
  END IF;
  v_lock := hashtextextended(
    p_source || '|' || p_config_digest || '|' || p_run_generation::text || '|' || v_level
      || '|' || p_logical_namespace || '|' || p_logical_table || '|' || p_logical_column,
    0::bigint
  );
  IF v_lock = 6404202605::bigint THEN
    v_lock := v_lock + 1;
  END IF;
  PERFORM pg_advisory_xact_lock(v_lock);
  SELECT existing.*
    INTO v_existing
  FROM _oo.physical_mappings AS existing
  WHERE existing.source = p_source
    AND existing.config_digest = p_config_digest
    AND existing.run_generation = p_run_generation
    AND existing.level = v_level
    AND existing.logical_namespace = p_logical_namespace
    AND existing.logical_table = p_logical_table
    AND existing.logical_column = p_logical_column;
  IF FOUND THEN
    IF v_existing.identity_digest = v_digest
       AND v_existing.physical_name = v_physical
       AND v_existing.algorithm = 'physical-mapping/v1'
       AND v_existing.scope_namespace_physical = v_scope_ns
       AND v_existing.scope_table_physical IS NOT DISTINCT FROM v_scope_table
    THEN
      RETURN 'replay';
    END IF;
    RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
  END IF;
  IF v_level IN ('t', 'c') THEN
    SELECT existing.physical_name
      INTO v_found
    FROM _oo.physical_mappings AS existing
    WHERE existing.source = p_source
      AND existing.config_digest = p_config_digest
      AND existing.run_generation = p_run_generation
      AND existing.level = 'n'
      AND existing.logical_namespace = p_logical_namespace
      AND existing.logical_table = ''
      AND existing.logical_column = '';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
    END IF;
    IF v_found IS DISTINCT FROM v_scope_ns THEN
      RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
    END IF;
  END IF;
  IF v_level = 'c' THEN
    SELECT existing.physical_name
      INTO v_found
    FROM _oo.physical_mappings AS existing
    WHERE existing.source = p_source
      AND existing.config_digest = p_config_digest
      AND existing.run_generation = p_run_generation
      AND existing.level = 't'
      AND existing.logical_namespace = p_logical_namespace
      AND existing.logical_table = p_logical_table
      AND existing.logical_column = '';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
    END IF;
    IF v_found IS DISTINCT FROM v_scope_table THEN
      RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
    END IF;
  END IF;
  BEGIN
    PERFORM set_config('_oo.physical_mapping_writer', 'register', true);
    INSERT INTO _oo.physical_mappings (
      source,
      config_digest,
      run_generation,
      level,
      logical_namespace,
      logical_table,
      logical_column,
      identity_digest,
      physical_name,
      scope_namespace_physical,
      scope_table_physical,
      algorithm,
      registered_at
    ) VALUES (
      p_source,
      p_config_digest,
      p_run_generation,
      v_level,
      p_logical_namespace,
      p_logical_table,
      p_logical_column,
      v_digest,
      v_physical,
      v_scope_ns,
      v_scope_table,
      'physical-mapping/v1',
      clock_timestamp()
    );
    PERFORM set_config('_oo.physical_mapping_writer', '', true);
  EXCEPTION
    WHEN unique_violation THEN
      PERFORM set_config('_oo.physical_mapping_writer', '', true);
      SELECT existing.*
        INTO v_existing
      FROM _oo.physical_mappings AS existing
      WHERE existing.source = p_source
        AND existing.config_digest = p_config_digest
        AND existing.run_generation = p_run_generation
        AND existing.level = v_level
        AND existing.logical_namespace = p_logical_namespace
        AND existing.logical_table = p_logical_table
        AND existing.logical_column = p_logical_column;
      IF FOUND
         AND v_existing.identity_digest = v_digest
         AND v_existing.physical_name = v_physical
         AND v_existing.algorithm = 'physical-mapping/v1'
         AND v_existing.scope_namespace_physical = v_scope_ns
         AND v_existing.scope_table_physical IS NOT DISTINCT FROM v_scope_table
      THEN
        RETURN 'replay';
      END IF;
      RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
    WHEN OTHERS THEN
      PERFORM set_config('_oo.physical_mapping_writer', '', true);
      RAISE;
  END;
  RETURN 'inserted';
END;
$pm$;

COMMENT ON FUNCTION _oo.register_physical_mapping(
  text, text, bigint, text, text, text, text, bytea
) IS
  'Binds one pgident/v1 logical name. An identical call returns replay and leaves registered_at in place. A different digest or a stored row that does not match raises OO022. Lookup is a separate function and does not hash.';

CREATE FUNCTION _oo.lookup_physical_mapping(
  p_source text,
  p_config_digest text,
  p_run_generation bigint,
  p_level text,
  p_logical_namespace text,
  p_logical_table text,
  p_logical_column text
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $pm$
DECLARE
  v_level text;
  v_name text;
BEGIN
  IF current_setting('server_encoding') <> 'UTF8' THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_source IS NULL OR NOT _oo.pm_source_ok(p_source) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_config_digest IS NULL OR NOT _oo.pm_config_ok(p_config_digest) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_run_generation IS NULL OR p_run_generation < 1 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_level IS NULL THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF convert_to(p_level, 'UTF8') = convert_to('namespace', 'UTF8') THEN
    v_level := 'n';
  ELSIF convert_to(p_level, 'UTF8') = convert_to('table', 'UTF8') THEN
    v_level := 't';
  ELSIF convert_to(p_level, 'UTF8') = convert_to('column', 'UTF8') THEN
    v_level := 'c';
  ELSE
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_logical_namespace IS NULL OR NOT _oo.pm_logical_ok(p_logical_namespace) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF v_level = 'n' THEN
    IF p_logical_table IS NULL
       OR octet_length(convert_to(p_logical_table, 'UTF8')) <> 0
       OR p_logical_column IS NULL
       OR octet_length(convert_to(p_logical_column, 'UTF8')) <> 0
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  ELSIF v_level = 't' THEN
    IF p_logical_table IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_table)
       OR p_logical_column IS NULL
       OR octet_length(convert_to(p_logical_column, 'UTF8')) <> 0
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  ELSE
    IF p_logical_table IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_table)
       OR p_logical_column IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_column)
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  END IF;
  SELECT existing.physical_name
    INTO v_name
  FROM _oo.physical_mappings AS existing
  WHERE existing.source = p_source
    AND existing.config_digest = p_config_digest
    AND existing.run_generation = p_run_generation
    AND existing.level = v_level
    AND existing.logical_namespace = p_logical_namespace
    AND existing.logical_table = p_logical_table
    AND existing.logical_column = p_logical_column;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
  END IF;
  RETURN v_name;
END;
$pm$;

COMMENT ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) IS
  'Returns the stored PostgreSQL identifier for one logical identity. A missing row raises OO024. This function does not derive a name.';

REVOKE ALL ON TABLE _oo.physical_mappings FROM PUBLIC;
REVOKE ALL ON TABLE _oo.physical_mappings FROM oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_rotr(bigint, integer) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_sha256(bytea) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_source_ok(text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_config_ok(text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_lower_hex(text, integer) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_logical_ok(text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_payload(text, text, bigint, text, text, text, text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_physical(text, bytea) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.reject_physical_mapping_write() FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.register_physical_mapping(text, text, bigint, text, text, text, text, bytea) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.register_physical_mapping(text, text, bigint, text, text, text, text, bytea) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_pipeline_runtime;
