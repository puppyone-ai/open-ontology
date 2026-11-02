//! Parts references are bounded here; the final Loader verifies contents and DB authority.
use super::*;
use std::io::Read;

pub fn retained_open(out:&str,before_generation:i64)->Result<Value> {
    use std::io::BufRead;
    let root=Path::new(out);
    let mut path=root.join("worker-stream.ndjson");
    if !path.exists() {
        // Resumable initial/delta captures retain one stream per lease. Never
        // select this lease, a future lease, or an export-only retry stream.
        let mut selected=None;
        for entry in std::fs::read_dir(root).map_err(|_|Error::new("mysql_export_original_open"))? {
            let entry=entry.map_err(|_|Error::new("mysql_export_original_open"))?;
            let name=entry.file_name();
            let Some(name)=name.to_str() else {continue};
            let Some(number)=name.strip_prefix("worker-stream-lease-").and_then(|s|s.strip_suffix(".ndjson")) else {continue};
            let Ok(generation)=number.parse::<i64>() else {continue};
            if generation<1 || generation>=before_generation || generation.to_string()!=number {continue;}
            if !entry.file_type().map_err(|_|Error::new("mysql_export_original_open"))?.is_file() {
                return Err(Error::new("mysql_export_original_open"));
            }
            if selected.as_ref().is_none_or(|(prior,_)|generation>*prior) {
                selected=Some((generation,entry.path()));
            }
        }
        path=selected.ok_or_else(||Error::new("mysql_export_original_open"))?.1;
    }
    let file=std::fs::File::open(path).map_err(|_|Error::new("mysql_export_original_open"))?;
    let mut line=String::new();
    std::io::BufReader::new(file).take(1024*1024+1).read_line(&mut line)
        .map_err(|_|Error::new("mysql_export_original_open"))?;
    if line.len()>1024*1024 {return Err(Error::new("mysql_export_original_open"));}
    let open:Value=serde_json::from_str(&line).map_err(|_|Error::new("mysql_export_original_open"))?;
    if open["kind"]!="open" {return Err(Error::new("mysql_export_original_open"));}
    Ok(open)
}

#[cfg(test)]
mod retained_tests {
    use super::*;

    #[test]
    fn sealed_initial_recovery_reads_prior_lease_stream_without_inventing_open() {
        let root=std::env::temp_dir().join(format!("retained-c1-stream-{}-{}",
            std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        for (name,kind) in [("worker-stream-lease-1.ndjson","open"),("worker-stream-lease-2.ndjson","open"),
            ("worker-stream-lease-3.ndjson","in_progress"),("worker-export-lease-2.ndjson","export_resume")] {
            std::fs::write(root.join(name),format!("{{\"kind\":\"{kind}\",\"retained\":\"{name}\"}}\n")).unwrap();
        }
        let out=root.to_str().unwrap();
        assert_eq!(retained_open(out,3).unwrap()["retained"],"worker-stream-lease-2.ndjson");
        assert_eq!(retained_open(out,2).unwrap()["retained"],"worker-stream-lease-1.ndjson");
        assert!(retained_open(out,1).is_err());
        std::fs::write(root.join("worker-stream-lease-2.ndjson"),"{\"kind\":\"not_open\"}\n").unwrap();
        assert!(retained_open(out,3).is_err());
        std::fs::write(root.join("worker-stream.ndjson"),"{\"kind\":\"open\",\"legacy\":true}\n").unwrap();
        assert_eq!(retained_open(out,3).unwrap()["legacy"],true);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Only the completed stage is recoverable here, never a partial initial fill.
pub async fn sealed_complete(admin:&tokio_postgres::Client,source:&str,task:&str)->Result<bool> {
    let row=admin.query_one(r#"
SELECT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_run r
 JOIN _oo.source_instance i ON i.source=r.source AND i.source_epoch=r.source_epoch
 JOIN _oo.c1_typed_checkpoint c ON c.source=r.source AND c.task=r.task_id
 WHERE r.source=$1 AND r.task_id=$2::text::uuid AND r.engine='mysql'
 AND r.state='awaiting_publish' AND r.l_end=r.applied_pos AND r.l_end>=r.l0_pos
 AND i.engine='mysql' AND i.lease_id=r.task_id AND i.measured_id=r.measured_id
 AND i.config_digest=r.config_digest AND i.schema_digest=r.schema_digest
 AND EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=r.source AND task=r.task_id)
 AND NOT EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=r.source AND task=r.task_id AND NOT snapshot_complete)
 AND c.seq=(SELECT count(*) FROM _oo.c1_typed_receipt WHERE source=r.source AND task=r.task_id)
 AND (c.seq=0 OR EXISTS(SELECT 1 FROM _oo.c1_typed_receipt
   WHERE source=r.source AND task=r.task_id AND seq=c.seq AND position=r.applied_pos))
 AND NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=r.source AND task_id=r.task_id AND acknowledged_at IS NOT NULL))
"#,&[&source,&task]).await.map_err(Error::sql)?;
    Ok(row.get(0))
}

pub fn verify_sealed_open(open:&Value,meta:&Value,config_sha:&str,source:&str,task:&str,generation:i64)->Result<()> {
    if open["kind"]!="open" || open["connection_config_sha256"]!=config_sha
        || meta["protocol"]!="oo.mysql.typed-export-binding/v1" || meta["engine"]!="mysql"
        || meta["source_id"]!=source || meta["task_id"]!=task || meta["lease_generation"]!=generation
        || ["l0","config_digest","schema_digest"].iter().any(|k|open[*k]!=meta[*k])
        || meta["source_instance"]["source"]!=source || meta["source_instance"]["lease_id"]!=task
        || meta["source_instance"]["source_epoch"]!=meta["source_epoch"] {
        return Err(Error::new("mysql_export_sealed_identity"));
    }
    let tables=meta["tables"].as_array().filter(|v|!v.is_empty()&&v.len()<=32)
        .ok_or_else(||Error::new("mysql_export_sealed_tables"))?;
    if open["typed_tables"].as_object().is_none_or(|v|v.len()!=tables.len())
        || tables.iter().any(|t|t["name"].as_str().is_none_or(|name|open["typed_tables"][name]!=t["spec"])) {
        return Err(Error::new("mysql_export_sealed_tables"));
    }
    Ok(())
}

pub fn validate(request:&Value,source:&str,task:&str,generation:i64)->Result<Value> {
    let fail=||Error::new("mysql_export_manifest");
    if request["c1_baseline_format"]!="typed-keys/v3"
        || request["typed_export"]["protocol"]!="oo.mysql.typed-export-parts/v1"
        || request["baseline_scope"]["protocol"]!="oo.mysql.typed-export-scope/v1"
        || request["baseline_scope"]["engine"]!="mysql"
        || request["baseline_scope"]["complete"]!=true
        || request["baseline_scope"]["consistency_grade"]!="T"
        || request["baseline_scope"]["cross_table_consistent"]!=true
        || request["source_identity"]!=request["baseline_scope"]["source_identity"]
        || request["source_identity"]["engine"]!="mysql"
        || request["source_identity"]["measured_id"]!=request["baseline_scope"]["lineage"] {
        return Err(fail());
    }
    let path=request["typed_export"]["path"].as_str().ok_or_else(fail)?;
    if !Path::new(path).is_absolute() || path.contains("..") {return Err(fail());}
    let file=std::fs::File::open(path).map_err(|_|fail())?;
    if file.metadata().map_err(|_|fail())?.len()>8*1024*1024 {return Err(fail());}
    let mut raw=Vec::new();
    std::io::Read::take(file,8*1024*1024+1).read_to_end(&mut raw).map_err(|_|fail())?;
    if raw.len()>8*1024*1024 || request["typed_export"]["sha256"]!=sha256_hex(&raw) {return Err(fail());}
    let manifest:Value=serde_json::from_slice(&raw).map_err(|_|fail())?;
    let binding=&manifest["binding"];
    if manifest["protocol"]!=request["typed_export"]["protocol"] || manifest["source_id"]!=source
        || manifest["task_id"]!=task || manifest["source_epoch"]!=request["source_epoch"]
        || manifest["source_generation"]!=request["source_generation"]
        || binding["source_id"]!=source || binding["task_id"]!=task || binding["engine"]!="mysql"
        || binding["source_epoch"]!=request["source_epoch"]
        || binding["config_digest"]!=request["baseline_scope"]["config_digest"]
        || binding["schema_digest"]!=request["baseline_scope"]["schema_digest"]
        || binding["measured_id"]!=request["source_identity"]["measured_id"]
        || binding["l0"]!=request["baseline_scope"]["l0"] {
        return Err(fail());
    }
    let tables=manifest["tables"].as_array().filter(|t|!t.is_empty()&&t.len()<=32).ok_or_else(fail)?;
    let entries=request["entries"].as_array().filter(|e|e.len()==tables.len()).ok_or_else(fail)?;
    let mut total=0u64;
    let mut names=std::collections::BTreeSet::new();
    for (table,entry) in tables.iter().zip(entries) {
        let schema=table["schema"].as_str().ok_or_else(fail)?;
        let name=table["table"].as_str().ok_or_else(fail)?;
        if !names.insert((schema,name)) || entry["schema"]!=schema || entry["table"]!=name
            || entry["row_count"]!=table["rows"] || entry["schema_sha256"]!=table["schema_digest"] {
            return Err(fail());
        }
        let parts=table["parts"].as_array().filter(|p|p.len()<=8192).ok_or_else(fail)?;
        let mut after="";
        let mut rows=0u64;
        for (i,p) in parts.iter().enumerate() {
            let n=p["rows"].as_u64().filter(|n|*n>0&&*n<=2048).ok_or_else(fail)?;
            let bytes=p["bytes"].as_u64().filter(|n|*n>0&&*n<=4*1024*1024).ok_or_else(fail)?;
            let part=p["path"].as_str().ok_or_else(fail)?;
            let last=p["last_key"].as_str().ok_or_else(fail)?;
            if p["index"]!=i+1 || p["after_key"]!=after || last<=after || last.len()>4000
                || last.len()%2!=0 || !last.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))
                || !transport_part_within(Path::new(path).parent().and_then(Path::to_str).ok_or_else(fail)?,part)
                || std::fs::metadata(part).map_err(|_|fail())?.len()!=bytes
                || !p["sha256"].as_str().is_some_and(is_hex64) || !p["values_sha256"].as_str().is_some_and(is_hex64) {
                return Err(fail());
            }
            rows=rows.checked_add(n).ok_or_else(fail)?;after=last;
        }
        if table["rows"]!=rows {return Err(fail());}
        total=total.checked_add(rows).ok_or_else(fail)?;
    }
    let limit=request["typed_export"]["max_rows"].as_u64().filter(|n|*n>0&&*n<=50_000_000).ok_or_else(fail)?;
    if total>limit {return Err(fail());}
    let mut invoked=request.clone();
    crate::capture_source::attach_observed_lease(&mut invoked,task,generation).map_err(|_|fail())?;
    Ok(invoked)
}

#[cfg(test)]
mod tests {
    use super::*;
    const TASK:&str="00000000-0000-4000-8000-000000000095";

    #[test]
    fn sealed_export_retry_requires_original_identity_and_every_table_spec() {
        let spec=json!({"columns":[{"name":"id","oid":20,"typmod":-1,"nullable":false}],"keys":[0]});
        let open=json!({"kind":"open","connection_config_sha256":"a","l0":"uuid:1-4",
            "config_digest":"b","schema_digest":"c","typed_tables":{"db.t":spec}});
        let meta=json!({"protocol":"oo.mysql.typed-export-binding/v1","engine":"mysql",
            "source_id":"s","task_id":TASK,"lease_generation":2,"source_epoch":1,
            "l0":"uuid:1-4","config_digest":"b","schema_digest":"c",
            "source_instance":{"source":"s","lease_id":TASK,"source_epoch":1},
            "tables":[{"name":"db.t","spec":spec}]});
        verify_sealed_open(&open,&meta,"a","s",TASK,2).unwrap();
        for (pointer,value) in [("/lease_generation",json!(1)),("/source_epoch",json!(2)),
            ("/source_id",json!("other")),("/engine",json!("oracle")),("/l0",json!("uuid:1-5")),
            ("/tables/0/spec/keys",json!([])),("/source_instance/lease_id",json!("other"))] {
            let mut changed=meta.clone();*changed.pointer_mut(pointer).unwrap()=value;
            assert!(verify_sealed_open(&open,&changed,"a","s",TASK,2).is_err(),"{pointer}");
        }
        assert!(verify_sealed_open(&open,&meta,"changed","s",TASK,2).is_err());
        let mut changed=meta.clone();changed["tables"]=json!([]);
        assert!(verify_sealed_open(&open,&changed,"a","s",TASK,2).is_err());
    }

    #[test]
    fn bounded_export_worker_checks_manifest_chain_identity_and_containment() {
        let dir=std::env::temp_dir().join(format!("mysql-export-worker-{}-{}",
            std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&dir).unwrap();
        let part=dir.join("part.arrow");std::fs::write(&part,b"x").unwrap();
        let path=dir.join("manifest.json");
        let identity=json!({"engine":"mysql","measured_id":"mysql.uuid"});
        let scope=json!({"protocol":"oo.mysql.typed-export-scope/v1","engine":"mysql","complete":true,
            "consistency_grade":"T","cross_table_consistent":true,"source_identity":identity,
            "lineage":"mysql.uuid","l0":"uuid:1-2","config_digest":"c","schema_digest":"d"});
        let manifest=json!({"protocol":"oo.mysql.typed-export-parts/v1","source_id":"s","task_id":TASK,
            "source_epoch":1,"source_generation":2,"binding":{"source_id":"s","task_id":TASK,
                "engine":"mysql","source_epoch":1,"config_digest":"c","schema_digest":"d",
                "measured_id":"mysql.uuid","l0":"uuid:1-2"},
            "tables":[{"schema":"db","table":"items","rows":1,"schema_digest":"e","parts":[
                {"index":1,"path":part,"rows":1,"bytes":1,"sha256":"a".repeat(64),
                 "values_sha256":"b".repeat(64),"after_key":"","last_key":"01"}]}]});
        let request=json!({"protocol":"oo.pipeline.native-c1-baseline/v1","c1_baseline_format":"typed-keys/v3",
            "source_identity":identity,"source_epoch":1,"source_generation":2,"baseline_scope":scope,
            "typed_export":{"protocol":"oo.mysql.typed-export-parts/v1","path":path,"max_rows":1},
            "entries":[{"schema":"db","table":"items","row_count":1,"schema_sha256":"e"}]});
        let write=|m:&Value| {
            let bytes=serde_json::to_vec(m).unwrap();std::fs::write(&path,&bytes).unwrap();
            let mut v=request.clone();v["typed_export"]["sha256"]=json!(sha256_hex(&bytes));v
        };
        let v=write(&manifest);
        assert_eq!(validate(&v,"s",TASK,3).unwrap()["lease_generation"],3);
        assert!(validate(&v,"wrong",TASK,3).is_err());
        for (pointer,value) in [
            ("/tables/0/parts/0/rows",json!(2049)),
            ("/tables/0/parts/0/bytes",json!(4194305)),
            ("/tables/0/parts/0/after_key",json!("00")),
            ("/tables/0/parts/0/index",json!(2)),
            ("/tables/0/parts/0/path",json!("/tmp/outside.arrow")),
            ("/tables/0/rows",json!(2)),
            ("/source_epoch",json!(2)),
        ] {
            let mut m=manifest.clone();*m.pointer_mut(pointer).unwrap()=value;
            assert!(validate(&write(&m),"s",TASK,3).is_err(),"{pointer}");
        }
        let mut m=manifest.clone();
        m["tables"][0]["parts"].as_array_mut().unwrap().push(manifest["tables"][0]["parts"][0].clone());
        assert!(validate(&write(&m),"s",TASK,3).is_err());
        let mut bad=write(&manifest);bad["typed_export"]["sha256"]=json!("0".repeat(64));
        assert!(validate(&bad,"s",TASK,3).is_err());
        std::fs::remove_file(path).unwrap();std::fs::remove_file(part).unwrap();std::fs::remove_dir(dir).unwrap();
    }
}
