-- Applied by the bootstrap root identity after the accounts exist.
-- oo_dba owns fixture DDL/DML, reads the statement log, and cancels the reader.
-- oo_reader receives SELECT on the fixture schema and nothing else.
GRANT ALL PRIVILEGES ON oo_synth.* TO 'oo_dba'@'localhost';
GRANT ALL PRIVILEGES ON oo_synth.* TO 'oo_dba'@'%';
GRANT SELECT ON mysql.general_log TO 'oo_dba'@'localhost';
GRANT SELECT ON mysql.general_log TO 'oo_dba'@'%';
-- PROCESS sees other sessions. CONNECTION ADMIN is the MariaDB privilege that
-- lets this fixture DBA run KILL QUERY against the reader. The reader has neither.
GRANT PROCESS ON *.* TO 'oo_dba'@'localhost';
GRANT PROCESS ON *.* TO 'oo_dba'@'%';
GRANT CONNECTION ADMIN ON *.* TO 'oo_dba'@'localhost';
GRANT CONNECTION ADMIN ON *.* TO 'oo_dba'@'%';
GRANT SELECT ON oo_synth.* TO 'oo_reader'@'localhost';
GRANT SELECT ON oo_synth.* TO 'oo_reader'@'%';
FLUSH PRIVILEGES;
