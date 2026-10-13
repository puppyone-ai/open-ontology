#![forbid(unsafe_code)]
//! Source-owned, read-only partition metadata guard for a frozen C4 occurrence.
use std::{env, fs, process::ExitCode};

use mysql_async::{prelude::Queryable, Conn, OptsBuilder, Row, Value};
use serde_json::{json, Value as Json};
use zeroize::Zeroize;

#[tokio::main]
async fn main() -> ExitCode {
    match inspect().await {
        Ok(value) => {
            println!("{}", value);
            ExitCode::SUCCESS
        }
        Err(_) => {
            eprintln!("starrocks-partition-meta: source metadata unavailable");
            ExitCode::FAILURE
        }
    }
}

async fn inspect() -> Result<Json, ()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 4 || args[0] != "--host" || args[2] != "--password-file" {
        return Err(());
    }
    let mut password = fs::read_to_string(&args[3]).map_err(|_| ())?;
    let opts = OptsBuilder::default()
        .ip_or_hostname(args[1].clone())
        .tcp_port(9030)
        .user(Some("oo_w45_reader".to_string()))
        .pass(Some(password.trim().to_string()))
        .db_name(Some("oo_w45_sr".to_string()))
        .prefer_socket(false)
        .stmt_cache_size(0);
    let mut conn = tokio::time::timeout(std::time::Duration::from_secs(15), Conn::new(opts))
        .await.map_err(|_| ())?.map_err(|_| ())?;
    password.zeroize();
    let parts = rows(&mut conn, "SHOW PARTITIONS FROM oo_w45_sr.dup_events").await?;
    let ddl = rows(&mut conn, "SHOW CREATE TABLE oo_w45_sr.dup_events").await?;
    let status = rows(&mut conn, "SHOW TABLE STATUS FROM oo_w45_sr LIKE 'dup_events'").await?;
    let alters = rows(&mut conn, "SHOW ALTER TABLE COLUMN FROM oo_w45_sr").await?;
    let rollups = rows(&mut conn, "SHOW ALTER TABLE ROLLUP FROM oo_w45_sr").await?;
    let routine = rows(&mut conn, "SHOW ROUTINE LOAD FROM oo_w45_sr").await?;
    let loads = rows(&mut conn, "SHOW LOAD FROM oo_w45_sr").await?;
    let _ = conn.disconnect().await;
    if !alters.is_empty() || !rollups.is_empty() || !routine.is_empty()
        || loads.iter().any(|row| row.get(2).map(String::as_str) != Some("FINISHED")) {
        return Err(());
    }
    let ddl = ddl.first().and_then(|row| row.get(1)).ok_or(())?;
    let create_time = status.first().and_then(|row| row.get(11)).ok_or(())?;
    let mut partitions = Vec::new();
    for row in parts {
        if row.len() < 20 || row[5] != "NORMAL" || row[6] != "bucket" {
            return Err(());
        }
        partitions.push(json!({"PartitionId":row[0],"PartitionName":row[1],
            "VisibleVersion":row[2],"State":row[5],"PartitionKey":row[6],
            "Range":row[7],"RowCount":row[17],"DataVersion":row[18],
            "VersionEpoch":row[19]}));
    }
    partitions.sort_by(|a,b| a["PartitionName"].as_str().cmp(&b["PartitionName"].as_str()));
    if partitions.len() != 2 { return Err(()); }
    Ok(json!({"protocol":"oo.starrocks.partition-meta/v1","ddl":ddl,
        "create_time":create_time,"partitions":partitions,
        "pending":{"alter_column":0,"alter_rollup":0,"routine_load":0,"load_nonfinished":0}}))
}

async fn rows(conn: &mut Conn, sql: &str) -> Result<Vec<Vec<String>>, ()> {
    let raw: Vec<Row> = conn.query(sql).await.map_err(|_| ())?;
    raw.into_iter().map(|row| row.unwrap().into_iter().map(cell).collect()).collect()
}

fn cell(value: Value) -> Result<String, ()> {
    Ok(match value {
        Value::NULL => "NULL".to_string(),
        Value::Bytes(bytes) => String::from_utf8(bytes).map_err(|_| ())?,
        Value::Int(value) => value.to_string(),
        Value::UInt(value) => value.to_string(),
        Value::Date(y,m,d,hh,mm,ss,us) => {
            if us != 0 {return Err(());}
            format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}")
        }
        _ => return Err(()),
    })
}
