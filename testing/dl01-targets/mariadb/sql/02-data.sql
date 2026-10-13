-- Synthetic rows loaded by oo_dba. Values are the fixture oracle inputs.
USE oo_synth;
SET NAMES utf8mb4;
SET time_zone = '+00:00';

INSERT INTO k_unsigned (id, label) VALUES
  (0, 'zero'),
  (1, 'one'),
  (4294967295, 'uint32-max'),
  (9223372036854775807, 'int64-max'),
  (9223372036854775808, 'int64-max-plus-one'),
  (18446744073709551615, 'uint64-max');

INSERT INTO k_decimal (id, payload) VALUES
  (-1.5000000000, -1.000000000000000000000000000000),
  (-0.0000000001, 0.000000000000000000000000000001),
  (0.0000000000, 0.000000000000000000000000000000),
  (0.0000000001, 0.100000000000000000000000000000),
  (12345678901234567890.1234567890, 12345678901234567890123456789012345.123456789012345678901234567890),
  (99999999999999999999.9999999999, 99999999999999999999999999999999999.999999999999999999999999999999);

INSERT INTO k_null (id, note, qty, money, payload, when_at, blob_key) VALUES
  (1, 'present', 7, 10.50, '{"ok":true}', '2020-01-02 03:04:05.000006', x'AA'),
  (2, NULL, NULL, NULL, NULL, NULL, NULL),
  (3, NULL, 0, NULL, NULL, NULL, NULL);

INSERT INTO k_null_unique (id, code) VALUES
  (1, NULL),
  (2, NULL),
  (3, 'A');

INSERT INTO k_json (id, doc) VALUES
  (1, '{"k":"v","n":1,"ok":true}'),
  (2, '[1,2,3]'),
  (3, 'null'),
  (4, NULL),
  (5, '{"名":"甲","a":{"b":[true,false,null]}}'),
  (6, '{}');

INSERT INTO k_temporal (id, d, t, ts, note) VALUES
  ('1000-01-01 00:00:00.000000', '1000-01-01', '-838:59:59.000000', NULL, 'datetime-floor'),
  ('1970-01-01 00:00:01.000001', '1970-01-01', '00:00:00.000001', '1970-01-01 00:00:01.000001', 'epoch-plus'),
  ('2024-02-29 12:00:00.123456', '2024-02-29', '12:00:00.123456', '2024-02-29 12:00:00.123456', 'leap-day'),
  ('2038-01-19 03:14:07.000000', '2038-01-19', '03:14:07.000000', '2038-01-19 03:14:07.000000', 'timestamp-end'),
  ('9999-12-31 23:59:59.999999', '9999-12-31', '838:59:59.000000', NULL, 'datetime-ceil');

INSERT INTO k_binary (id, label) VALUES
  (x'', 'empty'),
  (x'00', 'nul'),
  (x'0000', 'nul-nul'),
  (x'00ff', 'nul-ff'),
  (x'41', 'A'),
  (x'61', 'a'),
  (x'e4b8ad', 'utf8-zhong'),
  (x'e4b8ae', 'utf8-next'),
  (x'ff', 'ff');
