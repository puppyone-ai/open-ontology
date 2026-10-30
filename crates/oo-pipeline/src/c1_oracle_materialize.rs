//! The common Loader materializes its sealed typed target, then invokes the
//! ordinary admission sealer. A source codec is used only for local Arrow IO.
use crate::{c1_buffer_position, target_installer::sha256_hex};
use serde_json::{Value,json};
use std::{fs, io::{Read,Write}, path::{Path,PathBuf}, process::{Command,Stdio}};
type Result<T> = std::result::Result<T,&'static str>;

fn text<'a>(v:&'a Value,key:&str)->Result<&'a str>{v[key].as_str().filter(|s|!s.is_empty()&&!s.contains('\0')).ok_or("oracle_materialize_input")}
fn write(path:&Path,value:&Value)->Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f=fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|_|"oracle_materialize_file")?;
    f.write_all(&serde_json::to_vec(value).map_err(|_|"oracle_materialize_file")?).and_then(|_|f.sync_all()).map_err(|_|"oracle_materialize_file")
}
fn pinned(value:&Value,limit:u64)->Result<Vec<u8>>{
    let path=Path::new(text(value,"path")?);
    if !path.is_absolute()||path.components().any(|p|p==std::path::Component::ParentDir) {return Err("oracle_artifact");}
    let mut bytes=Vec::new();fs::File::open(path).map_err(|_|"oracle_artifact")?.take(limit+1).read_to_end(&mut bytes).map_err(|_|"oracle_artifact")?;
    if bytes.is_empty()||bytes.len() as u64>limit||sha256_hex(&bytes)!=text(value,"sha256")?
        || value.get("bytes").is_some_and(|n|n.as_u64()!=Some(bytes.len() as u64)) {return Err("oracle_artifact");}
    Ok(bytes)
}
fn doc(value:&Value,limit:u64)->Result<Value>{serde_json::from_slice(&pinned(value,limit)?).map_err(|_|"oracle_artifact")}
fn reference(path:&Path)->Result<Value>{
    let bytes=fs::read(path).map_err(|_|"oracle_artifact")?;
    Ok(json!({"path":path,"sha256":sha256_hex(&bytes),"bytes":bytes.len()}))
}
fn hash(v:&Value)->Result<String>{Ok(sha256_hex(&serde_json::to_vec(v).map_err(|_|"oracle_materialize_input")?))}

fn source_cell(value: &Value, column: &Value) -> Result<Value> {
    if value["type"] == "null" {
        if !value["value"].is_null() || column["nullable"] != "Y" {
            return Err("oracle_target_null");
        }
        return Ok(Value::Null);
    }
    let kind = text(column, "type")?;
    let raw = value["value"].as_str().ok_or("oracle_target_cell")?;
    if kind != "DATE" && !kind.starts_with("TIMESTAMP(") {
        return Ok(Value::String(raw.to_owned()));
    }
    if value["type"] != "text" || !raw.is_ascii() {
        return Err("oracle_target_temporal");
    }
    let (date, time) = raw.split_once(' ').ok_or("oracle_target_temporal")?;
    if date.len() != 10 || time.len() < 8 {
        return Err("oracle_target_temporal");
    }
    if kind.ends_with(" WITH TIME ZONE") {
        let offset = time.rfind(['+', '-']).filter(|n| *n >= 8).ok_or("oracle_target_temporal")?;
        let (civil, zone) = time.split_at(offset);
        let zone = match zone.len() {
            3 => format!("{zone}:00"),
            6 if zone.as_bytes()[3] == b':' => zone.to_owned(),
            _ => return Err("oracle_target_temporal"),
        };
        return Ok(json!(format!("{date}T{civil} {zone}")));
    }
    Ok(json!(format!("{date}T{time}")))
}

#[cfg(test)]
mod temporal_tests {
    use super::*;

    #[test]
    fn target_timestamp_text_returns_to_the_exact_source_codec_format() {
        for (kind, input, expected) in [
            ("DATE", "1969-12-31 23:59:59", "1969-12-31T23:59:59"),
            ("TIMESTAMP(9)", "2026-10-28 01:02:03.123456", "2026-10-28T01:02:03.123456"),
            ("TIMESTAMP(6) WITH TIME ZONE", "2026-10-28 02:30:00.123456+00",
             "2026-10-28T02:30:00.123456 +00:00"),
            ("TIMESTAMP(6) WITH TIME ZONE", "2026-10-28 02:00:00.123456-00:30",
             "2026-10-28T02:00:00.123456 -00:30"),
        ] {
            assert_eq!(source_cell(&json!({"type":"text","value":input}),
                                  &json!({"type":kind,"nullable":"Y"})).unwrap(), expected);
        }
        for kind in ["RAW", "VARCHAR2"] {
            assert_eq!(source_cell(&json!({"type":"text","value":""}),
                                  &json!({"type":kind,"nullable":"Y"})).unwrap(), "");
        }
        assert_eq!(source_cell(&json!({"type":"null","value":null}),
                              &json!({"type":"DATE","nullable":"Y"})).unwrap(), Value::Null);
        for value in [json!({"type":"int","value":"0"}), json!({"type":"text","value":"0"}),
                      json!({"type":"text","value":"2026-10-28 02:00:00"})] {
            assert!(source_cell(&value, &json!({"type":"TIMESTAMP(6) WITH TIME ZONE","nullable":"Y"})).is_err());
        }
        assert!(source_cell(&json!({"type":"null","value":null}),
                            &json!({"type":"DATE","nullable":"N"})).is_err());
    }

    #[test]
    fn utc_policy_requires_the_original_authenticated_codec_mapping() {
        let mut mapping = json!({"source_column":{"type":"TIMESTAMP(6) WITH TIME ZONE"},
            "arrow_type":"timestamp_micros_utc",
            "temporal_semantics":"explicit utc_instant policy; exact instant; original region/offset representation not retained"});
        assert_eq!(temporal_policy(&[mapping.clone()]).unwrap(), json!({"timezone":"utc_instant"}));
        mapping["temporal_semantics"] = json!("guessed UTC");
        assert!(temporal_policy(&[mapping]).is_err());
        assert_eq!(temporal_policy(&[]).unwrap(), json!({}));
    }
}

fn temporal_policy(mappings: &[Value]) -> Result<Value> {
    let mut zoned = false;
    for mapping in mappings {
        if mapping["source_column"]["type"].as_str().is_some_and(|s| s.ends_with(" WITH TIME ZONE")) {
            if mapping["arrow_type"] != "timestamp_micros_utc"
                || mapping["temporal_semantics"] != "explicit utc_instant policy; exact instant; original region/offset representation not retained" {
                return Err("oracle_temporal_policy");
            }
            zoned = true;
        }
    }
    Ok(if zoned { json!({"timezone":"utc_instant"}) } else { json!({}) })
}
fn codec(driver:&Value,request:&Value,out:&Path)->Result<Value>{
    pinned(driver,64*1024*1024)?;
    let mut child=Command::new(text(driver,"path")?).env_clear().env("PATH","/usr/bin:/bin")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|_|"oracle_codec")?;
    child.stdin.take().ok_or("oracle_codec")?.write_all(&serde_json::to_vec(request).map_err(|_|"oracle_codec")?).map_err(|_|"oracle_codec")?;
    let output=child.wait_with_output().map_err(|_|"oracle_codec")?;
    fs::write(out.join("codec.stdout"),&output.stdout).map_err(|_|"oracle_codec")?;
    fs::write(out.join("codec.stderr"),&output.stderr).map_err(|_|"oracle_codec")?;
    if !output.status.success(){return Err("oracle_codec");}
    let result:Value=serde_json::from_slice(&output.stdout).map_err(|_|"oracle_codec")?;
    if result["source_engine"]!="oracle"||result["typed_roundtrip"]!=true||result["target_apply"]!=false {return Err("oracle_codec");}
    Ok(result)
}

pub async fn stdio()->std::result::Result<(),u8>{
    let mut bytes=Vec::new();std::io::stdin().take(1024*1024+1).read_to_end(&mut bytes).map_err(|_|2u8)?;
    if bytes.len()>1024*1024 {return Err(2);}
    let value:Value=serde_json::from_slice(&bytes).map_err(|_|2u8)?;
    let evidence_root=value["transport_root"].as_str().map(PathBuf::from).filter(|p|
        fs::canonicalize(p).ok().zip(value["context"]["transport_root"].as_str().and_then(|r|fs::canonicalize(r).ok()))
            .is_some_and(|(a,b)|a==b));
    if let Some(root)=&evidence_root {let _=write(&root.join("materializer-input.json"),&value);}
    let (value,ok)=match materialize(&value).await {
        Ok(v)=>(v,true),Err(reason)=>(json!({"protocol":"oo.oracle.materialize-result/v1","engine":"oracle","error":reason}),false),
    };
    if let Some(root)=&evidence_root {let _=write(&root.join("materializer-output.json"),&value);}
    std::io::stdout().write_all(format!("{value}\n").as_bytes()).map_err(|_|1u8)?;
    if ok {Ok(())}else{Err(1)}
}

pub async fn materialize(v:&Value)->Result<Value>{
    if v["protocol"]!="oo.oracle.materialize-input/v1"||v["engine"]!="oracle" {return Err("oracle_materialize_input");}
    let ctx=&v["context"];let source=text(ctx,"source_id")?;let task=text(ctx,"lease_id")?;
    let generation=ctx["lease_generation"].as_i64().filter(|n|*n>0).ok_or("lease_required")?;
    let epoch=v["source_epoch"].as_i64().and_then(|n|i32::try_from(n).ok()).filter(|n|*n>0).ok_or("source_epoch")?;
    let source_generation=ctx["source_generation"].as_i64().filter(|n|*n>0).ok_or("oracle_generation")?;
    if ctx["engine"]!="oracle" {return Err("oracle_materialize_input");}
    let config=doc(&ctx["source_config"],1024*1024)?;
    if source_generation>1 {
        let rollover=serde_json::from_value(v["rollover"].clone()).map_err(|_|"oracle_rollover_pin")?;
        if config["rollover"]!=v["rollover"] {return Err("oracle_rollover_pin");}
        let descriptor=c1_buffer_position::delta(&rollover,source,&ctx["previous"])?;
        if descriptor["source_generation"]!=source_generation {return Err("oracle_generation");}
    } else if config.get("rollover").is_some() || v.get("rollover").is_some() {
        return Err("oracle_generation");
    }
    let bundle=doc(&v["bundle"],1024*1024)?;
    if config["bundle"]!=v["bundle"]||bundle["engine"]!="oracle"||bundle["source_id"]!=source
        || bundle["reader_sha256"]!=ctx["source_driver"]["sha256"] {return Err("oracle_bundle");}
    let admission=&config["admission_config"];
    let base_report=config.get("admission_report").filter(|r|r.is_object()).ok_or("oracle_permission_report_required")?;
    if admission["compat_mode"]!="oracle"||admission["source_id"]!=source||base_report["source_id"]!=source
        || base_report["compat_mode"]!="oracle"||base_report["permissions"]!=admission["permissions"]
        || base_report["driver"]["digest"]!=ctx["source_driver"]["sha256"] {return Err("oracle_permission_report");}
    let seal=&v["sealed_target"]["sealed"];
    let export=doc(&v["sealed_target"]["target_export"],32*1024*1024)?;
    if seal["outcome"]!="sealed"||seal["source_engine"]!="oracle"||export["seal"]!=*seal
        || export["protocol"]!="oo.pipeline.typed-target-export/v1"||export["source_engine"]!="oracle"
        || export["source_id"]!=source||export["task_id"]!=task||export["lease_generation"]!=generation
        || export["source_epoch"]!=epoch||seal["l_end"]!=v["l_end"] {return Err("oracle_target_export");}
    let mut conn=crate::native_update::connect_target(&ctx["target"]).await?;
    let stored:String=sqlx::query_scalar("SELECT _oo.c1_typed_export_binding($1,$2::uuid,$3,$4)::text")
        .bind(source).bind(task).bind(generation).bind(epoch).fetch_one(&mut conn).await.map_err(|_|"oracle_target_export")?;
    let stored:Value=serde_json::from_str(&stored).map_err(|_|"oracle_target_export")?;
    let raw:Vec<String>=sqlx::query_scalar("SELECT x::text FROM _oo.c1_typed_export($1,$2::uuid,$3,$4) AS x")
        .bind(source).bind(task).bind(generation).bind(epoch).fetch_all(&mut conn).await.map_err(|_|"oracle_target_export")?;
    let rows=raw.into_iter().map(|s|serde_json::from_str::<Value>(&s).map_err(|_|"oracle_target_export")).collect::<Result<Vec<_>>>()?;
    if export["rows"]!=json!(rows)||stored["engine"]!="oracle"||stored["l0"]!=v["l0"]
        || stored["config_digest"]!=hash(admission)? || stored["schema_digest"]!=seal["buffer"]["typed_schema_sha256"]
        || stored["l_end"]!=seal["applied_pos"] || stored["oracle_buffer"]!=seal["buffer"] {return Err("oracle_target_export");}
    let root=PathBuf::from(text(v,"transport_root")?);
    if !root.is_absolute()||root.components().any(|p|p==std::path::Component::ParentDir) {return Err("oracle_materialize_path");}
    let out=root.join("materialized");fs::create_dir(&out).map_err(|_|"oracle_materialize_path")?;
    let admission_bin=json!({"path":ctx["admission_binary"],"sha256":ctx["admission_sha256"]});
    pinned(&admission_bin,128*1024*1024)?;
    write(&out.join("admission-config.json"),admission)?;
    let mut entries=Vec::new();let mut requests=Vec::new();let mut tables=Vec::new();let mut scope_tables=Vec::new();
    let mut all_types=Vec::new();let mut table_columns=Vec::new();
    for (index,(qualified,spec)) in bundle["typed_tables"].as_object().ok_or("typed_tables")?.iter().enumerate(){
        let (schema,table)=qualified.split_once('.').ok_or("typed_table")?;
        let selected=rows.iter().filter(|r|r["schema"]==schema&&r["table"]==table&&r["deleted"]==false).collect::<Vec<_>>();
        let dir=out.join(format!("table-{index}"));fs::create_dir(&dir).map_err(|_|"oracle_materialize_path")?;
        let mappings=bundle["source_mappings"][qualified].as_array().ok_or("oracle_schema")?;
        let keys=spec["keys"].as_array().ok_or("typed_schema")?.iter().map(|n|spec["columns"][n.as_u64().unwrap_or(u64::MAX) as usize]["name"].clone()).collect::<Vec<_>>();
        let columns=mappings.iter().map(|m|m["source_column"].clone()).collect::<Vec<_>>();
        let temporal_policy = temporal_policy(mappings)?;
        let mut parts=Vec::new();
        let chunk_count=selected.len().max(1).div_ceil(4096);
        for number in 0..chunk_count {
            let cells=selected.iter().skip(number*4096).take(4096).map(|r|columns.iter().map(|c|
                source_cell(&r["value"][c["name"].as_str().unwrap_or("")], c))
                .collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?;
            let part=json!({"protocol":"oo.pipeline.typed-target-export/v1","source_engine":"oracle","source_id":source,
                "task_id":task,"source_checkpoint":seal["buffer"],"table":{"columns":columns,"keys":keys},
                "temporal_policy":temporal_policy,"rows":cells});
            let path=dir.join(format!("rows-{number}.json"));write(&path,&part)?;parts.push(reference(&path)?);
        }
        let arrow=dir.join("snapshot.arrow");
        let encoded=codec(&ctx["source_driver"],&json!({"action":"encode-series","inputs":parts,"output":arrow}),&dir)?;
        if encoded["typed_spec"]!=*spec||encoded["rows"]!=selected.len(){return Err("oracle_codec_schema");}
        let mut declarations=encoded["columns"].as_array().ok_or("oracle_codec_schema")?.clone();
        for (n,column) in declarations.iter_mut().enumerate(){
            column["key"]=json!(spec["keys"].as_array().unwrap().contains(&json!(n)));
            column.as_object_mut().ok_or("oracle_codec_schema")?.remove("logical_name");
            let mut typed=column.clone();let obj=typed.as_object_mut().ok_or("oracle_codec_schema")?;
            obj.remove("nullable");obj.remove("key");
            typed["table"]=json!(table);typed["source_type"]=mappings[n]["source_column"]["type"].clone();
            typed["target_oid"]=spec["columns"][n]["oid"].clone();typed["target_typmod"]=spec["columns"][n]["typmod"].clone();
            typed["caveat"]=Value::Null;all_types.push(typed);
        }
        let schema_doc=json!([{"table":table,"columns":declarations}]);write(&dir.join("schema-preimage.json"),&schema_doc)?;
        table_columns.push(declarations);
        let arrow_ref=reference(&arrow)?;let schema_sha=hash(&schema_doc)?;
        entries.push(json!({"schema":schema,"table":table,"source_generation":source_generation,"schema_sha256":schema_sha,
            "snapshot_arrow":{"path":arrow,"sha256":arrow_ref["sha256"]},"transactions":[]}));
        scope_tables.push(json!({"schema":schema,"table":table,"row_count":selected.len(),"arrow_sha256":arrow_ref["sha256"],"schema_sha256":schema_sha}));
        tables.push(json!({"schema":schema,"logical_schema":schema,"name":table,"arrow_sha256":arrow_ref["sha256"],"row_count":selected.len(),
            "route":"C1","consistency":"T","delete_mode":"native","quality_rejects":[]}));
    }
    write(&out.join("type-preimage.json"),&json!(all_types))?;
    let identity=json!({"engine":"oracle","measured_id":stored["measured_id"],"shard_id":ctx["target"]["shard"],"owner_token":ctx["target"]["owner"]});
    let mut scope=json!({"protocol":c1_buffer_position::SCOPE,"engine":"oracle","lineage":stored["measured_id"],
        "l0":v["l0"],"l_end":v["l_end"],"complete":true,"read_fence_digest":hash(&json!(rows))?,
        "config_digest":stored["config_digest"],"schema_digest":stored["schema_digest"],"consistency_grade":"T",
        "cross_table_consistent":true,"source_identity":identity,"tables":scope_tables,"buffer":seal["buffer"]});
    c1_buffer_position::validate_scope(&scope)?;
    let mut envelope=json!({"protocol":"oo.source-admission.c1-capture-envelope/v1","baseline_scope":scope,
        "entries":entries,"capture_digest":"0".repeat(64)});
    let digest=hash(&envelope)?;scope["capture_digest"]=json!(digest);envelope["capture_digest"]=json!(digest);
    let snapshot=json!({"kind":"oracle_buffer","value":v["l_end"]});
    write(&out.join("capturer.json"),&json!({"protocol":"oo.oracle.c1-buffer-capture/v1","source_engine":"oracle",
        "source_id":source,"source_generation":source_generation,"run_id":ctx["target"]["owner"],"tables":tables,"snapshot":snapshot,
        "product_reader_write_attempts":0,"source_server_contacted":false,"snapshot_token":false,
        "cross_table_consistency":"T","baseline_scope":scope,"capture_envelope":envelope}))?;
    for (index,entry) in entries.iter_mut().enumerate(){
        let dir=out.join(format!("table-{index}"));let schema=text(entry,"schema")?.to_string();let table=text(entry,"table")?.to_string();
        write(&dir.join("witness.json"),&json!({"protocol":"oo.source-admission.capture-witness/v1","source_engine":"oracle",
            "source_id":source,"source_generation":source_generation,"snapshot":snapshot,"run_id":ctx["target"]["owner"],
            "logical_schema":schema,"logical_table":table,"capturer_path":out.join("capturer.json"),
            "arrow_path":entry["snapshot_arrow"]["path"],"driver_path":ctx["source_driver"]["path"],
            "schema_preimage_path":dir.join("schema-preimage.json"),"type_preimage_path":out.join("type-preimage.json")}))?;
        let mut report=base_report.clone();report["capture_witness"]=json!(dir.join("witness.json"));
        write(&dir.join("report.json"),&report)?;
        let output=Command::new(text(&admission_bin,"path")?).args(["seal-native","--report"]).arg(dir.join("report.json"))
            .arg("--config").arg(out.join("admission-config.json")).arg("--out").arg(dir.join("sealed")).output().map_err(|_|"oracle_admission")?;
        fs::write(dir.join("seal.stdout"),&output.stdout).map_err(|_|"oracle_admission")?;fs::write(dir.join("seal.stderr"),&output.stderr).map_err(|_|"oracle_admission")?;
        fs::write(dir.join("seal.exit"),output.status.code().unwrap_or(-1).to_string()).map_err(|_|"oracle_admission")?;
        if !output.status.success(){return Err("oracle_admission");}
        let registry_path=dir.join("sealed/registry.json");let registry_ref=reference(&registry_path)?;let registry=doc(&registry_ref,1024*1024)?;
        let b=&registry["bindings"][0];
        let fields=b["fields"].as_array().ok_or("oracle_admission")?.iter().map(|f|{let mut f=f.clone();f["logical_name"]=f["name"].clone();f}).collect::<Vec<_>>();
        let mut request=json!({"protocol":"oo.pipeline.native-update/v1","source_engine":"oracle","source_id":source,
            "source_generation":source_generation,"parent_source_generation":if source_generation==1 {Value::Null}else{json!(source_generation-1)},"logical_schema":schema,"logical_table":table,
            "batch_id":format!("c1-{}-{}",text(ctx,"binding")?,ctx["pin"]),"run_id":b["run_id"],
            "capture_method":"c1_cdc","delete_detection":"native","consistency_grade":"T","cross_table_consistent":true,
            "position_codec":c1_buffer_position::CODEC,"position_lineage":stored["measured_id"],"applied_pos":seal["applied_pos"],
            "full_snapshot":true,"columns":fields,"key_columns":fields.iter().filter(|f|f["key"]==true).map(|f|f["name"].clone()).collect::<Vec<_>>(),
            "row_positions":rows.iter().filter(|r|r["schema"]==schema&&r["table"]==table&&r["deleted"]==false).map(|r|r["applied_pos"].clone()).collect::<Vec<_>>(),
            "tombstones":[],"arrow_path":entry["snapshot_arrow"]["path"],"max_arrow_bytes":33554432,
            "admission_path":b["admission_receipt_path"],"snapshot":b["snapshot"],"audit_digest":b["capture_receipt_sha256"],
            "target":ctx["target"],"mirror":false});
        for key in ["config_digest","schema_digest","type_digest","input_digest"] {request[key]=b[key].clone();}
        requests.push(request);entry["admission_registry_path"]=json!(registry_path);entry["admission_registry_sha256"]=registry_ref["sha256"].clone();
    }
    envelope["entries"]=json!(entries);write(&out.join("capture-envelope.json"),&envelope)?;
    let mut request=json!({"protocol":"oo.pipeline.native-c1-baseline/v1","c1_baseline_format":"typed-keys/v3","source_engine":"oracle","source_id":source,
        "binding":ctx["binding"],"pin":ctx["pin"],"source_generation":source_generation,"capture_method":"c1_cdc",
        "target_database":ctx["target"]["database"],"source_epoch":epoch,"source_identity":identity,"baseline_scope":scope,
        "entries":entries,"native_requests":requests,"capture_envelope_path":out.join("capture-envelope.json")});
    request["capture_envelope_sha256"]=reference(&out.join("capture-envelope.json"))?["sha256"].clone();
    write(&out.join("native-request.json"),&request)?;
    Ok(json!({"protocol":"oo.oracle.materialize-result/v1","engine":"oracle","source_id":source,"request":request}))
}
