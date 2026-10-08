\pset pager off
\echo === function gaps ===
SELECT p.proname,
       strpos(pg_get_functiondef(p.oid), 'pg_notify') AS notify_at,
       strpos(pg_get_functiondef(p.oid), 'random') AS random_at,
       strpos(pg_get_functiondef(p.oid), 'clock_timestamp') AS clock_at
FROM pg_proc p
JOIN pg_namespace n ON n.oid = p.pronamespace
WHERE n.nspname = 'jobs'
  AND p.proname = ANY (ARRAY[
    'enqueue', 'claim', 'heartbeat', 'complete_success',
    'complete_failure', 'retry_failure', 'cancel'
  ]::text[])
ORDER BY 1;
\echo === retry delay text ===
WITH def AS (
  SELECT regexp_replace(
           pg_get_functiondef('jobs.retry_failure(uuid,uuid,text,bigint,text)'::regprocedure),
           E'\\s+',
           ' ',
           'g'
         ) AS src
)
SELECT substr(src, strpos(src, 'attempt 1 waits'), 220) AS delay_text,
       strpos(src, 'random') AS random_at,
       strpos(src, 'pg_notify') AS notify_at,
       strpos(src, 'v_delay := 60') AS delay_60_at,
       strpos(src, '1::bigint <<') AS shift_at
FROM def;
