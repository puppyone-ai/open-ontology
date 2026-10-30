DROP TABLE IF EXISTS r;
CREATE TABLE r (
  c_bool bool, c_i2 int2, c_i4 int4, c_i8 int8, c_u64 numeric(20,0),
  c_dec numeric(10,2), c_dec0 numeric(30,4), c_dec1 numeric(38,10), c_dec_zero numeric(10,4), c_dec_wide numeric(65,30),
  c_f4 real, c_f8neg0 float8, c_f8big float8, c_f8small float8,
  c_varchar varchar(255), c_varchar_sp varchar(255), c_bpchar char(3),
  c_json json, c_bytea bytea,
  c_date date, c_date_min date,
  c_dt timestamp, c_dt_pre timestamp,
  c_tstz timestamptz,
  c_time_neg interval, c_time_pos interval, c_time_zero interval,
  c_null int
);
SET TIME ZONE 'Asia/Shanghai';
INSERT INTO r VALUES (
  true, -32768, 2147483647, -9223372036854775808, 18446744073709551615,
  -12.3, 0.0001, 1234.5, 0.0000, -12345678901234567890123456789012345.123456789012345678901234567890,
  0.1, '-0'::float8, 1.5e300, 1e-7,
  'héllo 世界 😀', 'a  ', 'a',
  '{"b": 1.0, "a": [true, null]}', '\x00ff10',
  '2024-02-29', '0001-01-01',
  '2024-01-02 03:04:05.000001', '1969-12-31 23:59:59.5',
  '2024-01-02 03:04:05.123456+00',
  '-838:59:59', '12:34:56.000007', '00:00:00',
  NULL
);
COPY r TO STDOUT;
SELECT json_agg(json_build_object('name', attname, 'oid', atttypid::int, 'typmod', atttypmod) ORDER BY attnum)::text
FROM pg_attribute WHERE attrelid = 'r'::regclass AND attnum > 0 AND NOT attisdropped;
