//! Published C1 ordinary shadows only. No sample courier, view fallback or grants.
use crate::business_positive::{require, text};
use crate::canon::canonical_sha;
use crate::error::{StoreError, query_error};
use serde_json::{Value, json};
use tokio_postgres::Transaction;

pub(crate) fn enabled(p: &Value) -> bool {
    p["protocol"] == "oo.release-coordinator/business-policy/v2"
}

pub(crate) fn validate(p: &Value) -> Result<(), StoreError> {
    crate::target::validate_native_context(&p["native_mapping"])?;
    let publication =
        crate::native_publication::from_context(&p["native_mapping"])?.ok_or(StoreError::Input)?;
    let r = &publication.receipt;
    require(
        r["position_codec"] == "mysql-gtid/v1"
            && r["consistency"] == "transactional"
            && r["consistency_grade"] == "T"
            && r["cross_table_consistent"] == true
            && r["run_ids"].is_array()
            && !crate::native_library::is_library(r),
    )?;
    let relations = p["relations"].as_array().ok_or(StoreError::Input)?;
    let tables = r["tables"].as_array().ok_or(StoreError::Input)?;
    require(
        relations.len() == tables.len()
            && relations.iter().all(|relation| {
                tables
                    .iter()
                    .filter(|t| {
                        format!(
                            "{}.{}",
                            t["schema"].as_str().unwrap_or(""),
                            t["table"].as_str().unwrap_or("")
                        ) == relation["name"].as_str().unwrap_or("")
                    })
                    .count()
                    == 1
            }),
    )?;
    require(
        p["statement_timeout_ms"]
            .as_u64()
            .is_some_and(|n| (1..=60000).contains(&n))
            && p["execution_deadline_ms"]
                .as_u64()
                .is_some_and(|n| (1..=300000).contains(&n)),
    )
}

pub(crate) async fn vector(
    tx: &Transaction<'_>,
    p: &Value,
    roles: Value,
    effective: &Value,
) -> Result<Value, StoreError> {
    validate(p)?;
    let context = crate::target::ValidatedNativeContext::new(&p["native_mapping"])?;
    let publication = context.publication().ok_or(StoreError::Input)?;
    let r = &publication.receipt;
    // Pipeline updates these authorities transactionally with data publication.
    // Locks survive through actor execution or the existing pointer CAS.
    let source = text(r, "source_id")?;
    let run_ids = r["run_ids"]
        .as_array()
        .ok_or(StoreError::Input)?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or(StoreError::Input))
        .collect::<Result<Vec<_>, _>>()?;
    require(
        tx.query(
            "SELECT source FROM _oo.source_watermark WHERE source=$1 FOR SHARE",
            &[&source],
        )
        .await
        .map_err(query_error)?
        .len()
            == 1,
    )?;
    tx.query("SELECT schema_name,table_name FROM _oo.table_freshness WHERE source=$1 ORDER BY schema_name,table_name FOR SHARE",
        &[&source]).await.map_err(query_error)?;
    require(tx.query("SELECT run_id FROM _oo.full_import_run WHERE run_id::text=ANY($1::text[]) ORDER BY run_id FOR SHARE",
        &[&run_ids]).await.map_err(query_error)?.len()==run_ids.len())?;
    require(tx.query("SELECT run_id FROM _oo.full_import_table WHERE run_id::text=ANY($1::text[]) ORDER BY run_id,table_index FOR SHARE",
        &[&run_ids]).await.map_err(query_error)?.len()==run_ids.len())?;
    let rows=tx.query("SELECT filename,checksum FROM _oo.schema_migrations WHERE filename=ANY($1::text[]) ORDER BY filename",
        &[&vec!["0004_physical_mapping.sql","0027_native_publication.sql","0036_native_c1_collection_publication.sql","0082_native_c1_oracle_tail.sql"]]).await.map_err(query_error)?;
    let pins = [
        (
            "0004_physical_mapping.sql",
            include_bytes!("../../../crates/oo-pipeline/migrations/0004_physical_mapping.sql")
                .as_slice(),
        ),
        (
            "0027_native_publication.sql",
            include_bytes!("../../../crates/oo-pipeline/migrations/0027_native_publication.sql")
                .as_slice(),
        ),
        (
            "0036_native_c1_collection_publication.sql",
            include_bytes!(
                "../../../crates/oo-pipeline/migrations/0036_native_c1_collection_publication.sql"
            )
            .as_slice(),
        ),
        (
            "0082_native_c1_oracle_tail.sql",
            include_bytes!("../../../crates/oo-pipeline/migrations/0082_native_c1_oracle_tail.sql")
                .as_slice(),
        ),
    ];
    require(
        rows.len() == pins.len()
            && pins.iter().all(|(name, bytes)| {
                rows.iter().any(|row| {
                    row.get::<_, String>(0) == *name
                        && row.get::<_, String>(1) == crate::sha256_hex(bytes)
                })
            }),
    )?;
    let schema_pins = rows
        .iter()
        .map(|row| json!({"filename":row.get::<_,String>(0),"checksum":row.get::<_,String>(1)}))
        .collect::<Vec<_>>();
    let w: Value = serde_json::from_str(
        &tx.query_one(
            "SELECT to_jsonb(w)::text FROM _oo.source_watermark w WHERE source=$1",
            &[&text(r, "source_id")?],
        )
        .await
        .map_err(query_error)?
        .get::<_, String>(0),
    )
    .map_err(|_| StoreError::Input)?;
    require(
        w["state"] == "live"
            && w["source_generation"] == r["source_generation"]
            && w["target_generation"] == r["target_generation"]
            && w["position_codec"] == r["position_codec"]
            && w["position_lineage"] == r["position_lineage"]
            && w["source_epoch"] == r["source_epoch"]
            && w["applied_pos"].to_string().trim_matches('"')
                == r["applied_pos"].as_str().ok_or(StoreError::Input)?
            && w["consistency"] == r["consistency"]
            && w["consistency_grade"] == r["consistency_grade"]
            && w["cross_table_consistent"] == true,
    )?;
    let mut entries = Vec::new();
    for relation in p["relations"].as_array().ok_or(StoreError::Input)? {
        let logical = text(relation, "name")?;
        let (schema, name) = logical.split_once('.').ok_or(StoreError::Input)?;
        let columns = relation["columns"]
            .as_array()
            .ok_or(StoreError::Input)?
            .iter()
            .map(|c| c.as_str().map(str::to_owned).ok_or(StoreError::Input))
            .collect::<Result<Vec<_>, _>>()?;
        let mapped = crate::target::resolve_native_binding_body(
            tx,
            &context,
            &(
                schema.into(),
                name.into(),
                text(relation, "role")?.into(),
                columns.clone(),
            ),
        )
        .await?;
        let table = r["tables"]
            .as_array()
            .ok_or(StoreError::Input)?
            .iter()
            .find(|t| t["schema"] == schema && t["table"] == name)
            .ok_or(StoreError::Input)?;
        require(table["physical_name"] == mapped.1)?;
        let actual=tx.query_one("SELECT to_jsonb(i)::text,to_jsonb(t)::text,to_jsonb(n)::text,to_jsonb(f)::text \
            FROM _oo.full_import_run i JOIN _oo.full_import_table t USING(run_id) \
            JOIN _oo.native_publication_receipt n USING(run_id) JOIN _oo.table_freshness f \
            ON f.source=i.source_id AND f.schema_name=t.schema_ident AND f.table_name=t.table_ident \
            WHERE i.run_id=$1::text::uuid AND t.schema_ident=$2 AND t.table_ident=$3",
            &[&text(table,"run_id")?,&schema,&name]).await.map_err(query_error)?;
        let parsed = (0..4)
            .map(|i| {
                serde_json::from_str::<Value>(&actual.get::<_, String>(i))
                    .map_err(|_| StoreError::Input)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (i, t, n, f) = (&parsed[0], &parsed[1], &parsed[2], &parsed[3]);
        let mapping = p["native_mapping"]["datasets"]
            .as_array()
            .ok_or(StoreError::Input)?
            .iter()
            .find(|m| m["dataset"] == logical)
            .ok_or(StoreError::Input)?;
        require(
            i["state"] == "published"
                && i["source_id"] == mapping["source"]
                && i["config_digest"] == mapping["config_digest"]
                && i["source_generation"] == mapping["generation"]
                && i["target_generation"] == table["target_generation"]
                && t["physical_name"] == table["physical_name"]
                && t["exhausted"] == true
                && t["applied_rows"] == table["accepted_rows"]
                && t["canonical_digest"] == table["canonical_digest"]
                && t["applied_pos"].to_string().trim_matches('"') == text(table, "applied_pos")?
                && n["receipt_sha256"] == publication.receipt_sha256
                && n["table_vector"] == r["tables"]
                && !n["publication_xid"].is_null()
                && n["source_id"] == r["source_id"]
                && n["source_generation"] == r["source_generation"]
                && n["target_generation"] == r["target_generation"]
                && n["applied_pos"].to_string().trim_matches('"') == text(r, "applied_pos")?
                && n["position_codec"] == r["position_codec"]
                && f["state"] == "live"
                && f["mode"] == "full"
                && f["source_generation"] == table["source_generation"]
                && f["target_generation"] == table["target_generation"]
                && f["physical_name"] == table["physical_name"]
                && f["last_pos"].to_string().trim_matches('"') == text(table, "applied_pos")?,
        )?;
        let qualified = format!("{}.{}", mapped.0, mapped.1);
        let catalog=tx.query_one("SELECT c.oid::bigint,c.relkind::text,c.relrowsecurity,c.relforcerowsecurity,\
            coalesce(c.relacl::text,''),c.relowner::bigint,\
            coalesce((SELECT string_agg(a.attname||':'||a.atttypid||':'||a.atttypmod||':'||a.attnotnull||':'||coalesce(a.attacl::text,''),',' ORDER BY attnum) \
            FROM pg_attribute a WHERE a.attrelid=c.oid AND attnum>0 AND NOT attisdropped),'') \
            FROM pg_class c WHERE c.oid=to_regclass($1)",&[&qualified]).await.map_err(query_error)?;
        require(
            catalog.get::<_, String>(1) == "r"
                && !catalog.get::<_, bool>(2)
                && !catalog.get::<_, bool>(3),
        )?;
        let attrs=tx.query("SELECT attname,atttypid::bigint,has_column_privilege($2,$1,attname,'SELECT'),format_type(atttypid,atttypmod) \
            FROM pg_attribute WHERE attrelid=to_regclass($1) AND attname=ANY($3::text[]) AND attnum>0 AND NOT attisdropped",
            &[&qualified,&mapped.2,&mapped.3]).await.map_err(query_error)?;
        require(
            attrs.len() == columns.len()
                && attrs.iter().all(|a| {
                    [20_i64, 21, 23, 1700].contains(&a.get::<_, i64>(1)) && a.get::<_, bool>(2)
                }),
        )?;
        if relation["role"] == p["role"] {
            let dataset = effective["datasets"]
                .as_array()
                .ok_or(StoreError::Input)?
                .iter()
                .find(|d| d["dataset"] == logical)
                .ok_or(StoreError::Input)?;
            for (logical_column, physical_column) in columns.iter().zip(&mapped.3) {
                let column = dataset["columns"]
                    .as_array()
                    .ok_or(StoreError::Input)?
                    .iter()
                    .find(|c| c["name"] == *logical_column)
                    .ok_or(StoreError::Input)?;
                let observed = attrs
                    .iter()
                    .find(|a| a.get::<_, String>(0) == *physical_column)
                    .ok_or(StoreError::Input)?
                    .get::<_, String>(3);
                require(
                    crate::apply::pg_type_identity(text(column, "data_type")?)
                        == crate::apply::pg_type_identity(&observed),
                )?;
            }
        }
        for who in p["actors"]
            .as_object()
            .ok_or(StoreError::Input)?
            .values()
            .chain(std::iter::once(&relation["role"]))
        {
            let who = who.as_str().ok_or(StoreError::Input)?;
            require(!tx.query_one("SELECT has_table_privilege($1,$2,'INSERT,UPDATE,DELETE,TRUNCATE') OR has_any_column_privilege($1,$2,'INSERT,UPDATE')",
                &[&who,&qualified]).await.map_err(query_error)?.get::<_,bool>(0))?;
        }
        entries.push(json!({"relation":logical,"role":mapped.2,"columns":columns,"physical_schema":mapped.0,
            "physical_relation":mapped.1,"physical_columns":mapped.3,"oid":catalog.get::<_,i64>(0),
            "acl":catalog.get::<_,String>(4),"owner_oid":catalog.get::<_,i64>(5),"schema":catalog.get::<_,String>(6),
            "run":i,"sealed_table":t,"publication":n,"freshness":f}));
    }
    Ok(
        json!({"protocol":"oo.release-coordinator/native-publication-vector/v1","native_mapping_sha256":canonical_sha(&p["native_mapping"]),
        "relations":entries,"roles":roles,"source_watermark":w,"schema_pins":schema_pins,"data_rows_transferred":0}),
    )
}

pub(crate) fn physical_sql(query: &Value, binding: &Value) -> Result<String, StoreError> {
    let row = binding["data_vector"]["relations"]
        .as_array()
        .ok_or(StoreError::Input)?
        .iter()
        .find(|r| r["relation"] == query["relation"])
        .ok_or(StoreError::Input)?;
    let index = row["columns"]
        .as_array()
        .ok_or(StoreError::Input)?
        .iter()
        .position(|c| c == &query["column"])
        .ok_or(StoreError::Input)?;
    let column = row["physical_columns"][index]
        .as_str()
        .ok_or(StoreError::Input)?;
    let schema = text(row, "physical_schema")?;
    let table = text(row, "physical_relation")?;
    require(
        schema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && table.starts_with("pt_")
            && column.starts_with("pc_"),
    )?;
    Ok(format!(
        "SELECT {}(\"{column}\") FROM \"{schema}\".\"{table}\"",
        text(query, "aggregate")?
    ))
}
