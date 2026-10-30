use arrow_ipc::reader::StreamReader;
use mysql_async::{Conn, Opts, prelude::Queryable};
use mysql_preview::c5_layered::{connect, produce};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};

async fn cycle(
    config: &Value,
    authority: &mut Value,
    target: &mut BTreeMap<String, Vec<Vec<String>>>,
    pin: &mut u64,
    out: &std::path::Path,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    loop {
        *pin += 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;
        let due = config
            .get("due")
            .cloned()
            .unwrap_or(json!([true, true, true, false]));
        let input = json!({"protocol":"oo.jobs.native-producer/v1","method":"C5","c5_mode":"layered","source_id":"fixture","binding":"00000000-0000-4000-8000-000000000001","pin":*pin,
            "acknowledged_source_generation":authority["source_generation"],"c5_due":due,"c5_observed_at_ms":now,
            "c5_layered":{"policy":{"duty_per_mille":1000,"threads_running_max":32,"execution_window_utc":[0,1440]},"recent_column":"biz","window_ms":60000,"overlap_ms":60000,"server_timeout_ms":5000,"client_timeout_ms":10000,"execution_timezone":"UTC"}});
        let request = produce(connect(config).await?, config, &input, authority, out).await?;
        if request["protocol"] == "oo.jobs.native-c5-nochange/v1" {
            authority["next_chunk_index"] = json!(0);
            return Ok(request);
        }
        assert_eq!(request["protocol"], "oo.pipeline.native-c5-range/v1");
        assert_eq!(request["source_engine"], "mysql");
        assert_eq!(request["capture_method"], "c5_diff");
        assert_eq!(request["delete_detection"], "reconcile");
        assert_eq!(request["position_codec"], "snapshot-zero/v1");
        assert_eq!(request["applied_pos"], "0");
        assert_eq!(request["parent_generation"], authority["source_generation"]);
        assert_eq!(
            request["source_generation"].as_u64().unwrap(),
            authority["source_generation"].as_u64().unwrap() + 1
        );
        for field in [
            "source_engine",
            "capture_method",
            "source_id",
            "schema",
            "table",
            "schema_fingerprint",
            "chunk_topology_revision",
            "chunk_id",
            "lo",
            "hi",
            "prior_membership_revision",
            "parent_generation",
            "source_generation",
            "arrow_sha256",
            "row_count",
            "delete_detection",
            "position_codec",
            "applied_pos",
        ] {
            assert!(request.get(field).is_some(), "{field}");
        }
        let mut source = connect(config).await?;
        let lo = mysql_preview::c5_layered::decode_bound(&request["lo"])?;
        let hi = mysql_preview::c5_layered::decode_bound(&request["hi"])?.unwrap();
        let cell = |v: &oo_connectors::mysql::chunk::KeyValue| match v {
            oo_connectors::mysql::chunk::KeyValue::Int(n) => mysql_async::Value::Int(*n as i64),
            oo_connectors::mysql::chunk::KeyValue::Text(b)
            | oo_connectors::mysql::chunk::KeyValue::Bytes(b) => {
                mysql_async::Value::Bytes(b.clone())
            }
        };
        let (predicate, values) = if let Some(lo) = lo {
            ("id > ? AND id <= ?", vec![cell(&lo[0]), cell(&hi[0])])
        } else {
            ("id <= ?", vec![cell(&hi[0])])
        };
        let expected: Vec<String> = source
            .exec(
                format!(
                    "SELECT CAST(id AS CHAR) FROM layered_fixture.{} WHERE {predicate} ORDER BY id",
                    config["table"].as_str().unwrap()
                ),
                mysql_async::Params::Positional(values),
            )
            .await?;
        source.disconnect().await?;
        let bytes = fs::read(request["arrow_path"].as_str().unwrap())?;
        assert_eq!(sha256_hex(&bytes), request["arrow_sha256"]);
        let reader = StreamReader::try_new(std::io::Cursor::new(&bytes), None)?;
        let mut rows: Vec<Vec<String>> = Vec::new();
        for batch in reader {
            let b = batch?;
            for row in 0..b.num_rows() {
                rows.push(
                    (0..b.num_columns())
                        .map(|col| format!("{:?}", b.column(col).slice(row, 1)))
                        .collect(),
                );
            }
        }
        assert_eq!(rows.len() as u64, request["row_count"].as_u64().unwrap());
        assert_eq!(rows.len(), expected.len());
        for (row, key) in rows.iter().zip(expected) {
            assert!(
                row[0]
                    .lines()
                    .any(|line| line.trim().trim_end_matches(',').trim_matches('"') == key),
                "key mismatch: {:?} {key}",
                row[0]
            );
        }
        let id = request["chunk_id"].as_str().unwrap().to_string();
        target.insert(id.clone(), rows);
        let mut chunk = request["c5_observation"]["chunk"].clone();
        let chunks = authority["chunks"].as_array_mut().unwrap();
        if let Some(old) = chunks.iter_mut().find(|c| c["chunk_id"] == id) {
            assert_eq!(
                old["membership_revision"],
                request["prior_membership_revision"]
            );
            *old = chunk.take();
        } else {
            chunks.push(chunk);
        }
        authority["next_chunk_index"] = request["c5_observation"]["next_chunk_index"].clone();
        authority["schema_fingerprint"] = request["schema_fingerprint"].clone();
        authority["source_generation"] = request["source_generation"].clone();
        authority["input_digest"] = request["arrow_sha256"].clone();
        fs::write(
            out.join(format!(
                "mock-loader-{}-{}.json",
                config["table"].as_str().unwrap(),
                request["source_generation"].as_u64().unwrap()
            )),
            serde_json::to_vec_pretty(&request)?,
        )?;
        if config["interrupt_after_one"] == true {
            return Ok(request);
        }
        if *pin > 100 {
            panic!("nonconvergent producer");
        }
    }
}

#[tokio::test]
async fn owned_mysql57_layered_rounds_and_mock_range_loader()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = std::env::temp_dir().join(format!("oo-wf-c5int-live-{}", std::process::id()));
    fs::create_dir_all(&root)?;
    fs::write(root.join("reader"), "fixture-reader")?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(root.join("reader"), fs::Permissions::from_mode(0o600))?;
    let mut admin = Conn::new(Opts::from_url("mysql://root:fixture-only@127.0.0.1:23361")?).await?;
    admin
        .query_drop("CREATE DATABASE IF NOT EXISTS layered_fixture")
        .await?;
    admin
        .query_drop("CREATE USER IF NOT EXISTS 'layer_reader'@'%' IDENTIFIED BY 'fixture-reader'")
        .await?;
    admin
        .query_drop("GRANT SELECT,LOCK TABLES,SHOW VIEW ON layered_fixture.* TO 'layer_reader'@'%'")
        .await?;
    admin
        .query_drop(
            "GRANT PROCESS,REPLICATION SLAVE,REPLICATION CLIENT ON *.* TO 'layer_reader'@'%'",
        )
        .await?;
    for (table, key) in [
        ("ints", "BIGINT"),
        ("strings", "VARCHAR(40) COLLATE utf8mb4_unicode_ci"),
    ] {
        admin
            .query_drop(format!("DROP TABLE IF EXISTS layered_fixture.{table}"))
            .await?;
        admin.query_drop(format!("CREATE TABLE layered_fixture.{table}(id {key} PRIMARY KEY,v VARCHAR(40),biz DATETIME,INDEX(biz)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4")).await?;
        admin.query_drop(format!("INSERT INTO layered_fixture.{table} VALUES ('1','one',NOW()),('2','two','2020-01-01'),('3','three',NOW())")).await?;
        let mut config = json!({"source_id":"fixture","schema":"layered_fixture","table":table,"max_rows":2,"max_bytes":1048576,"connection":{"host":"127.0.0.1","port":23361,"user":"layer_reader","secret_file":root.join("reader"),"tls":"disabled","plaintext_authorized":true},"target":{"database":"mock"}});
        let mut authority = json!({"source_id":"fixture","schema":"layered_fixture","table":table,"source_generation":0,"chunk_topology_revision":1,"chunks":[]});
        let mut target = BTreeMap::new();
        let mut pin = 0;
        let first = cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        assert_eq!(first["data_gen"], false);
        assert_eq!(target.values().map(Vec::len).sum::<usize>(), 3);
        println!("{table}: initial PASS");
        let before = authority["source_generation"].clone();
        admin
            .query_drop(format!(
                "UPDATE layered_fixture.{table} SET v='outside-window',biz='2020-01-01' WHERE id IN ('1','2')"
            ))
            .await?;
        config["due"] = json!([true, true, false, false]);
        cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        assert_eq!(authority["source_generation"], before);
        config["due"] = json!([false, false, true, false]);
        cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        assert!(format!("{target:?}").contains("outside-window"));
        println!("{table}: L2 miss / L3 repair PASS");
        config["due"] = json!([true, true, true, false]);

        for (name, sql, count) in [
            (
                "insert",
                format!("INSERT INTO layered_fixture.{table} VALUES ('4','four',NOW())"),
                4,
            ),
            (
                "update",
                format!("UPDATE layered_fixture.{table} SET v='changed' WHERE id='2'"),
                4,
            ),
            (
                "delete",
                format!("DELETE FROM layered_fixture.{table} WHERE id='2'"),
                3,
            ),
            (
                "key reuse",
                format!("INSERT INTO layered_fixture.{table} VALUES ('2','replacement',NOW())"),
                4,
            ),
        ] {
            if name == "insert" {
                config["due"] = json!([true, true, false, false]);
            } else {
                config["due"] = json!([true, true, true, false]);
            }
            admin.query_drop(sql).await?;
            cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
            assert_eq!(target.values().map(Vec::len).sum::<usize>(), count);
            if name == "update" {
                assert!(format!("{target:?}").contains("changed"));
            }
            if name == "key reuse" {
                assert!(format!("{target:?}").contains("replacement"));
                assert!(!format!("{target:?}").contains("changed"));
            }
            println!("{table}: {name} PASS");
        }
        admin
            .query_drop(format!(
                "UPDATE layered_fixture.{table} SET v=CONCAT(v,'-resumed')"
            ))
            .await?;
        config["interrupt_after_one"] = json!(true);
        let partial = cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        let completed_id = partial["chunk_id"].as_str().unwrap().to_string();
        let revision = partial["c5_observation"]["chunk"]["membership_revision"].clone();
        let persisted = serde_json::to_vec(&authority)?;
        fs::remove_file(root.join("cache.json"))?;
        authority = serde_json::from_slice(&persisted)?;
        config["interrupt_after_one"] = json!(false);
        cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        assert_eq!(
            authority["chunks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["chunk_id"] == completed_id)
                .unwrap()["membership_revision"],
            revision
        );
        println!("{table}: interrupted scan receipt resume PASS");
        admin
            .query_drop(format!(
                "DELETE FROM layered_fixture.{table} WHERE id IN ('1','2')"
            ))
            .await?;
        cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        assert!(target["0"].is_empty());
        println!("{table}: empty block PASS");
        let generation = authority["source_generation"].clone();
        cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        assert_eq!(generation, authority["source_generation"]);
        println!("{table}: nochange PASS");
        config["due"] = json!([false, false, false, true]);
        cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        assert_eq!(generation, authority["source_generation"]);
        println!("{table}: L4 full read without false data generation PASS");
        config["due"] = json!([true, true, true, false]);

        let restored: Value = serde_json::from_slice(&serde_json::to_vec(&authority)?)?;
        authority = restored;
        cycle(&config, &mut authority, &mut target, &mut pin, &root).await?;
        println!("{table}: authority resume PASS");
        let cli_root = root.join(format!("cli-{table}"));
        fs::create_dir(&cli_root)?;
        let mut cli_config = config.clone();
        cli_config["state_dir"] = json!(cli_root);
        cli_config["authority_file"] = json!(root.join(format!("{table}-authority.json")));
        fs::write(
            root.join(format!("{table}-authority.json")),
            serde_json::to_vec(&authority)?,
        )?;
        let cli_path = root.join(format!("{table}-config.json"));
        fs::write(&cli_path, serde_json::to_vec(&cli_config)?)?;
        let cli_input = json!({"protocol":"oo.jobs.native-producer/v1","method":"C5","c5_mode":"layered","source_id":"fixture","binding":"00000000-0000-4000-8000-000000000001","pin":999,"acknowledged_source_generation":authority["source_generation"],"c5_due":[true,true,true,false],"c5_observed_at_ms":1800000000000u64,
            "c5_layered":{"policy":{"duty_per_mille":1000,"threads_running_max":32,"execution_window_utc":[0,1440]},"recent_column":"biz","window_ms":60000,"overlap_ms":60000,"server_timeout_ms":5000,"client_timeout_ms":10000,"execution_timezone":"UTC"}});
        let invoke = |input: &Value| -> std::process::Output {
            use std::io::Write;
            let mut process = std::process::Command::new(env!("CARGO_BIN_EXE_mysql-preview"))
                .args(["c5-layered", "--config", cli_path.to_str().unwrap()])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            process
                .stdin
                .take()
                .unwrap()
                .write_all(&serde_json::to_vec(input).unwrap())
                .unwrap();
            process.wait_with_output().unwrap()
        };
        let first = invoke(&cli_input);
        assert!(first.status.success(), "{:?}", first);
        let replay = invoke(&cli_input);
        assert!(replay.status.success());
        assert_eq!(first.stdout, replay.stdout);
        let mut tampered = cli_input.clone();
        tampered["c5_due"] = json!([true, true, true, true]);
        assert!(!invoke(&tampered).status.success());
        println!("{table}: stdio immutable pin replay PASS");
        admin
            .query_drop(format!(
                "ALTER TABLE layered_fixture.{table} ADD COLUMN extra INT"
            ))
            .await?;
        let err = cycle(&config, &mut authority, &mut target, &mut pin, &root)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("SCHEMA_DRIFT"));
        let mut drift_input = cli_input.clone();
        drift_input["pin"] = json!(1000);
        let drift = invoke(&drift_input);
        assert!(!drift.status.success());
        assert!(String::from_utf8_lossy(&drift.stderr).contains("SCHEMA_DRIFT"));
        assert!(
            cli_root
                .join("00000000-0000-4000-8000-000000000001.schema-drift.json")
                .exists()
        );
        assert!(String::from_utf8_lossy(&invoke(&cli_input).stderr).contains("SCHEMA_DRIFT"));
        println!("{table}: SCHEMA_DRIFT PASS");
    }
    admin.disconnect().await?;
    fs::remove_dir_all(root)?;
    Ok(())
}
