-- w39_dba owns fixture setup. w39_reader is SELECT on oo_synth only.
-- The reader does not receive PROCESS or CONNECTION ADMIN.
GRANT ALL PRIVILEGES ON oo_synth.* TO 'w39_dba'@'localhost';
GRANT ALL PRIVILEGES ON oo_synth.* TO 'w39_dba'@'%';
GRANT SELECT ON mysql.general_log TO 'w39_dba'@'localhost';
GRANT SELECT ON mysql.general_log TO 'w39_dba'@'%';
GRANT PROCESS ON *.* TO 'w39_dba'@'localhost';
GRANT PROCESS ON *.* TO 'w39_dba'@'%';
GRANT CONNECTION ADMIN ON *.* TO 'w39_dba'@'localhost';
GRANT CONNECTION ADMIN ON *.* TO 'w39_dba'@'%';
GRANT SELECT ON oo_synth.* TO 'w39_reader'@'localhost';
GRANT SELECT ON oo_synth.* TO 'w39_reader'@'%';
FLUSH PRIVILEGES;
