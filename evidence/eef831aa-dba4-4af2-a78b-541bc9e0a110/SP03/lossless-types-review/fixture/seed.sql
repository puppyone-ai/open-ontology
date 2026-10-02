\set ON_ERROR_STOP on
CREATE SCHEMA lab;
REVOKE CREATE ON SCHEMA lab FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;

CREATE DOMAIN lab.flag_domain AS boolean;
CREATE DOMAIN lab.d_int AS integer;
CREATE DOMAIN lab.d_outer AS lab.d_int;
CREATE TYPE lab.hue AS ENUM ('red', 'blue', '雪');
CREATE TYPE lab.box AS (a integer);
CREATE TYPE lab.span AS RANGE (subtype = integer);

CREATE TABLE lab.canary_sink (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    note text
);
REVOKE ALL ON TABLE lab.canary_sink FROM PUBLIC;

CREATE FUNCTION lab.touch_canary() RETURNS void
LANGUAGE plpgsql
AS $fn$
BEGIN
    INSERT INTO lab.canary_sink (note) VALUES ('called');
END
$fn$;
REVOKE ALL ON FUNCTION lab.touch_canary() FROM PUBLIC;

CREATE TABLE lab.anchor (
    id integer PRIMARY KEY,
    n numeric(10, -2) NOT NULL,
    label text NOT NULL,
    flag boolean NOT NULL
);
INSERT INTO lab.anchor (id, n, label, flag) VALUES
    (1, 1200, '雪', true),
    (2, 0, '', false);

CREATE TABLE lab.budget_rows (
    id integer PRIMARY KEY,
    v integer NOT NULL
);
INSERT INTO lab.budget_rows (id, v) VALUES (1, 10), (2, 20), (3, 30);

CREATE TABLE lab.gold (
    id integer PRIMARY KEY,
    small int2 NOT NULL,
    mid int4 NOT NULL,
    big int8 NOT NULL,
    ratio float8 NOT NULL,
    narrow float4 NOT NULL,
    hi float8 NOT NULL,
    units numeric(10, -2) NOT NULL,
    fine numeric(4, 6) NOT NULL,
    wide numeric(40, 0) NOT NULL,
    place numeric(40, 0) NOT NULL,
    huge numeric(80, 10) NOT NULL,
    open_num numeric NOT NULL,
    negzero numeric NOT NULL,
    label text NOT NULL,
    blank text,
    code varchar(8) NOT NULL,
    padded char(3) NOT NULL,
    blob bytea NOT NULL,
    ext uuid NOT NULL,
    day date NOT NULL,
    clock time NOT NULL,
    endclock time NOT NULL,
    civil timestamp NOT NULL,
    instant timestamptz NOT NULL,
    instant_later timestamptz NOT NULL,
    span interval NOT NULL,
    nums int4[] NOT NULL,
    grid int4[] NOT NULL,
    empty_nums int4[] NOT NULL,
    flags lab.flag_domain[] NOT NULL,
    hues lab.hue[] NOT NULL,
    hue lab.hue NOT NULL,
    on_flag lab.flag_domain NOT NULL,
    outer_n lab.d_outer NOT NULL,
    doc json NOT NULL,
    docb jsonb NOT NULL,
    jnull json,
    note text,
    token text NOT NULL,
    bad_num numeric NOT NULL,
    far_day date NOT NULL,
    "paid;drop" money,
    pack lab.box,
    rng lab.span
);

INSERT INTO lab.gold (
    id, small, mid, big, ratio, narrow, hi, units, fine, wide, place, huge,
    open_num, negzero, label, blank, code, padded, blob, ext, day, clock,
    endclock, civil, instant, instant_later, span, nums, grid, empty_nums,
    flags, hues, hue, on_flag, outer_n, doc, docb, jnull, note, token,
    bad_num, far_day, "paid;drop", pack, rng
) VALUES (
    1, -32768, -2147483648, -9223372036854775808,
    -0.0::float8, -0.0::float4, 'Infinity'::float8,
    1200, 0.0012,
    -9999999999999999999999999999999999999999,
    10000,
    12345678901234567890.1234567890,
    '1.20'::numeric, '0'::numeric,
    '雪', '', 'ab', 'ab',
    '\x0000ff'::bytea,
    '12345678-1234-5678-1234-567812345678',
    DATE '2000-01-01',
    TIME '01:02:03.000004',
    TIME '00:00:00',
    TIMESTAMP '2024-03-10 02:30:00',
    TIMESTAMPTZ '2024-11-03 01:30:00-04',
    TIMESTAMPTZ '2024-11-03 01:30:00-05',
    INTERVAL '3 months 4 days 0.000005 seconds',
    '[2:4]={7,NULL,9}'::int4[],
    '[0:1][2:3]={{1,2},{3,4}}'::int4[],
    '{}'::int4[],
    ARRAY[true, NULL, false]::lab.flag_domain[],
    '[0:1]={red,blue}'::lab.hue[],
    'red', true, 7,
    '{"z":1,"a":2,"a":3}'::json,
    '{"a":3,"z":1}'::jsonb,
    'null'::json,
    'kept',
    'CANARY-TOKEN-a110',
    'NaN'::numeric,
    'infinity'::date,
    0::money,
    ROW(1)::lab.box,
    '[1,4)'::lab.span
), (
    2, 32767, 2147483647, 9223372036854775807,
    0.0::float8, 'NaN'::float4, '-Infinity'::float8,
    0, 0,
    0, 1,
    0,
    '1.2300'::numeric, '-0'::numeric,
    'x', NULL, 'cd', 'ab',
    ''::bytea,
    '00000000-0000-0000-0000-000000000000',
    DATE '1970-01-01',
    TIME '00:00:00',
    TIME '24:00:00',
    TIMESTAMP '1970-01-01 00:00:00',
    TIMESTAMPTZ '1970-01-01 00:00:00+00',
    TIMESTAMPTZ '1970-01-01 00:00:00+00',
    INTERVAL '0',
    '{}'::int4[],
    '{{5}}'::int4[],
    '{}'::int4[],
    ARRAY[false]::lab.flag_domain[],
    ARRAY['雪']::lab.hue[],
    '雪', false, -3,
    '{}'::json,
    '{}'::jsonb,
    NULL,
    NULL,
    'CANARY-TOKEN-a110',
    0,
    DATE '2000-01-01',
    NULL,
    NULL,
    NULL
);

COMMENT ON TABLE lab.gold IS 'CANARY-COMMENT-a110';
COMMENT ON COLUMN lab.gold.token IS 'CANARY-TOKEN-a110';

GRANT USAGE ON SCHEMA lab TO oo_a110_reader;
GRANT SELECT ON TABLE lab.anchor, lab.gold, lab.budget_rows TO oo_a110_reader;
