-- docs-suggest/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-consistency/1, inside the
-- installer transaction (advisory lock 6413202611). No BEGIN or COMMIT here;
-- the installer inserts the schema_migrations row.
-- L5 Suggest (10 11.1, B-S7-01): what an Agent suggests, what the impact rules did with it, and the
-- per-day "which Brief used which fact" view. The `suggestion` review kind, the `suggestion_accept`
-- overlay kind, the `suggest` fact origin and the `suggest:write` credential scope exist since 0139/0140.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-consistency/1') THEN
    RAISE EXCEPTION 'docs-consistency/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE TABLE _oo_docs.suggestion (
  id uuid PRIMARY KEY,
  share_id uuid NOT NULL REFERENCES _oo_docs.share(id),
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive(id),
  kind text NOT NULL CHECK (kind IN ('new_fact','supersede','correction','preference','type_change')),
  payload jsonb NOT NULL,                      -- the oo.suggest/v1 record as normalised
  status text NOT NULL CHECK (status IN ('pending','auto_accepted','superseded_silently','accepted',
        'rejected','undone','duplicate')),
  rule text NOT NULL CHECK (rule IN ('new_fact','supersede_unused','supersede_in_use','manual')),
  reason text,                                 -- review_item.reason_code of a pending one
  idempotency_key text NOT NULL CHECK (char_length(idempotency_key) BETWEEN 8 AND 128),
  fact_id uuid REFERENCES _oo_docs.fact(id),   -- the fact the suggestion made
  old_fact_id uuid REFERENCES _oo_docs.fact(id), -- the fact it supersedes or corrects
  review_id uuid REFERENCES _oo_docs.review_item(id),
  credential_id uuid REFERENCES _oo_docs.refs_credential(id),
  submitted_by text NOT NULL CHECK (submitted_by IN ('credential','collector')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  decided_at timestamptz, decided_by text, decision_note text CHECK (char_length(decision_note) <= 500),
  revision bigint NOT NULL DEFAULT 1,
  UNIQUE (share_id, idempotency_key));
CREATE INDEX suggestion_day ON _oo_docs.suggestion (((created_at AT TIME ZONE 'UTC')::date), status);
CREATE INDEX suggestion_pending ON _oo_docs.suggestion (created_at) WHERE status = 'pending';
CREATE INDEX suggestion_fact ON _oo_docs.suggestion (fact_id) WHERE fact_id IS NOT NULL;

-- Which Share used which fact on which UTC day (a Share's pack is compiled when the Share is created).
CREATE VIEW _oo_docs.brief_use AS
  SELECT (s.created_at AT TIME ZONE 'UTC')::date AS day, u.fact_id, f.fact_key,
         count(DISTINCT u.share_id) AS shares, array_agg(DISTINCT u.section ORDER BY u.section) AS sections
    FROM _oo_docs.share_fact_use u
    JOIN _oo_docs.share s ON s.id = u.share_id
    JOIN _oo_docs.fact f ON f.id = u.fact_id
   GROUP BY 1, 2, 3;

GRANT SELECT, INSERT, UPDATE ON _oo_docs.suggestion TO oo_docs_runtime;
GRANT SELECT ON _oo_docs.brief_use TO oo_docs_runtime;
