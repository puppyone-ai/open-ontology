use super::*;
use mysql_async::{OptsBuilder, SslOpts};
use oo_connectors::mysql::{
    chunk::{KeyColumn, KeyType},
    map::MappedColumn,
};
use oo_pipeline::native_arrow::{NativeCell, decode_arrow_stream};
use std::{path::PathBuf, process::Command};

#[test]
fn text_bounds_use_source_collation_and_binary_bounds_stay_binary() {
    let keys = vec![
        KeyColumn {
            name: "word".into(),
            kind: KeyType::Text,
            collation: Some("gbk_chinese_ci".into()),
        },
        KeyColumn {
            name: "raw".into(),
            kind: KeyType::Bytes,
            collation: None,
        },
        KeyColumn {
            name: "id".into(),
            kind: KeyType::Int,
            collation: None,
        },
    ];
    let lo = vec![
        KeyValue::Text("\u{5b57}".as_bytes().to_vec()),
        KeyValue::Bytes(vec![0, 255]),
        KeyValue::Int(-1),
    ];
    let (sql, values) = digest::source_predicate(
        &keys,
        &ChunkBound {
            lo: Some(lo.clone()),
            hi: None,
        },
    )
    .unwrap();
    assert!(
        sql.contains("`word` > CONVERT(CONVERT(? USING utf8mb4) USING gbk) COLLATE gbk_chinese_ci")
    );
    assert!(sql.contains("`raw` > ?"));
    assert!(sql.contains("`id` > ?"));
    assert_eq!(
        values,
        vec![
            lo[0].clone(),
            lo[0].clone(),
            lo[1].clone(),
            lo[0].clone(),
            lo[1].clone(),
            lo[2].clone()
        ]
    );
}

#[test]
fn streaming_audit_requires_one_real_snapshot_and_commit_without_record_ceiling() -> Result<()> {
    let mut state = AuditState::default();
    state.accept("Connect", "reader@localhost", "`S`.`T` WHERE ")?;
    state.accept(
        "Query",
        "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
        "`S`.`T` WHERE ",
    )?;
    for _ in 0..100_010 {
        state.accept(
            "Execute",
            "SELECT id FROM `s`.`t` WHERE id > 0 ORDER BY id LIMIT 1",
            "`S`.`T` WHERE ",
        )?;
    }
    assert!(!state.complete());
    state.accept("Query", "COMMIT", "`S`.`T` WHERE ")?;
    assert!(state.complete());
    assert!(
        state
            .accept(
                "Query",
                "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
                "`S`.`T` WHERE "
            )
            .is_err()
    );
    assert!(
        state
            .accept("Query", "DELETE FROM `s`.`t`", "`S`.`T` WHERE ")
            .is_err()
    );
    Ok(())
}

fn canonical(cell: &NativeCell) -> Value {
    match cell {
        NativeCell::Null => Value::Null,
        NativeCell::Int { value, .. } => json!(value.to_string()),
        NativeCell::Utf8(v) => json!(v),
        NativeCell::Binary(v) => json!(hex(v).to_ascii_uppercase()),
        NativeCell::Decimal {
            coefficient, scale, ..
        } => {
            let digits = format!(
                "{:0>width$}",
                coefficient.unsigned_abs().to_string(),
                width = *scale as usize + 1
            );
            let point = digits.len() - *scale as usize;
            json!(format!(
                "{}{}{}{}",
                if *coefficient < 0 { "-" } else { "" },
                &digits[..point],
                if *scale > 0 { "." } else { "" },
                &digits[point..]
            ))
        }
        NativeCell::Date32(v) => json!(v.to_string()),
        NativeCell::TimestampMicros(v) | NativeCell::TimestampMicrosUtc { value: v, .. } => {
            json!(v.to_string())
        }
        NativeCell::Float64(v) => json!(v.to_string()),
        NativeCell::Bool(v) => json!(if *v { "1" } else { "0" }),
        other => panic!("unqualified gold type {other:?}"),
    }
}

async fn gold(admin: &mut Conn, sql: &str, path: &Path) -> Result<u64> {
    let mut file = private_file(path)?;
    // Prepared (binary-protocol) values, independently projected by SQL, not
    // the product's capture expressions, digest SQL, or conversion helper.
    let mut stream = admin.exec_iter(sql, ()).await?;
    let mut count = 0;
    while let Some(row) = stream.next().await? {
        let cells = (0..row.len())
            .map(|i| {
                Ok(match row.as_ref(i).ok_or("gold width")? {
                    Cell::NULL => Value::Null,
                    Cell::Bytes(v) => json!(String::from_utf8(v.clone())?),
                    Cell::Int(v) => json!(v.to_string()),
                    Cell::UInt(v) => json!(v.to_string()),
                    Cell::Float(v) => json!(f64::from(*v).to_bits().to_string()),
                    Cell::Double(v) => json!(v.to_bits().to_string()),
                    _ => return Err("gold projection".into()),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        serde_json::to_writer(&mut file, &cells)?;
        file.write_all(b"\n")?;
        count += 1;
    }
    stream.drop_result().await?;
    file.sync_all()?;
    Ok(count)
}

fn verify(request: &Value, config: &Value, gold_path: &Path) -> Result<Value> {
    assert_eq!(request["protocol"], PROTOCOL);
    for key in ["arrow_path", "arrow_sha256", "members", "members_file"] {
        assert!(request.get(key).is_none(), "legacy field {key}");
    }
    let index = Path::new(text(&request["parts_index"], "path")?);
    assert_eq!(file_ref(index)?, request["parts_index"]);
    assert_eq!(request["input_digest"], request["parts_index"]["sha256"]);
    let root = index.parent().ok_or("index root")?;
    for name in ["schema-preimage.json", "type-preimage.json", "driver.json"] {
        assert_eq!(
            fs::metadata(root.join(name))?.permissions().mode() & 0o077,
            0
        );
    }
    let complete: Value = serde_json::from_slice(&fs::read(root.join("capture-complete.json"))?)?;
    assert_eq!(complete["snapshot_complete"], true);
    assert_eq!(complete["admission_seal"], false);
    let mut gold = BufReader::new(fs::File::open(gold_path)?).lines();
    let mut count = 0u64;
    let mut rows = 0u64;
    let mut max_bytes = 0u64;
    let mut normalized = 0u64;
    for line in BufReader::new(fs::File::open(index)?).lines() {
        let part: Value = serde_json::from_str(&line?)?;
        assert_eq!(part["ordinal"], count);
        let path = root.join(format!("part-{count}.arrow"));
        assert_eq!(part["arrow"], file_ref(&path)?);
        let bytes = fs::read(&path)?;
        assert!(bytes.len() as u64 <= number(config, "max_bytes")?);
        max_bytes = max_bytes.max(bytes.len() as u64);
        let decoded = decode_arrow_stream(&bytes).map_err(|_| "gold Arrow")?;
        assert_eq!(decoded.rows as u64, number(&part, "row_count")?);
        assert!(decoded.rows as u64 <= number(config, "max_rows")?);
        let witness: Value = serde_json::from_slice(&fs::read(
            root.join(format!("part-{count}.capture/capturer.json")),
        )?)?;
        assert_eq!(witness["part_ordinal"], count);
        assert_eq!(witness["snapshot"], complete["snapshot"]);
        assert_eq!(witness["source_identity"], complete["source_identity"]);
        assert_eq!(witness["source_generation"], request["source_generation"]);
        assert_eq!(
            witness["range_scope"]["parts_index"],
            request["parts_index"]
        );
        assert_eq!(
            witness["range_scope"]["protocol"],
            "oo.pipeline.native-c5-range-part/v1"
        );
        assert_eq!(
            witness["range_scope"]["range"]["chunk_id"],
            request["chunk_id"]
        );
        assert_eq!(witness["range_scope"]["part"], part);
        assert_eq!(witness["range_scope"]["total_rows"], request["row_count"]);
        assert_eq!(witness["range_scope"]["snapshot"], complete["snapshot"]);
        assert_eq!(witness["snapshot_complete"], true);
        if !witness["normalization_rejects"].is_null() {
            let r = &witness["normalization_rejects"];
            let notes = fs::read(text(r, "path")?)?;
            assert_eq!(sha256_hex(&notes), text(r, "sha256")?);
            let notes: Vec<Value> = serde_json::from_slice(&notes)?;
            for note in &notes {
                assert_eq!(note["input_digest"], part["arrow"]["sha256"]);
            }
            normalized += notes.len() as u64;
        }
        for row in 0..decoded.rows {
            let expected: Value = serde_json::from_str(&gold.next().ok_or("gold shorter")??)?;
            let actual = json!(
                decoded
                    .columns
                    .iter()
                    .map(|c| canonical(&c[row]))
                    .collect::<Vec<_>>()
            );
            assert_eq!(actual, expected, "gold mismatch at global row {rows}");
            rows += 1;
        }
        count += 1;
    }
    assert!(gold.next().is_none(), "capture shorter than gold");
    assert_eq!(count, number(request, "part_count")?);
    assert_eq!(rows, number(request, "row_count")?);
    assert!(count > 0);
    Ok(
        json!({"request":request,"rows_compared":rows,"cells_equal":true,
        "part_count":count,"largest_part_bytes":max_bytes,"normalization_notes":normalized,
        "gold":file_ref(gold_path)?,"complete":root.join("capture-complete.json")}),
    )
}

struct Fixture {
    out: PathBuf,
    schema: String,
    config: Value,
    admin: Conn,
    version: String,
}

async fn owned_admin() -> Result<Conn> {
    let ssl = SslOpts::default()
        .with_root_certs(vec![
            PathBuf::from("/tmp/oo-mysql-c1-20261027/certs/ca.pem").into(),
        ])
        .with_disable_built_in_roots(true);
    let root = crate::source_sql::secret_file(Path::new(
        "/tmp/oo-mysql-c1-20261027/secrets/mysql-root.pw",
    ))?;
    Ok(Conn::new(
        OptsBuilder::default()
            .ip_or_hostname("127.0.0.1")
            .tcp_port(23327)
            .user(Some("root"))
            .pass(Some(root.to_string()))
            .prefer_socket(false)
            .ssl_opts(ssl),
    )
    .await?)
}

async fn first_durable_part(dir: &Path) -> Result<PathBuf> {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if dir.exists() {
                for entry in fs::read_dir(dir)? {
                    let path = entry?.path();
                    let index = path.join("parts.jsonl.pending");
                    if index.exists() && fs::metadata(index)?.len() > 0 {
                        return Ok(path);
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| "first part deadline")?
}

impl Fixture {
    async fn new() -> Result<Self> {
        let evidence = PathBuf::from(std::env::var("OO_C5_PARTS_EVIDENCE")?);
        assert!(evidence.is_absolute());
        let inspected = Command::new("docker")
            .args(["inspect", "oo-mysql-c1-20261027-mysql57"])
            .output()?;
        assert!(inspected.status.success());
        let container: Value = serde_json::from_slice(&inspected.stdout)?;
        let container = &container[0];
        assert_eq!(container["Config"]["Labels"]["owner"], "mysql-c1-20261027");
        assert_eq!(container["State"]["Running"], true);
        assert!(
            container["Mounts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["Source"] == "/tmp/oo-mysql-c1-20261027/secrets"
                    && m["Destination"] == "/secrets"
                    && m["RW"] == false)
        );
        assert_eq!(
            container["NetworkSettings"]["Ports"]["3306/tcp"][0]["HostIp"],
            "127.0.0.1"
        );
        assert_eq!(
            container["NetworkSettings"]["Ports"]["3306/tcp"][0]["HostPort"],
            "23327"
        );
        let tag = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let schema = format!("c5_parts_{tag}");
        let out = evidence.join(format!("owned-{tag}"));
        fs::create_dir(&out)?;
        fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
        save(
            &out.join("ownership.json"),
            &json!({"container":"oo-mysql-c1-20261027-mysql57",
            "labels":container["Config"]["Labels"],"ports":container["NetworkSettings"]["Ports"],
            "new_schema":schema,"original_secret_path":"/tmp/oo-mysql-c1-20261027/secrets/mysql-root.pw",
            "customer_source_access":false,"shared_restart":false,"global_settings_changed":false}),
        )?;
        let mut admin = owned_admin().await?;
        let version: String = admin
            .query_first("SELECT VERSION()")
            .await?
            .ok_or("version")?;
        assert_eq!(version.trim_end_matches("-log"), "5.7.44");
        let exists: Option<String> = admin
            .exec_first(
                "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME=?",
                (&schema,),
            )
            .await?;
        assert!(exists.is_none(), "must create a NEW owned schema");
        admin
            .query_drop(format!(
                "CREATE DATABASE `{schema}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"
            ))
            .await?;
        let user = format!("c5p{tag}");
        let password: String = admin
            .query_first("SELECT REPLACE(UUID(),'-','')")
            .await?
            .ok_or("password")?;
        admin
            .query_drop(format!(
                "CREATE USER '{user}'@'%' IDENTIFIED BY '{password}' REQUIRE SSL"
            ))
            .await?;
        admin
            .query_drop(format!("GRANT SELECT ON `{schema}`.* TO '{user}'@'%'"))
            .await?;
        let secret = out.join("reader.secret");
        private_file(&secret)?.write_all(password.as_bytes())?;
        let config = json!({"source_id":schema,"schema":schema,"connection":{"host":"127.0.0.1","port":23327,
            "user":user,"secret_file":secret,"tls":"required","ca_file":"/tmp/oo-mysql-c1-20261027/certs/ca.pem"},
            "secret_ref":"secret://owned/c5-parts","max_rows":100000,"max_bytes":33554432,
            "audit":{"mode":"grant_enforced"}});
        Ok(Self {
            out,
            schema,
            config,
            admin,
            version,
        })
    }

    async fn capture(
        &self,
        table: &str,
        label: &str,
        config: &Value,
        observed: Option<Value>,
    ) -> Result<Value> {
        self.capture_at_duty(table, label, config, observed, 1000)
            .await
    }

    async fn capture_at_duty(
        &self,
        table: &str,
        label: &str,
        config: &Value,
        observed: Option<Value>,
        duty: u64,
    ) -> Result<Value> {
        self.capture_bound(table, label, config, observed, duty, &ChunkBound::default())
            .await
    }

    async fn capture_bound(
        &self,
        table: &str,
        label: &str,
        config: &Value,
        observed: Option<Value>,
        duty: u64,
        bound: &ChunkBound,
    ) -> Result<Value> {
        let mut config = config.clone();
        config["table"] = json!(table);
        let mut monitor = connect(&config).await?;
        let (keys, columns, fingerprint) = metadata(&mut monitor, &config).await?;
        let binding = RunBinding {
            source_id: self.schema.clone(),
            schema: self.schema.clone(),
            table: table.into(),
            run_generation: 2,
            input_digest: [0; 32],
        };
        let input = json!({"binding":"00000000-0000-4000-8000-000000000001","pin":1,"c5_layered":{
            "server_timeout_ms":30000,"policy":{"duty_per_mille":duty,"threads_running_max":64}}});
        let authority = json!({"source_generation":1,"chunk_topology_revision":1});
        let updated = json!({"chunk_id":"0","membership_revision":1,"arrow_sha256":"a".repeat(64),
            "digest":observed.unwrap_or(json!([0,"0000000000000000","0000000000000000"])),
            "digest_codec":digest::codec(&columns),"projection_sha256":projection_digest(&columns)?});
        let dir = self.out.join(label);
        fs::create_dir_all(&dir)?;
        let (monitor, request) = read_request(
            &config,
            &input,
            &authority,
            &dir,
            RequestRead {
                conn: monitor,
                binding: &binding,
                keys: &keys,
                columns: &columns,
                bound,
                updated,
                fingerprint,
                deadline: Duration::from_secs(60),
            },
        )
        .await?;
        monitor.disconnect().await?;
        save(&dir.join("request.json"), &request)?;
        Ok(request)
    }
}

async fn metadata(
    conn: &mut Conn,
    config: &Value,
) -> Result<(Vec<KeyColumn>, Vec<MappedColumn>, String)> {
    let schema = text(config, "schema")?;
    let table = text(config, "table")?;
    let selected = mysql::SelectedTable {
        schema: schema.into(),
        name: table.into(),
        columns: vec![],
    };
    let keys = chunk::key_columns(conn, schema, table).await?;
    let catalog = mysql::catalog_columns(conn, &mut vec![], &selected).await?;
    let mut columns = catalog
        .iter()
        .map(|c| mysql::map::map_capture_column(c, false, true, &Default::default()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for c in &mut columns {
        c.key = keys.iter().any(|k| k.name == c.plan.name);
        c.plan.key = c.key;
    }
    digest::apply_type_policy(&mut columns, digest::type_policy(config)?);
    let fingerprint = capture_schema_fingerprint(config, conn, &selected, &keys).await?;
    Ok((keys, columns, fingerprint))
}

#[tokio::test]
#[ignore = "explicit owned MySQL 5.7.44 / 23327; creates only a new c5_parts_* schema"]
async fn owned_mysql57_growing_parts_independent_gold() -> Result<()> {
    let mut f = Fixture::new().await?;
    let s = f.schema.clone();
    f.admin.query_drop("SET time_zone = '+00:00'").await?;
    f.admin
        .query_drop(format!(
            "CREATE TABLE `{s}`.digits (n INT PRIMARY KEY) ENGINE=InnoDB"
        ))
        .await?;
    f.admin
        .query_drop(format!(
            "INSERT INTO `{s}`.digits VALUES (0),(1),(2),(3),(4),(5),(6),(7),(8),(9)"
        ))
        .await?;
    f.admin
        .query_drop(format!(
            "CREATE TABLE `{s}`.growing (
        id BIGINT PRIMARY KEY,amount DECIMAL(38,8),note VARCHAR(128),payload VARBINARY(16),
        big BIGINT UNSIGNED) ENGINE=InnoDB"
        ))
        .await?;
    f.admin
        .query_drop(format!(
            "INSERT INTO `{s}`.growing
        SELECT n, IF(MOD(n,7)=0,NULL,123456789012345678901234567890.12345678),
        IF(MOD(n,11)=0,NULL,CONCAT('row-',n)),UNHEX('00ff0100'),18446744073709551615
        FROM (SELECT 1+a.n+10*b.n+100*c.n+1000*d.n+10000*e.n n
        FROM `{s}`.digits a CROSS JOIN `{s}`.digits b CROSS JOIN `{s}`.digits c
        CROSS JOIN `{s}`.digits d CROSS JOIN `{s}`.digits e) x"
        ))
        .await?;
    let select = format!(
        "SELECT CAST(id AS CHAR),CAST(amount AS CHAR),note,HEX(payload),CAST(big AS CHAR) FROM `{s}`.growing ORDER BY id"
    );
    let mut reports = Vec::new();
    let gold_path = f.out.join("gold-100000.jsonl");
    assert_eq!(gold(&mut f.admin, &select, &gold_path).await?, 100000);
    let first = f.capture("growing", "exact-page", &f.config, None).await?;
    assert_eq!(first["part_count"], 1);
    reports.push(verify(&first, &f.config, &gold_path)?);
    f.admin.query_drop(format!("INSERT INTO `{s}`.growing VALUES (100001,-123456789012345678901234567890.12345678,'growth',UNHEX('ff00'),18446744073709551615)")).await?;
    let gold_path = f.out.join("gold-100001.jsonl");
    assert_eq!(gold(&mut f.admin, &select, &gold_path).await?, 100001);
    let grown = f
        .capture(
            "growing",
            "growth-100001",
            &f.config,
            Some(first["c5_observation"]["chunk"]["digest"].clone()),
        )
        .await?;
    assert_eq!(grown["part_count"], 2);
    assert_eq!(grown["c5_observation"]["chunk"]["digest"][0], 100001);
    reports.push(verify(&grown, &f.config, &gold_path)?);
    let mut snapshot_config = f.config.clone();
    snapshot_config["max_rows"] = json!(50000);
    let mut writer = owned_admin().await?;
    let capture = f.capture_at_duty(
        "growing",
        "concurrent-snapshot",
        &snapshot_config,
        None,
        200,
    );
    let mutate = async {
        let path = first_durable_part(&f.out.join("concurrent-snapshot")).await?;
        writer.query_drop("START TRANSACTION").await?;
        writer
            .query_drop(format!(
                "UPDATE `{s}`.growing SET note='changed-after-first-page' WHERE id=100001"
            ))
            .await?;
        writer
            .query_drop(format!("DELETE FROM `{s}`.growing WHERE id=2"))
            .await?;
        writer
            .query_drop(format!(
                "INSERT INTO `{s}`.growing VALUES (100004,2,'concurrent-insert',NULL,0)"
            ))
            .await?;
        writer.query_drop("COMMIT").await?;
        save(
            &f.out.join("concurrent-mutation.json"),
            &json!({"after_durable_part":path,
            "schema":s,"only_owned_schema":true,"insert":100004,"update":100001,"delete":2}),
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    let (snapshot, mutated) = tokio::join!(capture, mutate);
    mutated?;
    let snapshot = snapshot?;
    // Gold was read BEFORE the concurrent commit. All later pages must still match it.
    reports.push(verify(&snapshot, &snapshot_config, &gold_path)?);
    writer.disconnect().await?;
    f.admin.query_drop("START TRANSACTION").await?;
    f.admin
        .query_drop(format!("DELETE FROM `{s}`.growing WHERE id IN (1,99999)"))
        .await?;
    f.admin
        .query_drop(format!(
            "UPDATE `{s}`.growing SET amount=-0.00000001,note='updated' WHERE id=100000"
        ))
        .await?;
    f.admin.query_drop(format!("INSERT INTO `{s}`.growing VALUES (100002,NULL,NULL,NULL,NULL),(100003,1.00000001,'new',UNHEX('01'),0)")).await?;
    f.admin.query_drop("COMMIT").await?;
    let gold_path = f.out.join("gold-iud.jsonl");
    assert_eq!(gold(&mut f.admin, &select, &gold_path).await?, 100001);
    let mut small = f.config.clone();
    small["max_rows"] = json!(1000);
    small["max_bytes"] = json!(65536);
    let iud = f
        .capture(
            "growing",
            "iud-byte-parts",
            &small,
            Some(grown["c5_observation"]["chunk"]["digest"].clone()),
        )
        .await?;
    assert!(number(&iud, "part_count")? > 100);
    reports.push(verify(&iud, &small, &gold_path)?);
    let interrupted_dir = {
        let capture = f.capture_at_duty("growing", "interrupted", &small, None, 1);
        let interrupted = f.out.join("interrupted");
        tokio::pin!(capture);
        tokio::select! {
            result = &mut capture => panic!("capture unexpectedly finished before interruption: {result:?}"),
            path = first_durable_part(&interrupted) => path?,
        }
    };
    assert!(!interrupted_dir.join("capture-complete.json").exists());
    assert!(!f.out.join("interrupted/request.json").exists());
    let failed_part_hash = chunk::file_sha256(&interrupted_dir.join("part-0.arrow"))?;
    let restarted = f.capture("growing", "interrupted", &small, None).await?;
    assert_ne!(
        Path::new(text(&restarted["parts_index"], "path")?)
            .parent()
            .unwrap(),
        interrupted_dir
    );
    assert_eq!(
        chunk::file_sha256(&interrupted_dir.join("part-0.arrow"))?,
        failed_part_hash
    );
    reports.push(verify(&restarted, &small, &gold_path)?);
    save(
        &f.out.join("interruption.json"),
        &json!({"failed_capture_retained":interrupted_dir,
        "failed_part_sha256":failed_part_hash,"restart_scope":"same logical range","new_snapshot":true,
        "failed_snapshot_complete":false,"partial_request_published":false}),
    )?;
    // An empty replacement must still carry its schema, never mean "no change".
    f.admin
        .query_drop(format!("DELETE FROM `{s}`.growing"))
        .await?;
    let gold_path = f.out.join("gold-empty.jsonl");
    assert_eq!(gold(&mut f.admin, &select, &gold_path).await?, 0);
    let empty = f
        .capture(
            "growing",
            "empty",
            &small,
            Some(iud["c5_observation"]["chunk"]["digest"].clone()),
        )
        .await?;
    assert_eq!(empty["part_count"], 1);
    assert_eq!(empty["row_count"], 0);
    reports.push(verify(&empty, &small, &gold_path)?);

    let mut normalized = small.clone();
    normalized["max_rows"] = json!(2);
    normalized["type_policy"] = json!({"zero_date_to_null":true,"strip_text_nul":true});
    f.admin.query_drop("SET SESSION sql_mode=''").await?;
    for (table, key, values, order, projection) in [
        (
            "text_keys",
            "k VARCHAR(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci PRIMARY KEY",
            "'z'|'A'|'e'|'B'|'y'",
            "k",
            "k",
        ),
        (
            "gbk_keys",
            "k VARCHAR(32) CHARACTER SET gbk COLLATE gbk_chinese_ci PRIMARY KEY",
            "CONVERT(0xd6d0 USING gbk)|CONVERT(0xb0a2 USING gbk)|'z'|'A'|'B'",
            "k",
            "CONVERT(k USING utf8mb4)",
        ),
        (
            "bytes_keys",
            "k VARBINARY(32) PRIMARY KEY",
            "UNHEX('ff')|UNHEX('00ff')|UNHEX('00')|UNHEX('0100')|UNHEX('01')",
            "k",
            "HEX(k)",
        ),
    ] {
        f.admin
            .query_drop(format!(
                "CREATE TABLE `{s}`.`{table}` ({key},note VARCHAR(32),d DATE,t DATETIME(6),
            f FLOAT,x DOUBLE) ENGINE=InnoDB"
            ))
            .await?;
        for (n, value) in values.split('|').enumerate() {
            f.admin.query_drop(format!("INSERT INTO `{s}`.`{table}` VALUES ({value},
                CONCAT('v',CHAR(0),'{n}'),'0000-00-00','2026-10-28 12:34:56.123456',0.1,1.23456789012345)")).await?;
        }
        let gold_path = f.out.join(format!("gold-{table}.jsonl"));
        gold(
            &mut f.admin,
            &format!(
                "SELECT {projection},REPLACE(note,CHAR(0),''),NULL,
            TIMESTAMPDIFF(MICROSECOND,'1970-01-01',t),f,x FROM `{s}`.`{table}` ORDER BY {order}"
            ),
            &gold_path,
        )
        .await?;
        let request = f.capture(table, table, &normalized, None).await?;
        assert_eq!(request["part_count"], 3);
        let report = verify(&request, &normalized, &gold_path)?;
        assert_eq!(report["normalization_notes"], 10);
        reports.push(report);
    }
    let bound = ChunkBound {
        lo: Some(vec![KeyValue::Text(b"B".to_vec())]),
        hi: Some(vec![KeyValue::Text("\u{963f}".as_bytes().to_vec())]),
    };
    let gold_path = f.out.join("gold-gbk-bounded.jsonl");
    assert_eq!(
        gold(
            &mut f.admin,
            &format!(
                "SELECT CONVERT(k USING utf8mb4),REPLACE(note,CHAR(0),''),NULL,
        TIMESTAMPDIFF(MICROSECOND,'1970-01-01',t),f,x FROM `{s}`.gbk_keys
        WHERE k>'B' AND k<=CONVERT(0xb0a2 USING gbk) ORDER BY k"
            ),
            &gold_path
        )
        .await?,
        2
    );
    let mut one = normalized.clone();
    one["max_rows"] = json!(1);
    let bounded = f
        .capture_bound("gbk_keys", "gbk-bounded", &one, None, 1000, &bound)
        .await?;
    assert_eq!(bounded["lo"], encode_bound(bound.lo.as_deref()));
    assert_eq!(bounded["hi"], encode_bound(bound.hi.as_deref()));
    reports.push(verify(&bounded, &one, &gold_path)?);
    f.admin
        .query_drop(format!(
            "CREATE TABLE `{s}`.composite (n BIGINT UNSIGNED NOT NULL,
        k VARCHAR(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
        raw VARBINARY(8) NOT NULL,amount DECIMAL(38,8),PRIMARY KEY(k,raw,n)) ENGINE=InnoDB"
        ))
        .await?;
    for k in ["a", "B", "z"] {
        for raw in ["00", "00ff", "ff"] {
            for n in ["1", "18446744073709551615"] {
                f.admin.query_drop(format!("INSERT INTO `{s}`.composite VALUES ({n},'{k}',UNHEX('{raw}'),123456789012345678901234567890.12345678)")).await?;
            }
        }
    }
    let gold_path = f.out.join("gold-composite.jsonl");
    gold(&mut f.admin,&format!("SELECT CAST(n AS CHAR),k,HEX(raw),CAST(amount AS CHAR) FROM `{s}`.composite ORDER BY k,raw,n"),&gold_path).await?;
    let request = f
        .capture("composite", "composite", &normalized, None)
        .await?;
    assert_eq!(request["part_count"], 9);
    assert_eq!(request["key_columns"], json!(["k", "raw", "n"]));
    reports.push(verify(&request, &normalized, &gold_path)?);
    f.admin
        .query_drop(format!(
            "CREATE TABLE `{s}`.arrow_pressure (id INT PRIMARY KEY,v VARCHAR(64)) ENGINE=InnoDB"
        ))
        .await?;
    f.admin
        .query_drop(format!(
            "INSERT INTO `{s}`.arrow_pressure SELECT 1+a.n+10*b.n,REPEAT('x',60)
        FROM `{s}`.digits a CROSS JOIN `{s}`.digits b"
        ))
        .await?;
    let mut pressure = small.clone();
    pressure["max_bytes"] = json!(2048);
    let gold_path = f.out.join("gold-arrow-pressure.jsonl");
    gold(
        &mut f.admin,
        &format!("SELECT CAST(id AS CHAR),v FROM `{s}`.arrow_pressure ORDER BY id"),
        &gold_path,
    )
    .await?;
    let request = f
        .capture("arrow_pressure", "arrow-pressure", &pressure, None)
        .await?;
    reports.push(verify(&request, &pressure, &gold_path)?);
    let mut impossible = pressure.clone();
    impossible["max_bytes"] = json!(16);
    let failure = f
        .capture("arrow_pressure", "indivisible-row", &impossible, None)
        .await
        .unwrap_err();
    assert!(failure.to_string().contains("C5_PART_ROW_BYTES_EXCEEDED"));
    assert!(!f.out.join("indivisible-row/request.json").exists());
    let failed = fs::read_dir(f.out.join("indivisible-row"))?
        .next()
        .unwrap()?
        .path();
    assert!(failed.join("capture-failed.json").exists());
    assert!(!failed.join("capture-complete.json").exists());
    save(
        &f.out.join("qualification.json"),
        &json!({
            "scope":"source-only owned MySQL 5.7.44; not target/admission/Gateway acceptance",
            "mysql_version":f.version,"schema":s,"reports":reports,
            "growth_exceeds_historical_100000":true,"independent_gold_per_cell":true,
            "concurrent_snapshot_iud":true,"interrupted_capture_preserved_and_range_restarted":true,
            "customer_source_access":false,"admission_seals_created":false,
        }),
    )?;
    println!(
        "C5_PARTS_QUALIFICATION={}",
        f.out.join("qualification.json").display()
    );
    f.admin.disconnect().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "owned 23327 only; requires already-enabled TABLE general log, never changes it"]
async fn owned_mysql57_streamed_general_log_witness() -> Result<()> {
    let mut f = Fixture::new().await?;
    general_enabled(&mut f.admin).await?;
    let s = &f.schema;
    let user = text(&f.config["connection"], "user")?;
    assert!(user.starts_with("c5p"));
    f.admin
        .query_drop(format!("GRANT SELECT ON mysql.general_log TO '{user}'@'%'"))
        .await?;
    f.admin
        .query_drop(format!(
            "CREATE TABLE `{s}`.audited (id INT PRIMARY KEY, v VARCHAR(32)) ENGINE=InnoDB"
        ))
        .await?;
    f.admin
        .query_drop(format!(
            "INSERT INTO `{s}`.audited VALUES (1,'a'),(2,NULL),(3,'c'),(4,'d'),(5,'e')"
        ))
        .await?;
    let mut config = f.config.clone();
    config["max_rows"] = json!(2);
    config["audit"] = json!({"mode":"general_log"});
    let gold_path = f.out.join("gold-audited.jsonl");
    gold(
        &mut f.admin,
        &format!("SELECT CAST(id AS CHAR),v FROM `{s}`.audited ORDER BY id"),
        &gold_path,
    )
    .await?;
    let request = f.capture("audited", "general-log", &config, None).await?;
    let checked = verify(&request, &config, &gold_path)?;
    let complete: Value = serde_json::from_slice(&fs::read(text(&checked, "complete")?)?)?;
    let audit = &complete["source_audit"];
    assert_eq!(audit["source"], "mysql.general_log");
    let reference = &audit["records_file"];
    assert_eq!(file_ref(Path::new(text(reference, "path")?))?, *reference);
    let mut starts = 0;
    let mut commits = 0;
    let mut connects = 0;
    let mut records = 0;
    for line in BufReader::new(fs::File::open(text(reference, "path")?)?).lines() {
        let record: Value = serde_json::from_str(&line?)?;
        assert_eq!(record["thread_id"], complete["connection_id"]);
        connects += u32::from(record["command_type"] == "Connect");
        starts += u32::from(
            record["argument"] == "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
        );
        commits += u32::from(record["argument"] == "COMMIT");
        records += 1;
    }
    assert_eq!((connects, starts, commits), (1, 1, 1));
    assert!(records > 10);
    save(
        &f.out.join("general-log-qualification.json"),
        &json!({"scope":"real owned source audit, not admission acceptance",
            "mysql_version":f.version,"report":checked,"source_audit":audit,
            "raw_records":records,"one_connect_start_commit":true,
            "global_settings_changed":false,"admission_seal_created":false}),
    )?;
    println!(
        "C5_GENERAL_LOG_QUALIFICATION={}",
        f.out.join("general-log-qualification.json").display()
    );
    f.admin.disconnect().await?;
    Ok(())
}

fn legacy_prepared_contract(
    request: &Value,
    ordinal: u64,
    out: &Path,
    secret_ref: &str,
) -> Result<Value> {
    let capture = Path::new(text(&request["parts_index"], "path")?)
        .parent()
        .ok_or("capture directory")?;
    let view = out.join("capture-view");
    fs::create_dir_all(&view)?;
    // Link unchanged real source evidence; the legacy preparer emits inputs,
    // not a receipt. Never edit the captured snapshot/ordinal to fake a baseline.
    fs::hard_link(
        capture.join(format!("part-{ordinal}.capture/capturer.json")),
        view.join("capturer.json"),
    )?;
    for name in ["schema-preimage.json", "type-preimage.json", "driver.json"] {
        fs::hard_link(capture.join(name), view.join(name))?;
    }
    let prepared = out.join("prepared");
    let result = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("prepare-native.py"))
        .arg("--capture")
        .arg(&view)
        .arg("--out")
        .arg(&prepared)
        .args([
            "--secret-ref",
            secret_ref,
            "--source-audit-policy",
            "grant_enforced",
        ])
        .output()?;
    assert!(
        result.status.success(),
        "legacy preparer: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let config_path = prepared.join(text(request, "table")?).join("config.json");
    let bytes = fs::read(&config_path)?;
    let logical: Value = serde_json::from_slice(&bytes)?;
    for field in [
        "ordinal",
        "parts_index",
        "part",
        "snapshot",
        "part_count",
        "run_id",
    ] {
        assert!(
            logical.get(field).is_none(),
            "transport metadata in logical config"
        );
    }
    let facts: Value = serde_json::from_slice(&fs::read(view.join("capturer.json"))?)?;
    assert_eq!(logical["budget"]["hard_limit"], facts["max_rows"]);
    Ok(json!({
        "config_digest":sha256_hex(&bytes),
        "schema_digest":chunk::file_sha256(&view.join("schema-preimage.json"))?,
        "type_digest":chunk::file_sha256(&view.join("type-preimage.json"))?,
        "capture_config_sha256":facts["config_sha256"],"logical_config":config_path,
        "max_rows":facts["max_rows"],"max_bytes":facts["max_bytes"],
    }))
}

#[tokio::test]
#[ignore = "owned 23327 only; freezes the original full-baseline preimage contract"]
async fn owned_mysql57_parts_preserve_baseline_contract_digests() -> Result<()> {
    let mut f = Fixture::new().await?;
    let s = &f.schema;
    f.admin.query_drop(format!(
        "CREATE TABLE `{s}`.contract_rows (id INT PRIMARY KEY, amount DECIMAL(38,18), v TEXT) ENGINE=InnoDB"
    )).await?;
    f.admin.query_drop(format!(
        "INSERT INTO `{s}`.contract_rows VALUES (1,12345678901234567890.123456789012345678,'a'),(2,NULL,NULL)"
    )).await?;
    let keys = chunk::key_columns(&mut f.admin, s, "contract_rows").await?;
    let mut config = f.config.clone();
    config["max_rows"] = json!(2);
    config["max_bytes"] = json!(8192);
    config["schema_fingerprint_codec"] = json!("mysql-full-catalog/v1");
    config["baseline_keys"] = json!(keys);
    let before_gold = f.out.join("gold-baseline-contract.jsonl");
    let select = format!(
        "SELECT CAST(id AS CHAR),CAST(amount AS CHAR),v FROM `{s}`.contract_rows ORDER BY id"
    );
    gold(&mut f.admin, &select, &before_gold).await?;
    let baseline = f
        .capture("contract_rows", "baseline", &config, None)
        .await?;
    assert_eq!(baseline["part_count"], 1);
    verify(&baseline, &config, &before_gold)?;
    let secret = text(&config, "secret_ref")?;
    let original =
        legacy_prepared_contract(&baseline, 0, &f.out.join("baseline-contract"), secret)?;
    f.admin
        .query_drop(format!(
            "UPDATE `{s}`.contract_rows SET v=REPEAT('x',6000) WHERE id=1"
        ))
        .await?;
    f.admin
        .query_drop(format!(
            "INSERT INTO `{s}`.contract_rows VALUES (3,-0.000000000000000001,'growth')"
        ))
        .await?;
    let after_gold = f.out.join("gold-growth-contract.jsonl");
    gold(&mut f.admin, &select, &after_gold).await?;
    let grown = f
        .capture(
            "contract_rows",
            "growth",
            &config,
            Some(baseline["c5_observation"]["chunk"]["digest"].clone()),
        )
        .await?;
    let checked = verify(&grown, &config, &after_gold)?;
    assert!(number(&grown, "part_count")? > 1);
    let mut part_contracts = Vec::new();
    let mut saw_shrunk_part = false;
    for line in BufReader::new(fs::File::open(text(&grown["parts_index"], "path")?)?).lines() {
        let part: Value = serde_json::from_str(&line?)?;
        let ordinal = number(&part, "ordinal")?;
        let contract = legacy_prepared_contract(
            &grown,
            ordinal,
            &f.out.join(format!("part-contract-{ordinal}")),
            secret,
        )?;
        for field in [
            "config_digest",
            "schema_digest",
            "type_digest",
            "capture_config_sha256",
            "max_rows",
            "max_bytes",
        ] {
            assert_eq!(
                contract[field], original[field],
                "parent contract changed: {field}"
            );
        }
        saw_shrunk_part |= number(&part, "row_count")? < number(&config, "max_rows")?;
        part_contracts.push(contract);
    }
    assert!(saw_shrunk_part);
    let capture = Path::new(text(&grown["parts_index"], "path")?)
        .parent()
        .unwrap();
    for name in ["schema-preimage.json", "type-preimage.json"] {
        let bytes = fs::read(capture.join(name))?;
        let logical: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(
            bytes,
            serde_json::to_vec_pretty(&logical)?,
            "full baseline encoding changed"
        );
        assert!(!bytes.ends_with(b"\n"));
    }
    save(
        &f.out.join("contract-digests-qualification.json"),
        &json!({
            "scope":"real source captures + unchanged legacy prepare-native inputs; no seal or target apply",
            "mysql_version":f.version,"baseline":baseline,"growth":checked,
            "original_contract":original,"part_contracts":part_contracts,
            "same_logical_config_schema_type_digests":true,"original_part_budgets_preserved":true,
            "transport_metadata_excluded_from_logical_config":true,
            "full_baseline_preimage_bytes_preserved":true,"admission_seals_created":false,
        }),
    )?;
    println!(
        "C5_CONTRACT_DIGESTS_QUALIFICATION={}",
        f.out.join("contract-digests-qualification.json").display()
    );
    f.admin.disconnect().await?;
    Ok(())
}
