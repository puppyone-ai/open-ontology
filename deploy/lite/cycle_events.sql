-- Lite-owned extension. Existing jobs migrations and their audit remain intact.
BEGIN;
SET LOCAL ROLE oo_jobs_owner;
CREATE TABLE IF NOT EXISTS jobs.lite_cycle_events (
    event_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id uuid NOT NULL,
    task_id uuid NOT NULL REFERENCES jobs.tasks(id),
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object')
);
REVOKE ALL ON jobs.lite_cycle_events FROM PUBLIC, oo_jobs_runtime;
GRANT SELECT ON jobs.lite_cycle_events TO oo_jobs_runtime;

CREATE OR REPLACE FUNCTION jobs.complete_lite_cycle(
    p_project uuid, p_task uuid, p_worker text, p_generation bigint, p_payload jsonb
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, jobs AS $$
DECLARE
    finished text;
    body jsonb;
BEGIN
    IF p_payload IS NULL OR jsonb_typeof(p_payload) <> 'object'
       OR octet_length(p_payload::text) > 1048576
       OR p_payload->>'result' IS NULL
       OR p_payload->>'result' NOT IN ('succeeded','partial','review_required','failed')
       OR (p_payload ? 'published_generation' AND p_payload->>'published_generation' IS NOT NULL
           AND p_payload->>'published_generation' !~ '^[A-Za-z0-9_-]{1,64}$')
       OR NOT EXISTS (SELECT 1 FROM jobs.tasks WHERE id=p_task
                      AND project_id=p_project AND kind='lite_retained') THEN
        RAISE EXCEPTION 'invalid lite cycle payload';
    END IF;
    IF p_payload->>'result' IN ('succeeded','partial') THEN
        SELECT status INTO finished FROM jobs.complete_success(p_project,p_task,p_worker,p_generation);
    ELSE
        SELECT status INTO finished FROM jobs.complete_failure(
            p_project,p_task,p_worker,p_generation,'dependency_unavailable');
    END IF;
    IF finished NOT IN ('succeeded','failed') THEN RETURN finished; END IF;
    body := jsonb_build_object(
        'result',p_payload->'result','reason',p_payload->'reason',
        'stale_tables',coalesce(p_payload->'stale_tables','[]'::jsonb),
        'review_pending',coalesce(p_payload->'review_pending','[]'::jsonb),
        'data_published_at',p_payload->'data_published_at',
        -- Last time a cycle confirmed the published data is current (activated
        -- or an exact replay); an unchanged source keeps this moving.
        'data_verified_at',p_payload->'data_verified_at',
        'published_generation',p_payload->'published_generation');
    IF p_payload->>'result' IN ('failed','review_required')
       AND p_payload->>'data_published_at' IS NULL THEN
        IF body->'stale_tables' = '[]'::jsonb THEN
            body := jsonb_set(body,'{stale_tables}',coalesce((
                SELECT payload->'stale_tables' FROM jobs.lite_cycle_events
                WHERE project_id=p_project ORDER BY event_id DESC LIMIT 1),'[]'::jsonb));
        END IF;
        IF body->'review_pending' = '[]'::jsonb THEN
            body := jsonb_set(body,'{review_pending}',coalesce((
                SELECT payload->'review_pending' FROM jobs.lite_cycle_events
                WHERE project_id=p_project ORDER BY event_id DESC LIMIT 1),'[]'::jsonb));
        END IF;
    END IF;
    -- Preserve the beginning of a consecutive stale interval, not the last retry.
    body := jsonb_set(body,'{stale_tables}',coalesce((
        SELECT jsonb_agg(s || jsonb_build_object('since',coalesce(
            (SELECT prior->'since' FROM jsonb_array_elements((
                SELECT payload->'stale_tables' FROM jobs.lite_cycle_events
                WHERE project_id=p_project ORDER BY event_id DESC LIMIT 1)) prior
             WHERE prior->'table'=s->'table' LIMIT 1),
            s->'since',to_jsonb(clock_timestamp()))))
        FROM jsonb_array_elements(body->'stale_tables') s),'[]'::jsonb));
    INSERT INTO jobs.lite_cycle_events(project_id,task_id,payload)
        VALUES(p_project,p_task,body);
    RETURN finished;
END;
$$;
REVOKE ALL ON FUNCTION jobs.complete_lite_cycle(uuid,uuid,text,bigint,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION jobs.complete_lite_cycle(uuid,uuid,text,bigint,jsonb) TO oo_jobs_runtime;
COMMIT;
