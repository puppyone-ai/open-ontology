//! Authorized customer metadata diagnostic; no source DML or business rows.
use mysql_async::{Conn, OptsBuilder, prelude::Queryable};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 { std::process::exit(2); }
    let targets: Value = serde_json::from_slice(&fs::read(&args[1]).expect("target config unavailable")).expect("target config invalid");
    let path = Path::new(targets["credential_file"].as_str().expect("credential reference required"));
    assert_eq!(fs::metadata(path).expect("credential unavailable").permissions().mode() & 0o777, 0o600, "private credential permissions");
    let secret: Value = serde_json::from_slice(&fs::read(path).expect("credential unavailable")).expect("credential format invalid");
    let mut reports = Vec::new();
    for (index, target) in targets["targets"].as_array().expect("targets required").iter().enumerate() {
        let opts = OptsBuilder::default().ip_or_hostname(target["host"].as_str().expect("host required"))
            .tcp_port(target["port"].as_u64().unwrap_or(3306) as u16)
            .user(secret["username"].as_str().map(str::to_owned))
            .pass(secret["password"].as_str().map(str::to_owned)).prefer_socket(false);
        let mut report = json!({"target_index":index,"database_selected_on_connect":false,"source_write_attempts":0,"business_rows_read":0,"tls_requested":false,"platform_admitted_reader":false});
        match tokio::time::timeout(Duration::from_secs(10), Conn::new(opts)).await {
            Ok(Ok(mut conn)) => {
                report["auth_ok"] = json!(true);
                // Count authorized table metadata only. No USE, SET, grant expansion or data query.
                let result = tokio::time::timeout(Duration::from_secs(5), conn.exec_first::<u64,_,_>(
                    "SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA = ?", (target["database"].as_str().unwrap_or(""),))).await;
                match result {
                    Ok(Ok(count)) => report["visible_table_count"] = json!(count),
                    Ok(Err(err)) => report["metadata_error"] = diagnostic(&err),
                    Err(_) => report["metadata_error"] = json!({"kind":"timeout"}),
                }
                let inventory = tokio::time::timeout(Duration::from_secs(5), conn.query::<(String,u64),_>(
                    "SELECT TABLE_SCHEMA,COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA NOT IN ('mysql','sys','information_schema','performance_schema') GROUP BY TABLE_SCHEMA ORDER BY TABLE_SCHEMA")).await;
                if let Ok(Ok(rows)) = inventory {
                    report["visible_non_system_schema_count"] = json!(rows.len());
                    let private = Path::new(&args[2]).with_extension(format!("target{index}.private.json"));
                    fs::write(&private, serde_json::to_vec(&rows).expect("private metadata serialization")).expect("private metadata write");
                    fs::set_permissions(&private, fs::Permissions::from_mode(0o600)).expect("private metadata permissions");
                }
                let _ = conn.disconnect().await;
            }
            Ok(Err(err)) => {report["auth_ok"] = json!(false); report["connect_error"] = diagnostic(&err);}
            Err(_) => {report["auth_ok"] = json!(false); report["connect_error"] = json!({"kind":"timeout"});}
        }
        reports.push(report);
    }
    let bytes = serde_json::to_vec_pretty(&json!({"protocol":"oo.mysql-native-host-metadata/v1","targets":reports})).expect("summary serialization");
    fs::write(&args[2], &bytes).expect("summary write");
    println!("{}", String::from_utf8(bytes).expect("summary utf8"));
}

fn diagnostic(error: &mysql_async::Error) -> Value {
    match error {
        mysql_async::Error::Server(server) => json!({"kind":"server","code":server.code,"sqlstate":server.state}),
        _ => json!({"kind":"driver"}),
    }
}
