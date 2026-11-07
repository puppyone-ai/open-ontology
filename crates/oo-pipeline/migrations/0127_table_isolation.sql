-- Owner-side cycle receipts. An interrupted/global-failure cycle is never
-- publishable, even if some table generations committed before the failure.
CREATE TABLE IF NOT EXISTS _oo_load.sync_cycles (
    source text NOT NULL,
    run text NOT NULL,
    scope jsonb NOT NULL,
    state text NOT NULL CHECK (state IN ('running','succeeded','partial','failed')),
    PRIMARY KEY (source,run)
);
CREATE TABLE IF NOT EXISTS _oo_load.table_health (
    identity text PRIMARY KEY,
    state text NOT NULL CHECK (state IN ('fresh','noop','stale')),
    reason text,
    since timestamptz,
    CHECK ((state='stale' AND reason IS NOT NULL AND since IS NOT NULL)
        OR (state<>'stale' AND reason IS NULL AND since IS NULL)),
    CHECK (reason IS NULL OR octet_length(reason) <= 192)
);
CREATE TABLE IF NOT EXISTS _oo_load.sync_table_results (
    source text NOT NULL,
    run text NOT NULL,
    identity text NOT NULL,
    table_ref jsonb NOT NULL,
    retained_run text,
    state text NOT NULL CHECK (state IN ('fresh','noop','stale')),
    reason text,
    since timestamptz,
    PRIMARY KEY (source,run,identity),
    FOREIGN KEY (source,run) REFERENCES _oo_load.sync_cycles(source,run),
    CHECK ((state='stale' AND reason IS NOT NULL AND since IS NOT NULL)
        OR (state<>'stale' AND reason IS NULL AND since IS NULL))
);
REVOKE ALL ON _oo_load.sync_cycles, _oo_load.table_health,
    _oo_load.sync_table_results FROM PUBLIC;
