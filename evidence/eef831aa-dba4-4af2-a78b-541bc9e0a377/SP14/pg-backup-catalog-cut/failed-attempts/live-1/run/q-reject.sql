\set VERBOSITY verbose
SET lock_timeout = '12s';
UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3;
