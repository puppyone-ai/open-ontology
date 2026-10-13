//! Native producer scheduling with a source-and-target generation gate.
use std::{path::Path, process::Stdio, time::Duration};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::Command};
use crate::{config::Config,error::{Error,Result},lease,session,sha256::sha256_hex,api::LeaseStatus};
const KIND:&str="native_capture";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Executable { pub path:String, pub sha256:String, pub args:Vec<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
 pub admission_binary:String, pub admission_sha256:String,
 #[serde(default,skip_serializing_if="Option::is_none")]
 pub registry_path:Option<String>,
 #[serde(default,skip_serializing_if="Option::is_none")]
 pub transaction_registry_vector_path:Option<String>,
 #[serde(default,skip_serializing_if="Option::is_none")]
 pub transaction_registry_vector_sha256:Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
 pub protocol:String, pub binding:String, pub project_id:String, pub source_id:String,
 pub method:String, pub cycle:String, pub period_ms:i64,
 pub producer:Executable, pub loader:Executable,
 #[serde(default,skip_serializing_if="Option::is_none")]
 pub registration:Option<Registration>,
}
fn executable_check(e:&Executable)->Result<()> {
 if !Path::new(&e.path).is_absolute() || e.path.contains("..") || e.sha256.len()!=64
  || !e.sha256.bytes().all(|b|b.is_ascii_hexdigit()) || e.args.iter().any(|a|a.contains('\0')||a.contains("secret://")||a.contains("password=")) {
  return Err(Error::new("executable"));
 }
 if sha256_hex(&std::fs::read(&e.path).map_err(|_|Error::new("executable"))?)!=e.sha256 {return Err(Error::new("hash"))}
 Ok(())
}
impl Manifest {
 fn shape(&self)->Result<()> {
  if self.protocol!="oo.jobs.native-schedule/v1" || !crate::api::is_uuid(&self.binding)
   || !crate::api::is_uuid(&self.project_id) || self.source_id.is_empty()
   || !["C1","C2","C3","C4","C5"].contains(&self.method.as_str())
   || !["detect","pull","reconcile"].contains(&self.cycle.as_str())
   || !(1000..=86400000).contains(&self.period_ms)
   || self.loader.args!=["import-native-update"] && self.loader.args!=["import-native-transaction"]
   || self.loader.args==["import-native-transaction"] && self.method!="C1" {return Err(Error::new("manifest"))}
  for e in [&self.producer,&self.loader] {
   if !Path::new(&e.path).is_absolute() || e.path.contains("..") || e.sha256.len()!=64 || !e.sha256.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(Error::new("manifest"))}
  }
  if let Some(r)=&self.registration {
   let group=self.loader.args==["import-native-transaction"];
   if group {
    let (Some(path),Some(sha))=(&r.transaction_registry_vector_path,&r.transaction_registry_vector_sha256) else {return Err(Error::new("registration"))};
    if r.registry_path.is_some() || !Path::new(path).is_absolute() || path.contains("..") || path.contains('\0') || sha.len()!=64 || !sha.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(Error::new("registration"))}
   } else if !r.registry_path.as_ref().is_some_and(|path|Path::new(path).is_absolute() && !path.contains("..") && !path.contains('\0'))
    || r.transaction_registry_vector_path.is_some() || r.transaction_registry_vector_sha256.is_some() {return Err(Error::new("registration"))}
  } else if self.loader.args==["import-native-transaction"] {return Err(Error::new("registration"))}
  Ok(())
 }
 fn check(&self)->Result<()> {
  self.shape()?;executable_check(&self.producer)?;executable_check(&self.loader)?;
  if let Some(r)=&self.registration {
   executable_check(&Executable{path:r.admission_binary.clone(),sha256:r.admission_sha256.clone(),args:vec![]})?;
   if let (Some(path),Some(sha))=(&r.transaction_registry_vector_path,&r.transaction_registry_vector_sha256)
    && sha256_hex(&std::fs::read(path).map_err(|_|Error::new("registration"))?)!=*sha {return Err(Error::new("registration"))}
  }
  Ok(())
 }
}

fn manifest_read(path:&str)->Result<Manifest> {
 let b=std::fs::read(path).map_err(|_|Error::new("manifest"))?;
 let m:Manifest=serde_json::from_slice(&b).map_err(|_|Error::new("manifest"))?; m.check()?;Ok(m)
}
fn canonical(m:&Manifest)->Result<(String,String)> {
 let s=serde_json::to_string(m).map_err(|_|Error::new("manifest"))?; let h=sha256_hex(s.as_bytes());Ok((s,h))
}
fn target_database(config:&Config,method:&str)->Result<String> {
 let database=if method=="C4" {config.target_database.as_deref().ok_or_else(||Error::new("target_database"))?} else {&config.database};
 if database.is_empty() || database.contains('\0') {return Err(Error::new("target_database"))}
 Ok(database.to_owned())
}
async fn target_owner(config:&Config,database:&str)->Result<tokio_postgres::Client> {
 if config.target_database.as_deref()==Some(database) {session::target_runtime(config).await}
 else if config.database==database {session::admin_db(config,database).await}
 else {Err(Error::new("target_database"))}
}
async fn confirmed_generation(config:&Config,control:&tokio_postgres::Client,source:&str,database:&str,task:&str,stored:&Option<String>)->Result<i64> {
 let row=control.query_one("SELECT source_generation,task_id::text,active_task_id::text FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2",&[&source,&database]).await.map_err(Error::sql)?;
 if row.get::<_,Option<String>>(2).as_deref()!=Some(task) {return Err(Error::new("source_active"))}
 let target=target_owner(config,database).await?;
 let applied=target.query_opt("SELECT source_generation FROM _oo.source_watermark WHERE source=$1",&[&source]).await.map_err(Error::sql)?.map(|r|r.get::<_,i64>(0)).unwrap_or(0);
 let mut confirmed:Option<i64>=row.get(0);
 let replay_generation=stored.as_ref().and_then(|s|serde_json::from_str::<Value>(s).ok()).and_then(|v|v.get("source_generation").and_then(Value::as_i64));
 if confirmed.is_none() && applied>0 && replay_generation!=Some(applied) {
  let historical=control.query_opt("SELECT w.source_generation,w.task_id::text,w.binding_id::text,b.method,w.result::text FROM jw_native.watermarks w JOIN jw_native.bindings b ON b.id=w.binding_id JOIN jobs.tasks t ON t.id=w.task_id LEFT JOIN jw_native.pending_acks a ON a.task_id=w.task_id WHERE b.source_id=$1 AND b.target_database=$2 AND w.source_generation=$3 AND t.state='succeeded' AND (b.method<>'C4' OR a.acknowledged_at IS NOT NULL) ORDER BY w.advanced_at DESC LIMIT 1",&[&source,&database,&applied]).await.map_err(Error::sql)?.ok_or_else(||Error::new("unconfirmed_baseline"))?;
  let method:String=historical.get(3);let result_text:String=historical.get(4);
  if method=="C4" {
   let result:Value=serde_json::from_str(&result_text).map_err(|_|Error::new("unconfirmed_baseline"))?;
   let ack=ack_request(&result,&historical.get::<_,String>(1),result.pointer("/partition_target_receipt/job_generation").and_then(Value::as_i64).ok_or_else(||Error::new("unconfirmed_baseline"))?)?;
   let exact=target.query_opt("SELECT acknowledged FROM _oo.native_partition_state WHERE source=$1 AND schema_name=$2 AND table_name=$3 AND partition_id=$4 AND run_id=$5::text::uuid AND batch_id=$6 AND payload_digest=$7 AND job_task_id=$8::text::uuid AND job_generation=$9",&[&source,&ack["logical_schema"].as_str(),&ack["logical_table"].as_str(),&ack["partition_id"].as_str(),&ack["run_id"].as_str(),&ack["batch_id"].as_str(),&ack["payload_digest"].as_str(),&ack["job_task_id"].as_str(),&ack["job_generation"].as_i64()]).await.map_err(Error::sql)?;
   if !exact.is_some_and(|r|r.get::<_,bool>(0)) {return Err(Error::new("unconfirmed_baseline"))}
  }
  let historical_task:String=historical.get(1);let historical_binding:String=historical.get(2);
  control.execute("UPDATE jw_native.source_progress SET source_generation=$3,task_id=$4::text::uuid,binding_id=$5::text::uuid,confirmed_at=clock_timestamp() WHERE source_id=$1 AND target_database=$2 AND source_generation IS NULL AND active_task_id=$6::text::uuid",&[&source,&database,&applied,&historical_task,&historical_binding,&task]).await.map_err(Error::sql)?;
  confirmed=Some(applied);
 }
 let base=confirmed.unwrap_or(0);
 if let Some(previous_task)=row.get::<_,Option<String>>(1) {
  let prior=control.query_opt("SELECT t.state,b.method,w.result::text,a.acknowledged_at IS NOT NULL FROM jobs.tasks t JOIN jw_native.watermarks w ON w.task_id=t.id JOIN jw_native.bindings b ON b.id=w.binding_id LEFT JOIN jw_native.pending_acks a ON a.task_id=t.id WHERE t.id=$1::text::uuid AND b.source_id=$2 AND b.target_database=$3 AND w.source_generation=$4",&[&previous_task,&source,&database,&base]).await.map_err(Error::sql)?.ok_or_else(||Error::new("unconfirmed_baseline"))?;
  if prior.get::<_,String>(0)!="succeeded" {return Err(Error::new("unconfirmed_baseline"))}
  if prior.get::<_,String>(1)=="C4" {
   if !prior.get::<_,bool>(3) {return Err(Error::new("unconfirmed_baseline"))}
   let result:Value=serde_json::from_str(&prior.get::<_,String>(2)).map_err(|_|Error::new("unconfirmed_baseline"))?;
   let receipt=result.get("partition_target_receipt").ok_or_else(||Error::new("unconfirmed_baseline"))?;
   let scope=receipt.get("partition_scope").ok_or_else(||Error::new("unconfirmed_baseline"))?;
   let exact=target.query_opt("SELECT acknowledged FROM _oo.native_partition_state WHERE source=$1 AND schema_name=$2 AND table_name=$3 AND partition_id=$4 AND run_id=$5::text::uuid AND batch_id=$6 AND payload_digest=$7 AND job_task_id=$8::text::uuid AND job_generation=$9",&[&source,&scope["logical_schema"].as_str(),&scope["logical_table"].as_str(),&scope["partition_id"].as_str(),&result["run_id"].as_str(),&receipt["batch_id"].as_str(),&receipt["payload_digest"].as_str(),&previous_task,&receipt["job_generation"].as_i64()]).await.map_err(Error::sql)?;
   if !exact.is_some_and(|r|r.get::<_,bool>(0)) {return Err(Error::new("unconfirmed_baseline"))}
  }
 }
 if applied!=base && (applied!=replay_generation.unwrap_or(-1) || applied!=base+1) {return Err(Error::new("target_ahead"))}
 Ok(base)
}
// C1 generations belong to a table binding. The source watermark is a
// cross-table position barrier and can be many generations ahead of a table.
async fn confirmed_c1_generation(config:&Config,control:&tokio_postgres::Client,source:&str,database:&str,binding:&str,task:&str,stored:&Option<String>)->Result<i64> {
 let active=control.query_opt("SELECT active_task_id::text FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2",&[&source,&database]).await.map_err(Error::sql)?;
 if active.and_then(|r|r.get::<_,Option<String>>(0)).as_deref()!=Some(task) {return Err(Error::new("source_active"))}
 let prior=control.query_opt("SELECT w.source_generation,w.applied_pos,t.state,o.request::text,w.result::text FROM jw_native.watermarks w JOIN jobs.tasks t ON t.id=w.task_id JOIN jw_native.occurrences o ON o.task_id=w.task_id WHERE w.binding_id=$1::text::uuid",&[&binding]).await.map_err(Error::sql)?;
 let replay:Option<Value>=stored.as_ref().map(|s|serde_json::from_str(s).map_err(|_|Error::new("request"))).transpose()?;
 let base=prior.as_ref().map(|r|r.get::<_,i64>(0)).unwrap_or(0);
 let lineage=prior.as_ref().map(|r|serde_json::from_str::<Value>(&r.get::<_,String>(3)).map_err(|_|Error::new("unconfirmed_baseline"))).transpose()?;
 if prior.as_ref().is_some_and(|r|r.get::<_,String>(2)!="succeeded") {return Err(Error::new("unconfirmed_baseline"))}
 let identity=replay.as_ref().or(lineage.as_ref());
 if let Some(request)=identity {
  let schema=request.get("logical_schema").and_then(Value::as_str).ok_or_else(||Error::new("unconfirmed_baseline"))?;
  let table=request.get("logical_table").and_then(Value::as_str).ok_or_else(||Error::new("unconfirmed_baseline"))?;
  if request.get("source_id").and_then(Value::as_str)!=Some(source)
   || lineage.as_ref().is_some_and(|p|p.get("logical_schema")!=request.get("logical_schema") || p.get("logical_table")!=request.get("logical_table")) {return Err(Error::new("unconfirmed_baseline"))}
  let target=target_owner(config,database).await?;
  if let Some(prior)=prior.as_ref() {
   let old=lineage.as_ref().ok_or_else(||Error::new("unconfirmed_baseline"))?;
   let result:Value=serde_json::from_str(&prior.get::<_,String>(4)).map_err(|_|Error::new("unconfirmed_baseline"))?;
   let run=result.get("run_id").and_then(Value::as_str).ok_or_else(||Error::new("unconfirmed_baseline"))?;
   let physical=result.get("physical_name").and_then(Value::as_str).ok_or_else(||Error::new("unconfirmed_baseline"))?;
   let target_generation=result.get("target_generation").and_then(Value::as_i64).ok_or_else(||Error::new("unconfirmed_baseline"))?;
   let digest=result.get("payload_digest").and_then(Value::as_str).ok_or_else(||Error::new("unconfirmed_baseline"))?;
   let batch=old.get("batch_id").and_then(Value::as_str).ok_or_else(||Error::new("unconfirmed_baseline"))?;
   let previous:String=prior.get(1);
   let sealed:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.full_import_run r JOIN _oo.full_import_table ft ON ft.run_id=r.run_id JOIN _oo.apply_receipts ar ON ar.source=r.source_id AND ar.table_generation=ft.physical_name WHERE r.run_id=$1::text::uuid AND r.state='awaiting_publish' AND r.source_id=$2 AND r.source_generation=$3 AND r.target_generation=$4 AND ft.schema_ident=$5 AND ft.table_ident=$6 AND ft.physical_name=$7 AND ft.exhausted AND ar.txn_or_batch_id=$8 AND ar.payload_digest=$9 AND ar.applied_pos=$10::text::numeric AND ar.txn_complete)",&[&run,&source,&base,&target_generation,&schema,&table,&physical,&batch,&digest,&previous]).await.map_err(Error::sql)?.get(0);
   if !sealed {return Err(Error::new("unconfirmed_baseline"))}
  }
  let row=target.query_opt("SELECT source_generation,last_pos::text FROM _oo.table_freshness WHERE source=$1 AND schema_name=$2 AND table_name=$3",&[&source,&schema,&table]).await.map_err(Error::sql)?;
  let next=replay.as_ref().and_then(|v|v.get("source_generation")).and_then(Value::as_i64);
  match (prior.as_ref(),row) {
   (Some(prior),Some(row))=>{
    let applied:i64=row.get(0);let pos:Option<String>=row.get(1);
    let previous:String=prior.get(1);
    if !(applied==base && pos.as_deref()==Some(previous.as_str())
      || next==Some(base+1) && applied==base+1) {return Err(Error::new("unconfirmed_baseline"))}
   }
   (None,None)=>{},
   (None,Some(row)) if next==Some(1) && row.get::<_,i64>(0)==1=>{},
   _=>return Err(Error::new("unconfirmed_baseline")),
  }
 }
 Ok(base)
}
async fn verify_applied(config:&Config,source:&str,database:&str,generation:i64)->Result<()> {
 let target=target_owner(config,database).await?;
 let row=target.query_opt("SELECT source_generation FROM _oo.source_watermark WHERE source=$1",&[&source]).await.map_err(Error::sql)?;
 if row.map(|r|r.get::<_,i64>(0))!=Some(generation) {return Err(Error::new("target_generation"))}
 Ok(())
}
async fn verify_c1_applied(config:&Config,source:&str,database:&str,request:&Value,result:&Value)->Result<()> {
 let target=target_owner(config,database).await?;
 let schema=request.get("logical_schema").and_then(Value::as_str).ok_or_else(||Error::new("position"))?;
 let table=request.get("logical_table").and_then(Value::as_str).ok_or_else(||Error::new("position"))?;
 let generation=request.get("source_generation").and_then(Value::as_i64).ok_or_else(||Error::new("position"))?;
 let pos=request.get("applied_pos").and_then(Value::as_str).ok_or_else(||Error::new("position"))?;
 let codec=request.get("position_codec").and_then(Value::as_str).ok_or_else(||Error::new("position"))?;
 if !["pg-commit-lsn/v1","snapshot-zero/v1"].contains(&codec) {return Err(Error::new("position"))}
 let run=result.get("run_id").and_then(Value::as_str).ok_or_else(||Error::new("target_generation"))?;
 let physical=result.get("physical_name").and_then(Value::as_str).ok_or_else(||Error::new("target_generation"))?;
 let target_generation=result.get("target_generation").and_then(Value::as_i64).ok_or_else(||Error::new("target_generation"))?;
 let digest=result.get("payload_digest").and_then(Value::as_str).ok_or_else(||Error::new("target_generation"))?;
 let batch=request.get("batch_id").and_then(Value::as_str).ok_or_else(||Error::new("target_generation"))?;
 let valid:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.table_freshness f JOIN _oo.source_watermark w ON w.source=f.source JOIN _oo.full_import_run r ON r.run_id=$7::text::uuid JOIN _oo.full_import_table ft ON ft.run_id=r.run_id JOIN _oo.apply_receipts ar ON ar.source=r.source_id AND ar.table_generation=ft.physical_name WHERE f.source=$1 AND f.schema_name=$2 AND f.table_name=$3 AND f.source_generation=$4 AND f.last_pos=$5::text::numeric AND f.physical_name=$8 AND w.position_codec=$6 AND w.applied_pos>=$5::text::numeric AND r.state='awaiting_publish' AND r.source_id=$1 AND r.source_generation=$4 AND r.target_generation=$9 AND ft.schema_ident=$2 AND ft.table_ident=$3 AND ft.physical_name=$8 AND ft.exhausted AND ar.txn_or_batch_id=$10 AND ar.payload_digest=$11 AND ar.applied_pos=$5::text::numeric AND ar.txn_complete)",&[&source,&schema,&table,&generation,&pos,&codec,&run,&physical,&target_generation,&batch,&digest]).await.map_err(Error::sql)?.get(0);
 if !valid {return Err(Error::new("target_generation"))} Ok(())
}
async fn verify_group_applied(config:&Config,source:&str,database:&str,request:&Value,result:&Value)->Result<()> {
 let scope=request.get("transaction_scope").ok_or_else(||Error::new("transaction_scope"))?;
 let end=pg_lsn_decimal(scope.get("end_lsn").and_then(Value::as_str).ok_or_else(||Error::new("transaction_scope"))?)?;
 let generation=request.get("source_generation").and_then(Value::as_i64).ok_or_else(||Error::new("source_generation"))?;
 let target_generation=result.get("target_generation").and_then(Value::as_i64).ok_or_else(||Error::new("target_generation"))?;
 let entries=request.get("entries").and_then(Value::as_array).ok_or_else(||Error::new("transaction_request"))?;
 let receipts=result.get("entries").and_then(Value::as_array).ok_or_else(||Error::new("target_generation"))?;
 if result.get("end_lsn").and_then(Value::as_str)!=Some(end.as_str()) || result.get("xid")!=scope.get("xid") || entries.len()!=receipts.len() {return Err(Error::new("target_generation"))}
 let target=target_owner(config,database).await?;
 let xid=scope.get("xid").and_then(Value::as_i64).ok_or_else(||Error::new("transaction_scope"))?;
 let attestation=target.query_one("SELECT _oo.native_transaction_read($1,$2,$3::text::numeric)::text",&[&source,&xid,&end]).await.map_err(Error::sql)?.get::<_,Option<String>>(0).ok_or_else(||Error::new("transaction_attestation"))?;
 let attestation:Value=serde_json::from_str(&attestation).map_err(|_|Error::new("transaction_attestation"))?;
 if attestation.get("source").and_then(Value::as_str)!=Some(source)
  || attestation.get("xid").and_then(Value::as_i64)!=Some(xid)
  || attestation.get("end_lsn").and_then(Value::as_u64)!=end.parse::<u64>().ok()
  || attestation.get("scope_digest")!=result.get("scope_digest")
  || attestation.get("job_task_id")!=result.get("job_task_id")
  || attestation.get("target_generation").and_then(Value::as_i64)!=Some(target_generation)
  || attestation.get("entries")!=result.get("entries") {return Err(Error::new("transaction_attestation"))}
 let watermark:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1 AND source_generation=$2 AND applied_pos=$3::text::numeric AND target_generation=$4 AND position_codec='pg-commit-lsn/v1' AND consistency='transactional' AND consistency_grade='T' AND cross_table_consistent)",&[&source,&generation,&end,&target_generation]).await.map_err(Error::sql)?.get(0);
 if !watermark {return Err(Error::new("target_generation"))}
 for (entry,receipt) in entries.iter().zip(receipts) {
  let schema=entry.get("logical_schema").and_then(Value::as_str).ok_or_else(||Error::new("transaction_request"))?;
  let table=entry.get("logical_table").and_then(Value::as_str).ok_or_else(||Error::new("transaction_request"))?;
  let table_generation=entry.get("source_generation").and_then(Value::as_i64).ok_or_else(||Error::new("transaction_request"))?;
  let batch=entry.get("batch_id").and_then(Value::as_str).ok_or_else(||Error::new("transaction_request"))?;
  let physical=receipt.get("physical_name").and_then(Value::as_str).ok_or_else(||Error::new("target_generation"))?;
  let run=receipt.get("run_id").and_then(Value::as_str).ok_or_else(||Error::new("target_generation"))?;
  let digest=receipt.get("payload_digest").and_then(Value::as_str).ok_or_else(||Error::new("target_generation"))?;
  if receipt.get("logical_schema").and_then(Value::as_str)!=Some(schema)
   || receipt.get("logical_table").and_then(Value::as_str)!=Some(table)
   || receipt.get("source_generation").and_then(Value::as_i64)!=Some(table_generation)
   || receipt.get("target_generation").and_then(Value::as_i64)!=Some(target_generation)
   || receipt.get("batch_id").and_then(Value::as_str)!=Some(batch) {return Err(Error::new("target_generation"))}
  let applied:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM _oo.table_freshness f JOIN _oo.full_import_run r ON r.run_id=$6::text::uuid JOIN _oo.full_import_table ft ON ft.run_id=r.run_id JOIN _oo.apply_receipts ar ON ar.source=r.source_id AND ar.table_generation=ft.physical_name WHERE f.source=$1 AND f.schema_name=$2 AND f.table_name=$3 AND f.source_generation=$4 AND f.last_pos=$5::text::numeric AND f.physical_name=$7 AND r.state='awaiting_publish' AND r.source_id=$1 AND r.source_generation=$4 AND r.target_generation=$8 AND ft.schema_ident=$2 AND ft.table_ident=$3 AND ft.physical_name=$7 AND ft.exhausted AND ar.txn_or_batch_id=$9 AND ar.payload_digest=$10 AND ar.applied_pos=$5::text::numeric AND ar.txn_complete)",&[&source,&schema,&table,&table_generation,&end,&run,&physical,&target_generation,&batch,&digest]).await.map_err(Error::sql)?.get(0);
  if !applied {return Err(Error::new("target_generation"))}
 }
 Ok(())
}
async fn adopt_claim(control:&tokio_postgres::Client,source:&str,database:&str,project:&str,task:&str,worker:&str,generation:i64)->Result<()> {
 // Upgrade old queued occurrences only after a real jobs.claim. This records
 // the active fence, never a source generation, ACK, or substitute request.
 let n=control.execute("INSERT INTO jw_native.source_progress(source_id,target_database,active_task_id) SELECT $1,$2,t.id FROM jobs.tasks t WHERE t.id=$3::text::uuid AND t.project_id=$4::text::uuid AND t.state='running' AND t.lease_owner=$5 AND t.generation=$6 AND t.lease_until>clock_timestamp() ON CONFLICT(source_id,target_database) DO UPDATE SET active_task_id=EXCLUDED.active_task_id WHERE jw_native.source_progress.active_task_id IS NULL OR jw_native.source_progress.active_task_id=EXCLUDED.active_task_id",&[&source,&database,&task,&project,&worker,&generation]).await.map_err(Error::sql)?;
 if n!=1 {return Err(Error::new("source_active"))} Ok(())
}
async fn release_terminal_native(config:&Config,project:&str)->Result<()> {
 let control=session::runtime(config).await?;
 let rows=control.query("SELECT p.source_id,p.target_database,p.active_task_id::text,o.request::text,b.method FROM jw_native.source_progress p JOIN jobs.tasks t ON t.id=p.active_task_id JOIN jw_native.occurrences o ON o.task_id=t.id JOIN jw_native.bindings b ON b.id=o.binding_id WHERE b.project_id=$1::text::uuid AND b.method IN ('C4','C5') AND t.state IN ('failed','cancelled') AND NOT EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=t.id)",&[&project]).await.map_err(Error::sql)?;
 for row in rows {
  let source:String=row.get(0);let database:String=row.get(1);let task:String=row.get(2);let stored:Option<String>=row.get(3);
  let method:String=row.get(4);
  if database!=target_database(config,&method)? {continue}
  let base=match confirmed_generation(config,&control,&source,&database,&task,&stored).await {
   Ok(base)=>base,Err(e)=>{eprintln!("terminal_release=refused task={task} reason={}",e.kind);continue}
  };
  let target=match target_owner(config,&database).await {
   Ok(target)=>target,Err(e)=>{eprintln!("terminal_release=refused task={task} reason={}",e.kind);continue}
  };
  let applied=match target.query_opt("SELECT source_generation FROM _oo.source_watermark WHERE source=$1",&[&source]).await {
   Ok(row)=>row.map(|r|r.get::<_,i64>(0)).unwrap_or(0),Err(e)=>{eprintln!("terminal_release=refused task={task} reason={}",Error::sql(e).kind);continue}
  };
  let sealed:bool=match target.query_one("SELECT CASE WHEN $1='C4' THEN EXISTS(SELECT 1 FROM _oo.native_partition_state WHERE job_task_id=$2::text::uuid) ELSE EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=$3 AND source_generation>$4) END",&[&method,&task,&source,&base]).await {
   Ok(row)=>row.get(0),Err(e)=>{eprintln!("terminal_release=refused task={task} reason={}",Error::sql(e).kind);continue}
  };
  if applied!=base || sealed {continue}
  let n=control.execute("UPDATE jw_native.source_progress p SET active_task_id=NULL WHERE p.source_id=$1 AND p.target_database=$2 AND p.active_task_id=$3::text::uuid AND EXISTS(SELECT 1 FROM jobs.tasks t WHERE t.id=p.active_task_id AND t.state IN ('failed','cancelled')) AND NOT EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=$3::text::uuid)",&[&source,&database,&task]).await.map_err(Error::sql)?;
  if n==1 {println!("source_active=released_terminal task={task} source={source}")}
 }
 Ok(())
}
pub async fn cancel_native(config:&Config,project:&str,task:&str,worker:&str,generation:i64)->Result<()> {
 if !crate::api::is_uuid(project) || !crate::api::is_uuid(task) || generation<0 {return Err(Error::new("input"))}
 let control=session::runtime(config).await?;
 let row=control.query_opt("SELECT b.source_id,b.target_database,o.request::text,b.method FROM jw_native.occurrences o JOIN jw_native.bindings b ON b.id=o.binding_id JOIN jobs.tasks t ON t.id=o.task_id JOIN jw_native.source_progress p ON p.source_id=b.source_id AND p.target_database=b.target_database AND p.active_task_id=t.id WHERE t.id=$1::text::uuid AND t.project_id=$2::text::uuid AND (b.method='C4' AND t.state IN ('queued','running') OR b.method='C5' AND t.state='queued') AND t.generation=$3 AND NOT EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=t.id)",&[&task,&project,&generation]).await.map_err(Error::sql)?.ok_or_else(||Error::new("cancel_identity"))?;
 let source:String=row.get(0);let database:String=row.get(1);let stored:Option<String>=row.get(2);let method:String=row.get(3);
 if database!=target_database(config,&method)? {return Err(Error::new("target_database"))}
 let base=confirmed_generation(config,&control,&source,&database,task,&stored).await?;
 let target=target_owner(config,&database).await?;
 let applied=target.query_opt("SELECT source_generation FROM _oo.source_watermark WHERE source=$1",&[&source]).await.map_err(Error::sql)?.map(|r|r.get::<_,i64>(0)).unwrap_or(0);
 let sealed:bool=target.query_one("SELECT CASE WHEN $1='C4' THEN EXISTS(SELECT 1 FROM _oo.native_partition_state WHERE job_task_id=$2::text::uuid) ELSE EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=$3 AND source_generation>$4) END",&[&method,&task,&source,&base]).await.map_err(Error::sql)?.get(0);
 if applied!=base || sealed {return Err(Error::new("cancel_target"))}
 let cancelled=lease::cancel(&control,project,task,worker,generation).await?;
 if cancelled.status!=LeaseStatus::Cancelled {return Err(Error::new("cancel_lease"))}
 release_terminal_native(config,project).await?;
 println!("native_cancel=cancelled task={task} generation={generation}");Ok(())
}
pub async fn install(config:&Config)->Result<()> {
 let c=session::admin_db(config,&config.database).await?;
 c.batch_execute(include_str!("../native_schedule.sql")).await.map_err(Error::sql)?;
 println!("native_schedule_schema=installed database={}",config.database);Ok(())
}
pub async fn put(config:&Config,path:&str)->Result<()> {
 let m=manifest_read(path)?; let (text,hash)=canonical(&m)?; let c=session::runtime(config).await?;
 let target_database=target_database(config,&m.method)?;
 let changed=c.execute("INSERT INTO jw_native.bindings(id,project_id,source_id,method,cycle,period_ms,manifest_sha,manifest,target_database) VALUES($1::text::uuid,$2::text::uuid,$3,$4,$5,$6,$7,$8::text::jsonb,$9) ON CONFLICT(id) DO UPDATE SET period_ms=EXCLUDED.period_ms,enabled=true,manifest_sha=EXCLUDED.manifest_sha,manifest=EXCLUDED.manifest,target_database=EXCLUDED.target_database WHERE jw_native.bindings.project_id=EXCLUDED.project_id AND jw_native.bindings.source_id=EXCLUDED.source_id AND jw_native.bindings.cycle=EXCLUDED.cycle AND COALESCE(jw_native.bindings.target_database,EXCLUDED.target_database)=EXCLUDED.target_database", &[&m.binding,&m.project_id,&m.source_id,&m.method,&m.cycle,&m.period_ms,&hash,&text,&target_database]).await.map_err(Error::sql)?;
 if changed!=1 {return Err(Error::new("binding_identity"))}
 println!("schedule=registered binding={} method={} cycle={} period_ms={} manifest_sha={}",m.binding,m.method,m.cycle,m.period_ms,hash);Ok(())
}
pub async fn enabled(config:&Config,binding:&str,value:bool)->Result<()> {
 if !crate::api::is_uuid(binding){return Err(Error::new("binding"))}
 let c=session::runtime(config).await?;
 let n=c.execute("UPDATE jw_native.bindings SET enabled=$2 WHERE id=$1::text::uuid",&[&binding,&value]).await.map_err(Error::sql)?;
 println!("schedule_enabled={value} rows={n}");Ok(())
}
pub async fn fire(config:&Config,project:&str)->Result<()> {
 release_terminal_native(config,project).await?;
 let mut c=session::runtime(config).await?;let tx=c.transaction().await.map_err(Error::sql)?;
 let rows=tx.query("SELECT id::text FROM jw_native.bindings WHERE project_id=$1::text::uuid AND enabled AND target_database IS NOT NULL AND next_run_at<=clock_timestamp() ORDER BY next_run_at,id",&[&project]).await.map_err(Error::sql)?;
 for row in rows {
  let binding:String=row.get(0);
  let Some(row)=tx.query_opt("SELECT pin,manifest_sha,period_ms,next_run_at::text,manifest::text,source_id,target_database FROM jw_native.bindings WHERE id=$1::text::uuid AND project_id=$2::text::uuid AND enabled AND next_run_at<=clock_timestamp() FOR UPDATE SKIP LOCKED",&[&binding,&project]).await.map_err(Error::sql)? else {continue};
  let pin:i64=row.get::<_,i64>(0)+1;let hash:String=row.get(1);let period:i64=row.get(2);let at:String=row.get(3);let manifest:String=row.get(4);let source:String=row.get(5);let target_database:String=row.get(6);
  tx.execute("INSERT INTO jw_native.source_progress(source_id,target_database) VALUES($1,$2) ON CONFLICT DO NOTHING",&[&source,&target_database]).await.map_err(Error::sql)?;
  let gate=tx.query_opt("SELECT active_task_id::text FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2 FOR UPDATE SKIP LOCKED",&[&source,&target_database]).await.map_err(Error::sql)?;
  let Some(gate)=gate else {continue};
  if gate.get::<_,Option<String>>(0).is_some() {continue}
  let older=tx.query_one("SELECT EXISTS(SELECT 1 FROM jw_native.bindings b JOIN jw_native.occurrences o ON o.binding_id=b.id JOIN jobs.tasks t ON t.id=o.task_id WHERE b.source_id=$1 AND b.target_database=$2 AND (t.state IN ('queued','running') OR EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=t.id AND a.acknowledged_at IS NULL)))",&[&source,&target_database]).await.map_err(Error::sql)?;
  if older.get::<_,bool>(0) {continue}
  let payload=crate::capture_plan::CapturePayload::text(&binding,pin);let digest=sha256_hex(payload.as_bytes());
  let dedupe=format!("native/{binding}/{pin}");
  let r=lease::enqueue(&tx,lease::Enqueue{project_id:project,kind:KIND,payload_version:1,payload:&payload,config_version:"jw-native-1",digest:&digest,dedupe_key:&dedupe,run_delay_seconds:0.,max_attempts:8}).await?;
  let task=r.task_id.ok_or_else(||Error::new("enqueue"))?;
  tx.execute("INSERT INTO jw_native.occurrences(binding_id,pin,task_id,manifest_sha,scheduled_at,manifest) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5::text::timestamptz,$6::text::jsonb)",&[&binding,&pin,&task,&hash,&at,&manifest]).await.map_err(Error::sql)?;
  tx.execute("UPDATE jw_native.source_progress SET active_task_id=$3::text::uuid WHERE source_id=$1 AND target_database=$2 AND active_task_id IS NULL",&[&source,&target_database,&task]).await.map_err(Error::sql)?;
  tx.execute("UPDATE jw_native.bindings SET pin=$2,next_run_at=clock_timestamp()+$3::bigint*interval '1 millisecond' WHERE id=$1::text::uuid",&[&binding,&pin,&period]).await.map_err(Error::sql)?;
  println!("fire=enqueued binding={binding} pin={pin} task={task} period_ms={period}");
 }
 tx.commit().await.map_err(Error::sql)?;Ok(())
}
// Drain pipes concurrently: large producer/loader outputs must never deadlock heartbeat.
#[allow(clippy::too_many_arguments)]
async fn exec(config:&Config,e:&Executable,stage:&str,input:Vec<u8>,registration:Option<&Registration>,binding:&str,pin:i64,project:&str,task:&str,worker:&str,generation:i64,target:Option<&tokio_postgres::Client>)->Result<Value> {
 executable_check(e)?;
 let mut command=Command::new(&e.path);
 command.args(&e.args).env_clear().env("PATH","/usr/bin:/bin").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
 if let Some(r)=registration {
  executable_check(&Executable{path:r.admission_binary.clone(),sha256:r.admission_sha256.clone(),args:vec![]})?;
  command.env("OO_NATIVE_ADMISSION_BIN",&r.admission_binary);
  if let Some(path)=&r.registry_path {
   let registry=path.replace("{binding}",binding).replace("{pin}",&pin.to_string());
   let bytes=std::fs::read(&registry).map_err(|_|Error::new("registration"))?;
   if bytes.len()>1024*1024 {return Err(Error::new("registration"))}
   command.env("OO_NATIVE_BINDING_REGISTRY",&registry).env("OO_NATIVE_BINDING_REGISTRY_SHA256",sha256_hex(&bytes));
  }
  if let (Some(path),Some(sha))=(&r.transaction_registry_vector_path,&r.transaction_registry_vector_sha256) {
   let bytes=std::fs::read(path).map_err(|_|Error::new("registration"))?;
   if bytes.len()>1024*1024 || sha256_hex(&bytes)!=*sha {return Err(Error::new("registration"))}
   command.env("OO_NATIVE_TRANSACTION_REGISTRY_VECTOR",path).env("OO_NATIVE_TRANSACTION_REGISTRY_VECTOR_SHA256",sha);
  }
 }
 let mut child=command.spawn().map_err(|_|Error::new("spawn"))?;
 let mut stdin=child.stdin.take().ok_or_else(||Error::new("spawn"))?;
 let writer=tokio::spawn(async move{stdin.write_all(&input).await?;stdin.shutdown().await});
 let stdout=child.stdout.take().ok_or_else(||Error::new("spawn"))?;
 let stderr=child.stderr.take().ok_or_else(||Error::new("spawn"))?;
 let out=tokio::spawn(async move{let mut b=Vec::new();stdout.take(4*1024*1024+1).read_to_end(&mut b).await.map(|_|b)});
 let err=tokio::spawn(async move{let mut b=Vec::new();stderr.take(65537).read_to_end(&mut b).await.map(|_|b)});
 let control=session::runtime(config).await?;
 let status=loop{tokio::select!{
  r=child.wait()=>break r.map_err(|_|Error::new("spawn"))?,
  _=tokio::time::sleep(Duration::from_secs(10))=>{
   let r=lease::heartbeat(&control,project,task,worker,generation).await?;
   if r.status!=LeaseStatus::Extended {let _=child.kill().await;return Err(Error::new("lost_lease"))}
   if let Some(target)=target {
    let r=lease::heartbeat(target,project,task,worker,generation).await?;
    if r.status!=LeaseStatus::Extended {let _=child.kill().await;return Err(Error::new("target_lease"))}
   }
  }
 }};
 writer.await.map_err(|_|Error::new("spawn"))?.map_err(|_|Error::new("spawn"))?;
 let bytes=out.await.map_err(|_|Error::new("spawn"))?.map_err(|_|Error::new("spawn"))?;
 let errors=err.await.map_err(|_|Error::new("spawn"))?.map_err(|_|Error::new("spawn"))?;
 let code=status.code().unwrap_or(-1);
 control.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING",&[&task,&generation,&stage,&e.sha256,&code,&bytes,&errors]).await.map_err(Error::sql)?;
 println!("process_stage={stage} task={task} generation={generation} exit_code={code} stdout_bytes={} stderr_bytes={} raw_receipt=jw_native.process_attempts",bytes.len(),errors.len());
 if !status.success() || bytes.len()>4*1024*1024 || errors.len()>65536 {return Err(Error::new("process"))}
 serde_json::from_slice(&bytes).map_err(|_|Error::new("protocol"))
}
pub async fn step(config:&Config,project:&str,worker:&str)->Result<()> {
 release_terminal_native(config,project).await?;
 let mut c=session::runtime(config).await?;
 let pending=c.query("SELECT a.task_id::text FROM jw_native.pending_acks a JOIN jw_native.bindings b ON b.id=a.binding_id WHERE b.project_id=$1::text::uuid AND a.acknowledged_at IS NULL ORDER BY a.task_id",&[&project]).await.map_err(Error::sql)?;
 let mut ack_error=None;
 let mut acknowledged=false;
 for row in pending {
  let pending_task:String=row.get(0);
  match drain_ack(config,&mut c,project,Some(&pending_task)).await {
   Ok(done)=>acknowledged|=done,
   Err(e)=>{eprintln!("ack_retry_error={} task={pending_task}",e.kind);ack_error=Some(e)},
  }
 }
 if acknowledged {return Ok(())}
 let selected_install=c.query_opt("SELECT checksum,filename FROM jobs.schema_migrations WHERE version='jobs-claim-selected/1'",&[]).await.map_err(Error::sql)?;
 if !selected_install.is_some_and(|r|r.get::<_,String>(0)=="d2daaedcdfec509334df7eda09e5ac6fa277ae2291b16d12293e1deb4270b9d0" && r.get::<_,String>(1)=="0009_claim_selected_expand.sql") {return Err(Error::new("claim_selected_install"))}
 // Select the immutable occurrence before claiming it. An older retry may
 // have a later run_at than its sibling; generic jobs.claim could steal the
 // sibling while the source's active fence still belongs to the retry.
 let candidate=c.query_opt("WITH waiting AS (SELECT t.id,t.project_id,t.run_at,t.state,t.lease_until,p.active_task_id,ROW_NUMBER() OVER (PARTITION BY b.source_id,b.target_database ORDER BY CASE WHEN b.method='C1' THEN (o.request->>'source_generation')::bigint END NULLS LAST,o.scheduled_at,t.id) AS source_order FROM jw_native.occurrences o JOIN jw_native.bindings b ON b.id=o.binding_id JOIN jobs.tasks t ON t.id=o.task_id LEFT JOIN jw_native.source_progress p ON p.source_id=b.source_id AND p.target_database=b.target_database WHERE t.kind='native_capture' AND t.state IN ('queued','running') AND b.target_database IS NOT NULL) SELECT id::text FROM waiting WHERE project_id=$1::text::uuid AND (active_task_id=id OR active_task_id IS NULL AND source_order=1) AND (state='queued' AND run_at<=clock_timestamp() OR state='running' AND lease_until<=clock_timestamp()) ORDER BY run_at,id LIMIT 1",&[&project]).await.map_err(Error::sql)?;
 let Some(candidate)=candidate else {println!("claim=none");return ack_error.map_or(Ok(()),Err)};
 let selected:String=candidate.get(0);
 let r=lease::claim_selected(&c,project,&selected,&[KIND.into()],worker).await?;
 if r.status==LeaseStatus::None {println!("claim=none");return ack_error.map_or(Ok(()),Err)}
 if r.status!=LeaseStatus::Claimed{return Err(Error::new("claim"))}
 let task=r.task_id.ok_or_else(||Error::new("claim"))?;let generation=r.generation.ok_or_else(||Error::new("claim"))?;
 if task!=selected {return Err(Error::new("claim_identity"))}
 println!("claim=claimed task={task} generation={generation} worker={worker}");
 match apply(config,&mut c,project,worker,&task,generation).await {
  Ok(())=>Ok(()),Err(e)=>{
   let r=lease::retry_failure(&c,project,&task,worker,generation,"worker_failed").await?;
   println!("apply=refused reason={} retry={}",e.kind,r.status.as_str());Err(e)
  }
 }
}
async fn apply(config:&Config,c:&mut tokio_postgres::Client,project:&str,worker:&str,task:&str,generation:i64)->Result<()> {
 let row=c.query_one("SELECT o.manifest::text,o.manifest_sha,o.request::text,o.pin,b.id::text,t.payload::text,t.artifact_digest,(SELECT pin FROM jw_native.watermarks WHERE binding_id=b.id),b.target_database,o.producer_input::text FROM jw_native.occurrences o JOIN jw_native.bindings b ON b.id=o.binding_id JOIN jobs.tasks t ON t.id=o.task_id WHERE t.id=$1::text::uuid AND b.project_id=$2::text::uuid",&[&task,&project]).await.map_err(Error::sql)?;
 let text:String=row.get(0);let pinned:String=row.get(1);let stored:Option<String>=row.get(2);let pin:i64=row.get(3);let binding:String=row.get(4);let payload:String=row.get(5);
 let m:Manifest=serde_json::from_str(&text).map_err(|_|Error::new("manifest"))?;m.shape()?;
 let group=m.loader.args==["import-native-transaction"];
 let database:String=row.get::<_,Option<String>>(8).ok_or_else(||Error::new("target_database"))?;
 if database!=target_database(config,&m.method)? {return Err(Error::new("target_database"))}
 let parsed=crate::capture_plan::CapturePayload::parse(&payload)?;
 let artifact:String=row.get(6);
 if canonical(&m)?.1!=pinned || parsed.binding!=binding || parsed.pin!=pin || artifact!=sha256_hex(crate::capture_plan::CapturePayload::text(&binding,pin).as_bytes()){return Err(Error::new("manifest_changed"))}
 adopt_claim(c,&m.source_id,&database,project,task,worker,generation).await?;
 let base=if m.method=="C1" && !group {
  confirmed_c1_generation(config,c,&m.source_id,&database,&binding,task,&stored).await?
 } else {confirmed_generation(config,c,&m.source_id,&database,task,&stored).await?};
 let mut request=match stored {
  Some(s)=>serde_json::from_str(&s).map_err(|_|Error::new("request"))?,
  None=>{
   let acknowledged_pin:Option<i64>=row.get(7);
   let input=match row.get::<_,Option<String>>(9) {
    Some(saved)=>serde_json::from_str::<Value>(&saved).map_err(|_|Error::new("producer_input"))?,
    None=>{
     let fresh=json!({"protocol":"oo.jobs.native-producer/v1","binding":binding,"pin":pin,"source_id":m.source_id,"method":m.method,"cycle":m.cycle,"period_ms":m.period_ms,"acknowledged_source_generation":base,"acknowledged_pin":acknowledged_pin});
     let n=c.execute("UPDATE jw_native.occurrences o SET producer_input=$2::text::jsonb FROM jobs.tasks t WHERE o.task_id=t.id AND t.id=$1::text::uuid AND t.state='running' AND t.lease_owner=$3 AND t.generation=$4 AND t.lease_until>clock_timestamp() AND o.producer_input IS NULL",&[&task,&fresh.to_string(),&worker,&generation]).await.map_err(Error::sql)?;
     if n!=1 {return Err(Error::new("lost_lease"))} fresh
    }
   };
   if input.get("protocol").and_then(Value::as_str)!=Some("oo.jobs.native-producer/v1") || input.get("binding").and_then(Value::as_str)!=Some(&binding) || input.get("pin").and_then(Value::as_i64)!=Some(pin) || input.get("source_id").and_then(Value::as_str)!=Some(&m.source_id) || input.get("method").and_then(Value::as_str)!=Some(&m.method) || input.get("acknowledged_source_generation").and_then(Value::as_i64)!=Some(base) {return Err(Error::new("producer_input"))}
   let value=exec(config,&m.producer,"producer",serde_json::to_vec(&input).unwrap(),None,&binding,pin,project,task,worker,generation,None).await?;
   let protocol=if group {"oo.jobs.native-transaction-producer/v1"} else {request_protocol(&m.method)};
   if value.get("protocol").and_then(Value::as_str)!=Some(protocol) || value.get("source_id").and_then(Value::as_str)!=Some(&m.source_id) || value.get("capture_method").and_then(Value::as_str)!=Some(method_token(&m.method)?){return Err(Error::new("source_binding"))}
   let s=serde_json::to_string(&value).unwrap();
   let n=c.execute("UPDATE jw_native.occurrences o SET request=$2::text::jsonb FROM jobs.tasks t WHERE o.task_id=t.id AND t.id=$1::text::uuid AND t.state='running' AND t.lease_owner=$3 AND t.generation=$4 AND t.lease_until>clock_timestamp() AND o.request IS NULL",&[&task,&s,&worker,&generation]).await.map_err(Error::sql)?;
   if n!=1{return Err(Error::new("lost_lease"))}value
  }
 };
 let protocol=if group {"oo.jobs.native-transaction-producer/v1"} else {request_protocol(&m.method)};
 if request.get("protocol").and_then(Value::as_str)!=Some(protocol) || request.get("source_id").and_then(Value::as_str)!=Some(&m.source_id) || request.get("capture_method").and_then(Value::as_str)!=Some(method_token(&m.method)?){return Err(Error::new("source_binding"))}
 if request.get("source_generation").and_then(Value::as_i64)!=Some(base+1) {return Err(Error::new("source_generation"))}
 let target_database=if m.method=="C4" {config.target_database.as_deref().ok_or_else(||Error::new("target_database"))?} else {&config.database};
 let loader_request=if group {
  let value=group_loader_request(&request,&binding,pin,&m.source_id,base,task,generation)?;
  if value["entries"].as_array().is_none_or(|entries|entries.iter().any(|entry|entry.pointer("/target/database").and_then(Value::as_str)!=Some(target_database))) {return Err(Error::new("target_database"))}
  value
 } else {
  if request.pointer("/target/database").and_then(Value::as_str)!=Some(target_database){return Err(Error::new("target_database"))}
  crate::capture_source::attach_observed_lease(&mut request,task,generation).map_err(|_|Error::new("lease"))?;
  request.clone()
 };
 let override_row=c.query_opt("SELECT operator_manifest::text,operator_manifest_sha FROM jw_native.loader_overrides WHERE binding_id=$1::text::uuid",&[&binding]).await.map_err(Error::sql)?;
 let effective=match override_row {
  Some(row)=>{
   let text:String=row.get(0);let sha:String=row.get(1);
   let selected:Manifest=serde_json::from_str(&text).map_err(|_|Error::new("loader_override"))?;selected.shape()?;
   if canonical(&selected)?.1!=sha || selected.binding!=m.binding || selected.project_id!=m.project_id || selected.source_id!=m.source_id || selected.method!=m.method || selected.cycle!=m.cycle {return Err(Error::new("loader_override"))}
   println!("loader_operator_override={sha} original_occurrence_manifest={pinned} source_request_rewritten=false");selected
  }
  None=>m.clone(),
 };
 // mirror is an invocation-only policy, excluded from the native source payload digest.
 // Preserve original stored request; require actual Loader mirror recovery before CAS.
 let mut loader_request=loader_request;
 if !group {invocation_mirror(&mut loader_request,&m.method)?}
 println!("invocation_mirror={} source_request_rewritten=false",!group && m.method!="C4");
 let target=if m.method=="C4" {Some(mirror_claim(config,c,project,task,worker,generation,&effective.loader).await?)} else {None};
 let result=exec(config,&effective.loader,"loader",serde_json::to_vec(&loader_request).unwrap(),effective.registration.as_ref(),&binding,pin,project,task,worker,generation,target.as_ref()).await?;
 let lease_id=result.pointer("/jobs_lease_receipt/id").and_then(Value::as_str);
 let lease_gen=result.pointer("/jobs_lease_receipt/generation").and_then(Value::as_i64);
 let lease_owner=result.pointer("/jobs_lease_receipt/owner").and_then(Value::as_str);
 if lease_id!=Some(task)||lease_gen!=Some(generation)||lease_owner!=Some(worker){return Err(Error::new("lease_result"))}
 let c4_ready=m.method=="C4" && result.get("source_id").and_then(Value::as_str)==Some(&m.source_id)
  && result.get("source_bound").and_then(Value::as_bool)==Some(true)
  && crate::capture_source::producer_trust_authoritative(&result)
  && crate::capture_source::fence_held(&result)
  && matches!(result.get("load_state").and_then(Value::as_str),Some("awaiting_publish"|"replay"))
  && (result.get("mirror_committed").and_then(Value::as_bool)==Some(false)
    || result.get("load_state").and_then(Value::as_str)==Some("replay")
     && result.get("receipt").and_then(Value::as_str)==Some("replay")
     && result.get("mirror_committed").is_none())
  && result.pointer("/partition_target_receipt/acknowledged").and_then(Value::as_bool)==Some(false);
 if m.method=="C4" && !c4_ready {return Err(Error::new("loader_refused"))}
 if group {
  if result.get("protocol").and_then(Value::as_str)!=Some("oo.pipeline.native-transaction-result/v1")
   || result.get("source_id").and_then(Value::as_str)!=Some(&m.source_id)
   || result.get("source_generation").and_then(Value::as_i64)!=request.get("source_generation").and_then(Value::as_i64)
   || result.get("source_bound").and_then(Value::as_bool)!=Some(true)
   || result.get("job_task_id").and_then(Value::as_str)!=Some(task)
   || result.get("job_generation").and_then(Value::as_i64)!=Some(generation)
   || !matches!(result.get("load_state").and_then(Value::as_str),Some("awaiting_publish"|"replay")) {return Err(Error::new("loader_refused"))}
 } else if m.method!="C4" && !matches!(crate::capture_source::native_decision(&result),crate::capture_source::NativeDecision::Advance|crate::capture_source::NativeDecision::Replay){return Err(Error::new("loader_refused"))}
 let source_generation=request.get("source_generation").and_then(Value::as_i64).ok_or_else(||Error::new("position"))?;
 let raw_pos=if group {result.get("end_lsn")} else if m.method=="C4" {result.pointer("/partition_target_receipt/partition_scope/partition_id")} else {request.get("applied_pos")}.ok_or_else(||Error::new("position"))?;
 let pos=raw_pos.as_str().map(str::to_owned).unwrap_or_else(||raw_pos.to_string());
 let state=result.get("load_state").and_then(Value::as_str).unwrap_or("").to_string();let result_text=result.to_string();
 if group {verify_group_applied(config,&m.source_id,&database,&request,&result).await?}
 else if m.method=="C1" {verify_c1_applied(config,&m.source_id,&database,&request,&result).await?}
 else {verify_applied(config,&m.source_id,&database,source_generation).await?}
 let tx=c.transaction().await.map_err(Error::sql)?;
 if m.method=="C4" {
  let ack=ack_request(&result,task,generation)?;
  tx.execute("INSERT INTO jw_native.pending_acks(task_id,binding_id,pin,generation,ack_request,loader_path,loader_sha256) VALUES($1::text::uuid,$2::text::uuid,$3,$4,$5::text::jsonb,$6,$7)",&[&task,&binding,&pin,&generation,&ack.to_string(),&effective.loader.path,&effective.loader.sha256]).await.map_err(Error::sql)?;
  tx.execute("UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",&[&task,&result_text]).await.map_err(Error::sql)?;
  let done=lease::complete_success(&tx,project,task,worker,generation).await?;
  if done.status!=LeaseStatus::Succeeded{return Err(Error::new("lost_lease"))}
  tx.commit().await.map_err(Error::sql)?;
  println!("complete=cas_committed_ack_pending binding={binding} pin={pin} task={task} generation={generation}");
  return drain_ack(config,c,project,Some(task)).await.and_then(|ok|if ok {Ok(())} else {Err(Error::new("ack_pending"))});
 }
 let done=lease::complete_success(&tx,project,task,worker,generation).await?;
 if done.status!=LeaseStatus::Succeeded{return Err(Error::new("lost_lease"))}
 tx.execute("INSERT INTO jw_native.watermarks(binding_id,pin,task_id,generation,source_generation,applied_pos,load_state,result) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5,$6,$7,$8::text::jsonb) ON CONFLICT(binding_id) DO UPDATE SET pin=EXCLUDED.pin,task_id=EXCLUDED.task_id,generation=EXCLUDED.generation,source_generation=EXCLUDED.source_generation,applied_pos=EXCLUDED.applied_pos,load_state=EXCLUDED.load_state,result=EXCLUDED.result,advanced_at=clock_timestamp() WHERE jw_native.watermarks.pin<EXCLUDED.pin",&[&binding,&pin,&task,&generation,&source_generation,&pos,&state,&result_text]).await.map_err(Error::sql)?;
 tx.execute("UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",&[&task,&result_text]).await.map_err(Error::sql)?;
 let n=if m.method=="C1" && !group {
  tx.execute("UPDATE jw_native.source_progress SET active_task_id=NULL WHERE source_id=$1 AND target_database=$2 AND active_task_id=$3::text::uuid",&[&m.source_id,&database,&task]).await.map_err(Error::sql)?
 } else {
  tx.execute("UPDATE jw_native.source_progress SET source_generation=$3,task_id=$4::text::uuid,binding_id=$5::text::uuid,active_task_id=NULL,confirmed_at=clock_timestamp() WHERE source_id=$1 AND target_database=$2 AND COALESCE(source_generation,0)=$6 AND active_task_id=$4::text::uuid",&[&m.source_id,&database,&source_generation,&task,&binding,&base]).await.map_err(Error::sql)?
 };
 if n!=1 {return Err(Error::new("source_active"))}
 tx.commit().await.map_err(Error::sql)?;
 println!("complete=succeeded binding={binding} pin={pin} task={task} generation={generation} load_state={state} applied_pos={pos}");Ok(())
}
async fn mirror_claim(config:&Config,control:&tokio_postgres::Client,project:&str,task:&str,worker:&str,generation:i64,loader:&Executable)->Result<tokio_postgres::Client> {
 let row=control.query_opt("SELECT row_to_json(t)::text FROM jobs.tasks t WHERE id=$1::text::uuid AND project_id=$2::text::uuid AND state='running' AND lease_owner=$3 AND generation=$4 AND lease_until>clock_timestamp()",&[&task,&project,&worker,&generation]).await.map_err(Error::sql)?.ok_or_else(||Error::new("lost_lease"))?;
 let task_json:String=row.get(0);
 let expected:Value=serde_json::from_str(&task_json).map_err(|_|Error::new("target_lease"))?;
 let attempt=expected.get("attempt").and_then(Value::as_i64).ok_or_else(||Error::new("target_lease"))?;
 let target=session::target_runtime(config).await?;
 let markers=target.query("SELECT version,checksum,filename FROM jobs.schema_migrations WHERE version IN ('jobs-mirror-rebind/1','jobs-mirror-retry-run-at/1','jobs-mirror-sync/1','jobs-mirror-read/1')",&[]).await.map_err(Error::sql)?;
 if markers.len()!=4 || !markers.iter().any(|r|r.get::<_,String>(0)=="jobs-mirror-rebind/1" && r.get::<_,String>(1)=="1b4b7ddf2c3539913764ea4f680d3e1740b6be9fb562c104ac165dae2d20ad37" && r.get::<_,String>(2)=="0005_mirror_rebind_expand.sql")
  || !markers.iter().any(|r|r.get::<_,String>(0)=="jobs-mirror-retry-run-at/1" && r.get::<_,String>(1)=="1b4a31486956019305f9f563798141e4764156db87aaad4b6b3f5fadb8fdb6dc" && r.get::<_,String>(2)=="0006_mirror_retry_run_at_expand.sql")
  || !markers.iter().any(|r|r.get::<_,String>(0)=="jobs-mirror-sync/1" && r.get::<_,String>(1)=="d391d7c30fc086afeac8d95edc967828eb3e1d4fbe8b67eff295f76945bbd368" && r.get::<_,String>(2)=="0007_verified_mirror_sync_expand.sql")
  || !markers.iter().any(|r|r.get::<_,String>(0)=="jobs-mirror-read/1" && r.get::<_,String>(1)=="e4ff243fabaa3bf7ba9947026db7b0af62a4a036b79002b8e4075cea3257442c" && r.get::<_,String>(2)=="0008_native_mirror_read_grants_expand.sql") {return Err(Error::new("mirror_install"))}
 for second in 0..40 {
  let old=target.query_opt("SELECT state,generation,attempt,lease_until<=clock_timestamp() FROM jobs.tasks WHERE id=$1::text::uuid",&[&task]).await.map_err(Error::sql)?;
  let Some(old)=old else {break};
  let old_state:String=old.get(0);let old_gen:i64=old.get(1);let old_attempt:i32=old.get(2);let expired:Option<bool>=old.get(3);
  if old_gen==generation || old_state=="running" && old_gen<generation && i64::from(old_attempt)<attempt && expired==Some(true) {break}
  if old_state!="running" || old_gen>=generation || i64::from(old_attempt)>=attempt {return Err(Error::new("target_lease"))}
  if second%8==0 && lease::heartbeat(control,project,task,worker,generation).await?.status!=LeaseStatus::Extended {return Err(Error::new("lost_lease"))}
  tokio::time::sleep(Duration::from_secs(1)).await;
 }
 sync_mirror(control,loader,project,task,worker,generation).await?;
 let matched:bool=target.query_one("SELECT EXISTS(SELECT 1 FROM jobs.tasks t WHERE id=$1::text::uuid AND state='running' AND lease_until>clock_timestamp() AND to_jsonb(t)-'updated_at'-'lease_until'=$2::text::jsonb-'updated_at'-'lease_until')",&[&task,&task_json]).await.map_err(Error::sql)?.get(0);
 if !matched{return Err(Error::new("target_lease"))}
 let beat=lease::heartbeat(&target,project,task,worker,generation).await?;
 if beat.status!=LeaseStatus::Extended{return Err(Error::new("target_lease"))}
 println!("target_claim=mirrored task={task} generation={generation}");Ok(target)
}
async fn sync_mirror(control:&tokio_postgres::Client,loader:&Executable,project:&str,task:&str,worker:&str,generation:i64)->Result<()> {
 executable_check(loader)?;
 let body=json!({"protocol":"oo.pipeline.native-task-mirror/v1","job_task_id":task});
 let mut command=Command::new(&loader.path);
 command.arg("sync-native-task-mirror").env_clear().env("PATH","/usr/bin:/bin")
  .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
 for (path_var,sha_var) in [("OO_NATIVE_CONTROL_TARGET","OO_NATIVE_CONTROL_TARGET_SHA256"),("OO_NATIVE_MIRROR_TARGET","OO_NATIVE_MIRROR_TARGET_SHA256")] {
  let path=std::env::var(path_var).map_err(|_|Error::new("mirror_target"))?;
  let digest=std::env::var(sha_var).map_err(|_|Error::new("mirror_target"))?;
  if !Path::new(&path).is_absolute() || path.contains("..") || digest.len()!=64 || !digest.bytes().all(|b|b.is_ascii_hexdigit())
   || sha256_hex(&std::fs::read(&path).map_err(|_|Error::new("mirror_target"))?)!=digest {return Err(Error::new("mirror_target"))}
  command.env(path_var,path).env(sha_var,digest);
 }
 let mut child=command.spawn().map_err(|_|Error::new("spawn"))?;
 let mut stdin=child.stdin.take().ok_or_else(||Error::new("spawn"))?;
 stdin.write_all(body.to_string().as_bytes()).await.map_err(|_|Error::new("spawn"))?;
 stdin.shutdown().await.map_err(|_|Error::new("spawn"))?;drop(stdin);
 if lease::heartbeat(control,project,task,worker,generation).await?.status!=LeaseStatus::Extended {return Err(Error::new("lost_lease"))}
 let output=tokio::time::timeout(Duration::from_secs(20),child.wait_with_output()).await.map_err(|_|Error::new("mirror_timeout"))?.map_err(|_|Error::new("spawn"))?;
 let response:Value=serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
 println!("process_stage=mirror_sync task={task} generation={generation} exit_code={} stdout_bytes={} stderr_bytes={}",output.status.code().unwrap_or(-1),output.stdout.len(),output.stderr.len());
 if !output.status.success() || response.get("protocol").and_then(Value::as_str)!=Some("oo.pipeline.native-task-mirror-result/v1")
  || response.get("mirrored").and_then(Value::as_bool)!=Some(true)
  || response.get("job_generation").and_then(Value::as_i64)!=Some(generation) {return Err(Error::new("mirror_refused"))}
 Ok(())
}
fn ack_request(result:&Value,task:&str,generation:i64)->Result<Value> {
 let receipt=result.get("partition_target_receipt").ok_or_else(||Error::new("ack_receipt"))?;
 let scope=receipt.get("partition_scope").ok_or_else(||Error::new("ack_receipt"))?;
 let string=|value:Option<&Value>|value.and_then(Value::as_str).filter(|s|!s.is_empty()).map(str::to_owned).ok_or_else(||Error::new("ack_receipt"));
 if receipt.get("acknowledged").and_then(Value::as_bool)!=Some(false)
  || receipt.get("job_task_id").and_then(Value::as_str)!=Some(task)
  || receipt.get("job_generation").and_then(Value::as_i64)!=Some(generation)
  || scope.get("source_id").and_then(Value::as_str)!=result.get("source_id").and_then(Value::as_str)
  || scope.get("logical_schema")!=result.get("logical_schema")
  || scope.get("logical_table")!=result.get("logical_table")
  || receipt.get("payload_digest")!=result.get("payload_digest")
  || result.pointer("/jobs_lease_receipt/state").and_then(Value::as_str)!=Some("running") {return Err(Error::new("ack_receipt"))}
 Ok(json!({"protocol":"oo.pipeline.native-partition-ack/v1",
  "source_id":string(result.get("source_id"))?,
  "logical_schema":string(scope.get("logical_schema"))?,
  "logical_table":string(scope.get("logical_table"))?,
  "partition_id":string(scope.get("partition_id"))?,
  "run_id":string(result.get("run_id"))?,
  "batch_id":string(receipt.get("batch_id"))?,
  "payload_digest":string(receipt.get("payload_digest"))?,
  "job_task_id":task,"job_generation":generation}))
}
async fn drain_ack(config:&Config,c:&mut tokio_postgres::Client,project:&str,only_task:Option<&str>)->Result<bool> {
 let tx=c.transaction().await.map_err(Error::sql)?;
 let row=tx.query_opt("SELECT a.task_id::text,a.binding_id::text,a.pin,a.generation,a.ack_request::text,a.loader_path,a.loader_sha256,o.request::text,o.result::text,t.state,t.generation,b.source_id,b.target_database FROM jw_native.pending_acks a JOIN jw_native.bindings b ON b.id=a.binding_id JOIN jw_native.occurrences o ON o.task_id=a.task_id JOIN jobs.tasks t ON t.id=a.task_id WHERE b.project_id=$1::text::uuid AND a.acknowledged_at IS NULL AND ($2::text IS NULL OR a.task_id=$2::text::uuid) ORDER BY o.scheduled_at FOR UPDATE OF a SKIP LOCKED LIMIT 1",&[&project,&only_task]).await.map_err(Error::sql)?;
 let Some(row)=row else{return Ok(false)};
 let task:String=row.get(0);let binding:String=row.get(1);let pin:i64=row.get(2);let generation:i64=row.get(3);
 let ack_text:String=row.get(4);let path:String=row.get(5);let sha:String=row.get(6);
 let request_text:String=row.get(7);let result_text:String=row.get(8);let state:String=row.get(9);let current_generation:i64=row.get(10);
 let source:String=row.get(11);let database:String=row.get::<_,Option<String>>(12).unwrap_or_else(||config.target_database.clone().unwrap_or_default());
 if database!=target_database(config,"C4")? {return Err(Error::new("target_database"))}
 if state!="succeeded" || current_generation!=generation {return Err(Error::new("jobs_cas_absent"))}
 let request:Value=serde_json::from_str(&request_text).map_err(|_|Error::new("request"))?;
 let result:Value=serde_json::from_str(&result_text).map_err(|_|Error::new("ack_receipt"))?;
 let expected=ack_request(&result,&task,generation)?;
 let ack:Value=serde_json::from_str(&ack_text).map_err(|_|Error::new("ack_receipt"))?;
 if ack!=expected {return Err(Error::new("ack_receipt"))}
 let executable=Executable{path,sha256:sha,args:vec!["ack-native-partition".into()]};executable_check(&executable)?;
 let mut command=Command::new(&executable.path);
 command.arg("ack-native-partition").env_clear().env("PATH","/usr/bin:/bin")
  .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
 for (path_var,sha_var) in [("OO_NATIVE_CONTROL_TARGET","OO_NATIVE_CONTROL_TARGET_SHA256"),("OO_NATIVE_ACK_TARGET","OO_NATIVE_ACK_TARGET_SHA256")] {
  let path=std::env::var(path_var).map_err(|_|Error::new("ack_target"))?;
  let digest=std::env::var(sha_var).map_err(|_|Error::new("ack_target"))?;
  if !Path::new(&path).is_absolute() || path.contains("..") || digest.len()!=64 || !digest.bytes().all(|b|b.is_ascii_hexdigit())
   || sha256_hex(&std::fs::read(&path).map_err(|_|Error::new("ack_target"))?)!=digest {return Err(Error::new("ack_target"))}
  command.env(path_var,path).env(sha_var,digest);
 }
 let mut child=command.spawn().map_err(|_|Error::new("spawn"))?;
 let mut stdin=child.stdin.take().ok_or_else(||Error::new("spawn"))?;
 stdin.write_all(ack_text.as_bytes()).await.map_err(|_|Error::new("spawn"))?;
 stdin.shutdown().await.map_err(|_|Error::new("spawn"))?;
 drop(stdin);
 let output=tokio::time::timeout(Duration::from_secs(60),child.wait_with_output()).await.map_err(|_|Error::new("ack_timeout"))?.map_err(|_|Error::new("spawn"))?;
 let code=output.status.code().unwrap_or(-1);
 tx.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,$2,'ack',$3,$4,$5,$6) ON CONFLICT(task_id,generation,stage) DO UPDATE SET exit_code=EXCLUDED.exit_code,stdout=EXCLUDED.stdout,stderr=EXCLUDED.stderr,observed_at=clock_timestamp()",&[&task,&generation,&executable.sha256,&code,&output.stdout,&output.stderr]).await.map_err(Error::sql)?;
 println!("process_stage=ack task={task} generation={generation} exit_code={code} stdout_bytes={} stderr_bytes={}",output.stdout.len(),output.stderr.len());
 let response:Value=serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
 if !output.status.success() || response.as_object().is_none_or(|o|o.len()!=2)
  || response.get("protocol").and_then(Value::as_str)!=Some("oo.pipeline.native-partition-ack-result/v1")
  || response.get("acknowledged").and_then(Value::as_bool)!=Some(true) {tx.commit().await.map_err(Error::sql)?;return Err(Error::new("ack_refused"))}
 let source_generation=request.get("source_generation").and_then(Value::as_i64).ok_or_else(||Error::new("position"))?;
 verify_applied(config,&source,&database,source_generation).await?;
 let raw_pos=result.pointer("/partition_target_receipt/partition_scope/partition_id").ok_or_else(||Error::new("position"))?;
 let pos=raw_pos.as_str().map(str::to_owned).unwrap_or_else(||raw_pos.to_string());
 let load_state=result.get("load_state").and_then(Value::as_str).ok_or_else(||Error::new("ack_receipt"))?;
 tx.execute("INSERT INTO jw_native.watermarks(binding_id,pin,task_id,generation,source_generation,applied_pos,load_state,result) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5,$6,$7,$8::text::jsonb) ON CONFLICT(binding_id) DO UPDATE SET pin=EXCLUDED.pin,task_id=EXCLUDED.task_id,generation=EXCLUDED.generation,source_generation=EXCLUDED.source_generation,applied_pos=EXCLUDED.applied_pos,load_state=EXCLUDED.load_state,result=EXCLUDED.result,advanced_at=clock_timestamp() WHERE jw_native.watermarks.pin<EXCLUDED.pin",&[&binding,&pin,&task,&generation,&source_generation,&pos,&load_state,&result_text]).await.map_err(Error::sql)?;
 tx.execute("UPDATE jw_native.pending_acks SET acknowledged_at=clock_timestamp() WHERE task_id=$1::text::uuid",&[&task]).await.map_err(Error::sql)?;
 let n=tx.execute("INSERT INTO jw_native.source_progress(source_id,target_database,source_generation,task_id,binding_id,confirmed_at) VALUES($1,$2,$3,$4::text::uuid,$5::text::uuid,clock_timestamp()) ON CONFLICT(source_id,target_database) DO UPDATE SET source_generation=EXCLUDED.source_generation,task_id=EXCLUDED.task_id,binding_id=EXCLUDED.binding_id,active_task_id=NULL,confirmed_at=clock_timestamp() WHERE COALESCE(jw_native.source_progress.source_generation,0)=$3-1 AND (jw_native.source_progress.active_task_id=$4::text::uuid OR jw_native.source_progress.active_task_id IS NULL)",&[&source,&database,&source_generation,&task,&binding]).await.map_err(Error::sql)?;
 if n!=1 {return Err(Error::new("source_active"))}
 tx.commit().await.map_err(Error::sql)?;
 println!("complete=acknowledged binding={binding} pin={pin} task={task} generation={generation}");Ok(true)
}
pub async fn serve(config:&Config,project:&str,worker:&str,seconds:u64)->Result<()> {
 if !crate::api::is_uuid(project)||seconds==0{return Err(Error::new("input"))}
 let start=tokio::time::Instant::now();
 loop {
  fire(config,project).await?;
  if let Err(e)=step(config,project,worker).await{eprintln!("cycle_error={}",e.kind)}
  if start.elapsed()>=Duration::from_secs(seconds){return Ok(())}
  tokio::time::sleep(Duration::from_secs(1)).await;
 }
}

fn method_token(method:&str)->Result<&'static str> {
 match method {"C1"=>Ok("c1_cdc"),"C2"=>Ok("c2_change_feed"),"C3"=>Ok("c3_cursor"),"C4"=>Ok("c4_partition"),"C5"=>Ok("c5_diff"),_=>Err(Error::new("method"))}
}
fn request_protocol(method:&str)->&'static str {
 if method=="C4" {"oo.pipeline.native-partition/v1"} else {"oo.pipeline.native-update/v1"}
}

fn pg_lsn_decimal(value:&str)->Result<String> {
 let (high,low)=value.split_once('/').ok_or_else(||Error::new("transaction_scope"))?;
 let high=u32::from_str_radix(high,16).map_err(|_|Error::new("transaction_scope"))?;
 let low=u32::from_str_radix(low,16).map_err(|_|Error::new("transaction_scope"))?;
 Ok(((u64::from(high)<<32)|u64::from(low)).to_string())
}

fn group_loader_request(request:&Value,binding:&str,pin:i64,source:&str,base:i64,task:&str,generation:i64)->Result<Value> {
 let object=request.as_object().ok_or_else(||Error::new("transaction_request"))?;
 if object.len()!=8 || request.get("protocol").and_then(Value::as_str)!=Some("oo.jobs.native-transaction-producer/v1")
  || request.get("binding").and_then(Value::as_str)!=Some(binding)
  || request.get("pin").and_then(Value::as_i64)!=Some(pin)
  || request.get("source_id").and_then(Value::as_str)!=Some(source)
  || request.get("capture_method").and_then(Value::as_str)!=Some("c1_cdc")
  || request.get("source_generation").and_then(Value::as_i64)!=Some(base+1) {return Err(Error::new("transaction_request"))}
 let scope=request.get("transaction_scope").ok_or_else(||Error::new("transaction_scope"))?;
 if scope.get("complete").and_then(Value::as_bool)!=Some(true) || scope.get("source_id").and_then(Value::as_str)!=Some(source) {return Err(Error::new("transaction_scope"))}
 let end=pg_lsn_decimal(scope.get("end_lsn").and_then(Value::as_str).ok_or_else(||Error::new("transaction_scope"))?)?;
 let tables=scope.get("tables").and_then(Value::as_array).ok_or_else(||Error::new("transaction_scope"))?;
 let entries=request.get("entries").and_then(Value::as_array).ok_or_else(||Error::new("transaction_request"))?;
 if entries.is_empty() || entries.len()!=tables.len() {return Err(Error::new("transaction_request"))}
 let mut checked=Vec::with_capacity(entries.len());let mut previous=None;
 for (entry,table) in entries.iter().zip(tables) {
  let schema=entry.get("logical_schema").and_then(Value::as_str).ok_or_else(||Error::new("transaction_request"))?;
  let name=entry.get("logical_table").and_then(Value::as_str).ok_or_else(||Error::new("transaction_request"))?;
  let key=(schema,name);
  if previous.is_some_and(|old|old>=key) || table.get("logical_schema").and_then(Value::as_str)!=Some(schema)
   || table.get("logical_table").and_then(Value::as_str)!=Some(name)
   || entry.get("protocol").and_then(Value::as_str)!=Some("oo.pipeline.native-update/v1")
   || entry.get("source_id").and_then(Value::as_str)!=Some(source)
   || entry.get("capture_method").and_then(Value::as_str)!=Some("c1_cdc")
   || entry.get("consistency_grade").and_then(Value::as_str)!=Some("T")
   || entry.get("cross_table_consistent").and_then(Value::as_bool)!=Some(true)
   || entry.get("position_codec").and_then(Value::as_str)!=Some("pg-commit-lsn/v1")
   || entry.get("applied_pos").and_then(Value::as_str)!=Some(end.as_str())
   || entry.get("input_digest")!=table.get("arrow_sha256")
   || entry.get("source_generation")!=table.get("source_generation")
   || entry.get("parent_source_generation")!=table.get("parent_source_generation")
   || entry.get("source_generation").and_then(Value::as_i64)!=entry.get("parent_source_generation").and_then(Value::as_i64).map(|g|g+1) {return Err(Error::new("transaction_request"))}
  previous=Some(key);
  let mut invoked=entry.clone();crate::capture_source::attach_observed_lease(&mut invoked,task,generation).map_err(|_|Error::new("lease"))?;
  checked.push(invoked);
 }
 Ok(json!({"protocol":"oo.pipeline.native-transaction/v1","entries":checked}))
}

fn invocation_mirror(request:&mut Value,method:&str)->Result<()> {
 let object=request.as_object_mut().ok_or_else(||Error::new("request"))?;
 if method=="C4" {
  if object.get("mirror").is_some_and(|v|v.as_bool()!=Some(false)) {return Err(Error::new("c4_mirror"))}
  object.insert("mirror".into(),Value::Bool(false));
 } else {object.insert("mirror".into(),Value::Bool(true));}
 Ok(())
}

#[cfg(test)]
mod invocation_tests {
 use super::*;
 #[test]
 fn frozen_c4_without_mirror_is_invoked_without_target_cas() {
  let frozen=json!({"protocol":"oo.pipeline.native-partition/v1","source_generation":7});
  let mut invoked=frozen.clone();
  invocation_mirror(&mut invoked,"C4").expect("invocation");
  assert!(frozen.get("mirror").is_none());
  assert_eq!(invoked["mirror"],false);
  assert!(invocation_mirror(&mut json!({"mirror":true}),"C4").is_err());
 }
 #[test]
 fn transaction_group_keeps_table_generations_and_freezes_one_lease() {
  let scope=json!({"complete":true,"source_id":"pg","end_lsn":"0/2D8FAF0","tables":[
   {"logical_schema":"a","logical_table":"one","arrow_sha256":"abc","parent_source_generation":3,"source_generation":4},
   {"logical_schema":"b","logical_table":"two","arrow_sha256":"def","parent_source_generation":4,"source_generation":5}]});
  let entries=[("a","one","abc",3,4),("b","two","def",4,5)].into_iter().map(|(schema,table,digest,parent,generation)|json!({
   "protocol":"oo.pipeline.native-update/v1","source_id":"pg","capture_method":"c1_cdc",
   "logical_schema":schema,"logical_table":table,"input_digest":digest,
   "parent_source_generation":parent,"source_generation":generation,"consistency_grade":"T",
   "cross_table_consistent":true,"position_codec":"pg-commit-lsn/v1","applied_pos":"47774448"
  })).collect::<Vec<_>>();
  let frozen=json!({"protocol":"oo.jobs.native-transaction-producer/v1","binding":"binding",
   "pin":1,"source_id":"pg","capture_method":"c1_cdc","source_generation":4,
   "transaction_scope":scope,"entries":entries});
  let task="00000000-0000-4000-8000-000000000001";
  let invoked=group_loader_request(&frozen,"binding",1,"pg",3,task,7).expect("group");
  assert_eq!(invoked["entries"][0]["source_generation"],4);
  assert_eq!(invoked["entries"][1]["source_generation"],5);
  assert_eq!(invoked["entries"][0]["lease_generation"],7);
  assert_eq!(invoked["entries"][1]["lease_id"],task);
  assert!(frozen["entries"][0].get("lease_id").is_none());
  let mut changed=frozen.clone();changed["entries"][1]["applied_pos"]=json!("47774449");
  assert!(group_loader_request(&changed,"binding",1,"pg",3,task,7).is_err());
 }
}

/// Read only the last successfully CAS-committed occurrence, never a capture candidate.
pub async fn state(config:&Config,binding:&str)->Result<()> {
 if !crate::api::is_uuid(binding){return Err(Error::new("binding"))}
 let c=session::runtime(config).await?;
 let row=c.query_opt("SELECT jsonb_build_object('protocol','oo.jobs.native-state/v1','binding',b.id,'source_id',b.source_id,'method',b.method,'cycle',b.cycle,'period_ms',b.period_ms,'enabled',b.enabled,'applied',w.task_id IS NOT NULL,'pin',w.pin,'task_id',w.task_id,'lease_generation',w.generation,'source_generation',w.source_generation,'applied_pos',w.applied_pos,'load_state',w.load_state,'advanced_at',w.advanced_at,'position_codec',o.request->'position_codec','position_lineage',o.request->'position_lineage','sqlserver_candidate_position',o.request->'sqlserver_candidate_position')::text FROM jw_native.bindings b LEFT JOIN jw_native.watermarks w ON w.binding_id=b.id LEFT JOIN jw_native.occurrences o ON o.task_id=w.task_id LEFT JOIN jobs.tasks t ON t.id=w.task_id WHERE b.id=$1::text::uuid AND (w.task_id IS NULL OR t.state='succeeded')",&[&binding]).await.map_err(Error::sql)?;
 let row=row.ok_or_else(||Error::new("binding"))?;let text:String=row.get(0);println!("{text}");Ok(())
}

/// Explicit operator replacement of only the loader/registration used by a binding.
/// Original occurrence manifest and source request remain immutable evidence.
pub async fn loader_put(config:&Config,path:&str)->Result<()> {
 let m=manifest_read(path)?;let (text,sha)=canonical(&m)?;let c=session::runtime(config).await?;
 let changed=c.execute("INSERT INTO jw_native.loader_overrides(binding_id,operator_manifest,operator_manifest_sha) SELECT id,$2::text::jsonb,$3 FROM jw_native.bindings WHERE id=$1::text::uuid AND project_id=$4::text::uuid AND source_id=$5 AND method=$6 AND cycle=$7 ON CONFLICT(binding_id) DO UPDATE SET operator_manifest=EXCLUDED.operator_manifest,operator_manifest_sha=EXCLUDED.operator_manifest_sha,approved_by=current_user,approved_at=clock_timestamp()",&[&m.binding,&text,&sha,&m.project_id,&m.source_id,&m.method,&m.cycle]).await.map_err(Error::sql)?;
 if changed!=1{return Err(Error::new("binding_identity"))}
 println!("loader_override=registered binding={} manifest_sha={sha} source_request_rewritten=false",m.binding);Ok(())
}

#[cfg(test)]
mod ack_tests {
 use super::*;
 #[test]
 fn c4_ack_is_derived_from_loader_receipt() {
  let task="d3b1aa26-1505-40be-b0bb-47c53996a8bf";
  let result=json!({"source_id":"clickhouse_268","logical_schema":"oo_dl01w40","logical_table":"readings","run_id":"ebcc9bdc-df0c-427b-9133-771875c0afb1","payload_digest":"4dab995f958e28b8da4c244cbe78b225c0dd50caffaf834d5bec763aedf44c9d","jobs_lease_receipt":{"state":"running"},"partition_target_receipt":{"acknowledged":false,"job_task_id":task,"job_generation":1,"batch_id":"t1.b1","payload_digest":"4dab995f958e28b8da4c244cbe78b225c0dd50caffaf834d5bec763aedf44c9d","partition_scope":{"source_id":"clickhouse_268","logical_schema":"oo_dl01w40","logical_table":"readings","partition_id":"202401"}}});
  let ack=ack_request(&result,task,1).unwrap();
  assert_eq!(ack["protocol"],"oo.pipeline.native-partition-ack/v1");
  assert_eq!(ack["partition_id"],"202401");
  assert!(ack_request(&result,task,2).is_err());
 }
}
