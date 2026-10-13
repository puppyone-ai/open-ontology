-- jobs-loader-hold/1. Execute as oo_jobs_owner after real lease operations.
-- The lock survives the function return until the caller transaction ends.
-- Runtime receives no direct UPDATE privilege on jobs.tasks.
CREATE FUNCTION jobs.loader_hold(p_task uuid, p_generation bigint)
RETURNS TABLE(id text, project_id text, state text, generation bigint,
              lease_owner text, live boolean)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, jobs
AS $hold$
BEGIN
  IF p_task IS NULL OR p_generation IS NULL OR p_generation < 1 THEN
    RAISE EXCEPTION 'claim identity required' USING ERRCODE = '22023';
  END IF;
  RETURN QUERY
    SELECT t.id::text, t.project_id::text, t.state, t.generation,
           t.lease_owner,
           COALESCE(t.lease_until > clock_timestamp(), false)
    FROM jobs.tasks t
    WHERE t.id = p_task AND t.generation = p_generation
    FOR UPDATE OF t;
END
$hold$;
REVOKE ALL ON FUNCTION jobs.loader_hold(uuid, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION jobs.loader_hold(uuid, bigint) TO oo_jobs_runtime;
COMMENT ON FUNCTION jobs.loader_hold(uuid, bigint) IS
  'Locks a real jobs.claim task for the enclosing target transaction. No lease is created, extended, moved across databases, or certified as a target fence.';
