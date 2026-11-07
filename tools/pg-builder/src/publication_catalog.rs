//! Local oo-load publications only. Private generations are never datasets.

use serde_json::{Value, json};

use crate::{PgBuilderError, Postgres, sha256_hex};

const TABLE: &str = "
SELECT json_build_object(
 'schema',n.nspname,'name',c.relname,'relkind',c.relkind,
 'replica_identity',c.relreplident,'rls_enabled',c.relrowsecurity,
 'rls_forced',c.relforcerowsecurity,'owner',pg_get_userbyid(c.relowner),
 'comment',obj_description(c.oid,'pg_class'),
 'primary_key','[]'::json,'primary_key_nulls_not_distinct',NULL,
 'unique_keys','[]'::json,'foreign_keys','[]'::json,
 'policies','[]'::json,'privileges','[]'::json,
 'columns',(SELECT json_agg(json_build_object(
   'attnum',a.attnum,'name',a.attname,'type_oid',a.atttypid::bigint,
   'type_schema',tn.nspname,'type_name',t.typname,'typtype',t.typtype,
   'typmod',a.atttypmod,'base_type_oid',NULLIF(t.typbasetype,0)::bigint,
   'not_null',a.attnotnull,'collation_oid',a.attcollation::bigint,
   'collation_name',co.collname,'identity',a.attidentity,'generated',a.attgenerated,
   'default_expr',pg_get_expr(d.adbin,d.adrelid),
   'comment',col_description(c.oid,a.attnum),
   'enum_labels',COALESCE((SELECT json_agg(e.enumlabel ORDER BY e.enumsortorder)
      FROM pg_enum e WHERE e.enumtypid=t.oid),'[]'::json),
   'domain_checks',COALESCE((SELECT json_agg(pg_get_constraintdef(k.oid) ORDER BY k.conname)
      FROM pg_constraint k WHERE k.contypid=t.oid),'[]'::json),
   'current_privileges',json_build_object('select',has_column_privilege(c.oid,a.attnum,'SELECT'),
      'insert',has_column_privilege(c.oid,a.attnum,'INSERT'),
      'update',has_column_privilege(c.oid,a.attnum,'UPDATE')))
   ORDER BY a.attnum)
   FROM pg_attribute a JOIN pg_type t ON t.oid=a.atttypid
   JOIN pg_namespace tn ON tn.oid=t.typnamespace
   LEFT JOIN pg_collation co ON co.oid=a.attcollation
   LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum
   WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped)
)::text
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname=$1 AND c.relname=$2 AND c.relkind='v'
AND has_schema_privilege(n.oid,'USAGE') AND has_table_privilege(c.oid,'SELECT')";

const KEY: &str = "
SELECT DISTINCT (p.plan->'key')::text
FROM _oo_load.publications p JOIN _oo_load.current active
  ON active.identity=p.identity AND active.run=p.run
JOIN pg_namespace storage_ns ON storage_ns.nspname='_oo_load'
JOIN pg_class storage ON storage.relnamespace=storage_ns.oid AND storage.relname=p.stage
JOIN pg_depend d ON d.refobjid=storage.oid AND d.refclassid='pg_class'::regclass
JOIN pg_rewrite r ON r.oid=d.objid AND d.classid='pg_rewrite'::regclass
JOIN pg_class v ON v.oid=r.ev_class
JOIN pg_namespace n ON n.oid=v.relnamespace
WHERE n.nspname=$1 AND v.relname=$2 AND v.relkind='v'
AND v.relname='t_'||left(p.identity,40)";

fn refused() -> PgBuilderError {
    PgBuilderError::new(
        "catalog.publication",
        "published Agent DB catalog was refused",
    )
}

fn evidence(value: &mut Value) {
    if let Some(text) = value.as_str() {
        *value = json!({"byte_len":text.len(),"sha256":sha256_hex(text.as_bytes())});
    }
}

fn bind_key(table: &mut Value, key: &Value) -> Result<(), PgBuilderError> {
    let names = match key["shape"].as_str() {
        Some("integer_primary_key" | "string_primary_key") => vec![key["column"].clone()],
        Some("composite_primary_key") => key["columns"].as_array().ok_or_else(refused)?.clone(),
        Some("no_key") => return Ok(()),
        // The plan does not expose unique-index null semantics. Do not invent them.
        _ => return Err(refused()),
    };
    if names.is_empty() {
        return Err(refused());
    }
    let columns = table["columns"].as_array().ok_or_else(refused)?;
    let primary = names
        .iter()
        .map(|name| {
            let column = columns
                .iter()
                .find(|column| &column["name"] == name)
                .ok_or_else(refused)?;
            Ok(json!({"attnum":column["attnum"],"name":column["name"]}))
        })
        .collect::<Result<Vec<_>, PgBuilderError>>()?;
    table["primary_key"] = json!(primary);
    table["primary_key_nulls_not_distinct"] = json!(false);
    Ok(())
}

pub(crate) async fn capture(pg: &Postgres, source: &str) -> Result<Vec<u8>, PgBuilderError> {
    if pg.tables.iter().any(|(schema, _)| schema != "oo_load_data") {
        return Err(refused());
    }
    let mut client = crate::profile::connect(pg).await?;
    let tx = client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(|_| refused())?;
    let timestamp: String = tx.query_one(
        "SELECT to_char(transaction_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')", &[],
    ).await.map_err(|_| refused())?.get(0);
    let mut tables = Vec::new();
    let mut bytes = 0;
    for (schema, name) in &pg.tables {
        let row = tx
            .query_one(TABLE, &[schema, name])
            .await
            .map_err(|_| refused())?;
        let text: String = row.get(0);
        bytes += text.len();
        if bytes > 8 * 1024 * 1024 {
            return Err(refused());
        }
        let mut table: Value = serde_json::from_str(&text).map_err(|_| refused())?;
        let key: String = tx
            .query_one(KEY, &[schema, name])
            .await
            .map_err(|_| refused())?
            .get(0);
        bind_key(
            &mut table,
            &serde_json::from_str(&key).map_err(|_| refused())?,
        )?;
        evidence(&mut table["comment"]);
        for column in table["columns"].as_array_mut().ok_or_else(refused)? {
            evidence(&mut column["comment"]);
            evidence(&mut column["default_expr"]);
            for check in column["domain_checks"].as_array_mut().ok_or_else(refused)? {
                evidence(check);
            }
        }
        tables.push(table);
    }
    tx.commit().await.map_err(|_| refused())?;
    let catalog = oo_builder::finalize_pg_capture(json!({
        "protocol":"oo.source.pgsql/v1","core_contract":"oo.config/v1",
        "source_id":source,"database":pg.database,"captured_at":timestamp,
        "hash_algorithm":"pg-catalog-hash/v1","ident_algorithm":"pgident/v1",
        "type_conversion":"unsupported","tables":tables
    }))
    .map_err(|_| refused())?;
    serde_json::to_vec(&catalog).map_err(|_| refused())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_keys_keep_member_order_and_refuse_missing_columns() {
        let mut table = json!({"columns":[{"name":"a","attnum":1},{"name":"b","attnum":2}]});
        bind_key(
            &mut table,
            &json!({"shape":"composite_primary_key","columns":["b","a"]}),
        )
        .unwrap();
        assert_eq!(
            table["primary_key"],
            json!([{"name":"b","attnum":2},{"name":"a","attnum":1}])
        );
        assert!(
            bind_key(
                &mut table,
                &json!({"shape":"integer_primary_key","column":"missing"})
            )
            .is_err()
        );
        assert!(bind_key(&mut table, &json!({"shape":"unique_index","columns":["a"]})).is_err());
    }
}
