\set ON_ERROR_STOP on
SET ROLE oo_target_owner;
UPDATE _oo.schema_migrations
SET phase = 'migrate'
WHERE version = 'target-fence-receipts/1';
