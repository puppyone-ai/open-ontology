SET NAMES utf8mb4;
SET time_zone = '+00:00';

SELECT CONCAT('unsigned|', CAST(id AS CHAR), '|', label)
FROM k_unsigned
ORDER BY id;

SELECT CONCAT('decimal|', CAST(id AS CHAR), '|', CAST(payload AS CHAR))
FROM k_decimal
ORDER BY id;

SELECT CONCAT(
  'null|', CAST(id AS CHAR), '|',
  IFNULL(note, '<NULL>'), '|',
  IFNULL(CAST(qty AS CHAR), '<NULL>'), '|',
  IFNULL(CAST(money AS CHAR), '<NULL>'), '|',
  IF(payload IS NULL, '<NULL>', JSON_COMPACT(payload)), '|',
  IF(when_at IS NULL, '<NULL>', DATE_FORMAT(when_at, '%Y-%m-%d %H:%i:%s.%f')), '|',
  IF(blob_key IS NULL, '<NULL>', HEX(blob_key))
)
FROM k_null
ORDER BY id;

SELECT CONCAT('null_unique|', CAST(id AS CHAR), '|', IFNULL(code, '<NULL>'))
FROM k_null_unique
ORDER BY id;

SELECT CONCAT(
  'json|', CAST(id AS CHAR), '|',
  IF(doc IS NULL, '<SQLNULL>', JSON_COMPACT(doc)), '|',
  IF(doc IS NULL, '<SQLNULL>', JSON_TYPE(doc)), '|',
  IFNULL(CAST(JSON_VALID(doc) AS CHAR), '<NULL>')
)
FROM k_json
ORDER BY id;

SELECT CONCAT(
  'temporal|',
  DATE_FORMAT(id, '%Y-%m-%d %H:%i:%s.%f'), '|',
  DATE_FORMAT(d, '%Y-%m-%d'), '|',
  CAST(t AS CHAR), '|',
  IF(ts IS NULL, '<NULL>', DATE_FORMAT(ts, '%Y-%m-%d %H:%i:%s.%f')), '|',
  note
)
FROM k_temporal
ORDER BY id;

SELECT CONCAT(
  'binary|',
  IF(id = '', '<EMPTY>', HEX(id)), '|',
  label
)
FROM k_binary
ORDER BY id;
