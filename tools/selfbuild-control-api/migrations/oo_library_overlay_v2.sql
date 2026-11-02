CREATE TABLE oo_selfbuild_api.library_overlay_manifest(singleton boolean PRIMARY KEY CHECK(singleton),sql_sha256 text NOT NULL CHECK(sql_sha256 ~ '^[0-9a-f]{64}$'));
CREATE TABLE oo_selfbuild_api.library_overlay_limits(project_id text PRIMARY KEY,max_bytes integer NOT NULL CHECK(max_bytes BETWEEN 1000000 AND 67108864));
CREATE FUNCTION oo_selfbuild_api.overlay_storage_ceiling(build text) RETURNS integer LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
SELECT COALESCE((SELECT l.max_bytes FROM oo_selfbuild_api.build_job j JOIN oo_selfbuild_api.library_overlay_limits l USING(project_id) WHERE j.build_id=build),1000000)
$$;
ALTER TABLE oo_selfbuild_api.overlay_doc DROP CONSTRAINT overlay_doc_body;
ALTER TABLE oo_selfbuild_api.overlay_doc ADD CONSTRAINT overlay_doc_body CHECK(octet_length(body) BETWEEN 1 AND oo_selfbuild_api.overlay_storage_ceiling(build_id));
REVOKE ALL ON oo_selfbuild_api.library_overlay_manifest,oo_selfbuild_api.library_overlay_limits FROM PUBLIC;
