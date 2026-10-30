use super::*;
use mysql_async::OptsBuilder;
use std::{fs, os::unix::fs::PermissionsExt};

fn config(table: &str) -> Value {
    json!({
        "source_id":"owned-empty","schema":"codec_fixture","table":table,
        "schema_fingerprint_codec":SCHEMA_CODEC,"incremental_only":true,
        "max_rows":100000,"max_bytes":33554432,"type_policy":{"zero_date_to_null":true,"strip_text_nul":true},
    })
}

fn authority(table: &str, fingerprint: &str) -> Value {
    json!({
        "protocol":AUTHORITY,"empty_only":true,"source_id":"owned-empty","schema":"codec_fixture","table":table,
        "source_generation":7,"table_source_generation":3,"schema_fingerprint":fingerprint,
        "chunk_topology_revision":1,"chunks":[],"key_columns":[],"next_chunk_index":0,
        "scan_id":null,"expected_chunks":null,"scan_layers":null,"scan_observed_at_ms":null,
        "input_digest":"b".repeat(64),"selected_columns":["v"],"columns":[{"name":"v"}],
        "projection_sha256":sha256_hex(&serde_json::to_vec(&json!(["v"])).unwrap()),
        "type_digest":"c".repeat(64),"type_policy_version":"type-policy/v1",
        "baseline_receipt":{"source_generation":3,"row_count":0,"input_digest":"b".repeat(64),
            "type_digest":"c".repeat(64),"type_policy_version":"type-policy/v1",
            "config_digest":"d".repeat(64),"retained_receipt":"synthetic-owned-target-receipt"},
        "empty_observation":{"id":"00000000-0000-4000-8000-000000000001",
            "lease_id":"00000000-0000-4000-8000-000000000002","lease_generation":1,
            "source_generation":7,"issued_at_ms":0},
        "last_empty_confirmed_at_ms":null,
    })
}

fn input() -> Value {
    json!({
        "protocol":"oo.jobs.native-producer/v1","method":"C5","c5_mode":"layered",
        "source_id":"owned-empty","binding":"00000000-0000-4000-8000-000000000003",
        "pin":1,"acknowledged_source_generation":7,"c5_observed_at_ms":now_ms().unwrap(),
        "c5_due":[true,true,true,false],"c5_layered":{
            "incremental_only":true,
            "execution_timezone":"UTC","server_timeout_ms":5000,"client_timeout_ms":10000,
            "policy":{"duty_per_mille":1000,"threads_running_max":32,"execution_window_utc":[0,1440]},
        },
    })
}

#[test]
fn exact_empty_query_is_unfiltered_bounded_and_quotes_identifiers() {
    assert_eq!(
        source_query("sample", "items").unwrap(),
        "SELECT 1 FROM `sample`.`items` LIMIT 1"
    );
    assert_eq!(
        source_query("s`x", "t`y").unwrap(),
        "SELECT 1 FROM `s``x`.`t``y` LIMIT 1"
    );
    for bad in ["", "bad\0name", &"x".repeat(65)] {
        assert!(source_query("sample", bad).is_err());
    }
}

#[test]
fn empty_authority_is_explicit_zero_only_and_context_bound() {
    let config = config("items");
    let authority = authority("items", &"a".repeat(64));
    validate(&config, &authority).unwrap();
    assert!(!selected(&json!({"chunks":[]})).unwrap());
    assert!(selected(&json!({"empty_only":true})).is_err());
    for (path, bad) in [
        ("/protocol", json!("other")),
        ("/empty_only", json!(false)),
        ("/source_id", json!("other")),
        ("/table", json!("other")),
        ("/chunks", json!([{}])),
        ("/key_columns", json!(["v"])),
        ("/scan_id", json!("invented")),
        ("/expected_chunks", json!([])),
        ("/next_chunk_index", json!(1)),
        ("/table_source_generation", json!(8)),
        ("/baseline_receipt/row_count", json!(1)),
        ("/baseline_receipt/input_digest", json!("e".repeat(64))),
        ("/projection_sha256", json!("f".repeat(64))),
        ("/selected_columns", json!(["hidden"])),
        ("/empty_observation/id", json!("not-a-uuid")),
        ("/empty_observation/lease_generation", json!(0)),
        ("/empty_observation/source_generation", json!(8)),
        ("/empty_observation/issued_at_ms", json!(-1)),
    ] {
        let mut changed = authority.clone();
        *changed.pointer_mut(path).unwrap() = bad;
        assert!(validate(&config, &changed).is_err(), "{path}");
    }
    assert!(require_schema(&"b".repeat(64), &authority).is_err());
}

#[test]
fn empty_schema_keeps_unsupported_declared_key_facts() {
    let keys = vec![(
        "PRIMARY".into(),
        0,
        1,
        "v".into(),
        None,
        Some("A".into()),
        "BTREE".into(),
        "decimal(50,0)".into(),
        None,
        "NO".into(),
    )];
    let first = fingerprint("sample", "items", &"a".repeat(64), &keys).unwrap();
    assert_ne!(
        first,
        fingerprint("sample", "items", &"a".repeat(64), &[]).unwrap()
    );
    let mut changed = keys.clone();
    changed[0].7 = "date".into();
    assert_ne!(
        first,
        fingerprint("sample", "items", &"a".repeat(64), &changed).unwrap()
    );
    changed = keys.clone();
    changed[0].2 = 2;
    assert_ne!(
        first,
        fingerprint("sample", "items", &"a".repeat(64), &changed).unwrap()
    );
    changed = keys.clone();
    changed[0].8 = Some("utf8mb4_bin".into());
    assert_ne!(
        first,
        fingerprint("sample", "items", &"a".repeat(64), &changed).unwrap()
    );
}

async fn connection(user: &str, password: &str) -> Result<Conn> {
    Ok(Conn::new(
        OptsBuilder::default()
            .ip_or_hostname("127.0.0.1")
            .tcp_port(23308)
            .user(Some(user))
            .pass(Some(password))
            .prefer_socket(false)
            .ssl_opts(None::<mysql_async::SslOpts>),
    )
    .await?)
}

#[tokio::test]
#[ignore = "owned MySQL 5.7.44 loopback fixture only; synthetic target authority"]
async fn owned_empty_snapshot_zero_only_and_failure_guards() -> Result<()> {
    let password = std::env::var("OO_C5_CODEC_FIXTURE_PASSWORD")?;
    let mut admin = connection("root", &password).await?;
    let version: String = admin
        .query_first("SELECT VERSION()")
        .await?
        .ok_or("version")?;
    assert_eq!(version, "5.7.44");
    admin
        .query_drop("CREATE DATABASE IF NOT EXISTS codec_fixture")
        .await?;
    let tag = now_ms()?;
    let user = format!("empty_{tag}");
    let password = format!("owned-empty-{tag}");
    admin
        .query_drop(format!(
            "CREATE USER '{user}'@'%' IDENTIFIED BY '{password}'"
        ))
        .await?;
    admin
        .query_drop(format!("GRANT SELECT ON codec_fixture.* TO '{user}'@'%'"))
        .await?;
    let out = std::env::temp_dir().join(format!("c5-empty-qualification-{tag}"));
    fs::create_dir(&out)?;
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
    let mut evidence = Vec::new();
    for (index, definition, insert) in [
        (0, "v VARCHAR(20) NULL,hidden INT", "NULL,1"),
        (1, "v DATE PRIMARY KEY", "'2026-10-26'"),
        (2, "v DECIMAL(50,0) PRIMARY KEY", "123"),
    ] {
        let table = format!("empty_{tag}_{index}");
        admin
            .query_drop(format!(
                "CREATE TABLE codec_fixture.`{table}` ({definition}) ENGINE=InnoDB"
            ))
            .await?;
        let config = config(&table);
        let mut reader = connection(&user, &password).await?;
        let fingerprint = schema_fingerprint(&mut reader, "codec_fixture", &table).await?;
        assert!(
            mysql::chunk::key_columns(&mut reader, "codec_fixture", &table)
                .await?
                .is_empty()
        );
        reader.disconnect().await?;
        let mut authority = authority(&table, &fingerprint);
        authority["empty_observation"]["issued_at_ms"] = json!(now_ms()?);
        if index == 0 {
            for max_bytes in [json!(0), json!(33_554_433u64), json!(-1), json!("33554432")] {
                let mut bad = config.clone();
                bad["max_bytes"] = max_bytes;
                assert!(
                    super::super::produce(
                        connection(&user, &password).await?,
                        &bad,
                        &input(),
                        &authority,
                        &out,
                    )
                    .await
                    .is_err()
                );
            }
        }
        let frozen_config = config.clone();
        let result = super::super::produce(
            connection(&user, &password).await?,
            &config,
            &input(),
            &authority,
            &out,
        )
        .await?;
        assert_eq!(
            config, frozen_config,
            "historical admission budgets remain unchanged"
        );
        let observation = &result["c5_observation"];
        assert_eq!(result["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(observation.as_object().unwrap().len(), 20);
        assert_eq!(observation["protocol"], OBSERVATION);
        assert_eq!(
            observation["baseline_receipt"],
            authority["baseline_receipt"]
        );
        assert_eq!(
            observation["source_query"],
            format!("SELECT 1 FROM `codec_fixture`.`{table}` LIMIT 1")
        );
        assert_eq!(observation["rows_returned"], 0);
        assert_eq!(observation["snapshot_complete"], true);
        assert_eq!(observation["read_only"], true);
        assert_eq!(observation["schema_checked_before"], true);
        assert_eq!(observation["schema_checked_after"], true);
        assert!(
            number(observation, "started_at_ms")?
                >= number(&authority["empty_observation"], "issued_at_ms")?
        );
        assert!(number(observation, "checked_at_ms")? >= number(observation, "started_at_ms")?);
        assert_eq!(result["source_generation"], 7);
        assert_eq!(observation["table_source_generation"], 3);
        assert_eq!(
            result["c5_completed_layers"],
            json!([false, false, false, false])
        );
        for key in ["data_gen", "apply", "ack"] {
            assert_eq!(result[key], false);
        }
        for key in ["chunks", "full_digest_scan_completed", "digest"] {
            assert!(observation.get(key).is_none());
        }
        assert_eq!(result["parent_digest"], authority["input_digest"]);
        validate_replay(&config, &authority, &result)?;
        let mut fresh_context = authority.clone();
        fresh_context["empty_observation"]["id"] = json!("00000000-0000-4000-8000-000000000004");
        assert!(validate_replay(&config, &fresh_context, &result).is_err());
        let mut future_context = authority.clone();
        future_context["empty_observation"]["issued_at_ms"] = json!(now_ms()? + 60_000);
        let refused = super::super::produce(
            connection(&user, &password).await?,
            &config,
            &input(),
            &future_context,
            &out,
        )
        .await
        .unwrap_err()
        .to_string();
        assert_eq!(refused, "C5_EMPTY_CONTEXT_NOT_ISSUED");
        if index == 0 {
            admin
                .query_drop(format!("LOCK TABLES codec_fixture.`{table}` WRITE"))
                .await?;
            let mut bounded = input();
            bounded["c5_layered"]["server_timeout_ms"] = json!(10);
            bounded["c5_layered"]["client_timeout_ms"] = json!(50);
            let result = super::super::produce(
                connection(&user, &password).await?,
                &config,
                &bounded,
                &authority,
                &out,
            )
            .await;
            admin.query_drop("UNLOCK TABLES").await?;
            let refused = result
                .err()
                .ok_or("locked empty table accepted")?
                .to_string();
            assert!(
                refused.contains("deadline")
                    || refused.contains("3024")
                    || refused.contains("maximum statement execution time"),
                "{refused}",
            );
        }
        admin
            .query_drop(format!(
                "INSERT INTO codec_fixture.`{table}` VALUES ({insert})"
            ))
            .await?;
        let refused = super::super::produce(
            connection(&user, &password).await?,
            &config,
            &input(),
            &authority,
            &out,
        )
        .await
        .unwrap_err()
        .to_string();
        assert_eq!(refused, "C5_EMPTY_BASELINE_NONEMPTY");
        evidence.push(json!({"table":table,"schema_fingerprint":fingerprint,"observation":observation,"nonempty_error":refused}));
        if index == 1 {
            admin
                .query_drop(format!(
                    "ALTER TABLE codec_fixture.`{table}` DROP PRIMARY KEY"
                ))
                .await?;
            let refused = super::super::produce(
                connection(&user, &password).await?,
                &config,
                &input(),
                &authority,
                &out,
            )
            .await
            .unwrap_err()
            .to_string();
            assert_eq!(refused, "SCHEMA_DRIFT");
        }
    }
    let mut root = connection("root", &std::env::var("OO_C5_CODEC_FIXTURE_PASSWORD")?).await?;
    let grants: Vec<String> = root.query("SHOW GRANTS FOR CURRENT_USER()").await?;
    assert!(mysql::full_read_grants(&grants).is_err());
    root.disconnect().await?;
    assert_eq!(
        fs::read_dir(&out)?.count(),
        0,
        "empty probe must not emit payload/capture artifacts"
    );
    c5plus::save(
        &out.join("qualification.json"),
        &json!({
            "scope":"owned MySQL source test with synthetic target authority; not worker/0071 E2E",
            "mysql_version":version,"schema_codec":SCHEMA_CODEC,"cases":evidence,
            "bounded_snapshot_timeout_refused":true,"new_context_replay_refused":true,
            "future_context_refused":true,"schema_drift_refused":true,"no_payload_artifacts":true,
        "admitted_max_rows":100000,"admitted_max_bytes":33554432,"over_budget_config_refused":true,
            "scheduler_layers_completed":[false,false,false,false],
        }),
    )?;
    println!(
        "C5_EMPTY_QUALIFICATION={}",
        out.join("qualification.json").display()
    );
    admin.disconnect().await?;
    Ok(())
}
