//! Source-only continuation: real ROW/FULL/GTID; independent row/operation oracle.
//! No target ACK, publication, worker or Gateway acceptance is claimed.
use mysql_async::{Conn, OptsBuilder, prelude::Queryable};
use mysql_preview::c1_typed_source::Capture;
use oo_pipeline::{
    c1_typed::Batch,
    native_arrow::{NativeCell, decode_arrow_stream},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf, time::{Duration, SystemTime, UNIX_EPOCH}};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Row = Vec<Option<String>>;

fn value(cell: &NativeCell) -> Option<String> {
    Some(match cell {
        NativeCell::Null => return None,
        NativeCell::Int { value, .. } => value.to_string(),
        NativeCell::Utf8(s) => s.clone(),
        NativeCell::Binary(b) => b.iter().map(|v|format!("{v:02x}")).collect(),
        NativeCell::Decimal { coefficient, scale, .. } if *scale == 2 =>
            format!("{}.{:02}",coefficient/100,coefficient%100),
        other => panic!("unexpected cell {other:?}"),
    })
}

fn apply_source_batch(batch: &Batch, rows: &mut BTreeMap<(String,String),Row>) -> [u64;3] {
    let mut ops=[0;3];
    for part in &batch.parts {
        let decoded=decode_arrow_stream(&fs::read(&part.path).unwrap()).unwrap();
        for n in 0..decoded.rows {
            let row=(0..5).map(|i|value(&decoded.columns[i][n])).collect::<Row>();
            let key=(row[0].clone().unwrap(),row[1].clone().unwrap());
            let NativeCell::Utf8(op)=&decoded.columns[6][n] else {panic!("op")};
            if op=="delete" {assert!(rows.remove(&key).is_some());ops[2]+=1;}
            else {
                assert_eq!(op,"upsert");
                if rows.insert(key,row).is_some() {ops[1]+=1;} else {ops[0]+=1;}
            }
        }
    }
    ops
}

async fn actual(c: &mut Conn, table: &str) -> Result<BTreeMap<(String,String),Row>> {
    let rows:Vec<mysql_async::Row>=c.query(format!(
        "SELECT CAST(id AS CHAR),code,payload,CAST(amount AS CHAR),LOWER(HEX(raw)) FROM c1_typed.{table}")).await?;
    Ok(rows.into_iter().map(|r| {
        let row=(0..5).map(|i|r.get::<Option<String>,_>(i).unwrap()).collect::<Row>();
        ((row[0].clone().unwrap(),row[1].clone().unwrap()),row)
    }).collect())
}

#[tokio::test]
#[ignore = "requires explicitly enabled owned loopback MySQL 5.7.44 fixture"]
async fn typed_real_post_baseline_iud_reconnect_replay() -> Result<()> {
    let home=PathBuf::from(std::env::var("OO_C1_TYPED_HOME")?);
    assert_eq!(home,PathBuf::from("/tmp/oo-mysql-c1-20261027"));
    let evidence=PathBuf::from(std::env::var("OO_C1_CONTINUATION_EVIDENCE")?);
    fs::create_dir_all(&evidence)?;
    let run=SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let table=format!("cont_{run}");
    let mut admin=Conn::new(OptsBuilder::default().ip_or_hostname("127.0.0.1").tcp_port(23327)
        .user(Some("root")).pass(Some(fs::read_to_string(home.join("secrets/mysql-root.pw"))?.trim())))
        .await?;
    let version:String=admin.query_first("SELECT VERSION()").await?.ok_or("version")?;
    assert!(version.starts_with("5.7.44"));
    admin.query_drop(format!("CREATE TABLE c1_typed.{table}(id BIGINT NOT NULL,code VARCHAR(32) NOT NULL,payload VARCHAR(64),amount DECIMAL(12,2) NOT NULL,raw VARBINARY(8) NOT NULL,PRIMARY KEY(code,id)) ENGINE=InnoDB")).await?;
    let mut expected=BTreeMap::new();
    for start in (1..=10000).step_by(1000) {
        let values=(start..start+1000).map(|n|format!("({n},'k{n:05}','b{n}',{n}.25,UNHEX('00ff'))")).collect::<Vec<_>>().join(",");
        admin.query_drop(format!("INSERT INTO c1_typed.{table} VALUES {values}")).await?;
        for n in start..start+1000 {
            expected.insert((n.to_string(),format!("k{n:05}")),
                vec![Some(n.to_string()),Some(format!("k{n:05}")),Some(format!("b{n}")),Some(format!("{n}.25")),Some("00ff".into())]);
        }
    }
    assert_eq!(actual(&mut admin,&table).await?,expected);
    let baseline:String=admin.query_first("SELECT @@GLOBAL.gtid_executed").await?.ok_or("gtid")?;
    let uuid=baseline.split(':').next().unwrap();
    let qualified=format!("c1_typed.{table}");
    let cfg=evidence.join("source-config.json");
    fs::write(&cfg,serde_json::to_vec_pretty(&json!({"database":"c1_typed","host":"127.0.0.1",
        "port":23327,"user":"oo_cdc","password_file":home.join("secrets/cdc.pw"),
        "ca_file":home.join("certs/ca.pem"),"tls_server_name":"localhost",
        "server_id":5791,"health_every_ms":200,"reconcile_every_ms":1000,"preserve_dir":evidence,
        "fixture":"owned-typed-continuation","tables":[qualified],"columns":{&qualified:["id","code","payload","amount","raw"]}}))?)?;
    let oracle=json!({"baseline_rows":10000,"insert_keys":[10001,14000],"update_keys":[1,4000],
        "delete_keys":[4001,6000],"expected_operations":{"I":4000,"U":4000,"D":2000},
        "scope":"source capture only; not a target ACK","baseline_gtid":baseline});
    fs::write(evidence.join("before-dml-oracle.json"),serde_json::to_vec_pretty(&oracle)?)?;
    let mut capture=Capture::continue_from(&cfg,&evidence.join("capture-01"),"owned_continuation",2,&baseline).await?;
    admin.query_drop("START TRANSACTION").await?;
    for start in (10001..=14000).step_by(1000) {
        let values=(start..start+1000).map(|n|format!("({n},'k{n:05}',NULL,{n}.50,UNHEX('ff00'))")).collect::<Vec<_>>().join(",");
        admin.query_drop(format!("INSERT INTO c1_typed.{table} VALUES {values}")).await?;
    }
    admin.query_drop("COMMIT").await?;
    let first=capture.next(1,Duration::from_secs(30)).await?;
    fs::write(evidence.join("unacknowledged-batch.json"),serde_json::to_vec_pretty(&first)?)?;
    drop(capture);
    let mut capture=Capture::continue_from(&cfg,&evidence.join("capture-replay"),"owned_continuation",2,&baseline).await?;
    let replay=capture.next(1,Duration::from_secs(30)).await?;
    assert_eq!(first.digest()?,replay.digest()?);
    assert_eq!(first.position,replay.position);
    assert_eq!(apply_source_batch(&replay,&mut expected),[4000,0,0]);
    let checkpoint=format!("{uuid}:1-{}",replay.position);
    drop(capture);
    admin.query_drop(format!("UPDATE c1_typed.{table} SET payload=CONCAT('u',id),amount=amount+1 WHERE id BETWEEN 1 AND 4000")).await?;
    admin.query_drop(format!("DELETE FROM c1_typed.{table} WHERE id BETWEEN 4001 AND 6000")).await?;
    // Changes committed while disconnected must be consumed from the exact checkpoint.
    let mut capture=Capture::continue_from(&cfg,&evidence.join("capture-resume"),"owned_continuation",2,&checkpoint).await?;
    let update=capture.next(2,Duration::from_secs(30)).await?;
    let delete=capture.next(3,Duration::from_secs(30)).await?;
    assert_eq!(update.previous_position,replay.position);
    assert_eq!(delete.previous_position,update.position);
    assert_eq!(apply_source_batch(&update,&mut expected),[0,4000,0]);
    assert_eq!(apply_source_batch(&delete,&mut expected),[0,0,2000]);
    let observed=actual(&mut admin,&table).await?;
    fs::write(evidence.join("source-rows.json"),serde_json::to_vec(&observed.values().collect::<Vec<_>>())?)?;
    fs::write(evidence.join("captured-rows.json"),serde_json::to_vec(&expected.values().collect::<Vec<_>>())?)?;
    assert_eq!(observed,expected);
    assert_eq!(expected.len(),12000);
    for (name,batch) in [("insert",&replay),("update",&update),("delete",&delete)] {
        fs::write(evidence.join(format!("{name}-batch.json")),serde_json::to_vec_pretty(batch)?)?;
    }
    let receipt:Value=json!({"scope":"source-only real binlog continuation; NOT production worker/target recovery",
        "version":version,"table":qualified,"baseline_gtid":baseline,"final_position":delete.position,
        "baseline_reimported":false,"rows":12000,"typed_cells":60000,"row_differences":0,"value_differences":0,
        "operation_differences":0,"operations":{"I":4000,"U":4000,"D":2000},
        "unacknowledged_source_replay_same_digest":true,"disconnected_commit_catchup":true,
        "actual_target_ack":false,"publication":false,"gateway":false,"retained_table":true});
    fs::write(evidence.join("receipt.json"),serde_json::to_vec_pretty(&receipt)?)?;
    println!("{receipt}");
    Ok(())
}
