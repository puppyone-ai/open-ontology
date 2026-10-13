-- oo.selfbuild-control-api/overlay/v1
-- Applied only after the frozen v1 migration. This script does not edit
-- install_manifest, does not CREATE ROLE, and does not publish.

CREATE TABLE oo_selfbuild_api.overlay_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT overlay_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT overlay_manifest_one CHECK (singleton),
    CONSTRAINT overlay_manifest_sql CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_selfbuild_api.overlay_doc (
    build_id text NOT NULL,
    kind text NOT NULL,
    revision bigint NOT NULL,
    doc_sha256 text NOT NULL,
    body text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT overlay_doc_pk PRIMARY KEY (build_id, kind, revision),
    CONSTRAINT overlay_doc_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT overlay_doc_kind CHECK (kind IN ('generated', 'curated')),
    CONSTRAINT overlay_doc_revision CHECK (revision >= 1),
    CONSTRAINT overlay_doc_hash CHECK (doc_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT overlay_doc_body CHECK (char_length(body) BETWEEN 1 AND 1000000)
);

CREATE TABLE oo_selfbuild_api.overlay_review (
    build_id text NOT NULL,
    revision bigint NOT NULL,
    name text NOT NULL,
    decision text NOT NULL,
    binds_fact text NOT NULL,
    signer_class text NOT NULL,
    business_owner_signed boolean NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT overlay_review_pk PRIMARY KEY (build_id, revision),
    CONSTRAINT overlay_review_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT overlay_review_revision CHECK (revision >= 1),
    CONSTRAINT overlay_review_decision CHECK (decision IN ('confirmed', 'rejected')),
    CONSTRAINT overlay_review_fact CHECK (binds_fact ~ '^[0-9a-f]{64}$'),
    CONSTRAINT overlay_review_signer CHECK (signer_class = 'control_plane_test'),
    CONSTRAINT overlay_review_owner CHECK (business_owner_signed = false)
);

CREATE TABLE oo_selfbuild_api.overlay_candidate (
    build_id text NOT NULL,
    revision bigint NOT NULL,
    note text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT overlay_candidate_pk PRIMARY KEY (build_id, revision),
    CONSTRAINT overlay_candidate_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT overlay_candidate_revision CHECK (revision >= 1),
    CONSTRAINT overlay_candidate_note CHECK (char_length(note) BETWEEN 1 AND 200)
);

CREATE TABLE oo_selfbuild_api.overlay_cache (
    cache_key text NOT NULL,
    build_id text NOT NULL,
    kind text NOT NULL,
    response_text text NOT NULL,
    llm_calls integer NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT overlay_cache_pk PRIMARY KEY (cache_key),
    CONSTRAINT overlay_cache_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT overlay_cache_key CHECK (cache_key ~ '^[0-9a-f]{64}$'),
    CONSTRAINT overlay_cache_kind CHECK (kind IN ('maintenance', 'curated', 'review', 'confirm', 'role_pack', 'gates', 'feedback')),
    CONSTRAINT overlay_cache_llm CHECK (llm_calls = 0),
    CONSTRAINT overlay_cache_body CHECK (char_length(response_text) BETWEEN 1 AND 1000000)
);

CREATE TABLE oo_selfbuild_api.maintenance_receipt (
    build_id text NOT NULL,
    route text NOT NULL,
    idempotency_key text NOT NULL,
    body_sha256 text NOT NULL,
    response_text text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT maintenance_receipt_pk PRIMARY KEY (build_id, route, idempotency_key),
    CONSTRAINT maintenance_receipt_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT maintenance_receipt_route CHECK (route IN ('maintenance', 'curated', 'review', 'confirm', 'role_pack', 'gates')),
    CONSTRAINT maintenance_receipt_key CHECK (idempotency_key ~ '^[A-Za-z0-9_-]{16,128}$'),
    CONSTRAINT maintenance_receipt_body CHECK (body_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_selfbuild_api.role_pack (
    build_id text NOT NULL,
    role_name text NOT NULL,
    revision bigint NOT NULL,
    pack_sha256 text NOT NULL,
    manifest_sha256 text NOT NULL,
    manifest text NOT NULL,
    files_json text NOT NULL,
    published boolean NOT NULL,
    active boolean NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT role_pack_pk PRIMARY KEY (build_id, role_name, revision),
    CONSTRAINT role_pack_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT role_pack_revision CHECK (revision >= 1),
    CONSTRAINT role_pack_role CHECK (role_name ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT role_pack_hash CHECK (pack_sha256 ~ '^[0-9a-f]{64}$' AND manifest_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT role_pack_published CHECK (published = false),
    CONSTRAINT role_pack_active CHECK (active = false)
);

CREATE TRIGGER overlay_doc_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.overlay_doc
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();

CREATE TRIGGER overlay_review_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.overlay_review
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();

CREATE TRIGGER overlay_candidate_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.overlay_candidate
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();

CREATE TRIGGER overlay_cache_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.overlay_cache
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();

CREATE TRIGGER maintenance_receipt_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.maintenance_receipt
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();

CREATE TRIGGER role_pack_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.role_pack
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();
