//! Resume materialization of an already sealed target; never reopen a source
//! snapshot or binlog stream. Only catalog/preflight/read-only audit queries run.
use super::*;

fn original_stream(out:&Path,settings:&Value)->Result<PathBuf,PreviewError> {
    if !initial_source::enabled(settings)? {return Ok(out.join("worker-stream.ndjson"));}
    fs::read_dir(out).map_err(|_|fault())?.filter_map(Result::ok).filter_map(|entry| {
        let path=entry.path();
        let lease=path.file_name()?.to_str()?.strip_prefix("worker-stream-lease-")?
            .strip_suffix(".ndjson")?;
        let n=lease.parse::<u64>().ok().filter(|n|*n>0&&n.to_string()==lease)?;
        Some((n,path))
    }).min_by_key(|(lease,_)|*lease).map(|(_,path)|path)
        .ok_or_else(||PreviewError::new("c1","sealed_export_trace_missing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_export_uses_oldest_retained_lease_without_requiring_lease_one() {
        let out=std::env::temp_dir().join(format!("initial-export-trace-{}-{}",
            std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&out).unwrap();
        let opt=json!({"c1_initial_resume":{"protocol":initial_source::PROTOCOL}});
        assert!(original_stream(&out,&opt).is_err());
        for name in ["worker-stream-lease-10.ndjson","worker-stream-lease-2.ndjson",
            "worker-stream-lease-01.ndjson","worker-export-lease-1.ndjson"] {
            fs::write(out.join(name),b"").unwrap();
        }
        assert_eq!(original_stream(&out,&opt).unwrap(),out.join("worker-stream-lease-2.ndjson"));
        assert_eq!(original_stream(&out,&json!({})).unwrap(),out.join("worker-stream.ndjson"));
        fs::remove_dir_all(out).unwrap();
    }
}

pub(super) async fn run(
    config:&Path,out:&Path,input:&mut impl BufRead,context:&mut Value,
) -> Result<(),PreviewError> {
    let refused=||PreviewError::new("c1","sealed_export_identity");
    let sealed=context["sealed_export"].clone();
    if sealed["protocol"]!="oo.mysql.typed-export-binding/v1"
        || sealed["source_id"]!=context["source_id"] || sealed["task_id"]!=context["lease_id"]
        || sealed["lease_generation"]!=context["lease_generation"] || !context["previous"].is_null()
        {return Err(refused());}
    let generation=crate::c1::handoff::initial_generation(context)?;
    let cfg=load_config(config)?;configured_selection(&cfg)?;
    let bytes=fs::read(config).map_err(|_|refused())?;
    let settings:Value=serde_json::from_slice(&bytes).map_err(|_|refused())?;
    let limit=export_row_limit(&settings)?.ok_or_else(refused)?;
    let session=out.join("typed-session.json");
    let saved=if session.exists() {read_document(&session)?["deadline_unix_ms"].as_u64()} else {None};
    if saved.is_none() && context["cycle_clock"].is_null() && settings.get("cycle_deadline_unix_ms").is_none() {
        return Err(PreviewError::new("c1","sealed_export_clock_missing"));
    }
    let deadline=cycle_clock::deadline(&settings,context,saved,now_ms()?)?;
    let original_path=original_stream(out,&settings)?;
    let mut first=String::new();
    BufReader::new(fs::File::open(original_path).map_err(|_|refused())?)
        .take(1024*1024+1).read_line(&mut first).map_err(|_|refused())?;
    if first.len()>1024*1024 {return Err(refused());}
    let original:Value=serde_json::from_str(&first).map_err(|_|refused())?;
    if original["kind"]!="open" || original["connection_config_sha256"]!=sha256_hex(&bytes)
        || original["l0"]!=sealed["l0"] || original["config_digest"]!=sealed["config_digest"]
        || original["schema_digest"]!=sealed["schema_digest"] {return Err(refused());}
    if initial_source::enabled(&settings)?
        && original["initial_header"]!=read_document(&out.join("typed-session.json"))? {
        return Err(refused());
    }
    let preserved=original.get("export_metadata").filter(|v|!v.is_null());
    let metadata_path=if let Some(reference)=preserved {
        let expected=out.join("export-metadata.json");
        if reference["path"]!=json!(expected) {return Err(refused());}
        expected
    } else {out.join("export-metadata-legacy-repair.json")};
    let mut source_connected=false;
    if !metadata_path.exists() {
        if preserved.is_some() || context["allow_legacy_metadata_probe"]!=true {
            return Err(PreviewError::new("c1","sealed_export_metadata_missing"));
        }
        durable_document(&out.join("legacy-metadata-probe-intent.json"),&json!({
            "source_id":context["source_id"],"task_id":context["lease_id"],
            "started_at_unix_ms":now_ms()?,"source_connected":true,"queries":"preflight/catalog/reader-write-audit only"}))?;
        let mut probe=connect_cdc(&cfg).await?;
        let preflight=preflight_values(&mut probe).await?;
        replication_grants_ok(&rows_first(&probe.query::<Row,_>("SHOW GRANTS FOR CURRENT_USER()")
            .await.map_err(mysql_error)?)?)?;
        let uuid=scalar(&mut probe,"SELECT @@GLOBAL.server_uuid").await?;
        let mut catalogs=BTreeMap::new();
        for qualified in &cfg.tables {
            let (schema,name)=qualified.split_once('.').ok_or_else(refused)?;
            let selected=mysql::SelectedTable{schema:schema.into(),name:name.into(),columns:cfg.columns[qualified].clone()};
            let catalog=mysql::catalog_columns(&mut probe,&mut Vec::new(),&selected).await?;
            let keys=mysql::chunk::key_columns(&mut probe,schema,name).await?;
            catalogs.insert(qualified.clone(),json!({"catalog":catalog,"keys":keys}));
        }
        drop(probe);
        let audit_dir=out.join("legacy-metadata-audit");
        fs::create_dir(&audit_dir).map_err(|_|refused())?;
        let writes=reader_write_audit(&cfg,&audit_dir).await?;
        durable_document(&metadata_path,&json!({"protocol":"oo.mysql.export-metadata/v1",
            "measurement_phase":"legacy_sealed_repair","source_connected":true,"measured_at_unix_ms":now_ms()?,
            "source_id":context["source_id"],"task_id":context["lease_id"],"l0":sealed["l0"],
            "source_uuid":uuid,"connection_config_sha256":sha256_hex(&bytes),
            "source_preflight":preflight,"reader_write_attempts":writes,"tables":catalogs}))?;
        source_connected=true;
    }
    let metadata_bytes=oo_pipeline::c1_typed_export::read_capped(&metadata_path,1024*1024)
        .map_err(|_|refused())?;
    let metadata_hash=sha256_hex(&metadata_bytes);
    if preserved.is_some_and(|r|r["sha256"]!=metadata_hash) {return Err(refused());}
    let metadata:Value=serde_json::from_slice(&metadata_bytes).map_err(|_|refused())?;
    if metadata["protocol"]!="oo.mysql.export-metadata/v1" || metadata["source_id"]!=context["source_id"]
        || metadata["task_id"]!=context["lease_id"] || metadata["l0"]!=sealed["l0"]
        || metadata["connection_config_sha256"]!=sha256_hex(&bytes)
        || metadata["tables"].as_object().is_none_or(|t|t.len()!=cfg.tables.len())
        || metadata["reader_write_attempts"]!=0 {return Err(refused());}
    let uuid=metadata["source_uuid"].as_str().ok_or_else(refused)?;
    crate::c1::handoff::validate(&settings,context,uuid)?;
    if sealed["measured_id"]!=format!("mysql.{uuid}") {return Err(refused());}
    let mut tables=BTreeMap::new();
    let reserved=oo_connectors::PG17_RESERVED.iter().map(|s|s.to_string()).collect();
    for qualified in &cfg.tables {
        let (schema,name)=qualified.split_once('.').ok_or_else(refused)?;
        let selected=mysql::SelectedTable{schema:schema.into(),name:name.into(),columns:cfg.columns[qualified].clone()};
        let catalog:Vec<MysqlColumn>=serde_json::from_value(metadata["tables"][qualified]["catalog"].clone()).map_err(|_|refused())?;
        let mut columns=mysql::keep_requested_keyed(
            mysql::map::map_keyed_table(&catalog,false,true,&reserved)?,&selected.columns)?;
        for (i,col) in columns.iter_mut().enumerate() {col.plan.attnum=i as i32+1;}
        let keys:Vec<KeyColumn>=serde_json::from_value(metadata["tables"][qualified]["keys"].clone()).map_err(|_|refused())?;
        let spec=TableSpec{columns:columns.iter().map(|c|Column{name:c.plan.name.clone(),
            oid:c.oid,typmod:c.typmod,nullable:!c.not_null}).collect(),
            keys:keys.iter().map(|k|columns.iter().position(|c|c.plan.name==k.name).ok_or_else(refused))
                .collect::<Result<Vec<_>,_>>()?};
        spec.validate().map_err(|_|refused())?;
        if original["typed_tables"][qualified]!=serde_json::to_value(&spec).map_err(|_|refused())? {
            return Err(refused());
        }
        tables.insert(qualified.clone(),Table{catalog,columns,keys,spec});
    }
    let schemas=json!(tables.iter().map(|(name,t)|Ok(json!({"table":name.split_once('.').ok_or_else(refused)?.1,
        "columns":crate::c1_materialize::schema_columns(&t.columns)?}))).collect::<Result<Vec<_>,PreviewError>>()?);
    if schemas!=original["schema_preimage"] {return Err(refused());}
    context["secret_ref"]=json!(cfg.secret_ref);
    context["source_preflight"]=metadata["source_preflight"].clone();
    let mut admission=crate::c1_materialize::admission_config(context,&cfg.tables);
    admission["budget"]["hard_limit"]=json!(limit);
    if admission!=original["admission_config"]
        || sha256_hex(&serde_json::to_vec(&admission).map_err(|_|refused())?)!=sealed["config_digest"]
        || sha256_hex(&serde_json::to_vec(&schemas).map_err(|_|refused())?)!=sealed["schema_digest"] {
        return Err(refused());
    }
    context["reader_write_attempts"]=metadata["reader_write_attempts"].clone();
    send_frame(&json!({"kind":"export_resume","source_id":context["source_id"],
        "task_id":context["lease_id"],"source_epoch":sealed["source_epoch"],"l0":sealed["l0"],
        "l_end":sealed["l_end"],"config_digest":sealed["config_digest"],"schema_digest":sealed["schema_digest"],
        "connection_config_sha256":sha256_hex(&bytes),"source_snapshot_reexecuted":false,"binlog_reopened":false,
        "source_connected":source_connected,"export_metadata":{"path":metadata_path,"sha256":metadata_hash},
        "measurement_phase":metadata["measurement_phase"]}))?;
    read_worker_reply(input,"export_resume")?;
    let source=context["source_id"].as_str().ok_or_else(refused)?;
    let binding=RunBinding{source_id:source.into(),schema:String::new(),table:String::new(),
        run_generation:generation,input_digest:[0;32]};
    let l0=sealed["l0"].as_str().ok_or_else(refused)?;
    let end=sealed["l_end"].as_str().ok_or_else(refused)?;
    let epoch=sealed["source_epoch"].as_u64().and_then(|n|u32::try_from(n).ok()).ok_or_else(refused)?;
    let view=export_parts::ExportView{tables:&tables,binding:&binding,l0,uuid};
    let request=export_parts::materialize(input,out,context,&view,&sealed,&schemas,&admission,
        epoch,&format!("{uuid}:1-{end}"),deadline,limit)?;
    write_json(&out.join("native-request.json"),&request)?;
    send_frame(&json!({"kind":"materialized","request":request}))?;
    Ok(())
}
