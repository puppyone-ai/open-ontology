-- Run once as the container bootstrap login (sa).
-- The runner substitutes __DBA_PASSWORD__ in a private copy. The token stays in git.
CREATE LOGIN oo_dl01w1_09_dba
    WITH PASSWORD = N'__DBA_PASSWORD__',
         CHECK_POLICY = ON,
         CHECK_EXPIRATION = OFF;
ALTER SERVER ROLE sysadmin ADD MEMBER oo_dl01w1_09_dba;
