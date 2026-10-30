-- Read access only: target CAS validates actual stopped/drained control authority.
DO $identity_access$
BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_owner') THEN
  GRANT USAGE ON SCHEMA jw_native TO oo_target_owner;
  GRANT SELECT ON jw_native.bindings,jw_native.occurrences,jw_native.source_progress,
   jw_native.watermarks,jw_native.pending_acks TO oo_target_owner;
  -- PostgreSQL row locking requires UPDATE on at least one column. The rebind
  -- function only locks these control rows; the worker performs its own CAS.
  GRANT UPDATE(manifest_sha) ON jw_native.bindings TO oo_target_owner;
  GRANT UPDATE(confirmed_at) ON jw_native.source_progress TO oo_target_owner;
 END IF;
END $identity_access$;
