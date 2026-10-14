#![forbid(unsafe_code)]
use std::{collections::HashMap,fs,path::Path,time::Duration};
use mysql_async::{Conn,OptsBuilder,Row,Value,prelude::Queryable};
use serde_json::{Value as Json,json};
use starrocks_probe::{audit::{Audit,SqlMode},civil,decimal,encode::{self,Cell,ColumnOut},identity,plan::{self,ArrowKind,TypePlan}};
use zeroize::Zeroize;
fn ident(s:&str)->Result<String,String>{
 if s.is_empty() || s.len()>64 || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b==b'_') {return Err("unsupported identifier".into())}
 Ok(format!("`{s}`"))
}
fn text(v:&Value)->Result<Option<String>,String>{match v{
 Value::NULL=>Ok(None),Value::Bytes(b)=>std::str::from_utf8(b).map(|s|Some(s.into())).map_err(|_|"non-UTF8 value".into()),
 Value::Int(v)=>Ok(Some(v.to_string())),Value::UInt(v)=>Ok(Some(v.to_string())),_=>Err("unsafe value representation".into())}}
fn cell(v:&Value,c:&ColumnOut)->Result<Cell,String>{
 let Some(s)=text(v)? else {return if c.nullable{Ok(Cell::Null)}else{Err("NULL in nonnullable column".into())}};
 let nums=|s:&str,sep:char|s.split(sep).map(|v|v.parse::<u32>().map_err(|_|"invalid civil value".to_owned())).collect::<Result<Vec<_>,_>>();
 match c.kind{
 ArrowKind::Int64=>s.parse().map(Cell::I64).map_err(|_|"int64 overflow".into()),
 ArrowKind::Decimal{scale,..}=>decimal::parse_unscaled(&s,scale).map(Cell::Decimal).map_err(|e|e.to_string()),
 ArrowKind::Utf8=>Ok(Cell::Text(s)),
 ArrowKind::Date32=>{let n=nums(&s,'-')?;if n.len()!=3{return Err("invalid date".into())} civil::ymd_to_days(n[0] as i32,n[1],n[2]).map(Cell::Date).map_err(|e|e.to_string())},
 ArrowKind::NaiveTimestampSecond=>{let (d,t)=s.split_once(' ').ok_or("invalid timestamp")?;let d=nums(d,'-')?;let t=nums(t,':')?;if d.len()!=3 || t.len()!=3{return Err("invalid timestamp".into())}civil::naive_seconds(d[0] as i32,d[1],d[2],t[0],t[1],t[2]).map(Cell::NaiveSecond).map_err(|e|e.to_string())}
 }
}
async fn query(conn:&mut Conn,audit:&Audit,sql:&str)->Result<Vec<Row>,String>{
 let class=audit.sql("business",sql,SqlMode::ReadOnly).map_err(|e|e.to_string())?;audit.note_connection("business",class,conn.id());conn.query(sql).await.map_err(|_|"native query failed".into())
}
fn row_text(row:Row)->Result<Vec<String>,String>{row.unwrap().iter().map(|v|text(v).map(|s|s.unwrap_or_default())).collect()}
async fn partitions(conn:&mut Conn,audit:&Audit,database:&str,table:&str)->Result<Json,String>{
 let rows=query(conn,audit,&format!("SHOW PARTITIONS FROM {}.{}",ident(database)?,ident(table)?)).await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?;
 let ddl=query(conn,audit,&format!("SHOW CREATE TABLE {}.{}",ident(database)?,ident(table)?)).await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?;
 let status=query(conn,audit,&format!("SHOW TABLE STATUS FROM {} LIKE '{table}'",ident(database)?)).await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?;
 for kind in ["COLUMN","ROLLUP"] {if !query(conn,audit,&format!("SHOW ALTER TABLE {kind} FROM {}",ident(database)?)).await?.is_empty(){return Err("pending source schema mutation".into())}}
 if !query(conn,audit,&format!("SHOW ROUTINE LOAD FROM {}",ident(database)?)).await?.is_empty(){return Err("pending source routine ingestion".into())}
 let loads=query(conn,audit,&format!("SHOW LOAD FROM {}",ident(database)?)).await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?;
 if loads.iter().any(|r|r.get(2).map(String::as_str)!=Some("FINISHED")){return Err("pending source load".into())}
 let mut parts=Vec::new();for r in rows{if r.len()<20 || r[5]!="NORMAL"{return Err("partition metadata unavailable".into())} parts.push(json!({"PartitionId":r[0],"PartitionName":r[1],"VisibleVersion":r[2],"State":r[5],"PartitionKey":r[6],"Range":r[7],"RowCount":r[17],"DataVersion":r[18],"VersionEpoch":r[19]}));}
 parts.sort_by(|a,b|a["PartitionName"].as_str().cmp(&b["PartitionName"].as_str()));
 Ok(json!({"protocol":"oo.starrocks.partition-meta/v1","ddl":ddl.first().and_then(|r|r.get(1)).ok_or("DDL unavailable")?,"create_time":status.first().and_then(|r|r.get(11)).ok_or("create time unavailable")?,"partitions":parts,"pending":{"alter_column":0,"alter_rollup":0,"routine_load":0,"load_nonfinished":0}}))
}
async fn run(cfg:&Json,out:&Path,audit:&Audit)->Result<(),String>{
 let source=&cfg["source"];let strv=|k:&str|source[k].as_str().ok_or_else(||format!("missing source.{k}"));
 let database=strv("database")?;ident(database)?;let user=strv("user")?;ident(user)?;
 let port=source["port"].as_u64().filter(|n|*n>0 && *n<=65535).ok_or("invalid source port")? as u16;
 let mut password=fs::read_to_string(strv("password_file")?).map_err(|_|"password file unavailable")?;
 let opts=OptsBuilder::default().ip_or_hostname(strv("host")?).tcp_port(port).db_name(Some(database)).user(Some(user)).pass(Some(password.trim_end().to_string())).prefer_socket(false).stmt_cache_size(0);password.zeroize();
 let mut conn=Conn::new(opts).await.map_err(|_|"native connect failed")?;audit.handshake("business",conn.id());
 let version=query(&mut conn,audit,"SELECT current_version(), @@version_comment, @@version").await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?;
 let present=query(&mut conn,audit,"SELECT LOWER(table_name) FROM information_schema.tables WHERE LOWER(table_schema) = 'information_schema' AND LOWER(table_name) IN ('be_tablets','tables_config') ORDER BY 1").await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?.into_iter().flatten().collect::<Vec<_>>();
 let v=version.first().filter(|v|v.len()==3).ok_or("missing engine identity")?;
 if !identity::decide(&v[0],&v[1],&v[2],&present,Some(strv("expected_build")?)).ok{return Err("StarRocks product identity refused".into())}
 let grants=query(&mut conn,audit,"SHOW GRANTS").await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?.into_iter().flatten().collect::<Vec<_>>();
 for grant in &grants {let g=grant.to_ascii_uppercase();if g.contains("ALL PRIVILEGES") || g.contains("WITH GRANT OPTION") || ["INSERT","UPDATE","DELETE","CREATE","DROP","ALTER","OPERATE","ALL PRIVILEGES"].iter().any(|w|g.split(|c:char|!c.is_ascii_alphanumeric() && c!='_').any(|x|x==*w)){return Err("source reader has write privileges".into())}}
 let metadata_sql=format!("SELECT TABLE_NAME,COLUMN_NAME,DATA_TYPE,COLUMN_TYPE,IS_NULLABLE,ORDINAL_POSITION FROM information_schema.columns WHERE TABLE_SCHEMA = '{database}' ORDER BY TABLE_NAME,ORDINAL_POSITION");
 let before=query(&mut conn,audit,&metadata_sql).await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?;
 let models=query(&mut conn,audit,&format!("SELECT TABLE_NAME,TABLE_MODEL,IFNULL(PRIMARY_KEY,''),IFNULL(PARTITION_KEY,''),IFNULL(DISTRIBUTE_KEY,''),IFNULL(SORT_KEY,'') FROM information_schema.tables_config WHERE TABLE_SCHEMA = '{database}'")).await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?;
 let tables=cfg["tables"].as_array().filter(|t|!t.is_empty() && t.len()<=32).ok_or("approved table selection required")?;
 let max_rows=cfg["budget"]["max_rows"].as_u64().filter(|n|*n>0 && *n<=1_000_000).ok_or("row budget required")?;
 let max_bytes=cfg["budget"]["max_bytes"].as_u64().filter(|n|*n>0 && *n<=268_435_456).ok_or("byte budget required")?;
 let mut outputs=Vec::new();let mut native_columns=Vec::new();let mut objects=Vec::new();let mut partition_meta=serde_json::Map::new();
 for table in tables{
  let name=table["name"].as_str().ok_or("table name required")?;ident(name)?;
  let fields=table["columns"].as_array().filter(|a|!a.is_empty() && a.len()<=512).ok_or("approved columns required")?;
  let base=table["derived_from"].as_str().unwrap_or(name);ident(base)?;
  let model=models.iter().find(|r|r.first().is_some_and(|n|n==base)).ok_or("native table not found")?;
  let model_kind=if base==name {plan::parse_key_model(&model[1]).map_err(|e|e.to_string())?} else {plan::KeyModel::Aggregate};
  let mut columns=Vec::new();let mut seen=std::collections::BTreeSet::new();
  for field in fields{
   let field=field.as_str().or_else(||field["name"].as_str()).ok_or("column string required")?;ident(field)?;if !seen.insert(field){return Err("duplicate column selection".into())}
   if base!=name {
    let spec=fields.iter().find(|v|v["name"]==field).ok_or("derived column contract required")?;let native=spec["native_type"].as_str().ok_or("derived type required")?;
    let TypePlan::Arrow(kind)=plan::classify_type(native,native) else{return Err("unsupported derived result type".into())};columns.push(ColumnOut{name:field.into(),nullable:spec["nullable"].as_bool().ok_or("derived nullability required")?,kind,native_type:native.into()});continue;
   }
   let m=before.iter().find(|r|r.len()==6 && r[0]==name && r[1]==field).ok_or("unapproved or missing native column")?;
   let TypePlan::Arrow(kind)=plan::classify_type(&m[2],&m[3]) else{return Err("selected native type is unsafe or unsupported".into())};
   columns.push(ColumnOut{name:field.into(),nullable:m[4].eq_ignore_ascii_case("YES"),kind,native_type:m[3].clone()});
   native_columns.push(json!({"table":name,"name":field,"data_type":m[2],"column_type":m[3],"nullable":m[4].eq_ignore_ascii_case("YES"),"approved":true}));
  }
  let selected=columns.iter().map(|c|ident(&c.name)).collect::<Result<Vec<_>,_>>()?.join(",");
  let mut partition_clause=String::new();let mut metadata=None;
  if table["method"]=="C4" {
   if base!=name || model_kind!=plan::KeyModel::Duplicate{return Err("C4 requires a native Duplicate Key partition table".into())}
   let part=table["partition"].as_str().ok_or("C4 partition required")?;ident(part)?;let actual=partitions(&mut conn,audit,database,name).await?;
   let observed=table["observed_partitions"].as_array().ok_or("complete approved partition set required")?;let mut observed=observed.iter().map(|v|v.as_str().ok_or("partition identifier required")).collect::<Result<Vec<_>,_>>()?;observed.sort();
   let actual_ids=actual["partitions"].as_array().unwrap().iter().map(|v|v["PartitionName"].as_str().unwrap()).collect::<Vec<_>>();if observed!=actual_ids || !observed.contains(&part){return Err("partition set drift".into())}
   partition_clause=format!(" PARTITION ({})",ident(part)?);metadata=Some(actual);
  }
  let sql=if base!=name {table["sql"].as_str().ok_or("derived read-only SQL required")?.to_owned()}else{format!("SELECT {selected} FROM {}.{}{partition_clause} ORDER BY {selected} LIMIT {}",ident(database)?,ident(name)?,max_rows+1)};
  let class=audit.sql("business",&sql,SqlMode::ReadOnly).map_err(|e|e.to_string())?;audit.note_connection("business",class,conn.id());let mut result=conn.query_iter(&sql).await.map_err(|_|"native read failed")?;
  let mut rows=Vec::new();let mut bytes=0;
  while let Some(row)=result.next().await.map_err(|_|"native row failed")?{
   if rows.len() as u64==max_rows{return Err("row budget exceeded; no complete capture".into())}
   let values=row.unwrap();if values.len()!=columns.len(){return Err("native column width changed".into())}
   let row=values.iter().zip(&columns).map(|(v,c)|cell(v,c)).collect::<Result<Vec<_>,_>>()?;
   bytes+=encode::estimate_row_bytes(&row);if bytes>max_bytes{return Err("byte budget exceeded; no complete capture".into())}rows.push(row);
  }
  result.drop_result().await.map_err(|_|"native result close failed")?;
  if let Some(before)=metadata {let after=partitions(&mut conn,audit,database,name).await?;if before!=after{return Err("partition metadata drift during read".into())}partition_meta.insert(name.into(),after);}
  let mut meta=HashMap::new();meta.insert("oo.source.protocol".into(),"oo.source.starrocks/v1".into());meta.insert("oo.starrocks.product_version".into(),v[0].clone());meta.insert("oo.starrocks.table".into(),name.into());meta.insert("oo.starrocks.database".into(),database.into());meta.insert("oo.starrocks.key_model".into(),model_kind.as_str().into());meta.insert("oo.starrocks.constraint_trust".into(),plan::trust_for(model_kind).as_str().into());
  let ipc=encode::write_ipc(&columns,&rows,meta,3).map_err(|e|e.to_string())?;outputs.push((name.to_owned(),ipc,rows.len()));objects.push(json!({"name":name,"key_model":model_kind.as_str(),"constraint_trust":plan::trust_for(model_kind).as_str(),"capture_method":table["method"].as_str().unwrap_or("C5"),"logical_dataset_kind":if base==name{"native_table"}else{"derived_projection"},"derived_from":if base==name{Json::Null}else{json!(base)},"physical_hints":{"primary_key":model[2],"partition_key":model[3],"distribute_key":model[4],"sort_key":model[5]}}));
 }
 let after=query(&mut conn,audit,&metadata_sql).await?.into_iter().map(row_text).collect::<Result<Vec<_>,_>>()?;if before!=after{return Err("schema changed during capture".into())}
 audit.cleanup("business",conn.id());conn.disconnect().await.map_err(|_|"native disconnect failed")?;
 for (name,ipc,_) in &outputs {fs::write(out.join(format!("{name}.arrow")),ipc).map_err(|_|"Arrow output failed")?;}
 let catalog=json!({"protocol":"oo.source.starrocks/v1","product":"StarRocks","product_version":v[0],"version_comment":v[1],"wire_protocol_version":v[2],"wire_is_not_product":true,"consistency_grade":"B","capture_method":if tables.iter().any(|t|t["method"]=="C4"){"mixed_C4_C5"}else{"C5"},"cross_table_consistent":false,"cdc":"absent","native_columns":native_columns,"objects":objects,"reader_user":user,"reader_grants":grants,"business_connections":1,"control_connections":0,"database":database});
 fs::write(out.join("catalog.json"),serde_json::to_vec_pretty(&catalog).unwrap()).map_err(|_|"catalog output failed")?;
 fs::write(out.join("partition-metadata.json"),serde_json::to_vec_pretty(&partition_meta).unwrap()).map_err(|_|"partition metadata output failed")?;
 fs::write(out.join("ready.json"),serde_json::to_vec_pretty(&json!({"protocol":"oo.source.starrocks/v1","ready":true,"tables":outputs.iter().map(|(n,b,r)|json!({"name":n,"rows":r,"ipc_bytes":b.len()})).collect::<Vec<_>>()})).unwrap()).map_err(|_|"ready output failed")?;Ok(())
}
#[tokio::main]
async fn main(){
 let args=std::env::args().skip(1).collect::<Vec<_>>();if args.len()!=4 || args[0]!="--config" || args[2]!="--out"{eprintln!("usage: --config <connection config> --out <new output>");std::process::exit(2)}
 let result=(||{let bytes=fs::read(&args[1]).map_err(|_|"config unavailable")?;let cfg=serde_json::from_slice::<Json>(&bytes).map_err(|_|"invalid config")?;if Path::new(&args[3]).exists(){return Err("output already exists")};fs::create_dir_all(&args[3]).map_err(|_|"output unavailable")?;Ok(cfg)})();
 let cfg=match result{Ok(c)=>c,Err(e)=>{eprintln!("{e}");std::process::exit(2)}};let audit=Audit::new();let ms=cfg["budget"]["deadline_ms"].as_u64().filter(|n|*n>0 && *n<=120_000).unwrap_or(60_000);
 let result=tokio::time::timeout(Duration::from_millis(ms),run(&cfg,Path::new(&args[3]),&audit)).await;
 let _=fs::write(Path::new(&args[3]).join("audit.jsonl"),audit.to_jsonl());let _=fs::write(Path::new(&args[3]).join("audit-counts.json"),audit.counts().to_string());
 if !matches!(result,Ok(Ok(()))){eprintln!("capture refused: {}",match result{Ok(Err(e))=>e,_=>"deadline exceeded".into()});std::process::exit(1)}
 println!("capture complete; native engine, configured connection, approved fields, bounded typed Arrow")
}
#[cfg(test)]mod tests{use super::*;#[test]fn trust_boundary(){assert!(ident("sales_2026").is_ok());assert!(ident("db;DROP").is_err());let c=ColumnOut{name:"amount".into(),nullable:true,kind:ArrowKind::Decimal{precision:18,scale:4},native_type:"decimal(18,4)".into()};assert_eq!(cell(&Value::Bytes(b"1.2300".to_vec()),&c).unwrap(),Cell::Decimal(12300));assert!(cell(&Value::Float(1.0),&c).is_err());assert!(cell(&Value::Bytes(b"1.23456".to_vec()),&c).is_err());}}
