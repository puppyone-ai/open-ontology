//! Source-side Arrow encoding of bounded, lease-bound sealed target pages.
use super::*;
use oo_pipeline::c1_typed_export::{Manifest, Part as ExportPart, Table as ExportTable, PART_BYTES, PART_ROWS, MAX_PARTS};

pub(super) struct ExportView<'a> {
    pub tables: &'a BTreeMap<String, Table>,
    pub binding: &'a RunBinding,
    pub l0: &'a str,
    pub uuid: &'a str,
}

fn target_cell(cell: &Value, column: &MappedColumn) -> Result<oo_connectors::types::DecodedCell, PreviewError> {
    if column.store == mysql::map::StoreKind::TimestampTz && cell["type"] != "null" {
        if cell["type"] != "text" {
            return Err(PreviewError::new("c1", "typed_export_timestamp"));
        }
        // These sealed values come from PostgreSQL, whose timestamptz text has
        // an explicit offset; do not parse them as timezone-free MySQL cells.
        let value = oo_pipeline::c1_parts::pg_text_cell(1184, cell["value"].as_str())
            .map_err(|_| PreviewError::new("c1", "typed_export_timestamp"))?;
        let oo_pipeline::c1_parts::Cell::Micros(micros) = value else {
            return Err(PreviewError::new("c1", "typed_export_timestamp"));
        };
        return Ok(oo_connectors::types::DecodedCell {
            presence: oo_connectors::types::CellPresence::Value,
            value: Some(oo_core::canonical::LogicalValue::TimestampTzMicros(micros)),
            arrow: Some(oo_connectors::types::ArrowScalar::TimestampUtc(micros)),
            reject: None, actual_precision: None, actual_scale: None, float_bits: None,
        });
    }
    let raw = crate::c1_materialize::raw_cell(cell, column.store)?;
    let (value, reject) = values::decode_cell(column.store, &raw, column.plan.mapping.nullable)?;
    if reject.is_some() {
        return Err(PreviewError::new("c1", "typed_export_materialize"));
    }
    Ok(value)
}

pub(super) fn materialize(
    input:&mut impl BufRead, out:&Path, context:&Value, capture:&ExportView<'_>,
    binding:&Value, schemas:&Value, config:&Value, epoch:u32, l_end:&str,
    deadline:u64, max_rows:u64,
) -> Result<Value,PreviewError> {
    let fail=||PreviewError::new("c1","typed_export_materialize");
    if binding["protocol"]!="oo.mysql.typed-export-binding/v1"
        || binding["source_id"]!=context["source_id"] || binding["task_id"]!=context["lease_id"]
        || binding["source_epoch"]!=epoch || binding["l0"]!=capture.l0
        || binding["measured_id"]!=format!("mysql.{}",capture.uuid)
        || binding["l_end"]!=l_end.rsplit('-').next().ok_or_else(fail)? {
        return Err(fail());
    }
    let mut pinned_binding=binding.clone();
    pinned_binding.as_object_mut().ok_or_else(fail)?.remove("lease_generation");
    let mut manifest=Manifest {protocol:oo_pipeline::c1_typed_export::PROTOCOL.into(),
        source_id:context["source_id"].as_str().ok_or_else(fail)?.into(),
        task_id:context["lease_id"].as_str().ok_or_else(fail)?.into(),
        source_generation:context["source_generation"].as_u64().ok_or_else(fail)?,
        source_epoch:epoch as i32,binding:pinned_binding,tables:Vec::new()};
    let mut total=0u64;
    for (index,(name,table)) in capture.tables.iter().enumerate() {
        let (schema,logical)=name.split_once('.').ok_or_else(fail)?;
        let dir=out.join(format!("export-table-{index:02}"));
        fs::create_dir_all(&dir).map_err(|_|fail())?;
        let schema_digest=sha256_hex(&serde_json::to_vec(&json!([{
            "table":logical,"columns":crate::c1_materialize::schema_columns(&table.columns)?}])).map_err(|_|fail())?);
        let mut exported=ExportTable {schema:schema.into(),table:logical.into(),spec:table.spec.clone(),
            rows:0,schema_digest,parts:Vec::new(),values_sha256:String::new()};
        let mut after=String::new();
        loop {
            if now_ms()?>=deadline {return Err(PreviewError::new("budget","cycle_deadline"));}
            send_frame(&json!({"kind":"export_page","table":name,"after_key":after}))?;
            let reply=read_worker_reply(input,"export_page")?;
            let reference=&reply["page"];
            if reference["protocol"]!="oo.mysql.typed-export-page-ref/v1" {return Err(fail());}
            let raw=oo_pipeline::c1_typed_export::read_capped(
                Path::new(reference["path"].as_str().ok_or_else(fail)?),PART_BYTES).map_err(|_|fail())?;
            if raw.len() as u64!=reference["bytes"].as_u64().ok_or_else(fail)?
                || sha256_hex(&raw)!=reference["sha256"] {return Err(fail());}
            let page:Value=serde_json::from_slice(&raw).map_err(|_|fail())?;
            if page["protocol"]!="oo.mysql.typed-export-page/v1" || page["table"]!=*name || page["after_key"]!=after {
                return Err(fail());
            }
            let rows=page["rows"].as_array().filter(|r|r.len()<=PART_ROWS).ok_or_else(fail)?;
            if serde_json::to_vec(rows).map_err(|_|fail())?.len()>PART_BYTES {return Err(fail());}
            if rows.is_empty() {
                if page["done"]!=true || page["last_key"]!=after {return Err(fail());}
                break;
            }
            if page["done"]!=false || exported.parts.len()>=MAX_PARTS {return Err(fail());}
            total=total.checked_add(rows.len() as u64).ok_or_else(fail)?;
            if total>max_rows {return Err(PreviewError::new("budget","typed_export_total_rows"));}
            let mut columns=table.columns.clone();
            columns.push(mysql::map::map_column(&MysqlColumn {
                name:"_oo_pos".into(),ordinal:columns.len() as i32+1,data_type:"decimal".into(),
                column_type:"decimal(38,0)".into(),nullable:false,char_len:None,
                precision:Some(38),scale:Some(0),charset:None,column_key:String::new(),
            },false,true,&BTreeSet::new())?);
            let mut data=Vec::with_capacity(rows.len());
            for row in rows {
                let cells=row["cells"].as_array().filter(|c|c.len()==table.columns.len()).ok_or_else(fail)?;
                let mut decoded=Vec::with_capacity(columns.len());
                for (cell,col) in cells.iter().zip(&table.columns) {
                    decoded.push(target_cell(cell, col)?);
                }
                let key=oo_pipeline::c1_key::identity(&table.spec.keys.iter()
                    .map(|&i|oo_pipeline::c1_key::source_component(&decoded[i]))
                    .collect::<Result<Vec<_>,_>>().map_err(|_|fail())?).map_err(|_|fail())?;
                if row["cursor"]!=oo_pipeline::c1_key::hex(&key) {return Err(fail());}
                let position=row["position"].as_str().ok_or_else(fail)?;
                let (pos,reject)=values::decode_cell(columns.last().ok_or_else(fail)?.store,
                    &RawCell::Bytes(position.as_bytes().to_vec()),false)?;
                if reject.is_some() {return Err(fail());}
                decoded.push(pos);data.push(decoded);
            }
            let mut run=capture.binding.clone();run.schema=schema.into();run.table=logical.into();
            let batch=assemble_batch(&run,columns.iter().map(|c|c.plan.clone()).collect(),
                &table.spec.keys.iter().map(|k|*k as i32+1).collect::<Vec<_>>(),data).map_err(|_|fail())?;
            let part_no=exported.parts.len()+1;
            let path=dir.join(format!("part-{part_no:08}.arrow"));
            // Re-export is deterministic over an immutable sealed stage; never replace
            // an already retained part with different bytes after an export interruption.
            let candidate=dir.join(format!("candidate-{part_no:08}.arrow"));
            crate::c1_chunked::write_part(&candidate,&batch)?;
            let bytes=fs::read(&candidate).map_err(|_|fail())?;
            if bytes.len()>PART_BYTES {return Err(PreviewError::new("budget","typed_export_part_bytes"));}
            if path.exists() {
                if fs::read(&path).map_err(|_|fail())?!=bytes {return Err(fail());}
                fs::remove_file(&candidate).map_err(|_|fail())?;
            } else {fs::rename(&candidate,&path).map_err(|_|fail())?;}
            let decoded=oo_pipeline::native_arrow::decode_arrow_stream(&bytes).map_err(|_|fail())?;
            let (digest,last,_)=oo_pipeline::c1_typed_export::rows_digest(&decoded,&table.spec,&after).map_err(|_|fail())?;
            if page["last_key"]!=last {return Err(fail());}
            exported.parts.push(ExportPart {index:part_no,path,sha256:sha256_hex(&bytes),bytes:bytes.len(),
                rows:rows.len(),after_key:after,last_key:last.clone(),values_sha256:digest});
            exported.rows+=rows.len() as u64;after=last;
        }
        let expected=binding["tables"].as_array().ok_or_else(fail)?.iter().find(|t|t["name"]==*name).ok_or_else(fail)?;
        if expected["rows"]!=exported.rows || expected["spec"]!=serde_json::to_value(&table.spec).map_err(|_|fail())? {
            return Err(fail());
        }
        exported.values_sha256=oo_pipeline::c1_typed_export::hash(&serde_json::to_vec(
            &exported.parts.iter().map(|p|(&p.values_sha256,p.rows)).collect::<Vec<_>>()).map_err(|_|fail())?);
        manifest.tables.push(exported);
    }
    let manifest_path=out.join("typed-export-manifest.json");
    let bytes=serde_json::to_vec(&manifest).map_err(|_|fail())?;
    if bytes.len()>oo_pipeline::c1_typed_export::MANIFEST_BYTES {return Err(fail());}
    if manifest_path.exists() {
        if fs::read(&manifest_path).map_err(|_|fail())?!=bytes {return Err(fail());}
    } else {fs::write(&manifest_path,&bytes).map_err(|_|fail())?;}
    let schema_bytes=serde_json::to_vec(schemas).map_err(|_|fail())?;
    let config_bytes=serde_json::to_vec(config).map_err(|_|fail())?;
    fs::write(out.join("admission-config.json"),&config_bytes).map_err(|_|fail())?;
    fs::write(out.join("schema-preimage.json"),&schema_bytes).map_err(|_|fail())?;
    let driver=std::env::current_exe().map_err(|_|fail())?;
    let driver_sha=sha256_hex(&fs::read(&driver).map_err(|_|fail())?);
    let report=json!({"protocol":"oo.source-admission.report/v1","target_id":manifest.source_id,
        "source_id":manifest.source_id,"secret_ref":config["secret_ref"],"compat_mode":"mysql-native",
        "permissions":config["permissions"],"r1w":null,"driver":{"kind":"native","name":"mysql_async",
        "version":"0.36.1","digest":driver_sha},"license_flags":["GPL-2.0"],
        "capabilities":[{"name":"Cdc","cursor":null,"feed":null,"cdc":"Binlog","token_order":"Total",
            "budget_unit":"Rows","level":"experimental","write_class":"R0","consistency":"T"}],
        "capture_methods":[{"method":"C1","write_class":"R0","consistency":"T","token_order":"Total",
            "retention_window":format!("{}s",context["source_preflight"]["binlog_expire_logs_seconds"].as_u64().ok_or_else(fail)?),
            "cursor":null,"feed":null,"cdc":"Binlog","delete_detection":"native"}],
        "cost_model":{"unit":"Rows","confidence":"unknown","estimate":"bounded sealed target parts"}});
    let report_path=out.join("typed-export-report.json");
    let report_bytes=serde_json::to_vec(&report).map_err(|_|fail())?;
    fs::write(&report_path,&report_bytes).map_err(|_|fail())?;
    let identity=json!({"engine":"mysql","measured_id":binding["measured_id"],
        "shard_id":context["target"]["shard"],"owner_token":context["target"]["owner"]});
    let entries=manifest.tables.iter().map(|t|json!({"schema":t.schema,"table":t.table,
        "row_count":t.rows,"schema_sha256":t.schema_digest,"parts_digest":t.values_sha256})).collect::<Vec<_>>();
    let scope=json!({"protocol":"oo.mysql.typed-export-scope/v1","engine":"mysql",
        "lineage":binding["measured_id"],"l0":capture.l0,"l_end":l_end,"complete":true,
        "config_digest":sha256_hex(&config_bytes),"schema_digest":sha256_hex(&schema_bytes),
        "consistency_grade":"T","cross_table_consistent":true,"source_identity":identity,"tables":entries});
    Ok(json!({"protocol":"oo.pipeline.native-c1-baseline/v1","c1_baseline_format":"typed-keys/v3",
        "capture_method":"c1_cdc","source_id":manifest.source_id,"source_generation":manifest.source_generation,
        "source_epoch":epoch,"source_identity":identity,"baseline_scope":scope,"entries":entries,
        "binding":context["binding"],"pin":context["pin"],"target_database":context["target"]["database"],
        "schema_preimage":{"path":out.join("schema-preimage.json"),"sha256":sha256_hex(&schema_bytes)},
        "target":context["target"],"typed_export":{"protocol":oo_pipeline::c1_typed_export::PROTOCOL,
            "path":manifest_path,"sha256":sha256_hex(&bytes),"max_rows":max_rows},
        "policy":{"source_id":manifest.source_id,"report_path":report_path,"report_sha256":sha256_hex(&report_bytes),
            "config_path":out.join("admission-config.json"),"config_sha256":sha256_hex(&config_bytes),
            "driver_path":driver,"driver_sha256":driver_sha,"admission_binary":context["admission_binary"],
            "admission_sha256":context["admission_sha256"],"source_preflight":context["source_preflight"],
            "reader_write_attempts":context["reader_write_attempts"]}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_target_timestamps_normalize_explicit_offsets_exactly() {
        let col = mysql::map::map_column(&MysqlColumn {
            name:"occurred_at".into(),ordinal:1,data_type:"timestamp".into(),
            column_type:"timestamp(6)".into(),nullable:true,char_len:None,
            precision:None,scale:None,charset:None,column_key:String::new(),
        }, false, true, &BTreeSet::new()).unwrap();
        for (text, micros) in [
            ("1970-01-01 00:00:00.000001+00", 1),
            ("1970-01-01 05:30:00.000001+05:30", 1),
            ("1969-12-31 23:59:59.999999+00", -1),
        ] {
            let actual = target_cell(&json!({"type":"text","value":text}), &col).unwrap();
            assert_eq!(actual.arrow, Some(oo_connectors::types::ArrowScalar::TimestampUtc(micros)));
        }
        assert_eq!(target_cell(&json!({"type":"null"}), &col).unwrap().presence,
            oo_connectors::types::CellPresence::Null);
        assert!(target_cell(&json!({"type":"text","value":"not a timestamp"}), &col).is_err());
        assert!(target_cell(&json!({"type":"text"}), &col).is_err());
        assert!(target_cell(&json!({"type":"bytes","value":"00"}), &col).is_err());
    }
}
