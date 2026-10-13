//! Real C5 read-only snapshot producer. Source bytes use the existing type decoder.
use std::{fs, path::Path};
use serde_json::{Value, json};
use oo_connectors::types::ArrowPhysical;
use oo_pipeline::target_installer::sha256_hex;
use mysql_async::prelude::Queryable;
use crate::{config, source, error::PreviewError};

pub async fn capture_native(config_path: &Path, out: &Path) -> Result<(), PreviewError> {
    let bytes = fs::read(config_path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let config = config::parse_config(text)?;
    if config.stop != config::Stop::Seal { return Err(PreviewError::new("config", "stop")); }
    fs::create_dir(out).map_err(|_| PreviewError::new("config", "out_exists"))?;
    let password = config::read_secret(&config.mysql_password_file)?;
    let read = source::read_source_export(&config, &password, &bytes, Some(out)).await?;
    // This is the server's instrumented per-user counter, including rejected writes.
    // A missing observation refuses capture authority; it never becomes zero.
    let mut auditor = source::open_mysql(&config, &password).await?;
    let audit: Vec<(String,u64,u64)> = auditor.exec(
        "SELECT EVENT_NAME, COUNT_STAR, SUM_ERRORS FROM performance_schema.events_statements_summary_by_user_by_event_name WHERE USER = ? AND EVENT_NAME IN ('statement/sql/alter_table','statement/sql/create_table','statement/sql/delete','statement/sql/delete_multi','statement/sql/drop_table','statement/sql/insert','statement/sql/insert_select','statement/sql/replace','statement/sql/replace_select','statement/sql/truncate','statement/sql/update','statement/sql/update_multi')", (&config.mysql_user,))
        .await.map_err(|_| PreviewError::new("privilege", "write_audit"))?;
    if audit.len() != 12 { return Err(PreviewError::new("privilege", "write_unobserved")); }
    let writes = audit.iter().map(|row| row.1).sum::<u64>();
    write(out,"reader-write-audit.json",&json!({"source_user":config.mysql_user,"server_counters":audit,"write_attempts":writes}))?;
    if writes != 0 { return Err(PreviewError::new("privilege", "write_attempt_observed")); }
    drop(auditor);
    let mut schema = Vec::new();
    let mut types = Vec::new();
    let mut tables = Vec::new();
    for table in &read.tables {
        let mut columns = Vec::new();
        for column in &table.columns {
            let arrow = column.plan.arrow.as_ref().ok_or_else(|| PreviewError::new("batch", "arrow_type"))?;
            let (kind, precision, scale) = arrow_description(arrow)?;
            let field = json!({"name":column.plan.name,"arrow_type":kind,"nullable":column.plan.mapping.nullable,"precision":precision,"scale":scale,"key":column.key,"document":column.oid == 114,"char_length":if column.oid == 1042 { Some(column.typmod - 4) } else { None }});
            columns.push(field.clone());
            let mut typ = field;
            typ["table"] = json!(table.name);
            typ["source_type"] = json!(column.plan.mapping.source_type);
            typ["target_oid"] = json!(column.oid);
            typ["target_typmod"] = json!(column.typmod);
            typ["caveat"] = json!(column.caveat);
            typ.as_object_mut().ok_or_else(|| PreviewError::new("batch", "arrow_type"))?.remove("nullable");
            typ.as_object_mut().ok_or_else(|| PreviewError::new("batch", "arrow_type"))?.remove("key");
            types.push(typ);
        }
        schema.push(json!({"table":table.name,"columns":columns}));
        let arrow = out.join(format!("table-{}.arrow",table.index));
        tables.push(json!({"name":table.name,"schema":table.schema,"arrow":format!("table-{}.arrow",table.index),"arrow_path":arrow,"arrow_sha256":sha256_hex(&fs::read(&arrow).map_err(|_| PreviewError::new("batch", "arrow_file"))?),"row_count":table.batches.iter().map(|batch|batch.rows).sum::<u64>(),"route":"C5","consistency":"S1","delete_mode":"reconcile","cursor_safety":"no_cursor","quality_rejects":table.rejects}));
    }
    write(out, "schema-preimage.json", &json!(schema))?;
    write(out, "type-preimage.json", &json!(types))?;
    write(out, "access.json", &source::access_document(&read))?;
    let driver = std::env::current_exe().map_err(|_| PreviewError::new("config", "driver"))?;
    write(out, "driver.json", &json!({"protocol":"oo.mysql.capture-driver/v1","kind":"mysql_async","version":"0.36.1","executable_path":driver,"executable_sha256":sha256_hex(&fs::read(&driver).map_err(|_| PreviewError::new("config", "driver"))?),"mysql_version":read.version,"source_id":config.source_id,"source_generation":config.generation}))?;
    write(out, "capturer.json", &json!({"protocol":"oo.mysql.capture-tick/v1","database":config.mysql_database,"label":read.config_digest,"source_id":config.source_id,"source_generation":config.generation,"run_id":config.owner,"snapshot_token":false,"product_reader_write_attempts":writes,"snapshot":{"kind":"mysql_consistent_snapshot","value":read.config_digest},"tables":tables,"source_statement_notes":read.statements.iter().map(|note|json!({"verb":note.verb,"sha256":note.sha256})).collect::<Vec<_>>()}))?;
    Ok(())
}

fn arrow_description(arrow: &ArrowPhysical) -> Result<(&'static str, Option<i32>, Option<i32>),PreviewError> {
    Ok(match arrow {
        ArrowPhysical::Boolean => ("bool",None,None),
        ArrowPhysical::Int16 => ("int16",None,None),
        ArrowPhysical::Int32 => ("int32",None,None),
        ArrowPhysical::Int64 => ("int64",None,None),
        ArrowPhysical::Float64 => ("float64",None,None),
        ArrowPhysical::Utf8 => ("utf8",None,None),
        ArrowPhysical::Binary => ("binary",None,None),
        ArrowPhysical::Date32 => ("date32",None,None),
        ArrowPhysical::DurationMicros => ("duration_micros",None,None),
        ArrowPhysical::TimestampMicrosNaive => ("timestamp_micros",None,None),
        ArrowPhysical::TimestampMicrosUtc => ("timestamp_micros_utc",None,None),
        ArrowPhysical::Decimal128{precision,scale} => ("decimal128",Some(i32::from(*precision)),Some(i32::from(*scale))),
        ArrowPhysical::Decimal256{precision,scale} => ("decimal256",Some(i32::from(*precision)),Some(i32::from(*scale))),
        _ => return Err(PreviewError::new("batch", "arrow_type")),
    })
}

fn write(out: &Path,name: &str,value: &Value) -> Result<(),PreviewError> {
    fs::write(out.join(name),serde_json::to_vec_pretty(value).map_err(|_|PreviewError::new("config","out"))?).map_err(|_|PreviewError::new("config","out"))
}
