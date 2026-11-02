//! Complete-source provenance. Database snapshots are authority; local files
//! must hash back to an actual successful load, never to a caller trust flag.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read};

const LIMIT: u64 = 16 * 1024 * 1024;
type Files = BTreeMap<String, String>;

fn err(kind:&'static str, detail:impl AsRef<str>)->Error {
    Error{kind,state:None,detail:detail.as_ref().chars().take(220).collect()}
}
fn text<'a>(v:&'a Value,k:&str)->Result<&'a str> {
    v[k].as_str().filter(|s|!s.is_empty()&&!s.contains('\0'))
        .ok_or_else(||err("handoff_provenance_field",k))
}
fn list(v:&Value)->Result<Vec<String>> {
    let a=v.as_array().filter(|a|!a.is_empty()).ok_or_else(||Error::new("handoff_projection"))?;
    let names=a.iter().map(|x|x.as_str().filter(|s|!s.is_empty()&&!s.contains('\0'))
        .map(str::to_owned).ok_or_else(||Error::new("handoff_projection"))).collect::<Result<Vec<_>>>()?;
    if names.iter().collect::<BTreeSet<_>>().len()!=names.len() {return Err(Error::new("handoff_duplicate_column"));}
    Ok(names)
}
fn set(names:&[String])->BTreeSet<&str> {names.iter().map(String::as_str).collect()}
fn hash(v:&Value)->Result<String> {
    Ok(sha256_hex(&serde_json::to_vec(v).map_err(|_|Error::new("handoff_json"))?))
}
fn open(path:&Path)->Result<std::fs::File> {
    if !path.is_absolute() || path.components().any(|c|c==std::path::Component::ParentDir)
        || !std::fs::symlink_metadata(path).is_ok_and(|m|m.is_file()&&!m.file_type().is_symlink()) {
        return Err(err("handoff_artifact_unavailable",path.to_string_lossy()));
    }
    std::fs::File::open(path).map_err(|_|err("handoff_artifact_unavailable",path.to_string_lossy()))
}
fn read(path:&Path,files:&mut Files)->Result<Value> {
    let mut bytes=Vec::new();
    open(path)?.take(LIMIT+1).read_to_end(&mut bytes).map_err(|_|Error::new("handoff_artifact_read"))?;
    if bytes.len() as u64>LIMIT {return Err(err("handoff_metadata_budget",path.to_string_lossy()));}
    let digest=sha256_hex(&bytes);
    let name=path.to_str().ok_or_else(||Error::new("handoff_artifact_path"))?.to_owned();
    if files.get(&name).is_some_and(|old|old!=&digest) {return Err(Error::new("handoff_artifact_changed"));}
    files.insert(name,digest);
    serde_json::from_slice(&bytes).map_err(|_|err("handoff_artifact_json",path.to_string_lossy()))
}
fn pinned(reference:&Value,files:&mut Files)->Result<Value> {
    let path=text(reference,"path")?;
    let value=read(Path::new(path),files)?;
    if files[path]!=text(reference,"sha256")?
        || reference.get("bytes").is_some_and(|n|n.as_u64()!=std::fs::metadata(path).ok().map(|m|m.len())) {
        return Err(err("handoff_artifact_digest",path));
    }
    Ok(value)
}

// Reverse the target's already authenticated pgident/v1 mapping. Do not
// reimplement keyword/NFC normalization or silently rename a source column.
fn original_ident(value:&str)->Result<String> {
    let Some(hex)=value.strip_prefix("q_") else {return Ok(value.to_owned())};
    if hex.is_empty() || hex.len()%2!=0 || !hex.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)) {
        return Err(Error::new("handoff_target_identifier"));
    }
    let bytes=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16)
        .map_err(|_|Error::new("handoff_target_identifier"))).collect::<Result<Vec<_>>>()?;
    String::from_utf8(bytes).map_err(|_|Error::new("handoff_target_identifier"))
}
fn table_key(schema:&str,table:&str)->Result<String> {
    if schema.is_empty() || table.is_empty() || schema.contains('.') || table.contains('.') {
        return Err(Error::new("handoff_qualified_name"));
    }
    Ok(format!("{schema}.{table}"))
}
fn native_columns(v:&Value)->Result<Vec<String>> {
    list(&json!(v["columns"].as_array().ok_or_else(||Error::new("handoff_capture_columns"))?
        .iter().map(|c|c.get("logical_name").unwrap_or(&c["name"]).clone()).collect::<Vec<_>>()))
}

#[derive(Clone)]
struct Template {
    layered:bool,
    config:Value,
    connection:Value,
    key:String,
    columns:Vec<String>,
}
fn connection(v:&Value,layered:bool)->Result<Value> {
    let c=&v["connection"];
    let database=if layered {text(v,"schema")?} else {text(c,"database")?};
    if c["tls"]!="required" && c["tls"]!="disabled" {return Err(Error::new("handoff_connection_tls"));}
    Ok(json!({"host":text(c,"host")?,"port":c["port"],"database":database,
        "user":text(c,"user")?,"secret_ref":text(v,"secret_ref")?,
        "password_file":text(c,"secret_file")?,"ca_file":c["ca_file"],
        "tls_server_name":c["tls_server_name"]}))
}
fn template(m:&Value,files:&mut Files)->Result<Template> {
    let args=m["producer"]["args"].as_array().ok_or_else(||Error::new("handoff_old_producer"))?;
    if args.len()!=2 || args[0]!="--config" || m["method"]!="C5" {
        return Err(Error::new("handoff_old_adapter"));
    }
    let adapter=read(Path::new(args[1].as_str().ok_or_else(||Error::new("handoff_old_producer"))?),files)?;
    let layered=m["c5_mode"]=="layered";
    if !layered && m.get("c5_mode").is_some_and(|v|v!="full") {
        return Err(Error::new("handoff_old_adapter"));
    }
    let config=read(Path::new(text(&adapter,if layered {"layered_config"}else{"capture_config"})?),files)?;
    let (schema,table,columns)=if layered {
        if m["c5_state"]["schema"]!=config["schema"] || m["c5_state"]["table"]!=config["table"]
            || m["c5_state"]["authority_file"]!=config["authority_file"]
            || m["c5_state"]["target"]!=config["target"] || adapter["target"]!=config["target"] {
            return Err(Error::new("handoff_layered_template_scope"));
        }
        (text(&config,"schema")?,text(&config,"table")?,list(&config["columns"])?)
    }else{
        if config["protocol"]!="oo.mysql-full/v1" {return Err(Error::new("handoff_old_adapter"));}
        (text(&config["table"],"schema")?,text(&config["table"],"table")?,list(&config["table"]["columns"])?)
    };
    if config["source_id"]!=m["source_id"] || adapter["secret_ref"]!=config["secret_ref"] {
        return Err(Error::new("handoff_old_template_source"));
    }
    Ok(Template{key:table_key(schema,table)?,connection:connection(&config,layered)?,columns,config,layered})
}
fn candidate_connection(v:&Value)->Result<Value> {
    Ok(json!({"host":text(v,"host")?,"port":v["port"],"database":text(v,"database")?,
        "user":text(v,"user")?,"secret_ref":text(v,"secret_ref")?,
        "password_file":text(v,"password_file")?,"ca_file":v["ca_file"],"tls_server_name":v["tls_server_name"]}))
}
fn compatible_template(current:&Template,captured:&Template)->Result<()> {
    if current.key!=captured.key || current.connection!=captured.connection
        || set(&current.columns)!=set(&captured.columns) {
        return Err(err("handoff_old_template_drift",&current.key));
    }
    if !current.layered && !captured.layered {
        for k in ["protocol","source_id","connection","secret_ref","table","approved_objects"] {
            if current.config[k]!=captured.config[k] {return Err(err("handoff_old_template_drift",k));}
        }
    }
    Ok(())
}

struct Capture {
    binding:String,
    task:String,
    config:Template,
    load:Value,
    native:Value,
    kind:&'static str,
}
fn capture_facts(native:&Value,expected_audit:&str,files:&mut Files)->Result<Value> {
    let arrow=Path::new(text(native,"arrow_path")?);
    let facts_path=arrow.parent().ok_or_else(||Error::new("handoff_capture_path"))?.join("capturer.json");
    let facts=read(&facts_path,files)?;
    if files[facts_path.to_str().ok_or_else(||Error::new("handoff_capture_path"))?]!=expected_audit
        || native["audit_digest"]!=expected_audit || facts["protocol"]!="oo.mysql.full-capture/v1"
        || facts["source_engine"]!="mysql" || facts["source_id"]!=native["source_id"]
        || facts["source_generation"]!=native["source_generation"] || facts["run_id"]!=native["run_id"]
        || facts["tables"].as_array().is_none_or(|t|t.len()!=1)
        || facts["tables"][0]["route"]!="C5"
        || facts["tables"][0]["schema"]!=native["logical_schema"]
        || facts["tables"][0]["name"]!=native["logical_table"]
        || facts["product_reader_write_attempts"]!=0 {
        return Err(Error::new("handoff_old_capture_provenance"));
    }
    Ok(facts)
}
fn full_capture(native:&Value,load:&Value,declared:&Template,files:&mut Files)->Result<Template> {
    let facts=capture_facts(native,text(&load["producer_trust"],"audit_sha256")?,files)?;
    let path=Path::new(text(native,"arrow_path")?).parent().and_then(Path::parent)
        .ok_or_else(||Error::new("handoff_capture_path"))?.join("capture-config.json");
    let config=read(&path,files)?;
    if config["protocol"]!="oo.mysql-full/v1" || config["source_id"]!=native["source_id"]
        || config["generation"]!=native["source_generation"]
        || config["table"]["schema"]!=native["logical_schema"] || config["table"]["table"]!=native["logical_table"]
        || facts["config_sha256"]!=files[path.to_str().ok_or_else(||Error::new("handoff_capture_path"))?] {
        return Err(Error::new("handoff_old_capture_config_seal"));
    }
    let actual=Template{layered:false,key:declared.key.clone(),columns:list(&config["table"]["columns"])?,
        connection:connection(&config,false)?,config};
    compatible_template(declared,&actual)?;
    if set(&native_columns(native)?)!=set(&actual.columns) {return Err(Error::new("handoff_capture_projection"));}
    Ok(actual)
}
fn layered_facts(facts:&Value,declared:&Template,native:&Value)->Result<()> {
    if facts["config_sha256"]!=hash(&declared.config)?
        || facts["source_id"]!=declared.config["source_id"]
        || set(&native_columns(native)?)!=set(&declared.columns)
        || facts["tables"][0]["schema"]!=declared.config["schema"]
        || facts["tables"][0]["name"]!=declared.config["table"] {
        return Err(err("handoff_layered_capture_config_seal",&declared.key));
    }
    Ok(())
}

fn parts_capture(request:&Value,load:&Value,declared:&Template,files:&mut Files)->Result<Value> {
    let index=&request["admitted_index"];
    if load["producer_trust"]["scope"]!="complete_c5_range"
        || load["producer_trust"]["admitted_index"]!=*index
        || load["producer_trust"]["parts_index"]!=request["parts_index"] {
        return Err(Error::new("handoff_parts_load_binding"));
    }
    let path=text(index,"path")?;
    let mut reader=BufReader::new(open(Path::new(path))?);
    let mut index_hash=Sha256::new();let mut audit=Sha256::new();
    let mut total=0u64;let mut ordinal=0u64;let mut first:Option<Value>=None;
    loop {
        let mut bytes=Vec::new();
        let count=reader.by_ref().take(LIMIT+1).read_until(b'\n',&mut bytes)
            .map_err(|_|Error::new("handoff_parts_read"))?;
        if count==0 {break;}
        if count as u64>LIMIT {return Err(Error::new("handoff_metadata_budget"));}
        index_hash.update(&bytes);total+=count as u64;
        let line:Value=serde_json::from_slice(&bytes).map_err(|_|Error::new("handoff_parts_json"))?;
        let registry=pinned(&line["registry"],files)?;
        let records=registry["bindings"].as_array().filter(|a|a.len()==1)
            .ok_or_else(||Error::new("handoff_parts_registry"))?;
        let record=&records[0];let native=&line["native_request"];let scope=&line["scope"];
        if registry["protocol"]!="oo.source-admission.binding-registry/v1"
            || record["range_scope"]!=*scope || scope["part"]["ordinal"]!=ordinal
            || scope["part_count"]!=request["part_count"] || scope["parts_index"]!=request["parts_index"]
            || scope["part"]["arrow"]["path"]!=native["arrow_path"]
            || scope["part"]["arrow"]["sha256"]!=native["input_digest"]
            || native["source_id"]!=request["source_id"] || native["source_generation"]!=request["source_generation"]
            || native["logical_schema"]!=request["schema"] || native["logical_table"]!=request["table"]
            || record["source_id"]!=native["source_id"] || record["source_generation"]!=native["source_generation"]
            || record["run_id"]!=native["run_id"] || record["capture_receipt_sha256"]!=native["audit_digest"] {
            return Err(Error::new("handoff_parts_identity"));
        }
        let fact_path=text(record,"capture_receipt_path")?;
        let facts=pinned(&json!({"path":fact_path,"sha256":record["capture_receipt_sha256"]}),files)?;
        if facts["range_scope"]!=*scope || facts["source_id"]!=request["source_id"]
            || facts["source_generation"]!=request["source_generation"] || facts["source_engine"]!="mysql"
            || facts["snapshot_complete"]!=true || facts["tables"].as_array().is_none_or(|t|t.len()!=1)
            || facts["tables"][0]["route"]!="C5" || facts["product_reader_write_attempts"]!=0 {
            return Err(Error::new("handoff_parts_capture"));
        }
        layered_facts(&facts,declared,native)?;
        if let Some(prior)=&first {
            for k in ["columns","key_columns","schema_digest","type_digest","config_digest"] {
                if prior[k]!=native[k] {return Err(Error::new("handoff_parts_projection"));}
            }
        } else {first=Some(native.clone());}
        audit.update(text(&line["registry"],"sha256")?.as_bytes());
        audit.update(text(native,"audit_digest")?.as_bytes());
        ordinal+=1;
    }
    let digest=index_hash.finalize().iter().map(|b|format!("{b:02x}")).collect::<String>();
    if index["sha256"]!=digest || index["bytes"]!=total || request["part_count"]!=ordinal
        || ordinal==0 || load["producer_trust"]["admissions"]!=ordinal
        || load["producer_trust"]["audit_sha256"]!=audit.finalize().iter().map(|b|format!("{b:02x}")).collect::<String>() {
        return Err(Error::new("handoff_parts_audit"));
    }
    files.insert(path.into(),digest);
    first.ok_or_else(||Error::new("handoff_parts_incomplete"))
}
fn actual_capture(row:&Value,files:&mut Files)->Result<Option<Capture>> {
    if row["previous_task"].is_null() {
        if !row["request"].is_null() || !row["load"].is_null() || !row["source_generation"].is_null() {
            return Err(Error::new("handoff_nochange_not_a_load"));
        }
        return Ok(None);
    }
    let task=text(row,"previous_task")?;
    let request=&row["request"];let load=&row["load"];let m=&row["capture_manifest"];
    if m["binding"]!=row["binding"] || m["source_id"]!=row["source_id"] || m["project_id"]!=row["project_id"]
        || load["protocol"]!="oo.pipeline.native-update-result/v1" || load["source_engine"]!="mysql"
        || load["source_id"]!=row["source_id"] || load["source_generation"]!=row["source_generation"]
        || load["jobs_lease_receipt"]["id"]!=task || load["jobs_lease_receipt"]["project_id"]!=row["project_id"]
        || !matches!(load["load_state"].as_str(),Some("awaiting_publish"|"replay"))
        || load["source_bound"]!=true || load["producer_trust"]["entry"]!="verify_inputs"
        || load["producer_trust"]["trusted"]!=true || load["producer_trust"]["document_bound"]!=true {
        return Err(Error::new("handoff_successful_capture_required"));
    }
    let declared=template(m,files)?;
    let (native,config,kind)=match request["protocol"].as_str() {
        Some("oo.pipeline.native-update/v1") if request["full_snapshot"]==true && !declared.layered=>{
            let config=full_capture(request,load,&declared,files)?;
            (request.clone(),config,"full")
        }
        Some("oo.pipeline.native-c5-range/v1") if declared.layered=>{
            let native=&request["native_request"];
            let facts=capture_facts(native,text(&load["producer_trust"],"audit_sha256")?,files)?;
            if facts["range_scope"].as_object().is_none_or(|scope|scope.iter().any(|(k,v)|request[k]!=*v)) {
                return Err(Error::new("handoff_range_scope"));
            }
            layered_facts(&facts,&declared,native)?;
            (native.clone(),declared,"layered_range")
        }
        Some("oo.pipeline.native-c5-range-parts/v1") if declared.layered=>{
            let native=parts_capture(request,load,&declared,files)?;
            (native,declared,"layered_parts")
        }
        _=>return Err(Error::new("handoff_capture_protocol_gap")),
    };
    if native["source_id"]!=row["source_id"] || native["source_generation"]!=row["source_generation"]
        || original_ident(text(load,"logical_schema")?)?!=text(&native,"logical_schema")?
        || original_ident(text(load,"logical_table")?)?!=text(&native,"logical_table")? {
        return Err(Error::new("handoff_capture_load_scope"));
    }
    Ok(Some(Capture{binding:text(row,"binding")?.into(),task:task.into(),config,load:load.clone(),native,kind}))
}

pub(super) async fn collect(c:&tokio_postgres::Client,old:&Value,new:&Value)->Result<Value> {
    let rows=old["bindings"].as_array().ok_or_else(||Error::new("handoff_complete_source_install_required"))?;
    let mut authorities=BTreeMap::new();
    for row in rows.iter().filter(|r|r["manifest"]["c5_mode"]=="layered") {
        let state=&row["manifest"]["c5_state"];
        let schema=text(state,"schema")?;let table=text(state,"table")?;
        let key=table_key(&original_ident(schema)?,&original_ident(table)?)?;
        if authorities.contains_key(&key) {continue;}
        let raw:Option<String>=c.query_one("SELECT _oo.c5_authority_export($1,$2,$3)::text",
            &[&text(old,"source_id")?,&schema,&table]).await.map_err(Error::sql)?.get(0);
        let authority=raw.map(|s|serde_json::from_str(&s).map_err(|_|Error::new("handoff_authority_json")))
            .transpose()?.unwrap_or(Value::Null);
        authorities.insert(key,authority);
    }
    verify(old,new,&authorities)
}

pub(super) fn verify(old:&Value,new:&Value,authorities:&BTreeMap<String,Value>)->Result<Value> {
    if old["scope_protocol"]!="oo.mysql.complete-source-handoff/v1" {
        return Err(Error::new("handoff_complete_source_install_required"));
    }
    let source=text(old,"source_id")?;
    let rows=old["bindings"].as_array().filter(|a|!a.is_empty())
        .ok_or_else(||Error::new("handoff_complete_source_install_required"))?;
    let tables=old["tables"].as_array().filter(|a|!a.is_empty()).ok_or_else(||Error::new("handoff_complete_source_tables"))?;
    let mut current=BTreeMap::new();
    for table in tables {
        let key=table_key(&original_ident(text(table,"schema_name")?)?,&original_ident(text(table,"table_name")?)?)?;
        if table["source"]!=source || table["state"]!="live"
            || current.insert(key,table).is_some() {return Err(Error::new("handoff_current_table_scope"));}
    }
    let configured=list(&new["tables"])?;
    if configured.iter().map(String::as_str).collect::<BTreeSet<_>>() != current.keys().map(String::as_str).collect()
        || new["columns"].as_object().is_none_or(|o|o.keys().collect::<BTreeSet<_>>()!=current.keys().collect()) {
        return Err(Error::new("handoff_complete_source_membership"));
    }
    for (key,table) in &current {
        let actual=list(&table["columns"])?.iter().map(|s|original_ident(s)).collect::<Result<Vec<_>>>()?;
        if set(&actual)!=set(&list(&new["columns"][key])?) {return Err(err("handoff_complete_source_projection",key));}
    }
    let mut files=Files::new();let mut captured=Vec::new();let mut templates=BTreeMap::new();
    let mut ids=BTreeSet::new();let mut anchor=false;
    let connection=candidate_connection(new)?;
    for row in rows {
        let id=text(row,"binding")?;let m=&row["manifest"];
        if !ids.insert(id) || row["source_id"]!=source || row["enabled"]!=false || row["method"]!="C5"
            || m["source_id"]!=source || m["binding"]!=id || m["project_id"]!=row["project_id"] {
            return Err(Error::new("handoff_binding_vector"));
        }
        if row["binding"]==old["binding"] {
            if row["project_id"]!=old["project_id"] || m!=&old["manifest"]
                || row["manifest_sha256"]!=old["manifest_sha256"]
                || row["previous_task"]!=old["previous_task"]
                || row["source_generation"]!=old["source_generation"] {return Err(Error::new("handoff_anchor"));}
            anchor=true;
        }
        let t=template(m,&mut files)?;
        if !current.contains_key(&t.key) || t.connection!=connection {
            return Err(err("handoff_source_or_connection_changed",&t.key));
        }
        if let Some(proof)=actual_capture(row,&mut files)? {captured.push(proof);}
        templates.insert(id.to_owned(),t);
    }
    if !anchor || new["c5_to_c1"]["binding"]!=old["binding"]
        || new["c5_to_c1"]["project_id"]!=old["project_id"] || new["c5_to_c1"]["source_id"]!=source {
        return Err(Error::new("handoff_progress_anchor_required"));
    }
    let mut coverage=Vec::new();
    for (key,table) in &current {
        let matching=captured.iter().filter(|p|p.config.key==*key && p.load["physical_name"]==table["physical_name"]
            && p.load["source_generation"]==table["source_generation"]
            && p.load["target_generation"]==table["target_generation"]).collect::<Vec<_>>();
        if matching.is_empty() {return Err(err("handoff_current_published_capture_missing",key));}
        for proof in &matching {
            if proof.config.connection!=connection || set(&proof.config.columns)!=set(&list(&new["columns"][key])?) {
                return Err(err("handoff_current_capture_projection",key));
            }
        }
        coverage.push(json!({"table":key,"physical_name":table["physical_name"],
            "source_generation":table["source_generation"],"target_generation":table["target_generation"],
            "columns":new["columns"][key],"capture_bindings":matching.iter().map(|p|&p.binding).collect::<Vec<_>>()}));
    }
    let mut binding_proofs=Vec::new();
    for row in rows {
        let id=text(row,"binding")?;let t=&templates[id];
        let direct=captured.iter().find(|p|p.binding==id);
        if let Some(direct)=direct {compatible_template(t,&direct.config)?;}
        let mut authority=Value::Null;
        let mut baseline_binding=Value::Null;
        if t.layered {
            authority=authorities.get(&t.key).cloned().ok_or_else(||err("handoff_layered_authority_missing",&t.key))?;
            let table=current[&t.key];
            if authority["source_id"]!=source || authority["schema"]!=row["manifest"]["c5_state"]["schema"]
                || authority["table"]!=row["manifest"]["c5_state"]["table"]
                || authority["schema_fingerprint"]!=row["manifest"]["c5_state"]["schema_fingerprint"]
                || authority["table_source_generation"]!=table["source_generation"]
                || authority["table_physical_name"]!=table["physical_name"]
                || authority["source_generation"]!=old["source_generation"] {
                return Err(err("handoff_layered_authority_scope",&t.key));
            }
            let baseline=if authority["empty_only"]==true {&authority["baseline_receipt"]} else {&authority["baseline"]};
            let proof=captured.iter().find(|p|p.kind=="full" && p.config.key==t.key
                && p.load["run_id"]==baseline["run_id"]
                && p.load["source_generation"]==baseline["source_generation"]
                && p.load["physical_name"]==baseline["physical_name"])
                .ok_or_else(||err("handoff_layered_baseline_capture_missing",&t.key))?;
            for key in ["config_digest","schema_digest","type_digest"] {
                if proof.native[key]!=baseline[key] {return Err(err("handoff_layered_baseline_digest",key));}
            }
            compatible_template(t,&proof.config)?;
            if !authority["selected_columns"].is_null() {
                let names=list(&authority["selected_columns"])?.iter().map(|s|original_ident(s)).collect::<Result<Vec<_>>>()?;
                if set(&names)!=set(&t.columns) {return Err(err("handoff_layered_authority_projection",&t.key));}
            }
            baseline_binding=json!(proof.binding);
        } else if direct.is_none() {
            let proof=captured.iter().find(|p|p.config.key==t.key && p.config.connection==t.connection
                && set(&p.config.columns)==set(&t.columns))
                .ok_or_else(||err("handoff_unloaded_binding_capture_missing",&t.key))?;
            baseline_binding=json!(proof.binding);
        }
        binding_proofs.push(json!({"binding":id,"project_id":row["project_id"],"table":t.key,
            "capture_task":direct.map(|p|&p.task),"capture_kind":direct.map(|p|p.kind),
            "baseline_capture_binding":baseline_binding,"authority":authority,
            "latest_success_is_not_load_authority":true}));
    }
    Ok(json!({"protocol":"oo.mysql.complete-source-handoff-provenance/v1","source_id":source,
        "anchor_binding":old["binding"],"bindings":binding_proofs,"tables":coverage,"files":files,
        "source_rows_read":0,"source_connected":false,"historical_bindings_preserved":true}))
}

#[cfg(test)]
#[path = "native_mysql_handoff_provenance_tests.rs"]
mod tests;
