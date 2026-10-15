//! Materialize only the worker's durable sealed stage, using the existing MySQL
//! type decoder and the ordinary admission sealer. This reader never applies it.
use std::{collections::BTreeSet, fs, path::Path, process::Command};
use arrow_ipc::writer::StreamWriter;
use oo_connectors::types::{RunBinding, assemble_batch};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use crate::{error::PreviewError, map::{MappedColumn, StoreKind}, values::{self, RawCell}};

type Result<T> = std::result::Result<T, PreviewError>;
fn err() -> PreviewError { PreviewError::new("baseline", "materialization") }
pub(crate) fn write(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec(value).map_err(|_|err())?).map_err(|_|err())
}
pub(crate) fn admission_config(context: &Value, tables: &[String]) -> Value {
    json!({"protocol":"oo.source-admission.config/v1","target_id":context["source_id"],"source_id":context["source_id"],
        "secret_ref":context["secret_ref"].as_str().unwrap_or("secret://mysql-native"),"compat_mode":"mysql-native",
        "permissions":tables.iter().map(|table| { let (schema,name)=table.split_once('.').unwrap_or(("",""));
            json!({"name":"SELECT","status":"known","granted":true,"scope":{"database":schema,"table":name}}) }).collect::<Vec<_>>(),
        "r1w":null,"budget":{"unit":"Rows","hard_limit":100000,"confidence":"unknown"},
        "write_options":{"signal_table":false,"heartbeat_write":false,"create_stream":false,"create_trigger":false},
        "debezium":{"signal_data_collection":null,"heartbeat_action_query":null,"snapshot_mode":"never","create_stream":false,"create_trigger":false},
        "requested_write_class":"R0"})
}
pub(crate) fn schema_columns(columns: &[MappedColumn]) -> Result<Vec<Value>> {
    columns.iter().map(|column| {
        let (kind,precision,scale)=crate::capture::arrow_description(column.plan.arrow.as_ref().ok_or_else(err)?)?;
        Ok(json!({"name":column.plan.name,"arrow_type":kind,"nullable":column.plan.mapping.nullable,"precision":precision,
            "scale":scale,"key":column.key,"document":column.oid==114,"char_length":if column.oid==1042 {Some(column.typmod-4)} else {None}}))
    }).collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn materialize(out: &Path, context: &Value, tables: &[(String,String,Vec<MappedColumn>)],
    rows: &[Value], l0: &str, l_end: &str, lineage: &str, epoch: u32, schemas: &Value, config: &Value) -> Result<Value> {
    let source=context["source_id"].as_str().ok_or_else(err)?;
    let generation=context["source_generation"].as_u64().ok_or_else(err)?;
    let driver=std::env::current_exe().map_err(|_|err())?;
    let admission=context["admission_binary"].as_str().ok_or_else(err)?;
    if sha256_hex(&fs::read(admission).map_err(|_|err())?) != context["admission_sha256"] {return Err(err());}
    let driver_sha=sha256_hex(&fs::read(&driver).map_err(|_|err())?);
    let config_bytes=serde_json::to_vec(config).map_err(|_|err())?;
    let schema_bytes=serde_json::to_vec(schemas).map_err(|_|err())?;
    let config_digest=sha256_hex(&config_bytes); let schema_digest=sha256_hex(&schema_bytes);
    write(&out.join("admission-config.json"),config)?; write(&out.join("schema-preimage.json"),schemas)?;
    let mut types=Vec::new();
    for (schema,table,columns) in tables {
        let _=schema;
        for (column,mut field) in columns.iter().zip(schema_columns(columns)?) {
            field["table"]=json!(table);field["source_type"]=json!(column.plan.mapping.source_type);
            field["target_oid"]=json!(column.oid);field["target_typmod"]=json!(column.typmod);field["caveat"]=json!(column.caveat);
            let map=field.as_object_mut().ok_or_else(err)?;map.remove("nullable");map.remove("key");types.push(field);
        }
    }
    write(&out.join("type-preimage.json"),&json!(types))?;
    let mut native_requests=Vec::new();let mut entries=Vec::new();let mut capture_tables=Vec::new();let mut scope_tables=Vec::new();
    for (index,(schema,table,columns)) in tables.iter().enumerate() {
        let selected=rows.iter().filter(|r|r["schema"]==*schema && r["table"]==*table && r["deleted"]==false).collect::<Vec<_>>();
        let mut decoded=Vec::with_capacity(selected.len());
        let mut rejects=Vec::new();
        let mut keys=BTreeSet::new();
        for (row_index,row) in selected.iter().enumerate() {
            if !keys.insert(serde_json::to_vec(&row["key"]).map_err(|_|err())?) {return Err(err());}
            let mut cells=Vec::with_capacity(columns.len());
            for col in columns {
                let raw=raw_cell(&row["value"][&col.plan.name],col.store)?;
                let (cell,reject)=values::decode_cell(col.store,&raw,col.plan.mapping.nullable)?;
                if let Some(note)=reject {
                    let refused=values::reject_for(&note);
                    rejects.push(json!({"attnum":col.plan.attnum,"column":col.plan.name,"key_digest":"","reason":refused.reason,"row":row_index,"table":table,"value_len":refused.value_len}));
                }
                cells.push(cell);
            }
            decoded.push(cells);
        }
        let binding=RunBinding { source_id:source.to_owned(),schema:schema.clone(),table:table.clone(),run_generation:generation,input_digest:[0;32] };
        let keys=columns.iter().filter(|c|c.key).map(|c|c.plan.attnum).collect::<Vec<_>>();
        let typed=assemble_batch(&binding,columns.iter().map(|c|c.plan.clone()).collect(),&keys,decoded).map_err(|_|err())?;
        for reject in &mut rejects {
            let row=typed.rows.get(reject["row"].as_u64().ok_or_else(err)? as usize).ok_or_else(err)?;
            let digest=row.key_digest.ok_or_else(err)?;
            reject["key_digest"]=json!(digest.iter().map(|b|format!("{b:02x}")).collect::<String>());
        }
        let batch=typed.record_batch().map_err(|_|err())?;
        let path=out.join(format!("table-{index}.arrow"));
        let mut writer=StreamWriter::try_new(fs::File::create(&path).map_err(|_|err())?,&batch.schema()).map_err(|_|err())?;
        writer.write(&batch).map_err(|_|err())?;writer.finish().map_err(|_|err())?;
        let sha=sha256_hex(&fs::read(&path).map_err(|_|err())?);
        let schema_sha=sha256_hex(&serde_json::to_vec(&json!([{ "table":table,"columns":schema_columns(columns)? }])).map_err(|_|err())?);
        entries.push(json!({"schema":schema,"table":table,"source_generation":generation,"schema_sha256":schema_sha,
            "snapshot_arrow":{"path":path,"sha256":sha},"transactions":[]}));
        scope_tables.push(json!({"schema":schema,"table":table,"row_count":selected.len(),"arrow_sha256":sha,"schema_sha256":schema_sha}));
        capture_tables.push(json!({"name":table,"schema":schema,"arrow_path":path,"arrow_sha256":sha,"row_count":selected.len(),
            "route":"C1","consistency":"T","delete_mode":"native","cursor_safety":"no_cursor","quality_rejects":rejects}));
    }
    let identity=json!({"engine":"mysql","measured_id":lineage,"shard_id":context["target"]["shard"],"owner_token":context["target"]["owner"]});
    let mut scope=json!({"protocol":"oo.source-admission.c1-baseline-scope/v1","engine":"mysql","lineage":lineage,
        "l0":l0,"l_end":l_end,"complete":true,"read_fence_digest":sha256_hex(&serde_json::to_vec(rows).map_err(|_|err())?),
        "config_digest":config_digest,"schema_digest":schema_digest,"consistency_grade":"T","cross_table_consistent":true,
        "source_identity":identity,"tables":scope_tables});
    let mut envelope=json!({"protocol":"oo.source-admission.c1-capture-envelope/v1","baseline_scope":scope,"entries":entries,"capture_digest":"0".repeat(64)});
    let capture_digest=sha256_hex(&serde_json::to_vec(&envelope).map_err(|_|err())?);scope["capture_digest"]=json!(capture_digest);
    // Admission binds source rows and the global manifest before registry refs exist.
    envelope["capture_digest"]=json!(capture_digest);
    let snapshot=json!({"kind":"mysql_cdc_first","value":l_end});
    let capturer=json!({"protocol":"oo.mysql.capture-tick/v1","source_id":source,"source_generation":generation,
        "run_id":context["target"]["owner"],"database":tables.first().ok_or_else(err)?.0,"source_preflight":context["source_preflight"],"snapshot":snapshot,"tables":capture_tables,"product_reader_write_attempts":context["reader_write_attempts"],"snapshot_token":false,
        "cross_table_consistency":"T","baseline_scope":scope,"capture_envelope":envelope});
    write(&out.join("capturer.json"),&capturer)?;
    for entry in &mut entries {
        let table=entry["table"].as_str().ok_or_else(err)?.to_owned();let schema=entry["schema"].as_str().ok_or_else(err)?.to_owned();
        let dest=out.join(&table);fs::create_dir(&dest).map_err(|_|err())?;
        let table_schema=schemas.as_array().ok_or_else(err)?.iter().find(|s|s["table"]==table).ok_or_else(err)?;
        write(&dest.join("schema-preimage.json"),&json!([table_schema]))?;
        let witness=json!({"protocol":"oo.source-admission.capture-witness/v1","source_engine":"mysql","source_id":source,
            "source_generation":generation,"snapshot":snapshot,"run_id":context["target"]["owner"],"logical_schema":schema,"logical_table":table,
            "capturer_path":out.join("capturer.json"),"arrow_path":entry["snapshot_arrow"]["path"],"driver_path":driver,
            "schema_preimage_path":dest.join("schema-preimage.json"),"type_preimage_path":out.join("type-preimage.json")});
        write(&dest.join("witness.json"),&witness)?;
        let report=json!({"protocol":"oo.source-admission.report/v1","target_id":source,"source_id":source,"secret_ref":config["secret_ref"],
            "compat_mode":"mysql-native","permissions":config["permissions"],"r1w":null,
            "driver":{"kind":"native","name":"mysql_async","version":"0.36.1","digest":driver_sha},"license_flags":["GPL-2.0"],
            "capabilities":[{"name":"Cdc","cursor":null,"feed":null,"cdc":"Binlog","token_order":"Total","budget_unit":"Rows","level":"experimental","write_class":"R0","consistency":"T"}],
            "capture_methods":[{"method":"C1","write_class":"R0","consistency":"T","token_order":"Total","retention_window":format!("{}s",context["source_preflight"]["binlog_expire_logs_seconds"].as_u64().ok_or_else(err)?),"cursor":null,"feed":null,"cdc":"Binlog","delete_detection":"native"}],
            "cost_model":{"unit":"Rows","confidence":"unknown","estimate":"bounded owned fixture"},"capture_witness":dest.join("witness.json")});
        write(&dest.join("report.json"),&report)?;
        let output=Command::new(admission).args(["seal-native","--report"]).arg(dest.join("report.json"))
            .arg("--config").arg(out.join("admission-config.json")).arg("--out").arg(dest.join("sealed")).output().map_err(|_|err())?;
        fs::write(dest.join("seal.stdout"),&output.stdout).map_err(|_|err())?;fs::write(dest.join("seal.stderr"),&output.stderr).map_err(|_|err())?;
        fs::write(dest.join("seal.exit"),output.status.code().unwrap_or(-1).to_string()).map_err(|_|err())?;
        if !output.status.success() { return Err(err()); }
        let registry_path=dest.join("sealed/registry.json");let registry=fs::read(&registry_path).map_err(|_|err())?;
        let registry_value:Value=serde_json::from_slice(&registry).map_err(|_|err())?;
        let b=&registry_value["bindings"][0];
        let columns=b["fields"].as_array().ok_or_else(err)?.iter().map(|f|{let mut f=f.clone();f["logical_name"]=f["name"].clone();f}).collect::<Vec<_>>();
        let mut request=json!({"protocol":"oo.pipeline.native-update/v1","source_engine":"mysql","source_id":source,"source_generation":generation,
            "parent_source_generation":if generation==1 {None} else {Some(generation-1)},"logical_schema":schema,"logical_table":table,
            "batch_id":format!("c1-{}-{}",context["binding"].as_str().ok_or_else(err)?,context["pin"]),"run_id":b["run_id"],
            "capture_method":"c1_cdc","delete_detection":"native","consistency_grade":"T","cross_table_consistent":true,
            "position_codec":"mysql-gtid/v1","position_lineage":lineage,"applied_pos":l_end.rsplit('-').next().ok_or_else(err)?,
            "full_snapshot":true,"columns":columns,"key_columns":columns.iter().filter(|f|f["key"]==true).map(|f|f["name"].clone()).collect::<Vec<_>>(),
            "row_positions":rows.iter().filter(|r|r["schema"]==schema && r["table"]==table && r["deleted"]==false).map(|r|r["applied_pos"].clone()).collect::<Vec<_>>(),
            "tombstones":[],"arrow_path":entry["snapshot_arrow"]["path"],"max_arrow_bytes":33554432,
            "admission_path":b["admission_receipt_path"],"snapshot":b["snapshot"],"audit_digest":b["capture_receipt_sha256"],"target":context["target"],"mirror":false});
        for key in ["config_digest","schema_digest","type_digest","input_digest"] {request[key]=b[key].clone();}
        native_requests.push(request);entry["admission_registry_path"]=json!(registry_path);entry["admission_registry_sha256"]=json!(sha256_hex(&registry));
    }
    // Keep the authenticated envelope entries equal to the exact request. Only
    // derived registry references are omitted from its capture digest projection.
    // Native loader requests are separate from the source capture envelope.
    let mut result=json!({"protocol":"oo.pipeline.native-c1-baseline/v1","source_id":source,"binding":context["binding"],"pin":context["pin"],
        "source_generation":generation,"capture_method":"c1_cdc","target_database":context["target"]["database"],"source_epoch":epoch,
        "source_identity":identity,"baseline_scope":scope,"entries":entries,"native_requests":native_requests,"capture_envelope_path":out.join("capture-envelope.json")});
    envelope["entries"]=result["entries"].clone();write(&out.join("capture-envelope.json"),&envelope)?;
    result["capture_envelope_sha256"]=json!(sha256_hex(&fs::read(out.join("capture-envelope.json")).map_err(|_|err())?));
    write(&out.join("native-request.json"),&result)?;Ok(result)
}

fn raw_cell(cell: &Value, store: StoreKind) -> Result<RawCell> {
    if cell["type"]=="null" {return Ok(RawCell::Null);}
    let text=cell["value"].as_str().ok_or_else(err)?;
    if cell["type"]=="int" {return text.parse::<i64>().map(RawCell::Int).or_else(|_|text.parse::<u64>().map(RawCell::Uint)).map_err(|_|err());}
    if cell["type"]=="bytes" {
        if text.len()%2!=0 {return Err(err());}
        let bytes=text.as_bytes().chunks_exact(2).map(|pair|u8::from_str_radix(std::str::from_utf8(pair).map_err(|_|err())?,16).map_err(|_|err())).collect::<Result<Vec<_>>>()?;
        return Ok(RawCell::Bytes(bytes));
    }
    if matches!(store,StoreKind::Date|StoreKind::TimestampNaive|StoreKind::TimestampTz) {
        let chunks=text.trim_end_matches('Z').split(['-','T',' ',':','.']).map(|s|s.parse::<u32>().map_err(|_|err())).collect::<Result<Vec<_>>>()?;
        if matches!(store,StoreKind::Date) && chunks.len()==3 {
            return Ok(RawCell::Date {year:u16::try_from(chunks[0]).map_err(|_|err())?,month:u8::try_from(chunks[1]).map_err(|_|err())?,day:u8::try_from(chunks[2]).map_err(|_|err())?,hour:0,minute:0,second:0,micros:0});
        }
        if chunks.len()!=6 && chunks.len()!=7 {return Err(err());}
        return Ok(RawCell::Date {year:u16::try_from(chunks[0]).map_err(|_|err())?,month:u8::try_from(chunks[1]).map_err(|_|err())?,day:u8::try_from(chunks[2]).map_err(|_|err())?,hour:u8::try_from(chunks[3]).map_err(|_|err())?,minute:u8::try_from(chunks[4]).map_err(|_|err())?,second:u8::try_from(chunks[5]).map_err(|_|err())?,micros:chunks.get(6).copied().unwrap_or(0)});
    }
    if matches!(store,StoreKind::Time) {
        let negative=text.starts_with('-');let text=text.trim_start_matches('-');
        let parts=text.split([':', '.']).map(|s|s.parse::<u32>().map_err(|_|err())).collect::<Result<Vec<_>>>()?;
        if parts.len()!=3 && parts.len()!=4 {return Err(err());}
        return Ok(RawCell::Time {negative,days:parts[0]/24,hours:u8::try_from(parts[0]%24).map_err(|_|err())?,minutes:u8::try_from(parts[1]).map_err(|_|err())?,seconds:u8::try_from(parts[2]).map_err(|_|err())?,micros:parts.get(3).copied().unwrap_or(0)});
    }
    Ok(RawCell::Bytes(text.as_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_cells_keep_large_unsigned_binary_null_and_signed_time() {
        assert!(matches!(raw_cell(&json!({"type":"int","value":"18446744073709551615"}),StoreKind::Numeric{precision:20,scale:0}).unwrap(),RawCell::Uint(u64::MAX)));
        assert!(matches!(raw_cell(&json!({"type":"bytes","value":"00ff80"}),StoreKind::Bytes).unwrap(),RawCell::Bytes(v) if v==vec![0,255,128]));
        assert!(matches!(raw_cell(&json!({"type":"null"}),StoreKind::Bytes).unwrap(),RawCell::Null));
        assert!(matches!(raw_cell(&json!({"type":"text","value":"-25:00:00.000001"}),StoreKind::Time).unwrap(),RawCell::Time{negative:true,days:1,hours:1,micros:1,..}));
        assert!(raw_cell(&json!({"type":"bytes","value":"0z"}),StoreKind::Bytes).is_err());
        assert!(raw_cell(&json!({"type":"bytes","value":"aåb"}),StoreKind::Bytes).is_err());
    }
    #[test]
    fn date_only_and_zero_date_follow_existing_nullable_quality_policy() {
        let raw=raw_cell(&json!({"type":"text","value":"2024-01-02"}),StoreKind::Date).unwrap();
        assert!(values::decode_cell(StoreKind::Date,&raw,true).unwrap().1.is_none());
        let raw=raw_cell(&json!({"type":"text","value":"0000-00-00"}),StoreKind::Date).unwrap();
        assert_eq!(values::decode_cell(StoreKind::Date,&raw,true).unwrap().1.unwrap().reason,"zero_date");
        assert!(values::decode_cell(StoreKind::Date,&raw,false).is_err());
    }

}
