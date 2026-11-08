-- Install as the trusted database administrator, after 0135.
-- 0135 binds relations and the relations views depend on (pg_rewrite ->
-- pg_class). A view can also call a routine owned by someone else, whose
-- CREATE OR REPLACE changes the view's rows without changing any relation.
-- This extends the identity with the routine closure of every relation in
-- that closure: routines referenced by view rules and row-security policies,
-- operator implementation functions, and recursively the routines those
-- routines depend on (SQL-standard bodies, aggregate support functions).
-- Each routine is bound by name, identity arguments, owner, kind, volatility,
-- security, configuration and pg_get_functiondef, without catalog OIDs.
-- Built-in (pinned) routines have no pg_depend rows and are not listed.
-- Replacing this function fires the DDL guard: any record whose identity now
-- includes routines is tombstoned and is re-minted by the next publisher
-- open/replay. Records without routine dependencies keep their digest.
BEGIN;

CREATE OR REPLACE FUNCTION _oo.publication_routine_identity(p_relations oid[])
RETURNS jsonb LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp AS $$
WITH RECURSIVE direct(oid) AS (
    SELECT d.refobjid FROM pg_rewrite rw
    JOIN pg_depend d ON d.classid='pg_rewrite'::regclass AND d.objid=rw.oid
    WHERE rw.ev_class=ANY(p_relations) AND d.refclassid='pg_proc'::regclass
    UNION
    SELECT o.oprcode::oid FROM pg_rewrite rw
    JOIN pg_depend d ON d.classid='pg_rewrite'::regclass AND d.objid=rw.oid
    JOIN pg_operator o ON o.oid=d.refobjid
    WHERE rw.ev_class=ANY(p_relations) AND d.refclassid='pg_operator'::regclass
    UNION
    SELECT d.refobjid FROM pg_policy p
    JOIN pg_depend d ON d.classid='pg_policy'::regclass AND d.objid=p.oid
    WHERE p.polrelid=ANY(p_relations) AND d.refclassid='pg_proc'::regclass
    UNION
    SELECT o.oprcode::oid FROM pg_policy p
    JOIN pg_depend d ON d.classid='pg_policy'::regclass AND d.objid=p.oid
    JOIN pg_operator o ON o.oid=d.refobjid
    WHERE p.polrelid=ANY(p_relations) AND d.refclassid='pg_operator'::regclass
), routines(oid) AS (
    SELECT oid FROM direct
    UNION
    SELECT d.refobjid FROM routines r
    JOIN pg_depend d ON d.classid='pg_proc'::regclass AND d.objid=r.oid
    WHERE d.refclassid='pg_proc'::regclass
)
SELECT jsonb_agg(jsonb_build_array(
    n.nspname,f.proname,pg_get_function_identity_arguments(f.oid),
    pg_get_userbyid(f.proowner),f.prokind,f.provolatile,f.prosecdef,f.proleakproof,
    f.proisstrict,f.proparallel,
    (SELECT array_agg(option ORDER BY option) FROM unnest(f.proconfig) option),
    pg_get_function_result(f.oid),
    CASE WHEN f.prokind IN ('f','p') THEN pg_get_functiondef(f.oid)
         ELSE (SELECT jsonb_build_array(a.aggkind,a.aggtransfn::regproc::text,
                   a.aggfinalfn::regproc::text,a.aggcombinefn::regproc::text,
                   format_type(a.aggtranstype,NULL),a.agginitval)
               FROM pg_aggregate a WHERE a.aggfnoid=f.oid)::text END)
    ORDER BY n.nspname,f.proname,pg_get_function_identity_arguments(f.oid))
FROM routines r JOIN pg_proc f ON f.oid=r.oid
JOIN pg_namespace n ON n.oid=f.pronamespace
$$;

CREATE OR REPLACE FUNCTION _oo.publication_relation_identity(p_schema text, p_name text)
RETURNS jsonb LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp AS $$
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
), routines AS (
    SELECT _oo.publication_routine_identity(array_agg(oid)) AS routines FROM relations
)
-- Routines are appended to the published relation's own definition, and only
-- when there are any: relations without routine dependencies keep their 0135
-- digest, so installing this migration invalidates nothing else.
SELECT jsonb_agg(CASE WHEN d.nspname=p_schema AND d.relname=p_name AND r.routines IS NOT NULL
    THEN d.definition || jsonb_build_array(jsonb_build_array('routines',r.routines))
    ELSE d.definition END ORDER BY d.nspname,d.relname)
FROM definitions d CROSS JOIN routines r
$$;

COMMIT;
