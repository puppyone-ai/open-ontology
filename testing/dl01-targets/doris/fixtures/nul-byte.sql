-- Observational. A VARCHAR key is not a binary key. NUL may be rejected or truncated.
USE oo_dl01w1_12;
INSERT INTO str_key VALUES (CHAR(0), 'nul');
