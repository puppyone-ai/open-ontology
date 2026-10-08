\pset pager off
\echo === review-once row ===
SELECT state, attempt, generation, lease_owner IS NULL AS owner_cleared, last_error
FROM jobs.tasks
WHERE project_id = 'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid
  AND dedupe_key = 'review-once';
\echo === lease span ===
SELECT floor(extract(epoch FROM (c.lease_until - c.claimed_at)))::bigint AS lease_span_seconds,
       c.generation, c.owner
FROM jw.claims c
JOIN jobs.tasks t ON t.id = c.task_id
WHERE t.dedupe_key = 'review-once'
ORDER BY c.generation;
\echo === effect rows for review-once ===
SELECT e.seq, e.generation, e.owner, e.txid
FROM jw.effects e
JOIN jobs.tasks t ON t.id = e.task_id
WHERE t.dedupe_key = 'review-once'
ORDER BY e.seq;
\echo === tables touched ===
SELECT n.nspname || '.' || c.relname AS rel, s.n_tup_ins, s.n_tup_upd
FROM pg_stat_user_tables s
JOIN pg_class c ON c.oid = s.relid
JOIN pg_namespace n ON n.oid = c.relnamespace
ORDER BY 1;
\echo === repeat complete ===
SELECT r.status AS repeat_complete
FROM jw.claims c
JOIN jobs.tasks t ON t.id = c.task_id
CROSS JOIN LATERAL jobs.complete_success(t.project_id, t.id, c.owner, c.generation) r
WHERE t.dedupe_key = 'review-once'
  AND c.generation = 1;
\echo === effect txids ===
SELECT count(*)::bigint AS effect_rows, count(DISTINCT txid)::bigint AS effect_txids
FROM jw.effects;
