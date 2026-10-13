-- Decimal literals are strings cast to DECIMAL so the parser does not round through float.
USE oo_dl01w1_12;

INSERT INTO dup_events VALUES
  (7, 'a', CAST('1.50' AS DECIMAL(10,2))),
  (7, 'b', CAST('2.25' AS DECIMAL(10,2))),
  (8, 'same', CAST('3.00' AS DECIMAL(10,2))),
  (8, 'same', CAST('3.00' AS DECIMAL(10,2))),
  (9, 'nil', NULL);

INSERT INTO agg_qty VALUES ('A', CAST('10.00' AS DECIMAL(10,2)), 'first');
INSERT INTO agg_qty VALUES ('A', CAST('5.00' AS DECIMAL(10,2)), 'second');
INSERT INTO agg_qty VALUES ('B', NULL, 'only');
INSERT INTO agg_qty VALUES ('B', CAST('4.00' AS DECIMAL(10,2)), 'four');
INSERT INTO agg_qty VALUES ('C', NULL, 'none');
INSERT INTO agg_qty VALUES ('D', CAST('0.00' AS DECIMAL(10,2)), 'zero');
INSERT INTO agg_qty VALUES ('E', CAST('1.00' AS DECIMAL(10,2)), 'kept');
INSERT INTO agg_qty VALUES ('E', NULL, NULL);

INSERT INTO uniq_acct VALUES (1, CAST('10.5000' AS DECIMAL(18,4)), 'old');
INSERT INTO uniq_acct VALUES (1, CAST('11.7500' AS DECIMAL(18,4)), 'new');
INSERT INTO uniq_acct VALUES (2, CAST('0.0001' AS DECIMAL(18,4)), 'tiny');
INSERT INTO uniq_acct VALUES (3, NULL, 'missing');
INSERT INTO uniq_acct VALUES (4, CAST('0.0000' AS DECIMAL(18,4)), '');

INSERT INTO dec_exact VALUES
  (1, CAST('0.0001' AS DECIMAL(18,4)), NULL),
  (2, CAST('-1.2500' AS DECIMAL(18,4)), NULL),
  (3, CAST('12345678901234.6789' AS DECIMAL(18,4)), CAST('1234567890123456789012345678901234.5678' AS DECIMAL(38,4))),
  (4, CAST('0.0000' AS DECIMAL(18,4)), NULL),
  (5, NULL, NULL),
  (6, CAST('0.1000' AS DECIMAL(18,4)), NULL),
  (7, CAST('0.2000' AS DECIMAL(18,4)), NULL);

INSERT INTO part_day VALUES ('2026-01-15', 'east', CAST('10.00' AS DECIMAL(12,2)));
INSERT INTO part_day VALUES ('2026-02-02', 'east', CAST('20.50' AS DECIMAL(12,2)));
INSERT INTO part_day VALUES ('2026-02-20', 'west', NULL);

INSERT INTO str_key VALUES ('A', 'up');
INSERT INTO str_key VALUES ('A ', 'space');
INSERT INTO str_key VALUES ('B', NULL);
INSERT INTO str_key VALUES ('a', 'low');
INSERT INTO str_key VALUES ('b', NULL);
INSERT INTO str_key VALUES ('中', 'cjk');
INSERT INTO str_key VALUES ('', 'empty');
