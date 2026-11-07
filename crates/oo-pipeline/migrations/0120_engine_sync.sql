-- Local typed SourceEngine publication. Not a Gateway publication.
CREATE TABLE IF NOT EXISTS _oo_load.sync_state (
    source text PRIMARY KEY,
    lineage text NOT NULL,
    scope jsonb NOT NULL,
    position jsonb NOT NULL,
    generation bigint NOT NULL DEFAULT 0 CHECK (generation >= 0),
    write_bytes bigint NOT NULL DEFAULT 0 CHECK (write_bytes >= 0)
);
CREATE TABLE IF NOT EXISTS _oo_load.sync_bindings (
    identity text PRIMARY KEY,
    source text NOT NULL REFERENCES _oo_load.sync_state(source),
    baseline_run text NOT NULL
);
