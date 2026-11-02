//! PG uses the normal scheduler, claim, process evidence, CAS and pending ACK queue.
//! Only this recovery path may advance the original source slot.
use super::*;
use tokio_postgres::Transaction;
fn field<'a>(v:&'a Value,k:&str)->Result<&'a str>{v[k].as_str().filter(|s|!s.is_empty()).ok_or_else(||Error::new("pg_field"))}
fn artifact(v:&Value)->Result<Value>{let path=field(v,"path")?;let sha=field(v,"sha256")?;verify_artifact(path,sha)?;serde_json::from_slice(&std::fs::read(path).map_err(|_|Error::new("pg_file"))?).map_err(|_|Error::new("pg_json"))}
pub(super) fn selected(m:&Manifest)->bool{m.c1_baseline_format.as_deref()==Some("pg-capture/v1")}
async fn proof(config:&Config,database:&str,source:&str,task:&str,expected:&Value)->Result<()> {
 let target=target_owner(config,database).await?;
 let row=target.query_opt("SELECT r.result::text, w.applied_pos::text, w.source_epoch::bigint, (SELECT count(*) FROM _oo.pg_current_tx x WHERE x.source=r.source) FROM _oo.pg_current_run r JOIN _oo.native_c1_baseline_run w ON w.source=r.source AND w.task_id=r.task WHERE r.source=$1 AND r.task=$2::text::uuid AND r.sealed_at IS NOT NULL AND w.engine='postgresql' AND w.state='awaiting_publish'",&[&source,&task]).await.map_err(Error::sql)?.ok_or_else(||Error::new("pg_target_proof"))?;
 let mut actual:Value=serde_json::from_str(&row.get::<_,String>(0)).map_err(|_|Error::new("pg_target_proof"))?;
 let mut expected=expected.clone();actual["replay"]=json!(false);expected["replay"]=json!(false);
 if actual!=expected||row.get::<_,String>(1)!=field(&actual,"applied_pos")?||row.get::<_,i64>(2)!=1||actual["source_generation"]!=1||actual["task"]!=task||actual["source_id"]!=source||actual["acknowledged"]!=false||actual["transaction_count"].as_i64()!=Some(row.get::<_,i64>(3)){return Err(Error::new("pg_target_proof"));}
 Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn apply(config:&Config,c:&mut tokio_postgres::Client,project:&str,worker:&str,task:&str,generation:i64,m:&Manifest,binding:&str,pin:i64,database:&str,stored:Option<String>)->Result<()> {
 let override_row=c.query_opt("SELECT operator_manifest::text,operator_manifest_sha FROM jw_native.loader_overrides WHERE binding_id=$1::text::uuid",&[&binding]).await.map_err(Error::sql)?;
 let effective=if let Some(row)=override_row {
  let selected:Manifest=serde_json::from_str(&row.get::<_,String>(0)).map_err(|_|Error::new("pg_override"))?;
  if canonical(&selected)?.1!=row.get::<_,String>(1){return Err(Error::new("pg_override"));}
  approved_override(&selected,m)?;selected.check()?;selected
 }else{m.clone()};
 let m=&effective;
 let base=confirmed_generation(config,c,&m.source_id,database,task,&stored).await?;
 if base!=0{return Err(Error::new("pg_capture_already_acknowledged"));}
 let registration=m.registration.as_ref().ok_or_else(||Error::new("pg_registration"))?;
 let selected=registration.registry_path.as_ref().ok_or_else(||Error::new("pg_registration"))?;
 let bundle_pin:Value=serde_json::from_slice(&std::fs::read(selected).map_err(|_|Error::new("pg_registration"))?).map_err(|_|Error::new("pg_registration"))?;
 let bundle=artifact(&bundle_pin)?;
 if bundle["source_id"]!=m.source_id{return Err(Error::new("pg_source"));}
 let mut request=match stored {
  Some(s)=>serde_json::from_str::<Value>(&s).map_err(|_|Error::new("pg_request"))?,
  None=>{
   let input=json!({"protocol":"oo.jobs.native-producer/v1","method":"C1","cycle":"pull","source_id":m.source_id,"binding":binding,"pin":pin,"acknowledged_source_generation":0});
   let value=exec(config,&m.producer,"producer",serde_json::to_vec(&input).map_err(|_|Error::new("pg_json"))?,None,None,binding,pin,project,task,worker,generation,None).await?;
   if value["protocol"]!="oo.pipeline.pg-current/v1"||value["binding"]!=binding||value["pin"]!=pin||value["source_id"]!=m.source_id||value["source_generation"]!=1||value["bundle"]!=bundle_pin||value["target"]["database"]!=database{return Err(Error::new("pg_producer_context"));}
   let n=c.execute("UPDATE jw_native.occurrences o SET request=$2::text::jsonb FROM jobs.tasks t WHERE o.task_id=t.id AND t.id=$1::text::uuid AND t.state='running' AND t.lease_owner=$3 AND t.generation=$4 AND t.lease_until>clock_timestamp() AND o.request IS NULL",&[&task,&value.to_string(),&worker,&generation]).await.map_err(Error::sql)?;
   if n!=1{return Err(Error::new("lost_lease"));}value
  }
 };
 if request["bundle"]!=bundle_pin||request["target"]["database"]!=database{return Err(Error::new("pg_request_identity"));}
 request["lease_id"]=json!(task);request["lease_generation"]=json!(generation);
 let result=exec(config,&m.loader,"loader",serde_json::to_vec(&request).map_err(|_|Error::new("pg_json"))?,Some(registration),None,binding,pin,project,task,worker,generation,None).await?;
 proof(config,database,&m.source_id,task,&result).await?;
 let source_config=artifact(&bundle["transactions_config"])?;
 let endpoint=source_config["source"].clone();
 let ack=json!({"protocol":"oo.jobs.pg-slot-ack/v1","source_id":m.source_id,"task":task,"bundle":bundle_pin,
   "source_identity":result["source_identity"],"endpoint":endpoint,"end_lsn":result["end_lsn"],"applied_pos":result["applied_pos"]});
 let tx=c.transaction().await.map_err(Error::sql)?;
 tx.execute("INSERT INTO jw_native.pending_acks(task_id,binding_id,pin,generation,ack_request,loader_path,loader_sha256) VALUES($1::text::uuid,$2::text::uuid,$3,$4,$5::text::jsonb,$6,$7)",&[&task,&binding,&pin,&generation,&ack.to_string(),&m.loader.path,&m.loader.sha256]).await.map_err(Error::sql)?;
 tx.execute("UPDATE jw_native.occurrences SET result=$2::text::jsonb WHERE task_id=$1::text::uuid",&[&task,&result.to_string()]).await.map_err(Error::sql)?;
 if lease::complete_success(&tx,project,task,worker,generation).await?.status!=LeaseStatus::Succeeded{return Err(Error::new("lost_lease"));}
 tx.commit().await.map_err(Error::sql)?;
 println!("complete=cas_committed_ack_pending engine=postgresql task={task} generation={generation}");
 if drain_ack(config,c,project,Some(task)).await?{Ok(())}else{Err(Error::new("pg_ack_pending"))}
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn drain(config:&Config,tx:Transaction<'_>,_project:&str,task:&str,binding:&str,pin:i64,generation:i64,source:&str,database:&str,ack:&Value,request_text:&str,result_text:&str,path:&str,sha:&str,state:&str,current_generation:i64)->Result<bool>{
 if state!="succeeded"||generation!=current_generation||database!=target_database(config,"C1")?{return Err(Error::new("pg_ack_cas"));}
 executable_check(&Executable{path:path.into(),sha256:sha.into(),args:vec![]})?;
 let request:Value=serde_json::from_str(request_text).map_err(|_|Error::new("pg_ack_request"))?;
 let result:Value=serde_json::from_str(result_text).map_err(|_|Error::new("pg_ack_result"))?;
 proof(config,database,source,task,&result).await?;
 let bundle=artifact(&ack["bundle"])?;let source_config=artifact(&bundle["transactions_config"])?;
 if request["bundle"]!=ack["bundle"]||ack["source_identity"]!=result["source_identity"]||ack["task"]!=task||ack["source_id"]!=source||ack["end_lsn"]!=result["end_lsn"]||ack["applied_pos"]!=result["applied_pos"]||ack["endpoint"]!=source_config["source"]{return Err(Error::new("pg_ack_identity"));}
 let identity=&ack["source_identity"];let endpoint=&ack["endpoint"];
 for k in ["host","port","database"]{if endpoint[k]!=identity[k]{return Err(Error::new("pg_ack_endpoint"));}}
 let password=std::fs::read_to_string(field(endpoint,"password_file")?).map_err(|_|Error::new("pg_ack_secret"))?;
 let ca=std::fs::read(field(endpoint,"ca_file")?).map_err(|_|Error::new("pg_ack_ca"))?;
 let client=crate::tls::connect(field(endpoint,"host")?,endpoint["port"].as_u64().and_then(|n|u16::try_from(n).ok()).ok_or_else(||Error::new("pg_ack_port"))?,field(endpoint,"database")?,field(endpoint,"username")?,password.trim_end(),&ca).await?;
 client.batch_execute("SET default_transaction_read_only=on; SET statement_timeout=10000").await.map_err(Error::sql)?;
 let slot=field(identity,"slot")?;let publication=field(identity,"publication")?;let end=field(ack,"end_lsn")?;
 let check=client.query_one("SELECT (SELECT system_identifier::text FROM pg_control_system()),(SELECT oid::bigint FROM pg_database WHERE datname=current_database()),(SELECT count(*) FROM pg_publication WHERE pubname=$2),plugin::text,database::text,active,invalidation_reason::text,confirmed_flush_lsn::text FROM pg_replication_slots WHERE slot_name=$1",&[&slot,&publication]).await.map_err(Error::sql)?;
 if check.get::<_,String>(0)!=field(identity,"system_identifier")?||identity["database_oid"].as_i64()!=Some(check.get::<_,i64>(1))||check.get::<_,i64>(2)!=1||check.get::<_,String>(3)!="pgoutput"||check.get::<_,String>(4)!=field(identity,"database")?||check.get::<_,bool>(5)||check.get::<_,Option<String>>(6).is_some(){return Err(Error::new("pg_ack_live_identity"));}
 let before:String=check.get(7);let before_pos=pg_lsn_decimal(&before)?.parse::<u64>().map_err(|_|Error::new("pg_ack_position"))?;let end_pos=pg_lsn_decimal(end)?.parse::<u64>().map_err(|_|Error::new("pg_ack_position"))?;
 let original=artifact(&bundle["baseline"])?;let first=pg_lsn_decimal(field(&original,"resume_lsn")?)?.parse::<u64>().map_err(|_|Error::new("pg_ack_position"))?;
 if before_pos!=first&&before_pos!=end_pos{return Err(Error::new("pg_ack_unexpected_boundary"));}
 if before_pos<end_pos{
  let advanced:String=client.query_one("SELECT end_lsn::text FROM pg_replication_slot_advance($1,$2::text::pg_lsn)",&[&slot,&end]).await.map_err(Error::sql)?.get(0);
  if pg_lsn_decimal(&advanced)?!=pg_lsn_decimal(end)?{return Err(Error::new("pg_ack_advance_boundary"));}
 }
 let after:String=client.query_one("SELECT confirmed_flush_lsn::text FROM pg_replication_slots WHERE slot_name=$1",&[&slot]).await.map_err(Error::sql)?.get(0);
 if pg_lsn_decimal(&after)?!=pg_lsn_decimal(end)?{return Err(Error::new("pg_ack_confirmation"));}
 let worker_sha=sha256_hex(&std::fs::read(std::env::current_exe().map_err(|_|Error::new("pg_worker"))?).map_err(|_|Error::new("pg_worker"))?);
 let observation=json!({"protocol":"oo.jobs.pg-slot-ack-observation/v1","source_identity":identity,"task":task,"before_lsn":before,"after_lsn":after,"replay":before_pos==end_pos,"target_and_job_verified":true});
 tx.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,$2,'ack',$3,0,$4,''::bytea) ON CONFLICT DO NOTHING",&[&task,&generation,&worker_sha.as_str(),&serde_json::to_vec(&observation).map_err(|_|Error::new("pg_json"))?]).await.map_err(Error::sql)?;
 tx.execute("INSERT INTO jw_native.watermarks(binding_id,pin,task_id,generation,source_generation,applied_pos,load_state,result) VALUES($1::text::uuid,$2,$3::text::uuid,$4,1,$5::text::numeric,'staged',$6::text::jsonb)",&[&binding,&pin,&task,&generation,&field(ack,"applied_pos")?,&result_text]).await.map_err(Error::sql)?;
 tx.execute("UPDATE jw_native.pending_acks SET acknowledged_at=clock_timestamp() WHERE task_id=$1::text::uuid",&[&task]).await.map_err(Error::sql)?;
 let n=tx.execute("UPDATE jw_native.source_progress SET source_generation=1,task_id=$3::text::uuid,binding_id=$4::text::uuid,active_task_id=NULL,confirmed_at=clock_timestamp() WHERE source_id=$1 AND target_database=$2 AND active_task_id=$3::text::uuid AND COALESCE(source_generation,0)=0",&[&source,&database,&task,&binding]).await.map_err(Error::sql)?;
 if n!=1{return Err(Error::new("pg_source_active"));}
 tx.commit().await.map_err(Error::sql)?;
 println!("complete=acknowledged engine=postgresql task={task} end_lsn={end} replay={}",before_pos==end_pos);Ok(true)
}

fn approved_override(selected:&Manifest,original:&Manifest)->Result<()> {
 let mut reference=selected.clone();
 if selected.loader.args!=original.loader.args || selected.producer!=original.producer {return Err(Error::new("pg_override_scope"));}
 reference.loader=original.loader.clone();
 let prior=original.registration.as_ref().ok_or_else(||Error::new("pg_registration"))?;
 let chosen=reference.registration.as_mut().ok_or_else(||Error::new("pg_registration"))?;
 chosen.admission_binary=prior.admission_binary.clone();chosen.admission_sha256=prior.admission_sha256.clone();
 if canonical(&reference)?.1!=canonical(original)?.1 {return Err(Error::new("pg_override_scope"));}Ok(())
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn override_cannot_rewrite_source_or_original_bundle() {
  let original:Manifest=serde_json::from_value(json!({"protocol":"oo.jobs.native-schedule/v1","binding":"00000000-0000-4000-8000-000000000001","project_id":"00000000-0000-4000-8000-000000000002","source_id":"owned","method":"C1","cycle":"pull","period_ms":1000,"c1_baseline_format":"pg-capture/v1","producer":{"path":"/source","sha256":"a".repeat(64),"args":["current-worker-producer","--config","/original.json"]},"loader":{"path":"/loader","sha256":"b".repeat(64),"args":["import-native-transaction"]},"registration":{"admission_binary":"/admission","admission_sha256":"c".repeat(64),"registry_path":"/original-pin.json"}})).unwrap();
  let mut selected=original.clone();selected.loader.path="/new-loader".into();selected.loader.sha256="d".repeat(64);selected.registration.as_mut().unwrap().admission_binary="/new-admission".into();assert!(approved_override(&selected,&original).is_ok());
  for key in ["source_id","binding","project_id","period_ms","producer","registration"] {
   let mut v=serde_json::to_value(&selected).unwrap();v[key]=match key{"period_ms"=>json!(2000),"producer"=>json!({"path":"/other","sha256":"e".repeat(64),"args":[]}),"registration"=>json!({"admission_binary":"/new","admission_sha256":"e".repeat(64),"registry_path":"/other.json"}),_=>json!("other")};
   let changed:Manifest=serde_json::from_value(v).unwrap();assert!(approved_override(&changed,&original).is_err(),"{key}");
  }
 }
}
