-- A source cursor is not proof that a quarantined table was applied.
CREATE TABLE IF NOT EXISTS _oo_load.sync_table_positions (
    identity text PRIMARY KEY REFERENCES _oo_load.sync_bindings(identity),
    position jsonb NOT NULL,
    stale_reason text,
    stale_since timestamptz,
    CHECK ((stale_reason IS NULL) = (stale_since IS NULL)),
    CHECK (stale_reason IS NULL OR octet_length(stale_reason) <= 192)
);
-- Old C1 commits were all-or-nothing across the complete binding scope.
INSERT INTO _oo_load.sync_table_positions(identity,position)
SELECT b.identity,s.position FROM _oo_load.sync_bindings b
JOIN _oo_load.sync_state s ON s.source=b.source
ON CONFLICT(identity) DO NOTHING;
REVOKE ALL ON _oo_load.sync_table_positions FROM PUBLIC;
