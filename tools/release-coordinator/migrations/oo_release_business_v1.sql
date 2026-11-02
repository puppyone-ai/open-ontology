-- Bounded test-scope business attestations. No role creation or production issuer.
CREATE TABLE oo_release_control.business_manifest (
    singleton boolean PRIMARY KEY CHECK (singleton),
    sql_sha text NOT NULL CHECK (sql_sha ~ '^[0-9a-f]{64}$')
);
CREATE TABLE oo_release_control.business_policy (
    policy_sha text PRIMARY KEY CHECK (policy_sha ~ '^[0-9a-f]{64}$'),
    candidate_id text NOT NULL REFERENCES oo_release_control.candidate(candidate_id),
    source_hash text NOT NULL,
    document text NOT NULL CHECK (octet_length(document) <= 1048576),
    installed_by text NOT NULL DEFAULT session_user,
    installed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE oo_release_control.business_revocation (
    policy_sha text PRIMARY KEY REFERENCES oo_release_control.business_policy(policy_sha),
    actor text NOT NULL DEFAULT session_user,
    revoked_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE oo_release_control.business_receipt (
    policy_sha text NOT NULL REFERENCES oo_release_control.business_policy(policy_sha),
    kind text NOT NULL CHECK (kind IN ('executor','oracle','evaluator','reviewer')),
    receipt_id text NOT NULL,
    actor text NOT NULL DEFAULT session_user,
    actor_oid oid NOT NULL,
    body text NOT NULL CHECK (octet_length(body) <= 1048576),
    body_sha text NOT NULL CHECK (body_sha ~ '^[0-9a-f]{64}$'),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(policy_sha,kind,receipt_id)
);
CREATE FUNCTION oo_release_control.business_append_only() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog AS $$
BEGIN
    RAISE EXCEPTION 'business evidence is immutable' USING ERRCODE='42501';
END $$;
CREATE TRIGGER business_policy_immutable BEFORE UPDATE OR DELETE ON oo_release_control.business_policy
FOR EACH ROW EXECUTE FUNCTION oo_release_control.business_append_only();
CREATE TRIGGER business_receipt_immutable BEFORE UPDATE OR DELETE ON oo_release_control.business_receipt
FOR EACH ROW EXECUTE FUNCTION oo_release_control.business_append_only();
CREATE TRIGGER business_revocation_immutable BEFORE UPDATE OR DELETE ON oo_release_control.business_revocation
FOR EACH ROW EXECUTE FUNCTION oo_release_control.business_append_only();
CREATE TRIGGER business_policy_no_truncate BEFORE TRUNCATE ON oo_release_control.business_policy
FOR EACH STATEMENT EXECUTE FUNCTION oo_release_control.business_append_only();
CREATE TRIGGER business_receipt_no_truncate BEFORE TRUNCATE ON oo_release_control.business_receipt
FOR EACH STATEMENT EXECUTE FUNCTION oo_release_control.business_append_only();
CREATE TRIGGER business_revocation_no_truncate BEFORE TRUNCATE ON oo_release_control.business_revocation
FOR EACH STATEMENT EXECUTE FUNCTION oo_release_control.business_append_only();
CREATE FUNCTION oo_release_control.business_actor() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE policy jsonb;
BEGIN
    SELECT document::jsonb INTO STRICT policy FROM oo_release_control.business_policy
      WHERE policy_sha=NEW.policy_sha;
    IF session_user <> policy->'actors'->>NEW.kind
       OR EXISTS (SELECT 1 FROM oo_release_control.business_revocation WHERE policy_sha=NEW.policy_sha)
       OR extract(epoch FROM clock_timestamp()) >= (policy->>'expires_at')::bigint THEN
        RAISE EXCEPTION 'business issuer refused' USING ERRCODE='42501';
    END IF;
    NEW.actor := session_user;
    SELECT oid INTO STRICT NEW.actor_oid FROM pg_catalog.pg_roles WHERE rolname=session_user;
    NEW.recorded_at := clock_timestamp();
    RETURN NEW;
END $$;
CREATE TRIGGER business_receipt_actor BEFORE INSERT ON oo_release_control.business_receipt
FOR EACH ROW EXECUTE FUNCTION oo_release_control.business_actor();
ALTER TABLE oo_release_control.business_policy ENABLE ROW LEVEL SECURITY;
ALTER TABLE oo_release_control.business_receipt ENABLE ROW LEVEL SECURITY;
ALTER TABLE oo_release_control.business_revocation ENABLE ROW LEVEL SECURITY;
CREATE POLICY business_policy_actor_read ON oo_release_control.business_policy FOR SELECT
USING (document::jsonb->'actors' @> jsonb_build_object('executor',session_user)
    OR document::jsonb->'actors' @> jsonb_build_object('oracle',session_user)
    OR document::jsonb->'actors' @> jsonb_build_object('evaluator',session_user)
    OR document::jsonb->'actors' @> jsonb_build_object('reviewer',session_user));
CREATE POLICY business_receipt_actor_read ON oo_release_control.business_receipt FOR SELECT
USING (EXISTS (SELECT 1 FROM oo_release_control.business_policy p WHERE p.policy_sha=business_receipt.policy_sha));
CREATE POLICY business_receipt_actor_insert ON oo_release_control.business_receipt FOR INSERT
WITH CHECK (actor=session_user AND EXISTS (
    SELECT 1 FROM oo_release_control.business_policy p WHERE p.policy_sha=business_receipt.policy_sha));
CREATE POLICY business_revocation_actor_read ON oo_release_control.business_revocation FOR SELECT
USING (EXISTS (SELECT 1 FROM oo_release_control.business_policy p WHERE p.policy_sha=business_revocation.policy_sha));
REVOKE ALL ON oo_release_control.business_manifest, oo_release_control.business_policy, oo_release_control.business_receipt,
    oo_release_control.business_revocation FROM PUBLIC;
REVOKE ALL ON FUNCTION oo_release_control.business_actor(),
    oo_release_control.business_append_only() FROM PUBLIC;
