-- Grant only the execution right required by the native pipeline runtime.
REVOKE ALL ON FUNCTION _oo.native_update_seal_quality(text, uuid, bigint, uuid, text, bigint, text, text, text, text, text, numeric, text, text, text, text, text, boolean, text[], bigint, text, bigint, text, integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_update_seal_quality(text, uuid, bigint, uuid, text, bigint, text, text, text, text, text, numeric, text, text, text, text, text, boolean, text[], bigint, text, bigint, text, integer) TO oo_pipeline_runtime;
