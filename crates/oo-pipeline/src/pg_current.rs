//! Original PG captures through the existing leased typed COPY store.
//! Admission is re-executed before the first connection; source ACK is worker-only.
use crate::{pg_current_capture as capture,c1_typed::{self,Column,TableSpec,Part},native_update};
use capture::{Artifact,Message,Verified,text};
use serde_json::{json,Value};
use sqlx::{Connection,PgConnection};
use std::{io::Write,path::Path};
// Replaced with the exact built PG-only admission binary before live execution.
const ADMISSION_SHA:&str="0dd22ba458941b1c52e75e9224bd99dd90a31ffc66d318c82ac5222020827456";
fn db(e:sqlx::Error)->&'static str{if let Some(d)=e.as_database_error(){eprintln!("pg target sqlstate={} {}",d.code().unwrap_or_default(),if d.code().is_some_and(|s|s.starts_with("OO")){d.message()}else{"database refusal"});} "pg_target"}
fn spec(table:&Value)->Result<TableSpec,&'static str>{let columns=table["columns"].as_array().ok_or("pg_columns")?.iter().map(|c|Ok(Column{name:text(c,"name")?.into(),oid:c["oid"].as_i64().ok_or("pg_oid")?,typmod:c["typmod"].as_i64().ok_or("pg_typmod")? as i32,nullable:c["nullable"].as_bool().ok_or("pg_nullable")?})).collect::<Result<Vec<_>,&str>>()?;let key=columns.iter().position(|c|table["primary_key"]==c.name).ok_or("pg_key")?;let s=TableSpec{columns,keys:vec![key]};s.validate()?;Ok(s)}
fn trusted(v:&Value)->Result<Verified,&'static str>{
 let pin:Artifact=serde_json::from_value(v["bundle"].clone()).map_err(|_|"pg_bundle_pin")?;
 let registry=std::env::var("OO_NATIVE_BINDING_REGISTRY").map_err(|_|"pg_registry")?;
 let registry_sha=std::env::var("OO_NATIVE_BINDING_REGISTRY_SHA256").map_err(|_|"pg_registry")?;
 let registered=Artifact{path:registry.into(),sha256:registry_sha}.document()?;
 if registered!=v["bundle"]{return Err("pg_registry_identity");}
 let bin=std::env::var("OO_NATIVE_ADMISSION_BIN").map_err(|_|"pg_admission")?;
 if capture::hash(&std::fs::read(&bin).map_err(|_|"pg_admission")?)!=ADMISSION_SHA{return Err("pg_unapproved_admission");}
 let mut child=std::process::Command::new(bin).arg("validate-pg-current").env_clear().stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().map_err(|_|"pg_admission")?;
 child.stdin.take().ok_or("pg_admission")?.write_all(serde_json::to_string(&pin).map_err(|_|"pg_json")?.as_bytes()).map_err(|_|"pg_admission")?;
 let result=child.wait_with_output().map_err(|_|"pg_admission")?;
 let decision:Value=serde_json::from_slice(&result.stdout).map_err(|_|"pg_admission")?;
 if !result.status.success()||decision["admitted"]!=true||decision["bundle_sha256"]!=pin.sha256||decision["source_id"]!=v["source_id"]{return Err("pg_admission_refused");}
 let verified=capture::verify(&pin)?;
 if verified.bundle.source_id!=text(v,"source_id")?{return Err("pg_source");}
 // Decode every Arrow page before target effects, including parts on a replay.
 let root=verified.bundle.baseline.path.parent().ok_or("pg_path")?;
 for table in verified.baseline["tables"].as_array().ok_or("pg_tables")?{
  let shape=verified.baseline["catalog"].as_array().ok_or("pg_catalog")?.iter().find(|s|s["schema"]==table["schema"]&&s["table"]==table["table"]).ok_or("pg_shape")?;
  let s=spec(shape)?;let name=format!("{}.{}",text(table,"schema")?,text(table,"table")?);let mut last=None;
  for p in table["parts"].as_array().ok_or("pg_parts")?{
   let a=capture::part(root,p)?;let part=Part{table:name.clone(),spec:s.clone(),path:a.path,sha256:a.sha256,bytes:p["bytes"].as_u64().ok_or("pg_bytes")?,rows:p["rows"].as_u64().ok_or("pg_rows")? as usize};
   c1_typed::copy_bytes(&part,"fill","0")?;
   let decoded=crate::native_arrow::decode_arrow_stream(&std::fs::read(&part.path).map_err(|_|"pg_part")?).map_err(|_|"pg_arrow")?;
   for cell in &decoded.columns[s.keys[0]]{let n=match cell{crate::native_arrow::NativeCell::Int{value,..}=>*value,_=>return Err("pg_key")};if last.is_some_and(|x|n<=x){return Err("pg_key_order");}last=Some(n);}
  }
 }
 Ok(verified)
}
pub async fn execute(v:&Value)->Result<Value,&'static str>{
 if v["protocol"]!="oo.pipeline.pg-current/v1"||v["source_generation"]!=1||v.as_object().is_none_or(|o|o.len()!=9||o.keys().any(|k|!["protocol","source_id","binding","pin","source_generation","bundle","target","lease_id","lease_generation"].contains(&k.as_str()))){return Err("pg_protocol");}
 let verified=trusted(v)?;
 let source=text(v,"source_id")?;let task=text(v,"lease_id")?;let generation=v["lease_generation"].as_i64().filter(|n|*n>0).ok_or("pg_lease")?;
 let mut conn=native_update::connect_target(&v["target"]).await?;
 let mut tx=conn.begin().await.map_err(db)?;native_update::lease_hold(&mut tx,task,Some(generation)).await?;
 let open=json!({"bundle_sha256":verified.digest,"source_identity":verified.baseline["source_identity"],"tables":verified.baseline["catalog"],"schema_digest":capture::hash(verified.baseline["catalog"].to_string().as_bytes()),"boundary":{"original_slot_lsn":verified.baseline["resume_lsn"],"snapshot":verified.baseline["snapshot"],"end_lsn":verified.transactions["resume_lsn"],"transactions":verified.transactions["transactions"]}});
 let raw:String=sqlx::query_scalar("SELECT _oo.pg_current_open($1,$2::uuid,$3,$4::jsonb,$5,$6::uuid)::text").bind(source).bind(task).bind(generation).bind(open.to_string()).bind(text(&v["target"],"shard")?).bind(text(&v["target"],"owner")?).fetch_one(&mut *tx).await.map_err(db)?;
 let opened:Value=serde_json::from_str(&raw).map_err(|_|"pg_result")?;
 tx.commit().await.map_err(db)?;
 if opened["load_state"]=="staged"{return Ok(opened);}
 baseline(&mut conn,v,&verified).await?;
 transactions(&mut conn,v,&verified).await?;
 let mut tx=conn.begin().await.map_err(db)?;native_update::lease_hold(&mut tx,task,Some(generation)).await?;
 let end=capture::lsn(text(&verified.transactions,"resume_lsn")?)?.to_string();
 let raw:String=sqlx::query_scalar("SELECT _oo.pg_current_seal($1,$2::uuid,$3,$4::numeric)::text").bind(source).bind(task).bind(generation).bind(&end).fetch_one(&mut *tx).await.map_err(db)?;
 tx.commit().await.map_err(db)?;serde_json::from_str(&raw).map_err(|_|"pg_result")
}
async fn baseline(conn:&mut PgConnection,v:&Value,verified:&Verified)->Result<(),&'static str>{
 let lease=c1_typed::Lease{source:text(v,"source_id")?.into(),task:text(v,"lease_id")?.into(),generation:v["lease_generation"].as_i64().ok_or("pg_lease")?,epoch:1};
 let root=verified.bundle.baseline.path.parent().ok_or("pg_path")?;let mut sequence=0i64;
 for table in verified.baseline["tables"].as_array().ok_or("pg_tables")?{
  let shape=verified.baseline["catalog"].as_array().ok_or("pg_catalog")?.iter().find(|s|s["schema"]==table["schema"]&&s["table"]==table["table"]).ok_or("pg_shape")?;
  let spec=spec(shape)?;let name=format!("{}.{}",text(table,"schema")?,text(table,"table")?);
  {let mut tx=conn.begin().await.map_err(db)?;native_update::lease_hold(&mut tx,&lease.task,Some(lease.generation)).await?;c1_typed::open(&mut tx,&lease,&name,&spec).await?;tx.commit().await.map_err(db)?;}
  for p in table["parts"].as_array().ok_or("pg_parts")?{
   sequence+=1;let artifact=capture::part(root,p)?;let part=Part{table:name.clone(),spec:spec.clone(),path:artifact.path,sha256:artifact.sha256,bytes:p["bytes"].as_u64().ok_or("pg_bytes")?,rows:p["rows"].as_u64().ok_or("pg_rows")? as usize};
   let batch=c1_typed::Batch{protocol:c1_typed::PROTOCOL.into(),sequence,previous_position:"0".into(),position:"0".into(),kind:"fill".into(),parts:vec![part.clone()]};let digest=batch.digest()?;let bytes=c1_typed::copy_bytes(&part,"fill","0")?;
   let mut tx=conn.begin().await.map_err(db)?;native_update::lease_hold(&mut tx,&lease.task,Some(lease.generation)).await?;
   let fresh:bool=sqlx::query_scalar("SELECT _oo.c1_typed_begin($1,$2::uuid,$3,1,$4,$5,0,0,'fill')").bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(sequence).bind(&digest).fetch_one(&mut *tx).await.map_err(db)?;
   if fresh{
    let stage:String=sqlx::query_scalar("SELECT _oo.c1_typed_open($1,$2::uuid,$3,1,$4,$5::jsonb)").bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(&name).bind(serde_json::to_string(&spec).map_err(|_|"pg_spec")?).fetch_one(&mut *tx).await.map_err(db)?;
    if !stage.starts_with("c3s_")||stage.len()!=36||!stage[4..].bytes().all(|b|b.is_ascii_hexdigit()){return Err("pg_stage");}
    let cols=(0..spec.columns.len()).map(|i|format!(",c{i}")).collect::<String>();
    let mut copy=tx.copy_in_raw(&format!("COPY _oo.{stage}(k,op{cols}) FROM STDIN WITH (FORMAT text)")).await.map_err(db)?;
    for chunk in bytes.chunks(256*1024){copy.send(chunk).await.map_err(db)?;}
    if copy.finish().await.map_err(db)?!=part.rows as u64{return Err("pg_copy_rows");}
    sqlx::query("SELECT _oo.c1_typed_apply_table($1,$2::uuid,$3,1,$4,'fill',0,$5)").bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(&name).bind(part.rows as i64).execute(&mut *tx).await.map_err(db)?;
    sqlx::query("SELECT _oo.c1_typed_finish($1,$2::uuid,$3,1,$4,$5,0,0,'fill')").bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(sequence).bind(&digest).execute(&mut *tx).await.map_err(db)?;
   }
   let _:bool=sqlx::query_scalar("SELECT _oo.c1_typed_ack($1,$2::uuid,$3,1,$4,$5)").bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(sequence).bind(&digest).fetch_one(&mut *tx).await.map_err(db)?;
   tx.commit().await.map_err(db)?;
  }
  let mut tx=conn.begin().await.map_err(db)?;native_update::lease_hold(&mut tx,&lease.task,Some(lease.generation)).await?;
  sqlx::query("SELECT _oo.c1_typed_complete($1,$2::uuid,$3,1,$4)").bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(&name).execute(&mut *tx).await.map_err(db)?;tx.commit().await.map_err(db)?;
 }
 Ok(())
}
async fn transactions(conn:&mut PgConnection,v:&Value,verified:&Verified)->Result<(),&'static str>{
 let source=text(v,"source_id")?;let task=text(v,"lease_id")?;let generation=v["lease_generation"].as_i64().ok_or("pg_lease")?;
 let scopes=verified.transactions["transactions"].as_array().ok_or("pg_scopes")?;
 let mut relations=capture::relations(verified)?;let mut messages=capture::Messages::new(verified)?;
 for (ordinal,scope) in scopes.iter().enumerate(){
  let mut tx=conn.begin().await.map_err(db)?;let held=native_update::lease_hold(&mut tx,task,Some(generation)).await?.0;
  let mut keeper=native_update::LeaseKeeper::for_lease(native_update::BASELINE_LEASE_TTL);let started=std::time::Instant::now();
  let prior:Option<String>=sqlx::query_scalar("SELECT scope::text FROM _oo.pg_current_tx WHERE source=$1 AND ordinal=$2").bind(source).bind(ordinal as i64+1).fetch_optional(&mut *tx).await.map_err(db)?;
  if prior.as_ref().is_some_and(|s|serde_json::from_str::<Value>(s).ok().as_ref()!=Some(scope)){return Err("pg_replay_scope");}
  let fresh=prior.is_none();let mut events=0i64;let end=capture::lsn(text(scope,"end_lsn")?)?.to_string();
  loop{
   let (_,message)=messages.next().ok_or("pg_incomplete")??;
   match message{
    Message::Relation(r)=>{relations.insert(r.oid,r);},
    Message::Begin{..}=>{},
    Message::Commit{..}=>break,
    Message::Event{kind,oid,old,new}=>{
     events+=1;let relation=relations.get(&oid).ok_or("pg_relation")?;
     if fresh {
      let old=old.map(|c|Value::Array(c.iter().map(|c|c.json()).collect()));let new=new.map(|c|Value::Array(c.iter().map(|c|c.json()).collect()));
      sqlx::query("SELECT _oo.pg_current_event($1,$2::uuid,$3,$4,$5,$6::jsonb,$7::jsonb,$8::numeric)").bind(source).bind(task).bind(generation).bind(format!("{}.{}",relation.schema,relation.table)).bind((kind as char).to_string()).bind(old.map(|v|v.to_string())).bind(new.map(|v|v.to_string())).bind(&end).execute(&mut *tx).await.map_err(db)?;
     }
     if events%100==0{native_update::renew_if_due(&mut tx,&held,&mut keeper,started).await?;}
    }
   }
  }
  if fresh {sqlx::query("SELECT _oo.pg_current_commit($1,$2::uuid,$3,$4,$5::jsonb,$6::numeric,$7)").bind(source).bind(task).bind(generation).bind(ordinal as i64+1).bind(scope.to_string()).bind(&end).bind(events).execute(&mut *tx).await.map_err(db)?;}
  native_update::renew_if_due(&mut tx,&held,&mut keeper,started).await?;tx.commit().await.map_err(db)?;
 }
 if messages.next().is_some(){return Err("pg_extra_transaction");}Ok(())
}
