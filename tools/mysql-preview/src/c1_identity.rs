//! Positive logical identity selection and live metadata checks for each C1 connection.
use super::*;
#[path = "../../source-onboarding/src/identity_contract.rs"]
pub(crate) mod contract;

pub(crate) fn verify(value: &Value) -> Result<Option<contract::Verified>, PreviewError> {
    contract::verify(value, value.get("onboarding").is_none(), sha256_hex)
        .map_err(|s| PreviewError::new("identity",s))
}

async fn documents(conn: &mut Conn, sql: &str) -> Result<Vec<Value>, PreviewError> {
    conn.query::<String,_>(sql).await.map_err(|e|mysql_err(&e))?.into_iter()
        .map(|s|serde_json::from_str(&s).map_err(|_|PreviewError::new("identity","metadata_json"))).collect()
}

pub(crate) async fn check_open(config: &Value, conn: &mut Conn) -> Result<(), PreviewError> {
    let Some(verified)=verify(config)? else {return Ok(())};
    let uuid=scalar(conn,"SELECT @@GLOBAL.server_uuid").await?;
    if verified.record["physical_identity"]!=json!({"engine":"mysql","server_uuid":uuid}) {
        return Err(PreviewError::new("identity","source_uuid_changed"));
    }
    for name in verified.record["members"].as_array().ok_or_else(||PreviewError::new("identity","members"))? {
        let (schema,table)=name.as_str().and_then(|n|n.split_once('.'))
            .filter(|(s,t)|oo_connectors::mysql::mysql_ident(s)&&oo_connectors::mysql::mysql_ident(t))
            .ok_or_else(||PreviewError::new("identity","table"))?;
        let columns=documents(conn,&format!("SELECT JSON_OBJECT('name',COLUMN_NAME,'ordinal',ORDINAL_POSITION,'type',COLUMN_TYPE,\
          'nullable',IS_NULLABLE,'charset',CHARACTER_SET_NAME,'collation',COLLATION_NAME,'default',COLUMN_DEFAULT,\
          'extra',EXTRA,'generation',GENERATION_EXPRESSION) FROM information_schema.COLUMNS \
          WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' ORDER BY ORDINAL_POSITION LIMIT 4097")).await?;
        let keys=documents(conn,&format!("SELECT JSON_OBJECT('name',INDEX_NAME,'unique',NON_UNIQUE,'seq',SEQ_IN_INDEX,\
          'column',COLUMN_NAME,'prefix',SUB_PART,'collation',COLLATION,'type',INDEX_TYPE) FROM information_schema.STATISTICS \
          WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' ORDER BY INDEX_NAME,SEQ_IN_INDEX LIMIT 4097")).await?;
        let table_meta=documents(conn,&format!("SELECT JSON_OBJECT('engine',ENGINE,'collation',TABLE_COLLATION,'type',TABLE_TYPE) \
          FROM information_schema.TABLES WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}'")).await?;
        if columns.is_empty() || columns.len()>4096 || keys.len()>4096 || table_meta.len()!=1
            || table_meta[0]["type"]!="BASE TABLE"
            || verified.record["known_table_schema_sha256"][name.as_str().unwrap()]
                !=contract::hash(&json!({"columns":columns,"keys":keys,"table":table_meta}),sha256_hex)
                    .map_err(|s|PreviewError::new("identity",s))? {
            return Err(PreviewError::new("identity","source_schema_changed"));
        }
    }
    if verify(config)?.is_none_or(|v|v.registration!=verified.registration) {
        return Err(PreviewError::new("identity","registration_changed_during_open"));
    }
    Ok(())
}

pub(crate) fn bind_context(config: &Value, context: &Value) -> Result<Option<Value>, PreviewError> {
    let Some(v)=verify(config)? else {
        if !context["source_registration"].is_null() {return Err(PreviewError::new("identity","missing_marker"));}
        return Ok(None);
    };
    if context["source_registration"]!=v.registration || context["source_id"]!=v.record["source_id"] {
        return Err(PreviewError::new("identity","worker_registration"));
    }
    Ok(Some(v.registration))
}
