\set ON_ERROR_STOP on
SET ROLE oo_target_owner;
INSERT INTO _oo.apply_receipts (source) VALUES ('warehouse');
