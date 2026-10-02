SET ROLE oo_target_owner;
ALTER TABLE _oo.apply_receipts DROP CONSTRAINT apply_receipts_segment_check;
RESET ROLE;
