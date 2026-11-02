-- 0044: grant the (integer, integer) advisory-lock overload to oo_target_owner.
--
-- native_source_instance_register (0024) takes
--   pg_advisory_xact_lock(hashtext('oo-source-instance'), hashtext(p_source))
-- The two-argument integer overload. Bootstrap 0000 revokes every pg_advisory%
-- overload from PUBLIC and grants oo_target_owner only pg_advisory_xact_lock(bigint).
-- Without this grant the register call fails with 42501 on a fresh chain.
--
-- This runs as the installing principal, not as oo_target_owner: only the owner of a
-- pg_catalog function (a superuser at bootstrap) can grant on it. It grants nothing to
-- PUBLIC and no table privilege.
DO $$
BEGIN
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(integer, integer) TO oo_target_owner';
END
$$;
