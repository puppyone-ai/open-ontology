-- Source facts and the published projection are owner-side retained state.
-- Never infer a seal from an older schema hash or from target column types.
CREATE TABLE IF NOT EXISTS _oo_load.column_catalog (
    identity text PRIMARY KEY,
    baseline_run text NOT NULL,
    columns jsonb NOT NULL CHECK (jsonb_typeof(columns) = 'array'),
    projection jsonb NOT NULL CHECK (jsonb_typeof(projection) = 'array'),
    sealed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON _oo_load.column_catalog FROM PUBLIC;
