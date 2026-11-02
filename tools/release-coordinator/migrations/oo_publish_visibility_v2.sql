CREATE OR REPLACE FUNCTION oo_publish_target.visible_vector()
RETURNS TABLE(role_name text,schema_name text,relation_name text,opened_columns text[],opened_pack_digest text,opened_version text,candidate_id text)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,oo_publish_target
AS $body$
 SELECT f.role_name,f.schema_name,f.relation_name,f.opened_columns,f.opened_pack_digest,f.opened_version,f.candidate_id
 FROM oo_publish_target.fence f WHERE f.state='open' AND pg_has_role(session_user,f.role_name,'SET')
$body$;
REVOKE ALL ON FUNCTION oo_publish_target.visible_vector() FROM PUBLIC;
