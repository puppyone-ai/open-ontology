-- Install as the trusted database administrator, after the publication fence.
-- Existing publications are NOT backfilled: close/reopen them through the
-- publisher after installation. Old pub1 receipts then fail PUBLICATION_DRIFT.
-- Restore the complete database with owners, ACLs and event triggers intact.
BEGIN;

CREATE TABLE _oo.publication_identity (
    role_name text NOT NULL,
    schema_name text NOT NULL,
    relation_name text NOT NULL,
    publication_id text NOT NULL,
    generation text NOT NULL,
    relation_identity_sha256 text NOT NULL,
    valid boolean NOT NULL DEFAULT true,
    PRIMARY KEY (role_name, schema_name, relation_name)
);
REVOKE ALL ON _oo.publication_identity FROM PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON _oo.publication_identity TO oo_target_admin;

-- Names, not catalog addresses. Deparse under a fixed search_path so types,
-- expressions and view references cannot depend on the caller's search_path.
CREATE FUNCTION _oo.publication_relation_identity(p_schema text, p_name text)
RETURNS jsonb LANGUAGE sql STABLE SET search_path = pg_catalog AS $$
WITH RECURSIVE relations(oid) AS (
    SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname=p_schema AND c.relname=p_name AND c.relkind IN ('r','p','v')
    UNION
    SELECT d.refobjid FROM relations r JOIN pg_rewrite rw ON rw.ev_class=r.oid
    JOIN pg_depend d ON d.classid='pg_rewrite'::regclass AND d.objid=rw.oid
    WHERE d.refclassid='pg_class'::regclass
), definitions AS (
    SELECT n.nspname, c.relname, jsonb_build_array(
        n.nspname,c.relname,c.relkind,pg_get_userbyid(c.relowner),
        c.relrowsecurity,c.relforcerowsecurity,
        (SELECT array_agg(option ORDER BY option) FROM unnest(c.reloptions) option),
        CASE WHEN c.relkind='v' THEN pg_get_viewdef(c.oid,false) END,
        CASE WHEN c.relkind IN ('r','p') THEN
            (SELECT t.tgenabled::text FROM pg_trigger t WHERE t.tgrelid=c.oid
             AND t.tgname='oo_publication_truncate'
             AND t.tgfoid=to_regprocedure('_oo.publication_identity_truncate()')) END,
        (SELECT jsonb_agg(jsonb_build_array(
            a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,
            cn.nspname,co.collname,a.attgenerated,a.attidentity,
            (SELECT pg_get_expr(ad.adbin,ad.adrelid,false) FROM pg_attrdef ad
             WHERE ad.adrelid=a.attrelid AND ad.adnum=a.attnum),
            (SELECT jsonb_agg(jsonb_build_array(
                pg_get_userbyid(x.grantor),
                CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.grantee) END,
                x.privilege_type,x.is_grantable)
                ORDER BY pg_get_userbyid(x.grantor),
                CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.grantee) END,
                x.privilege_type,x.is_grantable) FROM aclexplode(a.attacl) x))
            ORDER BY a.attnum)
         FROM pg_attribute a LEFT JOIN pg_collation co ON co.oid=a.attcollation
         LEFT JOIN pg_namespace cn ON cn.oid=co.collnamespace
         WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
        (SELECT jsonb_agg(jsonb_build_array(
            pg_get_userbyid(x.grantor),
            CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.grantee) END,
            x.privilege_type,x.is_grantable)
            ORDER BY pg_get_userbyid(x.grantor),
            CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.grantee) END,
            x.privilege_type,x.is_grantable)
         FROM aclexplode(coalesce(c.relacl,acldefault('r',c.relowner))) x),
        (SELECT jsonb_agg(jsonb_build_array(p.polname,p.polcmd,p.polpermissive,
            (SELECT jsonb_agg(CASE WHEN r=0 THEN 'PUBLIC' ELSE pg_get_userbyid(r) END
                ORDER BY CASE WHEN r=0 THEN 'PUBLIC' ELSE pg_get_userbyid(r) END)
             FROM unnest(p.polroles) r),
            pg_get_expr(p.polqual,p.polrelid,false),
            pg_get_expr(p.polwithcheck,p.polrelid,false)) ORDER BY p.polname)
         FROM pg_policy p WHERE p.polrelid=c.oid)
    ) AS definition
    FROM relations r JOIN pg_class c ON c.oid=r.oid
    JOIN pg_namespace n ON n.oid=c.relnamespace
)
SELECT jsonb_agg(definition ORDER BY nspname,relname) FROM definitions
$$;

CREATE FUNCTION _oo.publication_relation_digest(p_schema text, p_name text)
RETURNS text LANGUAGE sql STABLE SET search_path = pg_catalog AS $$
SELECT encode(sha256(convert_to(
    _oo.publication_relation_identity(p_schema,p_name)::text,'UTF8')),'hex')
$$;

CREATE FUNCTION _oo.publication_identity_truncate()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
    UPDATE _oo.publication_identity i SET valid=false WHERE i.valid AND EXISTS (
        SELECT 1 FROM jsonb_array_elements(
            _oo.publication_relation_identity(i.schema_name,i.relation_name)) d
        WHERE d->>0=TG_TABLE_SCHEMA AND d->>1=TG_TABLE_NAME);
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION _oo.publication_identity_truncate() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.publication_identity_truncate() TO oo_target_admin;

-- Runs with the fence writer's privileges. Merely owning a business relation
-- never grants permission to mint or repair an identity record.
CREATE FUNCTION _oo.publication_identity_publish()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog AS $$
DECLARE fingerprint text; dependency jsonb;
BEGIN
    IF NEW.state='open' THEN
        -- Hold identity stable through commit, including view dependencies.
        EXECUTE format('LOCK TABLE %I.%I IN ACCESS SHARE MODE',NEW.schema_name,NEW.relation_name);
        FOR dependency IN SELECT value FROM jsonb_array_elements(
            _oo.publication_relation_identity(NEW.schema_name,NEW.relation_name))
        LOOP
            IF dependency->>2 IN ('r','p') AND NOT EXISTS(
                SELECT 1 FROM pg_trigger WHERE tgrelid=to_regclass(format('%I.%I',dependency->>0,dependency->>1))
                AND tgname='oo_publication_truncate'
                AND tgfoid=to_regprocedure('_oo.publication_identity_truncate()') AND tgenabled='O')
            THEN
                EXECUTE format('CREATE OR REPLACE TRIGGER oo_publication_truncate BEFORE TRUNCATE ON %I.%I
                    FOR EACH STATEMENT EXECUTE FUNCTION _oo.publication_identity_truncate()',
                    dependency->>0,dependency->>1);
            END IF;
        END LOOP;
        fingerprint := _oo.publication_relation_digest(NEW.schema_name,NEW.relation_name);
        IF fingerprint IS NULL OR NEW.opened_version IS NULL OR NEW.opened_pack_digest IS NULL THEN
            RAISE EXCEPTION 'publication identity requires a complete relation and release';
        END IF;
        INSERT INTO _oo.publication_identity
            (role_name,schema_name,relation_name,publication_id,generation,relation_identity_sha256)
        VALUES (NEW.role_name,NEW.schema_name,NEW.relation_name,
            NEW.opened_pack_digest,NEW.opened_version,fingerprint)
        ON CONFLICT (role_name,schema_name,relation_name) DO UPDATE SET
            publication_id=EXCLUDED.publication_id,generation=EXCLUDED.generation,
            relation_identity_sha256=EXCLUDED.relation_identity_sha256,valid=true;
    ELSE
        UPDATE _oo.publication_identity SET valid=false
        WHERE role_name=NEW.role_name AND schema_name=NEW.schema_name AND relation_name=NEW.relation_name;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION _oo.publication_identity_publish() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.publication_identity_publish() TO oo_target_admin;
CREATE TRIGGER publication_identity_publish AFTER INSERT OR UPDATE ON oo_publish_target.fence
FOR EACH ROW EXECUTE FUNCTION _oo.publication_identity_publish();

-- An irreversible tombstone within the DDL transaction closes the ABA hole:
-- restoring the old name/definition later does not restore publisher approval.
-- pg_dump emits event triggers in post-data, after restoring these records.
CREATE FUNCTION _oo.publication_identity_invalidate()
RETURNS event_trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
    IF to_regclass('_oo.publication_identity') IS NULL THEN
        RETURN;
    END IF;
    UPDATE _oo.publication_identity i SET valid=false
    WHERE i.valid AND i.relation_identity_sha256 IS DISTINCT FROM
        _oo.publication_relation_digest(i.schema_name,i.relation_name);
END
$$;
REVOKE ALL ON FUNCTION _oo.publication_identity_invalidate() FROM PUBLIC;
CREATE EVENT TRIGGER oo_publication_identity_ddl ON ddl_command_end
EXECUTE FUNCTION _oo.publication_identity_invalidate();
CREATE EVENT TRIGGER oo_publication_identity_drop ON sql_drop
EXECUTE FUNCTION _oo.publication_identity_invalidate();
COMMIT;
