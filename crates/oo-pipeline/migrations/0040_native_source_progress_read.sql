-- The scheduled worker validates its acknowledged global source generation
-- before producing the next capture. Limit the runtime to these two identity
-- columns; target writes remain behind the existing native Loader functions.
GRANT SELECT (source, source_generation)
ON _oo.source_watermark TO oo_pipeline_runtime;
