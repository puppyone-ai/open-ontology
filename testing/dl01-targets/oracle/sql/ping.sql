SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 200
SELECT 'ping|' || open_mode FROM v$database;
SELECT 'pdb|' || name || '|' || open_mode FROM v$pdbs;
EXIT
