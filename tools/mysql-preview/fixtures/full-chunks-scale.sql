USE fixture;
CREATE TABLE large_items(id varchar(32) CHARACTER SET utf8 COLLATE utf8_general_ci NOT NULL PRIMARY KEY,payload varchar(32) CHARACTER SET utf8mb4 NOT NULL);
INSERT INTO large_items VALUES ('key0000000000','value');
DELIMITER //
CREATE PROCEDURE seed_large()
BEGIN
    DECLARE base INT DEFAULT 1;
    DECLARE digit INT;
    WHILE base < 10000000 DO
        SET digit=1;
        WHILE digit < 10 DO
            INSERT INTO large_items SELECT CONCAT('key',LPAD(CAST(SUBSTRING(id,4) AS UNSIGNED)+base*digit,10,'0')),payload FROM large_items WHERE id < CONCAT('key',LPAD(base,10,'0'));
            SET digit=digit+1;
        END WHILE;
        SET base=base*10;
    END WHILE;
END//
DELIMITER ;
CALL seed_large();
SELECT COUNT(*),MIN(id),MAX(id),@@log_bin,@@general_log FROM large_items;
CREATE TABLE kill_capture LIKE large_items;
INSERT INTO kill_capture SELECT * FROM large_items ORDER BY id LIMIT 100000;
