-- Fixture admin seed. The OO reader does not execute this file.
CREATE SCHEMA lab;
REVOKE CREATE ON SCHEMA lab FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA lab TO oo_type_reader;

CREATE DOMAIN lab.flag_domain AS boolean;
CREATE DOMAIN lab."bad;drop" AS integer;
CREATE TYPE lab.hue AS ENUM ('red', 'blue');
CREATE TYPE lab.box AS (a integer);

CREATE TABLE lab.sample (
    id integer PRIMARY KEY,
    flag boolean NOT NULL,
    small smallint NOT NULL,
    big bigint NOT NULL,
    ratio double precision NOT NULL,
    narrow real NOT NULL,
    hi double precision NOT NULL,
    units numeric(10,-2) NOT NULL,
    fine numeric(4,6) NOT NULL,
    wide numeric(40,0) NOT NULL,
    huge numeric(80,4) NOT NULL,
    open_num numeric NOT NULL,
    label text NOT NULL,
    blank text NOT NULL,
    code varchar(8) NOT NULL,
    padded char(3) NOT NULL,
    blob bytea NOT NULL,
    ext uuid NOT NULL,
    day date NOT NULL,
    clock time NOT NULL,
    civil timestamp NOT NULL,
    instant timestamp with time zone NOT NULL,
    instant_later timestamp with time zone NOT NULL,
    span interval NOT NULL,
    nums integer[] NOT NULL,
    grid integer[] NOT NULL,
    hue lab.hue NOT NULL,
    on_flag lab.flag_domain NOT NULL,
    doc json NOT NULL,
    docb jsonb NOT NULL,
    note text,
    bad_num numeric,
    far_day date,
    "paid;drop" money,
    pack lab.box
);

INSERT INTO lab.sample VALUES (
    1,
    true,
    -32768,
    -9223372036854775808,
    '-0'::float8,
    '-0'::float4,
    1.5::float8,
    1200::numeric(10,-2),
    0.0012::numeric(4,6),
    '-9999999999999999999999999999999999999999'::numeric(40,0),
    1.5::numeric(80,4),
    1.20::numeric,
    '雪',
    '',
    'hi',
    'ab',
    ''::bytea,
    '00000000-0000-0000-0000-000000000001'::uuid,
    DATE '2000-01-01',
    TIME '01:02:03.000004',
    TIMESTAMP '2024-03-10 02:30:00',
    TIMESTAMPTZ '2024-11-03 01:30:00-04',
    TIMESTAMPTZ '2024-11-03 01:30:00-05',
    INTERVAL '3 months 4 days 0.000005 seconds',
    '[2:4]={7,NULL,9}'::int4[],
    '[0:1][2:3]={{1,2},{3,4}}'::int4[],
    'red',
    true,
    '{"z":1,"a":2,"a":3}'::json,
    '{"z":1,"a":2,"a":3}'::jsonb,
    NULL,
    NULL,
    NULL,
    NULL,
    ROW(1)::lab.box
);

INSERT INTO lab.sample VALUES (
    2,
    false,
    32767,
    9223372036854775807,
    0::float8,
    'NaN'::float4,
    'Infinity'::float8,
    0::numeric(10,-2),
    0::numeric(4,6),
    0::numeric(40,0),
    0::numeric(80,4),
    0::numeric,
    'x',
    'y',
    'z',
    'q',
    '\x00ff'::bytea,
    '00000000-0000-0000-0000-000000000002'::uuid,
    DATE '1970-01-01',
    TIME '00:00:00',
    TIMESTAMP '1970-01-01 00:00:00',
    TIMESTAMPTZ '1970-01-01 00:00:00+00',
    TIMESTAMPTZ '1970-01-01 00:00:00+00',
    INTERVAL '0',
    '{1}'::int4[],
    '{1}'::int4[],
    'blue',
    false,
    'null'::json,
    'null'::jsonb,
    'kept',
    'NaN'::numeric,
    'infinity'::date,
    NULL,
    NULL
);

REVOKE ALL ON TABLE lab.sample FROM PUBLIC;
GRANT SELECT ON TABLE lab.sample TO oo_type_reader;

COMMENT ON TABLE lab.sample IS 'CANARY-COMMENT-SECRET-a098';
COMMENT ON COLUMN lab.sample.label IS 'CANARY-TOKEN-a098';
