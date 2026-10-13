-- full-import-jsonb/v1
-- Run as oo_target_owner after full-import/v1. This file does not rewrite
-- 0000-0008, does not copy rows, and does not create a publication.
-- oid 3802 typmod -1 is jsonb. oid 114 stays json. Any other jsonb typmod
-- is closed. CREATE OR REPLACE keeps the existing function owner and ACLs.

DO $fi$
BEGIN
  IF pg_catalog.to_regprocedure('_oo.full_import_type_sql(bigint, integer)') IS NULL THEN
    RAISE EXCEPTION 'full import jsonb requires full-import/v1'
      USING ERRCODE = 'OO002';
  END IF;
END
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
