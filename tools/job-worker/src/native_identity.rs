//! Opt-in logical source identity. Rebind uses the existing stopped binding,
//! never a second scheduler and never a replacement source snapshot.
use super::*;
#[path = "../../source-onboarding/src/identity_contract.rs"]
mod contract;
use std::os::unix::fs::PermissionsExt;

fn config_path(m:&Manifest)->Result<&str> {
    if m.producer.args.len()!=5 || m.producer.args[1]!="--config" || m.producer.args[3]!="--out" {
        return Err(Error::new("identity_producer_argv"));
    }
    Ok(&m.producer.args[2])
}

pub fn validate(m:&Manifest)->Result<contract::Verified> {
    if m.method!="C1" || m.c1_baseline_format.as_deref()!=Some("typed-keys/v3") {
        return Err(Error::new("identity_typed_mysql_only"));
    }
    let config=contract::document(Path::new(config_path(m)?)).map_err(Error::new)?;
    let v=contract::verify(&config,true,sha256_hex).map_err(Error::new)?
        .ok_or_else(||Error::new("identity_marker_required"))?;
    let o=&v.operator["onboarding"];
    if m.source_registration.as_ref()!=Some(&v.registration) || v.record["source_id"]!=m.source_id
        || o["binding"]!=m.binding || o["project_id"]!=m.project_id
        || o["producer"]!=m.producer.path || o["loader"]!=m.loader.path
        || o["admission"]!=m.registration.as_ref().ok_or_else(||Error::new("registration"))?.admission_binary
        || m.connection_rebind.as_ref().is_some_and(|r|*r!=o["connection_rebind"]) {
        return Err(Error::new("identity_manifest_binding"));
    }
    Ok(v)
}

fn admission(config:&Value,source:&str)->Value {
    let mut tables=config["tables"].as_array().cloned().unwrap_or_default();
    tables.sort_by(|a,b|a.as_str().cmp(&b.as_str()));
    json!({"protocol":"oo.source-admission.config/v1","target_id":source,"source_id":source,
        "secret_ref":config["secret_ref"],"compat_mode":"mysql-native",
        "permissions":tables.iter().map(|t| {
            let (s,n)=t.as_str().unwrap_or("").split_once('.').unwrap_or(("",""));
            json!({"name":"SELECT","status":"known","granted":true,"scope":{"database":s,"table":n}})
        }).collect::<Vec<_>>(),"r1w":null,
        "budget":{"unit":"Rows","hard_limit":config["c1_export_parts"]["max_rows"].as_u64().unwrap_or(100000),"confidence":"unknown"},
        "write_options":{"signal_table":false,"heartbeat_write":false,"create_stream":false,"create_trigger":false},
        "debezium":{"signal_data_collection":null,"heartbeat_action_query":null,"snapshot_mode":"never","create_stream":false,"create_trigger":false},
        "requested_write_class":"R0"})
}

fn document(v:&contract::Verified,admission:&Value,schema:&Value)->Result<Value> {
    Ok(json!({"registration":v.registration,"record":v.record,
        "invariant_config":contract::invariant_config(&v.operator).map_err(Error::new)?,
        "admission":admission,"config_digest":contract::hash(admission,sha256_hex).map_err(Error::new)?,
        "schema_digest":schema}))
}

fn live_check(m:&Manifest,v:&contract::Verified,request:&Value)->Result<()> {
    let root=Path::new(contract::text(request,"verification_out").map_err(Error::new)?);
    if !root.is_absolute() || root.components().any(|x|x==std::path::Component::ParentDir) {
        return Err(Error::new("identity_verification_path"));
    }
    std::fs::create_dir_all(root).map_err(|_|Error::new("identity_verification_path"))?;
    let meta=std::fs::symlink_metadata(root).map_err(|_|Error::new("identity_verification_path"))?;
    if meta.file_type().is_symlink(){return Err(Error::new("identity_verification_path"));}
    std::fs::set_permissions(root,std::fs::Permissions::from_mode(0o700)).map_err(|_|Error::new("identity_verification_path"))?;
    let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|Error::new("clock"))?.as_nanos();
    let out=root.join(format!("probe-{nonce}"));
    let result=std::process::Command::new(&m.producer.path).env_clear().env("PATH","/usr/bin:/bin")
        .args(["c1-onboarding-prepare","--config"]).arg(&v.operator_path).arg("--out").arg(&out)
        .args(["--c1-baseline-format","typed-keys/v3"]).output().map_err(|_|Error::new("identity_live_probe"))?;
    let receipt=json!({"exit":result.status.code(),"producer_sha256":m.producer.sha256,
        "output":out,"registration":v.registration,"source_rows_read":0});
    std::fs::write(root.join(format!("probe-{nonce}.json")),receipt.to_string()).map_err(|_|Error::new("identity_live_probe"))?;
    if !result.status.success(){return Err(Error::new("identity_live_probe"));}
    let actual=contract::document(&out.join("identity.json")).map_err(Error::new)?;
    let native=contract::document(Path::new(config_path(m)?)).map_err(Error::new)?;
    if actual["source_id"]!=m.source_id || actual["source_registration"]!=v.registration
        || actual["source_config_sha256"]!=contract::hash(&admission(&native,&m.source_id),sha256_hex).map_err(Error::new)?
        || actual["measured_id"]!=format!("mysql.{}",v.record["physical_identity"]["server_uuid"].as_str().unwrap_or("")) {
        return Err(Error::new("identity_live_probe"));
    }
    validate(m)?;
    Ok(())
}

pub async fn put(config:&Config,mut m:Manifest)->Result<()> {
    let v=validate(&m)?;
    let database=target_database(config,&m.method)?;
    if database!=config.database {return Err(Error::new("identity_atomic_control_target_required"));}
    let mut c=session::runtime(config).await?;
    let old=c.query_opt("SELECT manifest::text,manifest_sha,enabled FROM jw_native.bindings WHERE id=$1::text::uuid",
        &[&m.binding]).await.map_err(Error::sql)?;
    if let Some(request)=m.connection_rebind.clone() {
        if request["protocol"]!="oo.jobs.mysql-connection-rebind/v1"
            || request.as_object().is_none_or(|o|o.len()!=4) {
            return Err(Error::new("identity_rebind_request"));
        }
        let row=old.ok_or_else(||Error::new("identity_rebind_missing"))?;
        let old_m:Manifest=serde_json::from_str(&row.get::<_,String>(0)).map_err(|_|Error::new("manifest"))?;
        let old_sha:String=row.get(1);
        if row.get::<_,bool>(2) || request["expected_manifest_sha256"]!=old_sha
            || old_m.source_registration.as_ref().map(|v|&v["record"]["sha256"])!=Some(&request["expected_registration_sha256"])
            || old_m.binding!=m.binding || old_m.project_id!=m.project_id || old_m.source_id!=m.source_id
            || old_m.method!=m.method || old_m.cycle!=m.cycle || old_m.period_ms!=m.period_ms
            || old_m.producer.path!=m.producer.path || old_m.producer.sha256!=m.producer.sha256
            || serde_json::to_value(&old_m.loader).ok()!=serde_json::to_value(&m.loader).ok()
            || serde_json::to_value(&old_m.registration).ok()!=serde_json::to_value(&m.registration).ok() {
            return Err(Error::new("identity_stopped_cas"));
        }
        let old_ref=old_m.source_registration.as_ref().ok_or_else(||Error::new("identity_adoption_required"))?;
        if old_ref["registry"]!=v.registration["registry"] {
            return Err(Error::new("identity_registry_authority_changed"));
        }
        let previous=contract::pinned(&old_ref["record"],sha256_hex).map_err(Error::new)?;
        contract::successor(&previous,&v.record,contract::text(&request,"expected_registration_sha256").map_err(Error::new)?)
            .map_err(Error::new)?;
        let busy:bool=c.query_one("SELECT EXISTS(SELECT 1 FROM jw_native.bindings WHERE source_id=$1 AND target_database=$2 AND enabled) OR EXISTS(SELECT 1 FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2 AND active_task_id IS NOT NULL) OR EXISTS(SELECT 1 FROM jw_native.pending_acks a JOIN jw_native.bindings b ON b.id=a.binding_id WHERE b.source_id=$1 AND b.target_database=$2 AND a.acknowledged_at IS NULL)",
            &[&m.source_id,&database]).await.map_err(Error::sql)?.get(0);
        if busy {return Err(Error::new("identity_stop_drain_required"));}
        live_check(&m,&v,&request)?;
        let config_value=contract::document(Path::new(config_path(&m)?)).map_err(Error::new)?;
        let tx=c.transaction().await.map_err(Error::sql)?;
        let old_schema:String=tx.query_one("SELECT _oo.mysql_connection_schema($1)",
            &[&m.source_id]).await.map_err(Error::sql)?.get(0);
        let d=document(&v,&admission(&config_value,&m.source_id),&json!(old_schema))?;
        m.connection_rebind=None;
        let (text,hash)=canonical(&m)?;
        let receipt:String=tx.query_one("SELECT _oo.mysql_connection_rebind($1,$2::text::uuid,$3::text::uuid,$4,$5,$6::text::jsonb,$7)::text",
            &[&m.source_id,&m.binding,&m.project_id,&old_sha,&request["expected_registration_sha256"].as_str(),&d.to_string(),&hash])
            .await.map_err(Error::sql)?.get(0);
        let n=tx.execute("UPDATE jw_native.bindings SET manifest=$3::text::jsonb,manifest_sha=$4 WHERE id=$1::text::uuid AND manifest_sha=$2 AND NOT enabled",
            &[&m.binding,&old_sha,&text,&hash]).await.map_err(Error::sql)?;
        if n!=1 {return Err(Error::new("identity_rebind_cas"));}
        tx.commit().await.map_err(Error::sql)?;
        println!("{receipt}");
        return Ok(());
    }
    let (text,hash)=canonical(&m)?;
    if let Some(old)=old {
        if old.get::<_,String>(1)!=hash {return Err(Error::new("identity_explicit_rebind_required"));}
        println!("identity=unchanged binding={}",m.binding);
        return Ok(());
    }
    let adopted:bool=c.query_one("SELECT EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1) OR EXISTS(SELECT 1 FROM jw_native.bindings WHERE source_id=$1 AND target_database=$2)",
        &[&m.source_id,&database]).await.map_err(Error::sql)?.get(0);
    if adopted {return Err(Error::new("identity_adoption_required"));}
    c.execute("INSERT INTO jw_native.bindings(id,project_id,source_id,method,cycle,period_ms,manifest_sha,manifest,target_database) VALUES($1::text::uuid,$2::text::uuid,$3,$4,$5,$6,$7,$8::text::jsonb,$9)",
        &[&m.binding,&m.project_id,&m.source_id,&m.method,&m.cycle,&m.period_ms,&hash,&text,&database]).await.map_err(Error::sql)?;
    println!("identity=registered source={} binding={} manifest_sha={hash}",m.source_id,m.binding);
    Ok(())
}

pub async fn project_parent(target:&tokio_postgres::Client,source:&str,p:&Value)->Result<Value> {
    let raw:String=target.query_one("SELECT _oo.mysql_connection_project_parent($1,$2::text::jsonb)::text",
        &[&source,&p.to_string()]).await.map_err(Error::sql)?.get(0);
    serde_json::from_str(&raw).map_err(|_|Error::new("identity_parent"))
}

pub fn bootstrap(config:&Value,source:&str)->Result<Option<Value>> {
    let value=contract::verify(config,true,sha256_hex).map_err(Error::new)?;
    if let Some(v)=value {
        if v.record["source_id"]!=source{return Err(Error::new("identity_source"));}
        Ok(Some(v.registration))
    }else{Ok(None)}
}

pub async fn opened(target:&tokio_postgres::Client,config:&Value,frame:&Value,
    source:&str,task:&str,generation:i64,epoch:i64,binding:&str,project:&str)->Result<()> {
    let Some(v)=contract::verify(config,true,sha256_hex).map_err(Error::new)? else {return Ok(())};
    if frame["source_registration"]!=v.registration || frame["source_uuid"]!=v.record["physical_identity"]["server_uuid"]
        || frame["admission_config"]!=admission(config,source) {
        return Err(Error::new("identity_open_binding"));
    }
    let d=document(&v,&frame["admission_config"],&frame["schema_digest"])?;
    if d["config_digest"]!=frame["config_digest"] {return Err(Error::new("identity_admission_digest"));}
    target.query_one("SELECT _oo.mysql_connection_bind($1,$2::text::uuid,$3,$4::bigint::integer,$5::text::uuid,$6::text::uuid,$7::text::jsonb)",
        &[&source,&task,&generation,&epoch,&binding,&project,&d.to_string()]).await.map_err(Error::sql)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_uses_actual_secret_reference_and_sorted_scope() {
        let config=json!({"secret_ref":"secret://new","tables":["db.z","db.a"],"c1_export_parts":{"max_rows":128}});
        let actual=admission(&config,"stable_source");
        assert_eq!(actual["secret_ref"],"secret://new");
        assert_eq!(actual["permissions"][0]["scope"]["table"],"a");
        assert_eq!(actual["budget"]["hard_limit"],128);
        let mut before=actual.clone();before["secret_ref"]=json!("secret://old");
        assert_ne!(contract::hash(&actual,sha256_hex).unwrap(),contract::hash(&before,sha256_hex).unwrap());
        before["secret_ref"]=actual["secret_ref"].clone();
        assert_eq!(before,actual);
    }

    #[test]
    fn transport_invariants_do_not_ignore_scope_or_selected_columns() {
        let before=json!({"host":"localhost","port":23327,"database":"owned","user":"reader",
            "secret_ref":"secret://old","tables":["owned.t"],"columns":{"owned.t":["id","value"]},"server_id":57});
        let mut after=before.clone();after["port"]=json!(23577);after["secret_ref"]=json!("secret://new");
        assert_eq!(contract::invariant_config(&before).unwrap(),contract::invariant_config(&after).unwrap());
        after["columns"]["owned.t"]=json!(["id"]);
        assert_ne!(contract::invariant_config(&before).unwrap(),contract::invariant_config(&after).unwrap());
    }
}
