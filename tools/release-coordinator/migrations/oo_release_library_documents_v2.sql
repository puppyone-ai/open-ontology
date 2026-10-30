CREATE TABLE oo_release_control.library_document_manifest (
    singleton boolean PRIMARY KEY CHECK (singleton),
    sql_sha256 text NOT NULL CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);
CREATE TABLE oo_release_control.library_document_limits (
    singleton boolean PRIMARY KEY CHECK (singleton),
    max_document_bytes integer NOT NULL CHECK (max_document_bytes BETWEEN 1048576 AND 67108864),
    max_file_bytes integer NOT NULL CHECK (max_file_bytes BETWEEN 262144 AND 4194304)
);
INSERT INTO oo_release_control.library_document_limits VALUES (true,1048576,1048576);
CREATE FUNCTION oo_release_control.document_storage_ceiling() RETURNS integer
LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
    SELECT max_document_bytes FROM oo_release_control.library_document_limits WHERE singleton
$$;
CREATE FUNCTION oo_release_control.file_storage_ceiling() RETURNS integer
LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
    SELECT max_file_bytes FROM oo_release_control.library_document_limits WHERE singleton
$$;
ALTER TABLE oo_release_control.candidate DROP CONSTRAINT candidate_docs;
ALTER TABLE oo_release_control.candidate ADD CONSTRAINT candidate_docs CHECK (
    octet_length(effective_oos) BETWEEN 2 AND oo_release_control.document_storage_ceiling()
    AND octet_length(visibility_json) BETWEEN 2 AND oo_release_control.document_storage_ceiling()
);
ALTER TABLE oo_release_control.candidate_file DROP CONSTRAINT candidate_file_len;
ALTER TABLE oo_release_control.candidate_file ADD CONSTRAINT candidate_file_len CHECK (
    byte_len > 0 AND byte_len <= oo_release_control.file_storage_ceiling()
);
REVOKE ALL ON oo_release_control.library_document_manifest,oo_release_control.library_document_limits FROM PUBLIC;
