-- Current immutable range composition and last confirmed source digests.
CREATE TABLE IF NOT EXISTS _oo_load.c5_state (
    identity text PRIMARY KEY,
    run text NOT NULL,
    state jsonb NOT NULL
);
