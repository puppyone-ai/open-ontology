-- Private adapter ledger. A prepared data cut is not a semantic authorization.
CREATE TABLE IF NOT EXISTS _oo_load.gateway_publications (
    library_id text NOT NULL,
    source text NOT NULL,
    run text NOT NULL,
    generation bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    request jsonb NOT NULL,
    receipt_sha256 text REFERENCES _oo.native_library_publication(receipt_sha256),
    PRIMARY KEY (library_id, source, run)
);
REVOKE ALL ON _oo_load.gateway_publications FROM PUBLIC, oo_pipeline_runtime;
