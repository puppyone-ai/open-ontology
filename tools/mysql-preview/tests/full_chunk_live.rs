use arrow_ipc::reader::StreamReader;
use mysql_async::{OptsBuilder, prelude::Queryable};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn strings(path: &Path) -> Vec<Vec<Option<String>>> {
    let mut rows = Vec::new();
    for batch in StreamReader::try_new(fs::File::open(path).unwrap(), None).unwrap() {
        let batch = batch.unwrap();
        for row in 0..batch.num_rows() {
            let mut cells = Vec::new();
            for array in batch.columns() {
                assert_eq!(format!("{:?}", array.data_type()), "Utf8");
                if array.is_null(row) {
                    cells.push(None);
                    continue;
                }
                let data = array.to_data();
                let index = (data.offset() + row) * 4;
                let offsets = data.buffers()[0].as_slice();
                let start =
                    i32::from_le_bytes(offsets[index..index + 4].try_into().unwrap()) as usize;
                let end =
                    i32::from_le_bytes(offsets[index + 4..index + 8].try_into().unwrap()) as usize;
                cells.push(Some(
                    std::str::from_utf8(&data.buffers()[1].as_slice()[start..end])
                        .unwrap()
                        .to_owned(),
                ));
            }
            rows.push(cells);
        }
    }
    rows
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn full_and_chunk_paths_preserve_rows_and_both_audit_modes() {
    let run = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let mut conn = mysql_async::Conn::new(
        OptsBuilder::default()
            .ip_or_hostname("127.0.0.1")
            .tcp_port(23380)
            .user(Some("reader"))
            .pass(Some("fixture-only"))
            .db_name(Some("fixture"))
            .prefer_socket(false),
    )
    .await
    .unwrap();
    let source: Vec<(String, Option<String>)> = conn
        .query("SELECT id,payload FROM items ORDER BY id")
        .await
        .unwrap();
    let expected: Vec<_> = source
        .into_iter()
        .map(|(id, payload)| vec![Some(id), payload])
        .collect();
    let base: Value = serde_json::from_slice(&fs::read(run.join("auto.json")).unwrap()).unwrap();
    for (mode, explicit, general, limit) in [
        ("small", false, false, 20_000),
        ("auto", false, false, 100),
        ("explicit", true, false, 100),
        ("general", true, true, 100),
    ] {
        let mut config = base.clone();
        config["budgets"]["max_rows"] = json!(limit);
        config["budgets"]["max_bytes"] = json!(4 * 1024 * 1024);
        if explicit {
            config["chunking"] = json!(true);
        }
        if general {
            config["audit"] = json!({"user":"reader","secret_ref":"secret://fixture/reader","secret_file":run.join("reader")});
        }
        let config_path = run.join(format!("test-{mode}-{}.json", std::process::id()));
        fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let out = run.join(format!("test-{mode}-{}", std::process::id()));
        mysql_preview::capture_full(&config_path, &out)
            .await
            .unwrap();
        let capturer: Value =
            serde_json::from_slice(&fs::read(out.join("capturer.json")).unwrap()).unwrap();
        let actual = strings(&out.join("table-0.arrow"));
        if mode == "small" {
            assert_eq!(actual.len(), expected.len());
            assert!(!out.join("chunks.json").exists());
            assert_eq!(capturer["tables"][0]["consistency"], "S0");
            for row in &actual {
                assert!(expected.contains(row));
            }
        } else {
            assert_eq!(actual, expected);
            assert_eq!(capturer["tables"][0]["consistency"], "B");
            let entries = oo_connectors::mysql::chunk::load_manifest(&out, "items").unwrap();
            let mut from_chunks = Vec::new();
            for entry in entries {
                assert!(entry.rows <= limit);
                from_chunks.extend(strings(&out.join(format!("{}.arrow", entry.chunk_id))));
            }
            assert_eq!(from_chunks, expected);
        }
        if general {
            assert!(source_admission::mysql_full_audit_valid(
                &capturer["source_audit"]
            ));
        } else {
            assert!(source_admission::mysql_full_grant_audit_valid(
                &capturer["source_audit"]
            ));
        }
    }
    conn.disconnect().await.unwrap();
}

fn params(values: Vec<oo_connectors::mysql::chunk::KeyValue>) -> mysql_async::Params {
    use oo_connectors::mysql::chunk::KeyValue;
    mysql_async::Params::Positional(
        values
            .into_iter()
            .map(|v| match v {
                KeyValue::Int(v) if v < 0 => mysql_async::Value::Int(v.try_into().unwrap()),
                KeyValue::Int(v) => mysql_async::Value::UInt(v.try_into().unwrap()),
                KeyValue::Text(v) | KeyValue::Bytes(v) => mysql_async::Value::Bytes(v),
            })
            .collect(),
    )
}

async fn reader() -> mysql_async::Conn {
    mysql_async::Conn::new(
        OptsBuilder::default()
            .ip_or_hostname("127.0.0.1")
            .tcp_port(23380)
            .user(Some("reader"))
            .pass(Some("fixture-only"))
            .db_name(Some("fixture"))
            .prefer_socket(false),
    )
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture"]
async fn typed_ranges_match_mysql_order_and_use_primary_indexes() {
    use oo_connectors::{
        mysql::{self, chunk::*},
        types::RunBinding,
    };
    for table in [
        "unicode_items",
        "pairs",
        "integer_items",
        "unsigned_items",
        "binary_items",
    ] {
        let mut conn = reader().await;
        let selected = mysql::SelectedTable {
            schema: "fixture".into(),
            name: table.into(),
            columns: vec![],
        };
        let catalog = mysql::catalog_columns(&mut conn, &mut Vec::new(), &selected)
            .await
            .unwrap();
        let columns = catalog
            .iter()
            .map(|c| mysql::map::map_column(c, false, true, &Default::default()).unwrap())
            .collect::<Vec<_>>();
        let mut keys = Vec::new();
        for column in columns.iter().filter(|c| c.key) {
            let collation: Option<Option<String>> = conn.exec_first("SELECT COLLATION_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='fixture' AND TABLE_NAME=? AND COLUMN_NAME=?",(table,&column.plan.name)).await.unwrap();
            keys.push(KeyColumn {
                name: column.plan.name.clone(),
                kind: if matches!(table, "integer_items" | "unsigned_items") {
                    KeyType::Int
                } else if table == "binary_items" {
                    KeyType::Bytes
                } else {
                    KeyType::Text
                },
                collation: collation.flatten(),
            });
        }
        let order = keys
            .iter()
            .map(|k| format!("`{}`", k.name))
            .collect::<Vec<_>>()
            .join(",");
        let expected: Vec<mysql_async::Row> = conn
            .exec(
                format!("SELECT * FROM `fixture`.`{table}` ORDER BY {order}"),
                (),
            )
            .await
            .unwrap();
        let binding = RunBinding {
            source_id: "fixture".into(),
            schema: "fixture".into(),
            table: table.into(),
            run_generation: 1,
            input_digest: [0; 32],
        };
        let mut lo = None;
        let mut actual = Vec::new();
        loop {
            let (boundary_sql, boundary_params) =
                next_boundary_sql("fixture", table, &keys, lo.clone(), 2).unwrap();
            let explain: Vec<mysql_async::Row> = conn
                .exec(format!("EXPLAIN {boundary_sql}"), params(boundary_params))
                .await
                .unwrap();
            assert!(
                explain
                    .iter()
                    .any(|r| r.get::<Option<String>, _>("key").flatten().as_deref()
                        == Some("PRIMARY")),
                "{table}: {explain:?}"
            );
            let Some(hi) = next_boundary(&mut conn, "fixture", table, &keys, lo.clone(), 2)
                .await
                .unwrap()
            else {
                break;
            };
            let bound = ChunkBound {
                lo: lo.clone(),
                hi: Some(hi.clone()),
            };
            let (predicate, values) = range_where(&keys, &bound).unwrap();
            let explain: Vec<mysql_async::Row> = conn
                .exec(
                    format!(
                        "EXPLAIN SELECT * FROM `fixture`.`{table}`{predicate} ORDER BY {order}"
                    ),
                    params(values),
                )
                .await
                .unwrap();
            assert!(
                explain
                    .iter()
                    .any(|r| r.get::<Option<String>, _>("key").flatten().as_deref()
                        == Some("PRIMARY")),
                "{table}: {explain:?}"
            );
            let (returned, batch) = read_chunk(
                conn,
                ChunkRead {
                    schema: "fixture",
                    table,
                    keys: &keys,
                    columns: &columns,
                    binding: &binding,
                    bound: &bound,
                    max_rows: 2,
                    max_bytes: 65536,
                    deadline: std::time::Duration::from_secs(10),
                },
            )
            .await
            .unwrap();
            conn = returned;
            assert!(batch.rejects.is_empty());
            assert_eq!(
                batch.typed.record_batch().unwrap().num_rows(),
                batch.rows.len()
            );
            actual.extend(batch.rows);
            lo = Some(hi);
        }
        assert_eq!(
            actual.into_iter().map(|r| r.unwrap()).collect::<Vec<_>>(),
            expected.into_iter().map(|r| r.unwrap()).collect::<Vec<_>>()
        );
        conn.disconnect().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture"]
async fn killed_in_flight_range_returns_no_batch_and_replays_whole_range() {
    use oo_connectors::{
        mysql::{self, chunk::*},
        types::RunBinding,
    };
    let mut conn = reader().await;
    let id = conn.id();
    let selected = mysql::SelectedTable {
        schema: "fixture".into(),
        name: "kill_items".into(),
        columns: vec![],
    };
    let catalog = mysql::catalog_columns(&mut conn, &mut Vec::new(), &selected)
        .await
        .unwrap();
    let columns = mysql::map::map_table(&catalog, false, true, &Default::default()).unwrap();
    let keys = vec![KeyColumn {
        name: "id".into(),
        kind: KeyType::Text,
        collation: Some("utf8mb4_unicode_ci".into()),
    }];
    let hi = next_boundary(&mut conn, "fixture", "kill_items", &keys, None, 3)
        .await
        .unwrap()
        .unwrap();
    let bound = ChunkBound {
        lo: None,
        hi: Some(hi),
    };
    let binding = RunBinding {
        source_id: "fixture".into(),
        schema: "fixture".into(),
        table: "kill_items".into(),
        run_generation: 1,
        input_digest: [0; 32],
    };
    let mut lock = mysql_async::Conn::new(
        OptsBuilder::default()
            .ip_or_hostname("127.0.0.1")
            .tcp_port(23380)
            .user(Some("root"))
            .db_name(Some("fixture"))
            .prefer_socket(false),
    )
    .await
    .unwrap();
    lock.query_drop("LOCK TABLES fixture.kill_items WRITE")
        .await
        .unwrap();
    let mut control = reader().await;
    let kill = async {
        let mut blocked = false;
        for _ in 0..100 {
            let state: Option<String> = control
                .exec_first(
                    "SELECT STATE FROM information_schema.PROCESSLIST WHERE ID=?",
                    (id,),
                )
                .await
                .unwrap();
            if state.is_some_and(|s| s.contains("Waiting")) {
                blocked = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(blocked, "the range must actually be in flight before kill");
        control
            .query_drop(format!("KILL CONNECTION {id}"))
            .await
            .unwrap();
    };
    let request = || ChunkRead {
        schema: "fixture",
        table: "kill_items",
        keys: &keys,
        columns: &columns,
        binding: &binding,
        bound: &bound,
        max_rows: 3,
        max_bytes: 65536,
        deadline: std::time::Duration::from_secs(10),
    };
    let (result, ()) = tokio::join!(read_chunk(conn, request()), kill);
    assert!(result.is_err());
    lock.query_drop("UNLOCK TABLES").await.unwrap();
    let (conn, batch) = read_chunk(reader().await, request()).await.unwrap();
    assert_eq!(batch.rows.len(), 3);
    assert!(batch.rejects.is_empty());
    conn.disconnect().await.unwrap();
    lock.disconnect().await.unwrap();
    control.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn chunk_overflow_shrinks_and_single_row_overflow_records_a_reject() {
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let base: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    for table in ["items", "oversized", "oversized_blob"] {
        let mut config = base.clone();
        config["table"]["table"] = json!(table);
        config["approved_objects"] = json!([config["table"].clone()]);
        config["budgets"]["max_bytes"] = json!(8192);
        let path = root.join(format!("budget-{table}-{}.json", std::process::id()));
        let out = root.join(format!("budget-{table}-{}", std::process::id()));
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = mysql_preview::capture_full(&path, &out).await;
        if matches!(table, "oversized" | "oversized_blob") {
            assert_eq!(result.unwrap_err().code, "excluded");
            let rejects: Value =
                serde_json::from_slice(&fs::read(out.join("rejects.json")).unwrap()).unwrap();
            assert_eq!(rejects[0]["reason"], "row_budget_exceeded");
            assert!(!out.join("capturer.json").exists());
        } else {
            result.unwrap();
            let entries = oo_connectors::mysql::chunk::load_manifest(&out, table).unwrap();
            assert_eq!(entries.iter().map(|e| e.rows).sum::<u64>(), 10004);
            let mut retries = 0;
            for entry in entries {
                assert!(entry.bytes <= 8192);
                assert!(
                    fs::metadata(out.join(format!("{}.arrow", entry.chunk_id)))
                        .unwrap()
                        .len()
                        <= 8192
                );
                let metrics: Value = serde_json::from_slice(
                    &fs::read(out.join(format!("{}.metrics.json", entry.chunk_id))).unwrap(),
                )
                .unwrap();
                retries += metrics["budget_retries"].as_u64().unwrap();
            }
            assert!(retries > 0);
        }
    }
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn charset_temporal_decimal_and_zero_date_have_explicit_outcomes() {
    use oo_connectors::{
        mysql::{self, chunk::*},
        types::RunBinding,
    };
    for table in ["charset_items", "bad_charset"] {
        let mut conn = reader().await;
        conn.query_drop("SET time_zone = '+00:00'").await.unwrap();
        let catalog = mysql::catalog_columns(
            &mut conn,
            &mut Vec::new(),
            &mysql::SelectedTable {
                schema: "fixture".into(),
                name: table.into(),
                columns: vec![],
            },
        )
        .await
        .unwrap();
        let columns = catalog
            .iter()
            .map(|column| {
                mysql::map::map_capture_column(column, false, true, &Default::default()).unwrap()
            })
            .collect::<Vec<_>>();
        let keys = vec![KeyColumn {
            name: "id".into(),
            kind: KeyType::Text,
            collation: Some("utf8_general_ci".into()),
        }];
        let hi = next_boundary(&mut conn, "fixture", table, &keys, None, 10)
            .await
            .unwrap()
            .unwrap();
        let bound = ChunkBound {
            lo: None,
            hi: Some(hi),
        };
        let binding = RunBinding {
            source_id: "fixture".into(),
            schema: "fixture".into(),
            table: table.into(),
            run_generation: 1,
            input_digest: [0; 32],
        };
        let (conn, batch) = read_chunk(
            conn,
            ChunkRead {
                schema: "fixture",
                table,
                keys: &keys,
                columns: &columns,
                binding: &binding,
                bound: &bound,
                max_rows: 10,
                max_bytes: 65536,
                deadline: std::time::Duration::from_secs(10),
            },
        )
        .await
        .unwrap();
        if table == "bad_charset" {
            assert_eq!(batch.rejects.len(), 1);
            assert_eq!(batch.typed.rejects.len(), 1);
            assert!(batch.typed.rejects[0].key_digest.is_some());
        } else {
            assert!(batch.rejects.is_empty());
            assert!(
                matches!(mysql::raw_cell(&batch.rows[0],1).unwrap(),mysql::values::RawCell::Bytes(v) if v=="中文".as_bytes())
            );
            assert!(
                matches!(mysql::raw_cell(&batch.rows[0],2).unwrap(),mysql::values::RawCell::Bytes(v) if v=="中文".as_bytes())
            );
            let arrow = batch.typed.record_batch().unwrap();
            assert_eq!(
                format!("{:?}", arrow.column(3).data_type()),
                "Decimal128(38, 6)"
            );
            assert_eq!(
                i128::from_le_bytes(
                    arrow.column(3).to_data().buffers()[0].as_slice()[..16]
                        .try_into()
                        .unwrap()
                ),
                -12_345_123_456
            );
            assert_eq!(
                format!("{:?}", arrow.column(4).data_type()),
                "Timestamp(Microsecond, None)"
            );
            assert_eq!(
                format!("{:?}", arrow.column(5).data_type()),
                "Timestamp(Microsecond, Some(\"UTC\"))"
            );
            for index in [4, 5] {
                assert_eq!(
                    i64::from_le_bytes(
                        arrow.column(index).to_data().buffers()[0].as_slice()[..8]
                            .try_into()
                            .unwrap()
                    ),
                    1_709_210_096_123_456
                );
            }
        }
        conn.disconnect().await.unwrap();
    }
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let base: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    for table in ["charset_items", "zero_items", "bad_charset"] {
        let mut config = base.clone();
        config["table"] = json!({"schema":"fixture","table":table,"columns":[]});
        config["approved_objects"] = json!([config["table"].clone()]);
        let path = root.join(format!("types-{table}-{}.json", std::process::id()));
        let out = root.join(format!("types-{table}-{}", std::process::id()));
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = mysql_preview::capture_full(&path, &out).await;
        if table == "charset_items" {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().fact, "quality_rejects");
            let rejects: Value =
                serde_json::from_slice(&fs::read(out.join("rejects.json")).unwrap()).unwrap();
            assert_eq!(rejects.as_array().unwrap().len(), 1);
            if table == "zero_items" {
                assert_eq!(rejects[0]["reason"], "zero_date");
            }
            assert!(rejects[0]["key_digest"].is_string());
            assert!(!out.join("capturer.json").exists());
        }
    }
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn capture_selects_catalog_ordered_composite_or_nonnull_unique_keys_and_excludes_large_unkeyed_tables()
 {
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let base: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    for table in [
        "pairs",
        "reverse_pairs",
        "unique_items",
        "unkeyed_small",
        "unkeyed_large",
    ] {
        let mut config = base.clone();
        config["table"] = json!({"schema":"fixture","table":table,"columns":[]});
        config["approved_objects"] = json!([config["table"].clone()]);
        config["budgets"]["max_rows"] = json!(2);
        let path = root.join(format!("keys-{table}-{}.json", std::process::id()));
        let out = root.join(format!("keys-{table}-{}", std::process::id()));
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = mysql_preview::capture_full(&path, &out).await;
        if table == "unkeyed_large" {
            assert_eq!(result.unwrap_err().code, "excluded");
            assert!(!out.join("capturer.json").exists());
        } else {
            result.unwrap();
            let capturer: Value =
                serde_json::from_slice(&fs::read(out.join("capturer.json")).unwrap()).unwrap();
            assert_eq!(
                capturer["tables"][0]["row_count"],
                if matches!(table, "pairs" | "reverse_pairs") {
                    5
                } else if table == "unique_items" {
                    3
                } else {
                    2
                }
            );
            if table == "unkeyed_small" {
                assert!(!out.join("chunks.json").exists());
            } else {
                let binding: Value =
                    serde_json::from_slice(&fs::read(out.join("chunk-binding.json")).unwrap())
                        .unwrap();
                assert_eq!(
                    binding["keys"][0]["name"],
                    if table == "pairs" {
                        "a"
                    } else if table == "reverse_pairs" {
                        "b"
                    } else {
                        "id"
                    }
                );
                if matches!(table, "pairs" | "reverse_pairs") {
                    assert_eq!(
                        binding["keys"][1]["name"],
                        if table == "pairs" { "b" } else { "a" }
                    );
                }
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn resumed_capture_refuses_changed_identity_schema_collation_and_corrupt_arrow() {
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let mut config: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    config["table"] = json!({"schema":"fixture","table":"unicode_items","columns":[]});
    config["approved_objects"] = json!([config["table"].clone()]);
    config["budgets"]["max_rows"] = json!(2);
    let path = root.join(format!("identity-{}.json", std::process::id()));
    let out = root.join(format!("identity-{}", std::process::id()));
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    mysql_preview::capture_full(&path, &out).await.unwrap();
    fs::remove_file(out.join("capturer.json")).unwrap();
    let original: Value =
        serde_json::from_slice(&fs::read(out.join("chunk-binding.json")).unwrap()).unwrap();
    for field in [
        "source_id",
        "schema",
        "table",
        "run_generation",
        "config_sha256",
        "schema_sha256",
        "collation",
    ] {
        let mut changed = original.clone();
        if field == "collation" {
            changed["keys"][0]["collation"] = json!("utf8mb4_bin");
        } else {
            changed[field] = json!("different");
        }
        fs::write(
            out.join("chunk-binding.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert_eq!(
            mysql_preview::capture_full(&path, &out)
                .await
                .unwrap_err()
                .fact,
            "binding_mismatch",
            "{field}"
        );
        assert!(!out.join("capturer.json").exists());
    }
    fs::write(
        out.join("chunk-binding.json"),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    let manifest = fs::read(out.join("chunks.json")).unwrap();
    let mut changed: Value = serde_json::from_slice(&manifest).unwrap();
    changed[0]["rows"] = json!(1);
    fs::write(
        out.join("chunks.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    assert_eq!(
        mysql_preview::capture_full(&path, &out)
            .await
            .unwrap_err()
            .fact,
        "manifest_rows"
    );
    fs::write(out.join("chunks.json"), manifest).unwrap();
    fs::write(out.join("0.arrow"), b"corrupt").unwrap();
    assert_eq!(
        mysql_preview::capture_full(&path, &out)
            .await
            .unwrap_err()
            .fact,
        "arrow_sha256"
    );
    assert!(!out.join("capturer.json").exists());
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn capture_reconnects_and_replays_an_externally_killed_range() {
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let mut config: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    config["table"] = json!({"schema":"fixture","table":"kill_capture","columns":[]});
    config["approved_objects"] = json!([config["table"].clone()]);
    config["budgets"]["max_rows"] = json!(50000);
    let path = root.join(format!("kill-product-{}.json", std::process::id()));
    let out = root.join(format!("kill-product-{}", std::process::id()));
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let kill = async {
        let mut control = reader().await;
        let mut lock = mysql_async::Conn::new(
            OptsBuilder::default()
                .ip_or_hostname("127.0.0.1")
                .tcp_port(23380)
                .user(Some("root"))
                .db_name(Some("fixture"))
                .prefer_socket(false),
        )
        .await
        .unwrap();
        for _ in 0..1000 {
            if out.join("chunk-binding.json").exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert!(out.join("chunk-binding.json").exists());
        lock.query_drop("LOCK TABLES fixture.kill_capture WRITE")
            .await
            .unwrap();
        let mut killed = false;
        for _ in 0..1000 {
            let rows:Vec<(u64,String,String)>=control.query("SELECT ID,INFO,STATE FROM information_schema.PROCESSLIST WHERE USER='reader' AND INFO IS NOT NULL").await.unwrap();
            if let Some((id, _, _)) = rows
                .iter()
                .find(|(_, sql, state)| sql.contains("kill_capture") && state.contains("Waiting"))
            {
                control
                    .query_drop(format!("KILL CONNECTION {id}"))
                    .await
                    .unwrap();
                killed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        lock.query_drop("UNLOCK TABLES").await.unwrap();
        lock.disconnect().await.unwrap();
        control.disconnect().await.unwrap();
        assert!(
            killed,
            "the product range/boundary must be blocked in flight before kill"
        );
    };
    let (result, ()) = tokio::join!(mysql_preview::capture_full(&path, &out), kill);
    result.unwrap();
    let entries = oo_connectors::mysql::chunk::load_manifest(&out, "kill_capture").unwrap();
    assert_eq!(entries.iter().map(|entry| entry.rows).sum::<u64>(), 100000);
    let retries: u64 = entries
        .iter()
        .map(|entry| {
            let metrics: Value = serde_json::from_slice(
                &fs::read(out.join(format!("{}.metrics.json", entry.chunk_id))).unwrap(),
            )
            .unwrap();
            metrics["disconnect_retries"].as_u64().unwrap()
        })
        .sum();
    assert!(retries > 0);
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn empty_and_unkeyed_conversion_cases_do_not_publish_invalid_values() {
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let base: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    for table in ["empty_items", "empty_unkeyed", "unkeyed_bad"] {
        let mut config = base.clone();
        config["table"] = json!({"schema":"fixture","table":table,"columns":[]});
        config["approved_objects"] = json!([config["table"].clone()]);
        let path = root.join(format!("empty-{table}-{}.json", std::process::id()));
        let out = root.join(format!("empty-{table}-{}", std::process::id()));
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = mysql_preview::capture_full(&path, &out).await;
        if table == "unkeyed_bad" {
            assert_eq!(result.unwrap_err().fact, "quality_rejects");
            assert!(out.join("rejects.json").exists());
            assert!(!out.join("capturer.json").exists());
        } else {
            result.unwrap();
            let capture: Value =
                serde_json::from_slice(&fs::read(out.join("capturer.json")).unwrap()).unwrap();
            assert_eq!(capture["tables"][0]["row_count"], 0);
        }
    }
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn removing_the_source_key_cannot_bypass_resume_identity_checks() {
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let table = format!("identity_lost_key_{}", std::process::id());
    let mut admin = mysql_async::Conn::new(
        OptsBuilder::default()
            .ip_or_hostname("127.0.0.1")
            .tcp_port(23380)
            .user(Some("root"))
            .db_name(Some("fixture"))
            .prefer_socket(false),
    )
    .await
    .unwrap();
    admin.query_drop(format!("CREATE TABLE `{table}` (id varchar(32) CHARACTER SET utf8 NOT NULL PRIMARY KEY,payload text CHARACTER SET utf8mb4)")).await.unwrap();
    admin
        .query_drop(format!(
            "INSERT INTO `{table}` VALUES ('a','one'),('b','two')"
        ))
        .await
        .unwrap();
    let mut config: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    config["table"] = json!({"schema":"fixture","table":table,"columns":[]});
    config["approved_objects"] = json!([config["table"].clone()]);
    let path = root.join(format!("lost-key-{}.json", std::process::id()));
    let out = root.join(format!("lost-key-{}", std::process::id()));
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    mysql_preview::capture_full(&path, &out).await.unwrap();
    fs::remove_file(out.join("capturer.json")).unwrap();
    admin
        .query_drop(format!("ALTER TABLE `{table}` DROP PRIMARY KEY"))
        .await
        .unwrap();
    let result = mysql_preview::capture_full(&path, &out).await;
    assert_eq!(result.unwrap_err().fact, "binding_mismatch");
    assert!(!out.join("capturer.json").exists());
    admin.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn empty_chunk_capture_still_enforces_the_ipc_byte_budget() {
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let mut config: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    config["table"] = json!({"schema":"fixture","table":"empty_items","columns":[]});
    config["approved_objects"] = json!([config["table"].clone()]);
    config["budgets"]["max_bytes"] = json!(1);
    let path = root.join(format!("empty-budget-{}.json", std::process::id()));
    let out = root.join(format!("empty-budget-{}", std::process::id()));
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        mysql_preview::capture_full(&path, &out)
            .await
            .unwrap_err()
            .fact,
        "arrow_bytes"
    );
    assert!(!out.join("capturer.json").exists());
}

#[tokio::test]
#[ignore = "requires the owned oo-wf-chunk MySQL 5.7 fixture and OO_WF_CHUNK_RUN"]
async fn access_evidence_marks_source_collation_only_for_chunks() {
    let root = std::path::PathBuf::from(std::env::var("OO_WF_CHUNK_RUN").unwrap());
    let base: Value =
        serde_json::from_slice(&fs::read(root.join("explicit.json")).unwrap()).unwrap();
    for chunked in [false, true] {
        let mut config = base.clone();
        config["table"] = json!({"schema":"fixture","table":"unicode_items","columns":[]});
        config["approved_objects"] = json!([config["table"].clone()]);
        config["chunking"] = json!(chunked);
        let path = root.join(format!("order-{chunked}-{}.json", std::process::id()));
        let out = root.join(format!("order-{chunked}-{}", std::process::id()));
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        mysql_preview::capture_full(&path, &out).await.unwrap();
        let access: Value =
            serde_json::from_slice(&fs::read(out.join("access.json")).unwrap()).unwrap();
        if chunked {
            assert_eq!(access["chunk_key_order"], "source_collation");
        } else {
            assert!(access.get("chunk_key_order").is_none());
        }
    }
}
