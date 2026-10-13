SET NAMES utf8mb4;
SET time_zone = '+00:00';

SELECT CONCAT('count|unsigned|', COUNT(*)) FROM k_unsigned;
SELECT CONCAT('count|decimal|', COUNT(*)) FROM k_decimal;
SELECT CONCAT('count|null|', COUNT(*)) FROM k_null;
SELECT CONCAT('count|null_unique|', COUNT(*)) FROM k_null_unique;
SELECT CONCAT('count|json|', COUNT(*)) FROM k_json;
SELECT CONCAT('count|temporal|', COUNT(*)) FROM k_temporal;
SELECT CONCAT('count|binary|', COUNT(*)) FROM k_binary;
SELECT CONCAT('count|zero_probe|', COUNT(*)) FROM k_zero_probe;
SELECT CONCAT('null_all|', COUNT(*)) FROM k_null
  WHERE note IS NULL AND qty IS NULL AND money IS NULL AND payload IS NULL
    AND when_at IS NULL AND blob_key IS NULL;
SELECT CONCAT('null_codes|', COUNT(*)) FROM k_null_unique WHERE code IS NULL;
SELECT CONCAT('zero_qty|', COUNT(*)) FROM k_null WHERE qty = 0;
SELECT CONCAT('order|unsigned|', CAST(id AS CHAR)) FROM k_unsigned ORDER BY id;
SELECT CONCAT('order|decimal|', CAST(id AS CHAR)) FROM k_decimal ORDER BY id;
SELECT CONCAT('order|temporal|', DATE_FORMAT(id, '%Y-%m-%d %H:%i:%s.%f')) FROM k_temporal ORDER BY id;
SELECT CONCAT('order|binary|', IF(id = '', '<EMPTY>', HEX(id))) FROM k_binary ORDER BY id;
