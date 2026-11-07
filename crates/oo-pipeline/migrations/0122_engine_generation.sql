-- Immutable storage recipes, not Gateway publication or permission receipts.
CREATE TABLE IF NOT EXISTS _oo_load.generations (
    identity text NOT NULL,
    run text NOT NULL,
    relation_name text NOT NULL UNIQUE,
    recipe jsonb NOT NULL,
    sealed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (identity, run),
    CHECK (relation_name ~ '^g_[0-9a-f]{40}$')
);
REVOKE ALL ON _oo_load.generations FROM PUBLIC;
