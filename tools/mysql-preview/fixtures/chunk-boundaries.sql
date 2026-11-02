SET NAMES utf8mb4;
CREATE DATABASE chunk_fixture;
USE chunk_fixture;
CREATE TABLE items(id varchar(32) CHARACTER SET utf8 COLLATE utf8_general_ci NOT NULL PRIMARY KEY, payload text);
INSERT INTO items VALUES ('A1','a'),('b1 ','b'),('中文','c'),('z1','d');
CREATE TABLE pairs(a varchar(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,b int NOT NULL,PRIMARY KEY(a,b));
INSERT INTO pairs VALUES ('A1',1),('a1',2),('中文',1),('z',3);
SELECT VERSION(), @@log_bin, @@general_log;
DELIMITER //
CREATE PROCEDURE assert_collation()
BEGIN
    DECLARE duplicate_seen BOOLEAN DEFAULT FALSE;
    BEGIN
        DECLARE CONTINUE HANDLER FOR 1062 SET duplicate_seen = TRUE;
        INSERT INTO items VALUES ('a1','duplicate');
    END;
    IF NOT duplicate_seen THEN SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='case duplicate accepted'; END IF;
    SET duplicate_seen = FALSE;
    BEGIN
        DECLARE CONTINUE HANDLER FOR 1062 SET duplicate_seen = TRUE;
        INSERT INTO items VALUES ('b1','duplicate');
    END;
    IF NOT duplicate_seen THEN SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='pad duplicate accepted'; END IF;
    IF CONVERT(CONVERT(_utf8mb4'中文' USING gbk) USING utf8mb4) != _utf8mb4'中文' THEN
        SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='gbk conversion failed';
    END IF;
    IF CONVERT(CONVERT(_utf8mb4'中文' USING ucs2) USING utf8mb4) != _utf8mb4'中文' THEN
        SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='ucs2 conversion failed';
    END IF;
END//
DELIMITER ;
CALL assert_collation();
SELECT MAX(id) FROM (SELECT id FROM items ORDER BY id LIMIT 2) x;
EXPLAIN SELECT MAX(id) FROM (SELECT id FROM items WHERE id > 'A1' ORDER BY id LIMIT 2) x;
EXPLAIN SELECT * FROM items WHERE id > 'A1' AND id <= 'b1 ' ORDER BY id;
EXPLAIN SELECT a,b FROM pairs WHERE a > 'A1' OR (a = 'A1' AND b > 1) ORDER BY a,b LIMIT 2;
SELECT a,b FROM (SELECT a,b FROM pairs WHERE a > 'A1' OR (a = 'A1' AND b > 1) ORDER BY a,b LIMIT 2) x ORDER BY a DESC,b DESC LIMIT 1;
