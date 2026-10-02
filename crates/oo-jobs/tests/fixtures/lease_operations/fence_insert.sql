\ir prelude.sql
INSERT INTO jobs.leases (project_id, target_key)
VALUES ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', 'fence-key')
RETURNING 'fence|' || target_fence_certified::text;
