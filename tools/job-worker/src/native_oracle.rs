//! Oracle input checks inside the existing native worker, not another scheduler.
use super::*;
use std::collections::BTreeMap;
#[path = "native_oracle_delta.rs"]
mod delta;

pub struct Buffer {
    pub context_ref:Value,
    pub context:Value,
    pub first_batch:Value,
    pub positions:BTreeMap<String,Value>,
    pub index:Value,
    pub index_ref:Value,
    pub bundle:Value,
    pub rollover:Option<Value>,
    pub rollover_ref:Value,
    pub batches:BTreeMap<i64,Value>,
    pub initial_fill:Option<Value>,
}
fn pinned(v:&Value)->Result<Vec<u8>> {
    let path=v["path"].as_str().ok_or_else(||Error::new("oracle_artifact"))?;
    let hash=v["sha256"].as_str().ok_or_else(||Error::new("oracle_artifact"))?;
    verify_artifact(path,hash)?;
    let bytes=std::fs::read(path).map_err(|_|Error::new("oracle_artifact"))?;
    if v.get("bytes").is_some_and(|n|n.as_u64()!=Some(bytes.len() as u64)) {return Err(Error::new("oracle_artifact"));}
    Ok(bytes)
}
fn document(v:&Value)->Result<Value>{
    serde_json::from_slice(&pinned(v)?).map_err(|_|Error::new("oracle_artifact"))
}
fn digest(batch:&Value)->Result<String>{
    let mut value=batch.clone();
    for part in value["parts"].as_array_mut().ok_or_else(||Error::new("typed_parts"))? {
        part.as_object_mut().ok_or_else(||Error::new("typed_parts"))?.remove("path");
    }
    Ok(sha256_hex(&serde_json::to_vec(&value).map_err(|_|Error::new("typed_batch"))?))
}
fn batch_protocol_supported(batch:&Value)->bool {
    batch["protocol"]=="oo.c1.typed-batch/v3"
        || batch["protocol"]=="oo.c1.typed-ordered-batch/v1" && batch["kind"]=="cdc"
}
impl Buffer {
    pub fn read(config:&Value,source:&str)->Result<Self>{
        if config["protocol"]!="oo.oracle.producer-argv/v1"||config["engine"]!="oracle" {
            return Err(Error::new("oracle_config"));
        }
        let index=document(&config["buffer_index"])?;
        let context_ref=index["context"].clone();let context=document(&context_ref)?;
        let bundle=document(&config["bundle"])?;
        if index["protocol"]!="oo.oracle.buffer-index/v1" || context["protocol"]!="oo.c1.oracle-buffer-context/v1"
            || context["source_id"]!=source || context["source_identity"]["engine"]!="oracle"
            || context["bundle"]!=config["bundle"] || bundle["source_id"]!=source || bundle["engine"]!="oracle"
            || context["source_resume"]["target_ack_updates_vendor_offset"]!=false {
            return Err(Error::new("oracle_context"));
        }
        let root=Path::new(config["bundle"]["path"].as_str().ok_or_else(||Error::new("oracle_bundle"))?).parent().ok_or_else(||Error::new("oracle_bundle"))?;
        let bytes=pinned(&json!({"path":root.join("batches.jsonl"),"sha256":bundle["artifacts"]["batches.jsonl"]}))?;
        let mut batches=BTreeMap::new();
        for line in bytes.split(|b|*b==b'\n').filter(|line|!line.is_empty()) {
            let frame:Value=serde_json::from_slice(line).map_err(|_|Error::new("oracle_bundle"))?;
            let n=frame["batch"]["sequence"].as_i64().ok_or_else(||Error::new("oracle_bundle"))?;
            if n!=(batches.len()+1) as i64 {return Err(Error::new("oracle_bundle_sequence"));}
            batches.insert(n,frame["batch"].clone());
        }
        let first_batch=batches.get(&1).ok_or_else(||Error::new("oracle_bundle"))?.clone();
        let mut positions=BTreeMap::new();
        for entry in index["transactions"].as_array().ok_or_else(||Error::new("oracle_index"))? {
            let payload=document(entry)?;
            let pos=payload["batch"]["position"].as_str().ok_or_else(||Error::new("oracle_index"))?.to_string();
            if payload["protocol"]!="oo.oracle.buffer-consumer-input/v1" || payload["source_id"]!=source
                || payload["buffer_context"]!=context_ref || positions.insert(pos,payload).is_some() {
                return Err(Error::new("oracle_index"));
            }
        }
        let rollover_ref=config["rollover"].clone();
        let rollover=if rollover_ref.is_null() {None}else{Some(document(&rollover_ref)?)};
        let mut buffer=Self{context_ref,context,first_batch,positions,index,
            index_ref:config["buffer_index"].clone(),bundle,rollover,rollover_ref,
            batches,initial_fill:None};
        buffer.check_lineage(config)?;
        buffer.initial_fill=buffer.fill_plan(config)?;
        Ok(buffer)
    }
    pub fn measured(&self)->Result<String>{
        let id=&self.context["source_identity"];
        let t=|key|id[key].as_str().ok_or_else(||Error::new("oracle_identity"));
        Ok(format!("oracle.{}.{}.{}",t("dbid")?,t("resetlogs_change")?,t("pdb_guid")?.to_ascii_lowercase()))
    }
    pub fn check_batch(&self,frame:&Value)->Result<()> {
        let batch=&frame["batch"];
        if !batch_protocol_supported(batch) {return Err(Error::new("oracle_batch_protocol"));}
        if self.rollover.is_some() && batch["kind"]!="cdc" {return Err(Error::new("oracle_delta_only"));}
        if batch["kind"]=="cdc" {
            let pos=batch["position"].as_str().ok_or_else(||Error::new("oracle_position"))?;
            let expected=self.positions.get(pos).ok_or_else(||Error::new("oracle_position"))?;
            if expected["source_position"]!=frame["source_position"]
                || frame["buffer_context"]!=self.context_ref || digest(&expected["batch"])?!=digest(batch)? {
                return Err(Error::new("oracle_position"));
            }
        }
        if self.is_initial_fill() {
            let sequence=batch["sequence"].as_i64().ok_or_else(||Error::new("oracle_fill_batch"))?;
            let expected=self.batches.get(&sequence).ok_or_else(||Error::new("oracle_fill_batch"))?;
            if digest(expected)?!=digest(batch)? {return Err(Error::new("oracle_fill_batch"));}
        }
        Ok(())
    }

    pub fn is_initial_fill(&self)->bool {self.initial_fill.is_some()}
    pub fn is_initial_mixed(&self)->bool {
        self.initial_fill.as_ref().is_some_and(|p|p["protocol"]=="oo.oracle.initial-mixed/v1")
    }

    fn fill_plan(&self,config:&Value)->Result<Option<Value>> {
        if config["initial_mixed_resume"]==true {
            if config["rollover_module"].is_null()||self.rollover.is_some()||self.positions.is_empty() {
                return Err(Error::new("oracle_mixed_opt_in"));
            }
            return self.mixed_plan(config).map(Some);
        }
        if config["rollover_module"].is_null()||self.rollover.is_some()||!self.positions.is_empty() {return Ok(None);}
        if self.context["initial_cursor"]!=json!({"transaction_ordinal":"0","transaction_id":null})
            || self.index["final_cursor"]!=self.context["initial_cursor"]
            || self.bundle["boundary"]["final_position"]!="0" {
            return Err(Error::new("oracle_fill_cursor"));
        }
        if self.batches.is_empty()||self.batches.len()>4096
            ||self.bundle["batch_count"].as_u64()!=Some(self.batches.len() as u64) {
            return Err(Error::new("oracle_fill_count"));
        }
        let mut tables:BTreeMap<String,Value>=BTreeMap::new();
        let mut entries=Vec::new();
        for (n,batch) in &self.batches {
            if batch["protocol"]!="oo.c1.typed-batch/v3"||batch["kind"]!="fill"
                ||batch["position"]!="0"||batch["previous_position"]!="0" {
                return Err(Error::new("oracle_fill_batch"));
            }
            let parts=batch["parts"].as_array().filter(|p|!p.is_empty()).ok_or_else(||Error::new("oracle_fill_parts"))?;
            for part in parts {
                let name=part["table"].as_str().ok_or_else(||Error::new("oracle_fill_table"))?;
                let rows=part["rows"].as_u64().ok_or_else(||Error::new("oracle_fill_rows"))?;
                if self.bundle["typed_tables"].get(name).is_none()
                    ||part["spec"]!=self.bundle["typed_tables"][name] {return Err(Error::new("oracle_fill_table"));}
                let entry=tables.entry(name.to_owned()).or_insert(json!({"last_sequence":n,"rows":0}));
                let total=entry["rows"].as_u64().unwrap_or(0).checked_add(rows).ok_or_else(||Error::new("oracle_fill_rows"))?;
                *entry=json!({"last_sequence":n,"rows":total});
            }
            entries.push(json!({"sequence":n,"digest":digest(batch)?}));
        }
        let expected=self.bundle["typed_tables"].as_object().ok_or_else(||Error::new("oracle_fill_tables"))?;
        let total=tables.values().try_fold(0u64,|a,v|a.checked_add(v["rows"].as_u64().unwrap_or(0))).ok_or_else(||Error::new("oracle_fill_rows"))?;
        if tables.len()!=expected.len()||tables.keys().any(|k|!expected.contains_key(k))
            ||self.bundle["final_rows"].as_u64()!=Some(total) {
            return Err(Error::new("oracle_fill_coverage"));
        }
        let config_digest=sha256_hex(&serde_json::to_vec(&config["admission_config"]).map_err(|_|Error::new("oracle_fill_config"))?);
        Ok(Some(json!({"protocol":"oo.oracle.initial-fill/v1","source_id":self.context["source_id"],
            "config_digest":config_digest,"typed_schema_sha256":self.context["typed_schema_sha256"],
            "buffer_index":self.index_ref,"buffer_context":self.context_ref,
            "bundle_sha256":self.context["bundle"]["sha256"],"batch_count":entries.len(),
            "batches":entries,"tables":tables})))
    }

    fn mixed_plan(&self,config:&Value)->Result<Value> {
        if self.context["initial_cursor"]!=json!({"transaction_ordinal":"0","transaction_id":null})
            ||self.bundle["boundary"]["mode"]!="oracle_scn_snapshot_and_log"
            ||self.batches.len()<2||self.batches.len()>4096
            ||self.bundle["batch_count"].as_u64()!=Some(self.batches.len() as u64) {
            return Err(Error::new("oracle_mixed_binding"));
        }
        let mut tables:BTreeMap<String,Value>=BTreeMap::new();
        let mut entries=Vec::new();let mut fills=0;let mut pos=0u64;
        for (n,batch) in &self.batches {
            if !batch_protocol_supported(batch) {return Err(Error::new("oracle_mixed_batch"));}
            let mut entry=json!({"sequence":n,"digest":digest(batch)?,"kind":batch["kind"],
                "previous_position":batch["previous_position"],"position":batch["position"]});
            if batch["kind"]=="fill" {
                if pos!=0||batch["position"]!="0"||batch["previous_position"]!="0" {
                    return Err(Error::new("oracle_mixed_fill_after_cdc"));
                }
                fills+=1;
                for part in batch["parts"].as_array().filter(|a|!a.is_empty()).ok_or_else(||Error::new("oracle_mixed_parts"))? {
                    let name=part["table"].as_str().ok_or_else(||Error::new("oracle_mixed_table"))?;
                    let rows=part["rows"].as_u64().ok_or_else(||Error::new("oracle_mixed_rows"))?;
                    if self.bundle["typed_tables"].get(name).is_none()||part["spec"]!=self.bundle["typed_tables"][name] {
                        return Err(Error::new("oracle_mixed_table"));
                    }
                    let table=tables.entry(name.into()).or_insert(json!({"rows":0}));
                    let total=table["rows"].as_u64().unwrap_or(0).checked_add(rows).ok_or_else(||Error::new("oracle_mixed_rows"))?;
                    *table=json!({"rows":total,"last_sequence":n});
                }
            }else if batch["kind"]=="cdc"&&fills>0 {
                if batch["previous_position"]!=pos.to_string()||batch["position"]!=(pos+1).to_string() {
                    return Err(Error::new("oracle_mixed_cursor"));
                }
                let payload=self.positions.get(&(pos+1).to_string()).ok_or_else(||Error::new("oracle_mixed_transaction"))?;
                if payload["batch"]!=*batch||payload["buffer_context"]!=self.context_ref
                    ||payload["source_position"]["typed_batch_digest"]!=entry["digest"]
                    ||payload["source_position"]["source_offset_write"]!=false {
                    return Err(Error::new("oracle_mixed_transaction"));
                }
                entry["source_evidence_sha256"]=json!(sha256_hex(
                    &serde_json::to_vec(&payload["source_position"]).map_err(|_|Error::new("oracle_mixed_transaction"))?));
                pos+=1;
            }else{return Err(Error::new("oracle_mixed_kind"));}
            entries.push(entry);
        }
        let snapshot=document(&self.bundle["boundary"]["native_snapshot"])?;
        let rows=tables.values().try_fold(0u64,|a,t|a.checked_add(t["rows"].as_u64().unwrap_or(0)))
            .ok_or_else(||Error::new("oracle_mixed_rows"))?;
        let specs=self.bundle["typed_tables"].as_object().ok_or_else(||Error::new("oracle_mixed_tables"))?;
        if fills==0||pos==0||pos as usize!=self.positions.len()||snapshot["complete"]!=true
            ||snapshot["rows"].as_u64()!=Some(rows)||tables.len()!=specs.len()
            ||tables.keys().any(|k|!specs.contains_key(k))
            ||self.index["transaction_count"].as_u64()!=Some(pos)
            ||self.index["final_cursor"]["transaction_ordinal"]!=pos.to_string()
            ||self.bundle["boundary"]["final_position"]!=pos.to_string() {
            return Err(Error::new("oracle_mixed_coverage"));
        }
        Ok(json!({"protocol":"oo.oracle.initial-mixed/v1","source_id":self.context["source_id"],
            "config_digest":sha256_hex(&serde_json::to_vec(&config["admission_config"]).map_err(|_|Error::new("oracle_mixed_config"))?),
            "typed_schema_sha256":self.context["typed_schema_sha256"],"buffer_index":self.index_ref,
            "buffer_context":self.context_ref,"bundle_sha256":self.context["bundle"]["sha256"],
            "batch_count":entries.len(),"fill_batch_count":fills,"final_position":pos.to_string(),
            "batches":entries,"tables":tables}))
    }

    pub fn fill_open(&self,frame:&Value,resume:&Value,sequence:i64,epoch:i64,request:&mut Value)->Result<()> {
        let plan=self.initial_fill.as_ref().ok_or_else(||Error::new("oracle_fill_only"))?;
        if frame["stream_mode"]!="initial_fill"||frame["initial_fill"]!=*plan
            ||frame["buffer_index"]!=self.index_ref||frame["resume_sequence"]!=sequence
            ||request["config_digest"]!=plan["config_digest"]
            ||request["schema_digest"]!=plan["typed_schema_sha256"]
            ||(!resume.is_null()&&(resume["source_epoch"]!=epoch||resume["resume_sequence"]!=sequence
                ||resume["protocol"]!="oo.jobs.oracle-fill-resume/v1"))
            ||(resume.is_null()&&sequence!=0) {
            return Err(Error::new("oracle_fill_open"));
        }
        request["stream_mode"]=json!("initial_fill");
        request["initial_fill"]=plan.clone();
        request["buffer_index"]=self.index_ref.clone();
        request["resume_sequence"]=json!(sequence);
        request["resume"]=resume.clone();
        Ok(())
    }

    pub fn fill_open_ack(&self,receipt:&Value,resume:&Value,sequence:i64,epoch:i64,reply:&mut Value)->Result<()> {
        let completed=if resume.is_null() {json!([])}else{resume["completed_tables"].clone()};
        if !self.is_initial_fill()||receipt["outcome"]!="open"||receipt["source_epoch"]!=epoch
            ||receipt["resumed_sequence"]!=sequence||receipt["buffer_context_sha256"]!=self.context_ref["sha256"]
            ||receipt["bundle_sha256"]!=self.context["bundle"]["sha256"]
            ||receipt["completed_tables"]!=completed {
            return Err(Error::new("oracle_fill_open_proof"));
        }
        for key in ["source_epoch","resumed_sequence","buffer_context_sha256","bundle_sha256","completed_tables"] {
            reply[key]=receipt[key].clone();
        }
        Ok(())
    }

    fn fill_tables(&self,tables:&Value,sequence:i64)->Result<std::collections::BTreeSet<String>> {
        let plan=self.initial_fill.as_ref().ok_or_else(||Error::new("oracle_fill_only"))?;
        let expected=self.bundle["typed_tables"].as_object().ok_or_else(||Error::new("oracle_fill_tables"))?;
        let actual=tables.as_object().ok_or_else(||Error::new("oracle_fill_tables"))?;
        if actual.len()!=expected.len() {return Err(Error::new("oracle_fill_tables"));}
        let mut completed=std::collections::BTreeSet::new();
        for (name,spec) in expected {
            let table=actual.get(name).ok_or_else(||Error::new("oracle_fill_tables"))?;
            if table["spec"]!=*spec||table["complete"].as_bool().is_none() {
                return Err(Error::new("oracle_fill_tables"));
            }
            if table["complete"]==true {
                if plan["tables"][name]["last_sequence"].as_i64().is_none_or(|last|last>sequence) {
                    return Err(Error::new("oracle_fill_incomplete"));
                }
                completed.insert(name.clone());
            }
        }
        Ok(completed)
    }

    pub async fn fill_completed_tables(&self,admin:&tokio_postgres::Client,task:&str,generation:i64,epoch:i64,sequence:i64)
        ->Result<std::collections::BTreeSet<String>> {
        let source=self.context["source_id"].as_str().ok_or_else(||Error::new("oracle_source"))?;
        let row=admin.query_one(
            "SELECT jsonb_object_agg(t.name,jsonb_build_object('spec',t.spec,'complete',t.snapshot_complete))::text FROM _oo.c1_typed_table t JOIN _oo.native_c1_baseline_run r ON r.source=t.source AND r.task_id=t.task JOIN jobs.tasks j ON j.id=t.task JOIN _oo.c1_typed_checkpoint c ON c.source=t.source AND c.task=t.task WHERE t.source=$1 AND t.task=$2::text::uuid AND r.lease_generation=$3 AND r.source_epoch::bigint=$4 AND j.state='running' AND j.generation=$3 AND j.lease_until>clock_timestamp() AND c.seq=$5",
            &[&source,&task,&generation,&epoch,&sequence]).await.map_err(Error::sql)?;
        let raw:Option<String>=row.get(0);
        let tables:Value=serde_json::from_str(&raw.ok_or_else(||Error::new("oracle_fill_tables"))?).map_err(|_|Error::new("oracle_fill_tables"))?;
        self.fill_tables(&tables,sequence)
    }
}

pub fn export_rows(root:&str,rows:&Value,seal:&Value,source:&str,task:&str,generation:i64,epoch:i64)->Result<Value> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let bytes=serde_json::to_vec(&json!({"protocol":"oo.pipeline.typed-target-export/v1","source_engine":"oracle",
        "source_id":source,"task_id":task,"lease_generation":generation,"source_epoch":epoch,"seal":seal,"rows":rows}))
        .map_err(|_|Error::new("oracle_export"))?;
    if bytes.len()>32*1024*1024 {return Err(Error::new("oracle_export_budget"));}
    let path=Path::new(root).join("sealed-target-export.json");
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(|_|Error::new("oracle_export"))?;
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|_|Error::new("oracle_export"))?;
    Ok(json!({"path":path,"sha256":sha256_hex(&bytes),"bytes":bytes.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_protocol_is_cdc_only_without_relabeling_legacy_fill() {
        assert!(batch_protocol_supported(&json!({"protocol":"oo.c1.typed-batch/v3","kind":"fill"})));
        assert!(batch_protocol_supported(&json!({"protocol":"oo.c1.typed-ordered-batch/v1","kind":"cdc"})));
        assert!(!batch_protocol_supported(&json!({"protocol":"oo.c1.typed-ordered-batch/v1","kind":"fill"})));
        assert!(!batch_protocol_supported(&json!({"protocol":"unknown","kind":"cdc"})));
    }
    #[test]
    fn transport_alias_is_allowed_but_symlink_escape_is_not() {
        let base=std::env::temp_dir().join(format!("oo-oracle-transport-{}",std::process::id()));
        std::fs::create_dir_all(base.join("root/parts")).unwrap();
        std::fs::write(base.join("root/parts/ok"),b"part").unwrap();
        std::fs::write(base.join("outside"),b"outside").unwrap();
        std::os::unix::fs::symlink(base.join("root"),base.join("alias")).unwrap();
        std::os::unix::fs::symlink(base.join("outside"),base.join("root/parts/escape")).unwrap();
        assert!(transport_part_within(base.join("root").to_str().unwrap(),base.join("alias/parts/ok").to_str().unwrap()));
        assert!(!transport_part_within(base.join("root").to_str().unwrap(),base.join("root/parts/escape").to_str().unwrap()));
        std::fs::remove_dir_all(base).unwrap();
    }
}
