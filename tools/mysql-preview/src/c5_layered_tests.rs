use super::*;
use oo_connectors::{
    mysql::{
        chunk::{KeyColumn, KeyType},
        map::{MappedColumn, MysqlColumn},
        values::RawCell,
    },
    types::assemble_batch,
};

#[test]
fn source_projection_is_checked_against_canonical_target_identifiers() {
    let raw = vec!["id".to_owned(), "refEnum".into(), "select".into(), "q_literal".into(), "LongMixedCase".repeat(4)];
    let canonical = canonical_projection(&raw).unwrap();
    let authority = json!({"source_generation":7,"selected_columns":canonical});
    assert_eq!(selected_projection(&json!({"columns":raw}), &authority).unwrap(), raw);
    assert!(selected_projection(&json!({}), &authority).is_err());
    assert!(selected_projection(&json!({"columns":canonical}), &authority).is_err());
}

fn columns() -> Vec<MappedColumn> {
    [
        MysqlColumn {
            name: "id".into(),
            ordinal: 1,
            data_type: "bigint".into(),
            column_type: "bigint".into(),
            nullable: false,
            char_len: None,
            precision: Some(19),
            scale: Some(0),
            charset: None,
            column_key: "PRI".into(),
        },
        MysqlColumn {
            name: "body".into(),
            ordinal: 2,
            data_type: "varchar".into(),
            column_type: "varchar(100)".into(),
            nullable: true,
            char_len: Some(100),
            precision: None,
            scale: None,
            charset: Some("utf8mb4".into()),
            column_key: "".into(),
        },
    ]
    .iter()
    .map(|c| mysql::map::map_capture_column(c, false, true, &Default::default()).unwrap())
    .collect()
}

fn keys() -> Vec<KeyColumn> {
    vec![KeyColumn {
        name: "id".into(),
        kind: KeyType::Int,
        collation: None,
    }]
}

fn float_columns() -> Vec<MappedColumn> {
    let mut mapped = vec![columns()[0].clone()];
    for (name, kind) in [("f", "float"), ("d", "double")] {
        mapped.push(
            mysql::map::map_capture_column(
                &MysqlColumn {
                    name: name.into(),
                    ordinal: mapped.len() as i32 + 1,
                    data_type: kind.into(),
                    column_type: kind.into(),
                    nullable: true,
                    char_len: None,
                    precision: None,
                    scale: None,
                    charset: None,
                    column_key: "".into(),
                },
                false,
                true,
                &Default::default(),
            )
            .unwrap(),
        );
    }
    mapped
}

fn arrow(columns: &[MappedColumn], rows: &[Vec<RawCell>]) -> Vec<u8> {
    let binding = RunBinding {
        source_id: "fixture".into(),
        schema: "sample".into(),
        table: "items".into(),
        run_generation: 1,
        input_digest: [0; 32],
    };
    let cells = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .zip(row)
                .map(|(column, raw)| {
                    let (cell, reject) = mysql::values::decode_mapped(column, raw).unwrap();
                    assert!(reject.is_none());
                    cell
                })
                .collect()
        })
        .collect();
    let batch = assemble_batch(
        &binding,
        columns.iter().map(|c| c.plan.clone()).collect(),
        &[1],
        cells,
    )
    .unwrap()
    .record_batch()
    .unwrap();
    let mut bytes = Vec::new();
    let mut writer = StreamWriter::try_new(&mut bytes, &batch.schema()).unwrap();
    writer.write(&batch).unwrap();
    writer.finish().unwrap();
    drop(writer);
    bytes
}

fn source_digest(value: u64) -> Value {
    json!([1, format!("{value:016x}"), "0000000000000000"])
}

fn chunk_entry(index: usize, value: u64) -> Value {
    observed_chunk(
        &json!({
            "chunk_id":index.to_string(),
            "lo":if index == 0 { Value::Null } else { encode_bound(Some(&[KeyValue::Int(index as i128)])) },
            "hi":encode_bound(Some(&[KeyValue::Int(index as i128 + 1)])),
            "membership_revision":1,"row_count":1,"arrow_sha256":"a".repeat(64),
        }),
        source_digest(value),
        digest::SOURCE_MD5,
        "projection",
        10,
    )
}

#[test]
fn warm_nochange_skips_payload_but_never_skips_independent_full_read() {
    let entry = chunk_entry(0, 7);
    assert!(!needs_capture(
        &entry,
        &source_digest(7),
        "projection",
        false,
        digest::SOURCE_MD5
    ));
    assert!(needs_capture(
        &entry,
        &source_digest(7),
        "projection",
        true,
        digest::SOURCE_MD5
    ));
    assert!(needs_capture(
        &entry,
        &source_digest(8),
        "projection",
        false,
        digest::SOURCE_MD5
    ));
    assert!(needs_capture(
        &entry,
        &source_digest(7),
        "different",
        false,
        digest::SOURCE_MD5
    ));
}

#[test]
fn changed_middle_block_receipts_keep_the_prefix_and_resume_only_after_confirmation() {
    let chunks = vec![chunk_entry(0, 1), chunk_entry(1, 2), chunk_entry(2, 3)];
    let authority = json!({
        "source_id":"shared","schema":"sample","table":"items","source_generation":9,
        "chunk_topology_revision":2,"chunks":chunks,"next_chunk_index":0,
    });
    let source = [source_digest(1), source_digest(20), source_digest(30)];
    let mut observed = Vec::new();
    let mut visits = Vec::new();
    for (index, entry) in chunks
        .iter()
        .enumerate()
        .skip(scan_cursor(&authority, &chunks).unwrap())
    {
        visits.push(index);
        let updated = observed_chunk(
            entry,
            source[index].clone(),
            digest::SOURCE_MD5,
            "projection",
            11,
        );
        observed.push(updated);
        if needs_capture(
            entry,
            &source[index],
            "projection",
            false,
            digest::SOURCE_MD5,
        ) {
            break;
        }
    }
    assert_eq!(visits, vec![0, 1]);
    let receipt = observation(&authority, &columns(), "schema", &observed, 2, 11);
    assert_eq!(receipt["chunks"].as_array().unwrap().len(), 2);
    assert_eq!(receipt["chunks"][0]["digest"], source[0]);
    assert_eq!(receipt["parent_generation"], 9);
    assert_eq!(receipt["table"], "items");
    assert_eq!(receipt["full_digest_scan_completed"], false);
    // Merely producing a receipt cannot advance the input authority.
    assert_eq!(scan_cursor(&authority, &chunks).unwrap(), 0);
    let mut confirmed = authority.clone();
    for (index, value) in observed.into_iter().enumerate() {
        confirmed["chunks"][index] = value;
    }
    confirmed["next_chunk_index"] = receipt["next_chunk_index"].clone();
    confirmed["source_generation"] = json!(10);
    let confirmed_chunks = confirmed["chunks"].as_array().unwrap();
    visits.extend(scan_cursor(&confirmed, confirmed_chunks).unwrap()..confirmed_chunks.len());
    assert_eq!(visits, vec![0, 1, 2]);
    assert_eq!(authority["source_generation"], 9);
}

#[test]
fn cursor_bounds_and_missing_projection_fail_closed() {
    let chunks = vec![chunk_entry(0, 1)];
    for bad in [json!(-1), json!(2), json!("1"), Value::Null] {
        assert!(scan_cursor(&json!({"next_chunk_index":bad}), &chunks).is_err());
    }
    assert_eq!(
        scan_cursor(&json!({"next_chunk_index":1}), &chunks).unwrap(),
        1
    );
    let published = json!({"source_generation":1});
    assert!(selected_projection(&json!({}), &published).is_err());
}

#[test]
fn resume_cannot_upgrade_recent_scan_to_full_or_advance_its_window() {
    let recent = [true, true, false, false];
    let authority = json!({"scan_id":"owned","scan_layers":recent,"scan_observed_at_ms":1000});
    assert_eq!(scan_time(&authority, &recent, 2, 2000).unwrap(), 1000);
    assert!(scan_time(&authority, &[true, true, true, false], 2, 2000).is_err());
    assert!(scan_time(&json!({}), &recent, 2, 2000).is_err());
    assert!(scan_time(&authority, &recent, 2, 900).is_err());
    assert_eq!(scan_time(&authority, &recent, 0, 2000).unwrap(), 1000);
    assert_eq!(scan_time(&json!({}), &recent, 0, 2000).unwrap(), 2000);
}

#[test]
fn deferred_l4_does_not_override_l1_l3_after_target_confirmation() {
    let columns = columns();
    let projection = projection_digest(&columns).unwrap();
    let mut chunks = vec![chunk_entry(0, 1), chunk_entry(1, 2), chunk_entry(2, 3)];
    for entry in &mut chunks {
        entry["projection_sha256"] = json!(projection);
    }
    let mut authority = json!({"schema_fingerprint":"bound","next_chunk_index":0});
    let policy = mysql::values::TypePolicy::default();
    let required = |authority: &Value, chunks: &[Value], due: [bool; 4]| {
        full_digest_scan_required(
            true,
            &due,
            authority,
            chunks,
            digest::SOURCE_MD5,
            &projection,
            policy,
        )
        .unwrap()
    };
    // A non-null hydration seed still needs an initial source confirmation.
    assert!(required(&authority, &chunks, [false, false, false, true]));
    for entry in &mut chunks {
        entry["last_digest_scan_id"] = json!("target-confirmed-initial-l3");
    }
    for due in [
        [false, false, false, true],
        [true, false, false, true],
        [true, true, false, true],
    ] {
        assert!(!required(&authority, &chunks, due));
    }
    assert!(required(&authority, &chunks, [false, false, true, true]));
    authority["next_chunk_index"] = json!(1);
    assert!(required(&authority, &chunks, [false, false, false, true]));
    authority["next_chunk_index"] = json!(0);
    chunks[1]["digest"] = Value::Null;
    assert!(required(&authority, &chunks, [false, false, false, true]));
}

#[test]
fn incremental_only_must_match_the_worker_pin_and_is_a_strict_boolean() {
    assert!(!incremental_only(&json!({}), &json!({"c5_layered":{}})).unwrap());
    assert!(
        incremental_only(
            &json!({"incremental_only":true}),
            &json!({"c5_layered":{"incremental_only":true}})
        )
        .unwrap()
    );
    for (config, input) in [
        (json!({"incremental_only":true}), json!({"c5_layered":{}})),
        (json!({}), json!({"c5_layered":{"incremental_only":true}})),
        (
            json!({"incremental_only":true}),
            json!({"c5_layered":{"incremental_only":false}}),
        ),
        (
            json!({"incremental_only":"true"}),
            json!({"c5_layered":{"incremental_only":true}}),
        ),
        (
            json!({"incremental_only":true}),
            json!({"c5_layered":{"incremental_only":1}}),
        ),
        (json!({}), json!({"c5_layered":{"incremental_only":null}})),
    ] {
        assert!(incremental_only(&config, &input).is_err());
    }
}

#[test]
fn projection_is_ordered_pinned_and_never_implicitly_expanded() {
    let authority = json!({"source_generation":1,"selected_columns":["body","id"]});
    assert_eq!(
        selected_projection(&json!({}), &authority).unwrap(),
        vec!["body", "id"]
    );
    assert_eq!(
        selected_projection(&json!({"columns":[]}), &authority).unwrap(),
        vec!["body", "id"]
    );
    for columns in [
        json!(["id", "body"]),
        json!(["body", "id", "new"]),
        json!(["id", "id"]),
        json!([1]),
    ] {
        assert!(selected_projection(&json!({"columns":columns}), &authority).is_err());
    }
    let mut mapped = columns();
    let first = projection_digest(&mapped).unwrap();
    mapped.reverse();
    assert_ne!(first, projection_digest(&mapped).unwrap());
}

#[test]
fn schema_fingerprint_uses_historical_full_catalog_encoding() {
    let mut catalog = vec![
        MysqlColumn {
            name: "id".into(),
            ordinal: 1,
            data_type: "bigint".into(),
            column_type: "bigint(20)".into(),
            nullable: false,
            char_len: None,
            precision: Some(19),
            scale: Some(0),
            charset: None,
            column_key: "PRI".into(),
        },
        MysqlColumn {
            name: "unselected".into(),
            ordinal: 2,
            data_type: "varchar".into(),
            column_type: "varchar(100)".into(),
            nullable: true,
            char_len: Some(100),
            precision: None,
            scale: None,
            charset: Some("gbk".into()),
            column_key: "".into(),
        },
    ];
    // This is the full.rs:475 preimage, not the old layered columns+keys object.
    let historical = json!([
        {"name":"id","ordinal":1,"data_type":"bigint","column_type":"bigint(20)","nullable":false,
            "char_len":null,"precision":19,"scale":0,"charset":null,"column_key":"PRI"},
        {"name":"unselected","ordinal":2,"data_type":"varchar","column_type":"varchar(100)","nullable":true,
            "char_len":100,"precision":null,"scale":null,"charset":"gbk","column_key":""},
    ]);
    let full = sha256_hex(&serde_json::to_vec(&historical).unwrap());
    assert_eq!(catalog_fingerprint(&catalog).unwrap(), full);
    assert_ne!(catalog_fingerprint(&catalog[..1]).unwrap(), full);
    catalog[1].charset = Some("utf8mb4".into());
    assert_ne!(catalog_fingerprint(&catalog).unwrap(), full);
    catalog[1].charset = Some("gbk".into());
    catalog[1].nullable = false;
    assert_ne!(catalog_fingerprint(&catalog).unwrap(), full);
    catalog[1].nullable = true;
    catalog.reverse();
    assert_ne!(catalog_fingerprint(&catalog).unwrap(), full);
}

#[test]
fn schema_fingerprint_mode_is_explicit_and_pins_full_key_metadata() {
    assert!(!full_catalog_fingerprint(&json!({})).unwrap());
    assert!(
        !full_catalog_fingerprint(
            &json!({"schema_fingerprint_codec":"mysql-layered-columns-keys/v1"})
        )
        .unwrap()
    );
    let keys = vec![
        KeyColumn {
            name: "b".into(),
            kind: KeyType::Text,
            collation: Some("gbk_chinese_ci".into()),
        },
        KeyColumn {
            name: "a".into(),
            kind: KeyType::Int,
            collation: None,
        },
    ];
    let config = json!({"schema_fingerprint_codec":"mysql-full-catalog/v1","baseline_keys":keys});
    assert!(full_catalog_fingerprint(&config).unwrap());
    check_baseline_keys(&config, &keys).unwrap();
    let mut changed = keys.clone();
    changed.reverse();
    assert!(check_baseline_keys(&config, &changed).is_err());
    changed = keys.clone();
    changed[0].collation = Some("gbk_bin".into());
    assert!(check_baseline_keys(&config, &changed).is_err());
    changed = keys.clone();
    changed[0].kind = KeyType::Bytes;
    assert!(check_baseline_keys(&config, &changed).is_err());
    for invalid in [
        json!({"schema_fingerprint_codec":"unknown"}),
        json!({"schema_fingerprint_codec":null}),
        json!({"schema_fingerprint_codec":"mysql-full-catalog/v1"}),
        json!({"schema_fingerprint_codec":"mysql-full-catalog/v1","baseline_keys":[]}),
        json!({"schema_fingerprint_codec":"mysql-full-catalog/v1","baseline_keys":[{"name":"id","kind":"Int","collation":null,"extra":true}]}),
    ] {
        assert!(full_catalog_fingerprint(&invalid).is_err());
    }
}

#[test]
fn capture_preimages_match_full_bytes_and_preserve_legacy_encoding() -> Result<()> {
    let tag = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let out = std::env::temp_dir().join(format!("c5-preimage-parity-{}-{tag}", std::process::id()));
    fs::create_dir(&out)?;
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
    let mut config = json!({
        "schema_fingerprint_codec":"mysql-full-catalog/v1","baseline_keys":keys(),
        "source_id":"fixture","schema":"sample","table":"items","max_rows":100000,"max_bytes":33554432,
    });
    let request = json!({
        "protocol":"oo.pipeline.native-c5-range/v1","source_engine":"mysql","capture_method":"c5_diff",
        "source_id":"fixture","schema":"sample","table":"items","schema_fingerprint":"a".repeat(64),
        "chunk_topology_revision":1,"chunk_id":"0","lo":null,"hi":null,"prior_membership_revision":1,
        "parent_generation":1,"source_generation":2,"arrow_sha256":"b".repeat(64),
        "row_count":0,"members":[],"delete_detection":"reconcile","position_codec":"snapshot-zero/v1",
        "applied_pos":"0","arrow_path":out.join("empty.arrow"),"key_columns":["id"],
    });
    let audit = json!({"privileges":["SELECT"]});
    let run = "00000000-0000-4000-8000-000000000001";
    let mut columns = columns();
    write_capture(&out, &config, &request, &columns, &audit, run)?;
    let facts: Value = serde_json::from_slice(&fs::read(out.join("capturer.json"))?)?;
    assert_eq!(facts["max_rows"], config["max_rows"]);
    assert_eq!(facts["max_bytes"], config["max_bytes"]);
    assert_eq!(
        facts["config_sha256"],
        sha256_hex(&serde_json::to_vec(&config)?)
    );
    let mut original = Vec::new();
    for name in ["schema-preimage.json", "type-preimage.json"] {
        let bytes = fs::read(out.join(name))?;
        let logical: Value = serde_json::from_slice(&bytes)?;
        // The historical full writer uses this exact serializer, not a canonical JSON hash.
        let full_bytes = serde_json::to_vec_pretty(&logical)?;
        assert_eq!(bytes, full_bytes, "{name}");
        assert_eq!(sha256_hex(&bytes), sha256_hex(&full_bytes));
        assert!(
            !bytes.ends_with(b"\n"),
            "{name}: full.rs does not append a newline"
        );
        assert_ne!(
            sha256_hex(&bytes),
            sha256_hex(&serde_json::to_vec(&logical)?)
        );
        assert_eq!(
            fs::metadata(out.join(name))?.permissions().mode() & 0o077,
            0
        );
        assert!(!out.join(name).with_extension("json.pending").exists());
        original.push(bytes);
    }
    columns[1].plan.mapping.nullable = false;
    write_capture(&out, &config, &request, &columns, &audit, run)?;
    assert_ne!(
        fs::read(out.join("schema-preimage.json"))?,
        original[0],
        "schema bytes must be regenerated from current columns, never copied from the baseline"
    );
    assert_eq!(fs::read(out.join("type-preimage.json"))?, original[1]);
    columns[1].plan.mapping.nullable = true;
    config
        .as_object_mut()
        .unwrap()
        .remove("schema_fingerprint_codec");
    write_capture(&out, &config, &request, &columns, &audit, run)?;
    for (index, name) in ["schema-preimage.json", "type-preimage.json"]
        .iter()
        .enumerate()
    {
        let logical: Value = serde_json::from_slice(&original[index])?;
        assert_eq!(
            fs::read(out.join(name))?,
            serde_json::to_vec(&logical)?,
            "legacy compact preimage identity must not change"
        );
    }
    fs::remove_dir_all(out)?;
    Ok(())
}

#[test]
fn null_digest_bootstrap_requires_capture_even_if_current_source_is_available() {
    let mut entry = chunk_entry(0, 1);
    entry["digest"] = Value::Null;
    let probe = source_digest(2);
    assert!(needs_capture(
        &entry,
        &probe,
        "projection",
        false,
        digest::SOURCE_MD5
    ));
    let observed = observed_chunk(&entry, probe.clone(), digest::SOURCE_MD5, "projection", 12);
    let (confirmed_candidate, equal) =
        captured_chunk(observed, probe.clone(), &"a".repeat(64), 1).unwrap();
    assert!(equal);
    assert_eq!(confirmed_candidate["membership_revision"], 1);
    assert_eq!(confirmed_candidate["digest"], probe);
    // Nothing gets installed in authority until observe succeeds.
    assert!(entry["digest"].is_null());
    assert!(!needs_capture(
        &confirmed_candidate,
        &probe,
        "projection",
        false,
        digest::SOURCE_MD5
    ));
}

#[test]
fn changed_first_delta_and_source_change_between_probe_and_capture_are_not_hidden() {
    let mut baseline = chunk_entry(0, 1);
    baseline["digest"] = Value::Null;
    let probe = source_digest(2);
    let captured = source_digest(3);
    let updated = observed_chunk(
        &baseline,
        probe.clone(),
        digest::SOURCE_MD5,
        "projection",
        12,
    );
    let (receipt, equal) = captured_chunk(updated, captured.clone(), &"b".repeat(64), 1).unwrap();
    assert!(!equal);
    assert_eq!(receipt["membership_revision"], 2);
    assert_eq!(receipt["digest"], captured);
    assert_ne!(receipt["digest"], probe);
    assert!(needs_capture(
        &baseline,
        &captured,
        "projection",
        false,
        digest::SOURCE_MD5
    ));
    assert!(captured_chunk(receipt, source_digest(3), &"b".repeat(64), 0).is_err());
}

#[test]
fn source_reverting_to_baseline_still_receipts_capture_digest_not_probe() {
    let baseline = chunk_entry(0, 1);
    let updated = observed_chunk(
        &baseline,
        source_digest(2),
        digest::SOURCE_MD5,
        "projection",
        12,
    );
    let (receipt, equal) = captured_chunk(updated, source_digest(1), &"a".repeat(64), 1).unwrap();
    assert!(equal);
    assert_eq!(receipt["digest"], source_digest(1));
    assert_eq!(receipt["membership_revision"], 1);
}

#[test]
fn unreceipted_arrow_hash_never_establishes_nochange() {
    let mut entry = chunk_entry(0, 1);
    entry["membership_revision"] = json!(0);
    let (_, equal) = captured_chunk(entry, source_digest(1), &"a".repeat(64), 1).unwrap();
    assert!(!equal);
}

#[test]
fn empty_capture_is_a_changed_range_not_a_missing_or_unchanged_block() {
    let baseline = chunk_entry(0, 1);
    let empty = digest::Digest::default().json();
    let (receipt, unchanged) = captured_chunk(baseline, empty.clone(), &"c".repeat(64), 0).unwrap();
    assert!(!unchanged);
    assert_eq!(receipt["row_count"], 0);
    assert_eq!(receipt["membership_revision"], 2);
    assert_eq!(receipt["digest"], empty);
    assert!(!needs_capture(
        &receipt,
        &empty,
        "projection",
        false,
        digest::SOURCE_MD5
    ));
}

#[test]
fn raw_aggregate_contract_matches_existing_c5plus_and_capture_uses_same_hash() {
    let columns = columns();
    let sql = digest::aggregate_sql(
        "sample",
        "items",
        &keys(),
        &columns,
        "`id` > ?",
        5000,
        digest::SOURCE_MD5,
    )
    .unwrap();
    assert_eq!(
        sql,
        c5plus::digest_sql(
            "sample",
            "items",
            &["id"],
            &["id", "body"],
            "`id` > ?",
            5000
        )
        .unwrap()
    );
    let row_hash = digest::row_hash_sql(&columns, digest::SOURCE_MD5).unwrap();
    assert!(sql.contains(&format!("SUBSTRING({row_hash},1,16)")));
    let mut accumulator = digest::Digest::default();
    accumulator.add("00000000000000020000000000000004").unwrap();
    accumulator.add("00000000000000030000000000000005").unwrap();
    assert_eq!(
        accumulator.json(),
        json!([2, "0000000000000001", "0000000000000001"])
    );
    assert!(accumulator.add("not-a-hash").is_err());
}

#[test]
fn local_candidate_distinguishes_null_empty_bytes_and_row_changes() {
    let columns = columns();
    let read = |body: RawCell| {
        let bytes = arrow(&columns, &[vec![RawCell::Int(1), body]]);
        let decoded = oo_pipeline::native_arrow::decode_arrow_stream(&bytes).unwrap();
        digest::arrow_digest(&decoded, &columns).unwrap()
    };
    let null = read(RawCell::Null);
    let empty = read(RawCell::Bytes(vec![]));
    let a = read(RawCell::Bytes(b"a".to_vec()));
    let b = read(RawCell::Bytes(b"b".to_vec()));
    assert_ne!(null, empty);
    assert_ne!(a, b);
    let mut expected = digest::Digest::default();
    expected.add(&sha256_hex(b"31#0#61#0")).unwrap();
    assert_eq!(a, expected.json());
    let mut cold = chunk_entry(0, 0);
    cold["digest"] = Value::Null;
    // An unconfirmed hash without its archive proof never establishes nochange.
    assert!(needs_capture(
        &cold,
        &a,
        "projection",
        false,
        digest::SOURCE_MD5
    ));
}

#[test]
fn local_candidate_requires_receipted_bounded_exact_arrow() {
    let columns = columns();
    let bytes = arrow(
        &columns,
        &[vec![RawCell::Int(1), RawCell::Bytes(b"baseline".to_vec())]],
    );
    let path = std::env::temp_dir().join(format!("c5-bootstrap-{}.arrow", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    use std::io::Write;
    file.write_all(&bytes).unwrap();
    let config = json!({"baseline_arrows":{"0":path},"max_rows":10,"max_bytes":65536});
    let mut entry = chunk_entry(0, 0);
    entry["arrow_sha256"] = json!(sha256_hex(&bytes));
    let result = digest::baseline_digest(&config, &entry, &columns)
        .unwrap()
        .unwrap();
    assert_eq!(result[0], 1);
    for cap in [16_777_216, 16_777_217, 33_554_432] {
        let mut approved = config.clone();
        approved["max_bytes"] = json!(cap);
        let before = approved.clone();
        assert_eq!(
            digest::baseline_digest(&approved, &entry, &columns).unwrap(),
            Some(result.clone()),
        );
        assert_eq!(approved, before, "historical budget must not be rewritten");
    }
    for cap in [0, 33_554_433] {
        let mut refused = config.clone();
        refused["max_bytes"] = json!(cap);
        assert_eq!(
            digest::baseline_digest(&refused, &entry, &columns)
                .unwrap_err()
                .to_string(),
            "baseline budget",
        );
    }
    entry["arrow_sha256"] = json!("b".repeat(64));
    assert!(digest::baseline_digest(&config, &entry, &columns).is_err());
    entry["arrow_sha256"] = json!(sha256_hex(&bytes));
    entry["row_count"] = json!(2);
    assert!(digest::baseline_digest(&config, &entry, &columns).is_err());
    entry["row_count"] = json!(1);
    let mut wrong_projection = columns.clone();
    wrong_projection.reverse();
    assert!(digest::baseline_digest(&config, &entry, &wrong_projection).is_err());
    let mut tiny_budget = config.clone();
    tiny_budget["max_bytes"] = json!(10);
    assert!(digest::baseline_digest(&tiny_budget, &entry, &columns).is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn unsupported_local_types_cannot_enable_baseline_shortcut() {
    let mut columns = columns();
    columns[1].store = mysql::map::StoreKind::Json;
    assert_eq!(digest::codec(&columns), digest::SOURCE_MD5);
    assert!(digest::row_hash_sql(&columns, digest::ARROW_SHA256).is_err());
    assert!(
        digest::baseline_digest(&json!({}), &json!({}), &columns)
            .unwrap()
            .is_none()
    );
}

#[test]
fn float_digest_uses_shared_injective_codec_and_refuses_lossy_float4() {
    let columns = float_columns();
    assert_eq!(digest::codec(&columns), digest::ARROW_SHA256);
    for (f, d) in [
        (0.0_f32, 0.0_f64),
        (-0.0, -0.0),
        (0.1, 1.2345678901234567),
        (f32::from_bits(1), f64::from_bits(1)),
        (f32::MAX, f64::MAX),
    ] {
        let bytes = arrow(
            &columns,
            &[vec![RawCell::Int(1), RawCell::Float(f), RawCell::Double(d)]],
        );
        let mut stream = oo_pipeline::native_arrow::decode_arrow_stream(&bytes).unwrap();
        let mut expected = digest::Digest::default();
        expected
            .add(&sha256_hex(
                format!(
                    "31#0#{}#0#{}#0",
                    hex(oo_pipeline::native_c5_float::canonical(f64::from(f))
                        .unwrap()
                        .as_bytes())
                    .to_uppercase(),
                    hex(oo_pipeline::native_c5_float::canonical(d)
                        .unwrap()
                        .as_bytes())
                    .to_uppercase(),
                )
                .as_bytes(),
            ))
            .unwrap();
        assert_eq!(
            digest::arrow_digest(&stream, &columns).unwrap(),
            expected.json()
        );
        stream.columns[1][0] = oo_pipeline::native_arrow::NativeCell::Float64(0.1_f64.to_bits());
        assert!(digest::arrow_digest(&stream, &columns).is_err());
        stream.columns[1][0] =
            oo_pipeline::native_arrow::NativeCell::Float64(f64::from(f).to_bits());
        stream.columns[2][0] =
            oo_pipeline::native_arrow::NativeCell::Float64(f64::INFINITY.to_bits());
        assert!(digest::arrow_digest(&stream, &columns).is_err());
    }
    let sql = digest::row_hash_sql(&columns, digest::ARROW_SHA256).unwrap();
    assert!(sql.contains("JSON_ARRAY(`f`)"));
    assert!(sql.contains("JSON_ARRAY(`d`)"));
    assert!(!sql.contains(" FROM "));
    assert!(!sql.contains("FORMAT("));
}

#[test]
fn candidate_decimal_and_datetime_use_explicit_canonical_forms_not_pg_display_text() {
    let mut columns = columns();
    columns[1] = mysql::map::map_capture_column(
        &MysqlColumn {
            name: "amount".into(),
            ordinal: 2,
            data_type: "decimal".into(),
            column_type: "decimal(12,4)".into(),
            nullable: true,
            char_len: None,
            precision: Some(12),
            scale: Some(4),
            charset: None,
            column_key: "".into(),
        },
        false,
        true,
        &Default::default(),
    )
    .unwrap();
    columns.push(
        mysql::map::map_capture_column(
            &MysqlColumn {
                name: "at_time".into(),
                ordinal: 3,
                data_type: "datetime".into(),
                column_type: "datetime(6)".into(),
                nullable: true,
                char_len: None,
                precision: None,
                scale: None,
                charset: None,
                column_key: "".into(),
            },
            false,
            true,
            &Default::default(),
        )
        .unwrap(),
    );
    let bytes = arrow(
        &columns,
        &[vec![
            RawCell::Int(1),
            RawCell::Bytes(b"-0.5000".to_vec()),
            RawCell::Date {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 1,
                micros: 120000,
            },
        ]],
    );
    let stream = oo_pipeline::native_arrow::decode_arrow_stream(&bytes).unwrap();
    let mut expected = digest::Digest::default();
    expected
        .add(&sha256_hex(
            format!(
                "31#0#{}#0#{}#0",
                hex(b"-0.5000").to_uppercase(),
                hex(b"1120000").to_uppercase(),
            )
            .as_bytes(),
        ))
        .unwrap();
    assert_eq!(
        digest::arrow_digest(&stream, &columns).unwrap(),
        expected.json()
    );
    let candidate = digest::row_hash_sql(&columns, digest::ARROW_SHA256).unwrap();
    assert!(candidate.contains("DECIMAL(12,4)"));
    assert!(candidate.contains("TIMESTAMPDIFF(MICROSECOND,"));
    let deployed = digest::row_hash_sql(&columns, digest::SOURCE_MD5).unwrap();
    assert!(!deployed.contains("TIMESTAMPDIFF"));
    assert!(deployed.starts_with("MD5("));
}

#[test]
fn raw_digest_does_not_hash_transcoded_capture_expressions() {
    let mut columns = columns();
    columns[1] = mysql::map::map_capture_column(
        &MysqlColumn {
            name: "body".into(),
            ordinal: 2,
            data_type: "varchar".into(),
            column_type: "varchar(100)".into(),
            nullable: true,
            char_len: Some(100),
            precision: None,
            scale: None,
            charset: Some("gbk".into()),
            column_key: "".into(),
        },
        false,
        true,
        &Default::default(),
    )
    .unwrap();
    assert!(mysql::map::capture_expression(&columns[1]).contains("CONVERT("));
    let raw = digest::row_hash_sql(&columns, digest::SOURCE_MD5).unwrap();
    assert!(raw.contains("HEX(CAST(`body` AS BINARY))"));
    assert!(!raw.contains("CONVERT("));
    assert!(
        digest::row_hash_sql(&columns, digest::ARROW_SHA256)
            .unwrap()
            .contains("CONVERT(")
    );
}

#[test]
fn work_reports_bootstrap_payload_separately_from_warm_digest_only() {
    let mut work = ScanWork::default();
    work.digest_blocks = 2;
    work.digest_rows = 10;
    work.captured(
        &json!({"unchanged":true,"c5_capture_work":{"rows":5,"bytes":128}}),
        true,
    )
    .unwrap();
    work.captured(&json!({"c5_capture_work":{"rows":5,"bytes":256}}), false)
        .unwrap();
    let report = work.json();
    assert_eq!(report["bootstrap_blocks"], 1);
    assert_eq!(report["bootstrap_rows"], 5);
    assert_eq!(report["bootstrap_bytes"], 128);
    assert_eq!(report["captured_blocks"], 2);
    assert_eq!(report["captured_bytes"], 384);
    assert_eq!(report["baseline_digest_shortcut"], false);
    assert_eq!(report["full_refresh_fallback"], false);
    assert_eq!(report["payload_mode"], "bounded_bootstrap");
    assert_eq!(ScanWork::default().json()["captured_bytes"], 0);
}

#[test]
fn bounded_capture_queries_preserve_limits_and_embed_hash_without_an_extra_probe() {
    let columns = columns();
    let keys = keys();
    let binding = RunBinding {
        source_id: "fixture".into(),
        schema: "sample".into(),
        table: "items".into(),
        run_generation: 1,
        input_digest: [0; 32],
    };
    let bound = ChunkBound {
        lo: Some(vec![KeyValue::Int(5)]),
        hi: Some(vec![KeyValue::Int(10)]),
    };
    let mut read = chunk::ChunkRead {
        schema: "sample",
        table: "items",
        keys: &keys,
        columns: &columns,
        binding: &binding,
        bound: &bound,
        max_rows: 10,
        max_bytes: 65536,
        deadline: Duration::from_secs(10),
    };
    let (size_sql, rows_sql, params) =
        digest::capture_queries(&read, 5000, digest::SOURCE_MD5).unwrap();
    assert!(rows_sql.contains("MD5(CONCAT_WS("));
    assert!(rows_sql.contains("`__oo_c5_digest`"));
    assert!(rows_sql.ends_with("ORDER BY `id` LIMIT 11"));
    assert!(rows_sql.contains("WHERE ((`id` > ?)) AND ((`id` <= ?))"));
    assert!(rows_sql.contains("MAX_EXECUTION_TIME(5000)"));
    assert!(size_sql.contains("+32) AS __oo_row_bytes"));
    assert_eq!(params, vec![KeyValue::Int(5), KeyValue::Int(10)]);
    for limit in [0, 100_001] {
        read.max_rows = limit;
        assert!(digest::capture_queries(&read, 5000, digest::SOURCE_MD5).is_err());
    }
    read.max_rows = 10;
    for limit in [1, 16_777_216, 16_777_217, 33_554_432] {
        read.max_bytes = limit;
        assert!(digest::capture_queries(&read, 5000, digest::SOURCE_MD5).is_ok());
    }
    read.max_rows = 100_000;
    assert!(digest::capture_queries(&read, 5000, digest::SOURCE_MD5).is_ok());
    read.max_rows = 10;
    for limit in [0, 33_554_433] {
        read.max_bytes = limit;
        assert!(digest::capture_queries(&read, 5000, digest::SOURCE_MD5).is_err());
    }
    read.max_bytes = 65536;
    assert!(digest::capture_queries(&read, 30001, digest::SOURCE_MD5).is_err());
    read.keys = &[];
    assert!(digest::capture_queries(&read, 5000, digest::SOURCE_MD5).is_err());
}

#[test]
fn added_digest_projection_still_passes_readonly_source_audit() {
    let columns = float_columns();
    let hash = digest::row_hash_sql(&columns, digest::ARROW_SHA256).unwrap();
    let sql = format!(
        "SELECT /*+ MAX_EXECUTION_TIME(5000) */ `id`,`f`,`d`,{hash} AS `__oo_c5_digest` FROM `sample`.`items` WHERE 1 ORDER BY `id` LIMIT 11"
    );
    let audit = json!({
        "source":"mysql.general_log","schema":"sample","table":"items",
        "connection_id":7,"enabled_before":true,"enabled_after":true,"chunked":true,
        "privileges":["SELECT"],"records":[
            {"thread_id":7,"command_type":"Connect","argument":"reader"},
            {"thread_id":7,"command_type":"Query","argument":"SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"},
            {"thread_id":7,"command_type":"Query","argument":"START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY"},
            {"thread_id":7,"command_type":"Query","argument":"SHOW GRANTS FOR CURRENT_USER()"},
            {"thread_id":7,"command_type":"Query","argument":sql},
            {"thread_id":7,"command_type":"Query","argument":"COMMIT"},
        ],
    });
    assert!(source_admission::mysql_full_audit_valid(&audit));
    let mut no_commit = audit.clone();
    no_commit["records"].as_array_mut().unwrap().pop();
    assert!(!source_admission::mysql_full_audit_valid(&no_commit));
}

#[test]
fn historical_policy_widens_dates_but_never_normalizes_keys() {
    let policy = digest::type_policy(
        &json!({"type_policy":{"zero_date_to_null":true,"strip_text_nul":true}}),
    )
    .unwrap();
    let mut columns = columns();
    columns.push(
        mysql::map::map_capture_column(
            &MysqlColumn {
                name: "required_date".into(),
                ordinal: 3,
                data_type: "date".into(),
                column_type: "date".into(),
                nullable: false,
                char_len: None,
                precision: None,
                scale: None,
                charset: None,
                column_key: "".into(),
            },
            false,
            true,
            &Default::default(),
        )
        .unwrap(),
    );
    assert!(!columns[2].plan.mapping.nullable);
    digest::apply_type_policy(&mut columns, policy);
    assert!(columns[2].plan.mapping.nullable);
    assert!(!columns[2].not_null);
    let date = RawCell::Date {
        year: 0,
        month: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        micros: 0,
    };
    let (_, note) = mysql::values::decode_mapped(&columns[2], &date).unwrap();
    assert!(digest::normalization_allowed(&columns[2], &note.unwrap()));
    let (_, note) =
        mysql::values::decode_mapped(&columns[1], &RawCell::Bytes(b"a\0b".to_vec())).unwrap();
    assert!(digest::normalization_allowed(&columns[1], &note.unwrap()));
    assert!(!digest::policy_matches(&json!({}), policy));
    assert!(digest::policy_matches(
        &json!({"type_policy":policy}),
        policy
    ));
    let sql = digest::row_hash_sql(&columns, digest::ARROW_SHA256).unwrap();
    assert!(sql.contains("DAYOFMONTH(`required_date`)=0"));
    assert!(sql.contains("REPLACE(CAST((`body`) AS BINARY),0x00"));
    let mut key = self::columns()[1].clone();
    key.key = true;
    key.plan.key = true;
    key.not_null = true;
    key.plan.mapping.nullable = false;
    digest::apply_type_policy(std::slice::from_mut(&mut key), policy);
    assert!(mysql::values::decode_mapped(&key, &RawCell::Bytes(b"a\0b".to_vec())).is_err());
    assert!(
        !digest::row_hash_sql(&[key], digest::ARROW_SHA256)
            .unwrap()
            .contains("REPLACE(")
    );
    assert!(digest::type_policy(&json!({"type_policy":{"strip_text_nul":"true"}})).is_err());
    assert!(digest::type_policy(&json!({"type_policy":{"unreviewed":true}})).is_err());
}

mod owned_codec {
    use super::*;
    use mysql_async::{OptsBuilder, prelude::Queryable};
    use oo_pipeline::native_arrow::DecodedStream;
    use std::io::Write;

    async fn connection(user: &str, password: &str) -> Result<Conn> {
        // Only this explicitly owned loopback fixture uses a test-only plaintext connection.
        let mut conn = Conn::new(
            OptsBuilder::default()
                .ip_or_hostname("127.0.0.1")
                .tcp_port(23308)
                .user(Some(user))
                .pass(Some(password))
                .prefer_socket(false)
                .ssl_opts(None::<mysql_async::SslOpts>),
        )
        .await?;
        conn.query_drop("SET time_zone = '+00:00'").await?;
        conn.query_drop("SET NAMES utf8mb4").await?;
        Ok(conn)
    }

    fn encoded(batch: &mysql::Batch) -> Result<Vec<u8>> {
        let batch = batch.typed.record_batch()?;
        let mut bytes = Vec::new();
        let mut writer = StreamWriter::try_new(&mut bytes, &batch.schema())?;
        writer.write(&batch)?;
        writer.finish()?;
        drop(writer);
        Ok(bytes)
    }

    async fn aggregate(
        conn: &mut Conn,
        table: &str,
        keys: &[KeyColumn],
        columns: &[MappedColumn],
        predicate: &str,
        params: Params,
    ) -> Result<Value> {
        let sql = digest::aggregate_sql(
            "codec_fixture",
            table,
            keys,
            columns,
            predicate,
            5000,
            digest::ARROW_SHA256,
        )?;
        let (n, a, b): (u64, u64, u64) = conn.exec_first(sql, params).await?.ok_or("aggregate")?;
        Ok(json!([n, format!("{a:016x}"), format!("{b:016x}")]))
    }

    async fn mapped(conn: &mut Conn, table: &str) -> Result<(Vec<KeyColumn>, Vec<MappedColumn>)> {
        let keys = chunk::key_columns(conn, "codec_fixture", table).await?;
        let catalog = mysql::catalog_columns(
            conn,
            &mut vec![],
            &mysql::SelectedTable {
                schema: "codec_fixture".into(),
                name: table.into(),
                columns: vec![],
            },
        )
        .await?;
        let columns = catalog
            .iter()
            .map(|c| mysql::map::map_capture_column(c, true, true, &Default::default()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok((keys, columns))
    }

    #[tokio::test]
    #[ignore = "owned run2 chunk-binding path and loopback MySQL fixture required"]
    async fn owned_run2_full_chunk_binding_schema_fingerprint_matches() -> Result<()> {
        let path = std::env::var("OO_C5_SCHEMA_BINDING_FIXTURE")?;
        let historical: Value = serde_json::from_slice(&fs::read(&path)?)?;
        assert_eq!(historical["protocol"], "oo.mysql.chunk-binding/v1");
        assert_eq!(historical["schema"], "delta_fixture");
        let selected = mysql::SelectedTable {
            schema: "delta_fixture".into(),
            name: text(&historical, "table")?.into(),
            columns: vec![],
        };
        let password = std::env::var("OO_C5_CODEC_FIXTURE_PASSWORD")?;
        let mut reader = connection("root", &password).await?;
        let keys = chunk::key_columns(&mut reader, &selected.schema, &selected.name).await?;
        assert_eq!(
            json!(keys),
            historical["keys"],
            "historical key binding is separate and ordered"
        );
        let config = json!({"schema_fingerprint_codec":"mysql-full-catalog/v1","baseline_keys":historical["keys"]});
        let fingerprint =
            capture_schema_fingerprint(&config, &mut reader, &selected, &keys).await?;
        assert_eq!(fingerprint, text(&historical, "schema_sha256")?);
        let legacy = capture_schema_fingerprint(&json!({}), &mut reader, &selected, &keys).await?;
        assert_ne!(
            legacy, fingerprint,
            "legacy layered users are not silently migrated"
        );
        let selected_subset = mysql::SelectedTable {
            columns: vec![keys[0].name.clone()],
            ..selected.clone()
        };
        assert_eq!(
            full_schema_fingerprint(&mut reader, &selected_subset).await?,
            fingerprint,
            "full historical catalog scope survives selected projection"
        );
        println!(
            "C5_RUN2_SCHEMA_MATCH table={} historical_sha256={} binding={path}",
            selected.name, fingerprint
        );
        reader.disconnect().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicit owned MySQL 5.7.44 fixture on 127.0.0.1:23308 only"]
    async fn owned_mysql5744_float_parts_cadence_and_growth_guard() -> Result<()> {
        let password = std::env::var("OO_C5_CODEC_FIXTURE_PASSWORD")?;
        let mut admin = connection("root", &password).await?;
        let version: String = admin
            .query_first("SELECT VERSION()")
            .await?
            .ok_or("version")?;
        assert_eq!(version, "5.7.44");
        admin
            .query_drop("CREATE DATABASE IF NOT EXISTS codec_fixture CHARACTER SET utf8mb4")
            .await?;
        let tag = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let table = format!("float_growth_{tag}");
        let user = format!("fg_{tag}");
        let password = format!("owned-float-growth-{tag}");
        admin
            .query_drop(format!(
                "CREATE USER '{user}'@'%' IDENTIFIED BY '{password}'"
            ))
            .await?;
        admin
            .query_drop(format!("GRANT SELECT ON codec_fixture.* TO '{user}'@'%'"))
            .await?;
        admin.query_drop(format!(
            "CREATE TABLE codec_fixture.`{table}` (id BIGINT PRIMARY KEY,f FLOAT,d DOUBLE) ENGINE=InnoDB"
        )).await?;
        let values = [
            (0.0_f32, 0.0_f64),
            (-0.0, -0.0),
            (f32::from_bits(1), f64::from_bits(1)),
            (-f32::from_bits(1), -f64::from_bits(1)),
            (0.1, 1.2345678901234567),
            (
                f32::from_bits(1.0_f32.to_bits() + 1),
                f64::from_bits(1.0_f64.to_bits() + 1),
            ),
            (
                f32::from_bits(1.0_f32.to_bits() - 1),
                f64::from_bits(1.0_f64.to_bits() - 1),
            ),
            (f32::MAX, f64::MAX),
            (1e20, 1e20),
        ];
        for (index, (f, d)) in values.iter().enumerate() {
            admin
                .exec_drop(
                    format!("INSERT INTO codec_fixture.`{table}` VALUES (?,?,?)"),
                    Params::Positional(vec![
                        Cell::Int(index as i64 + 1),
                        Cell::Float(*f),
                        Cell::Double(*d),
                    ]),
                )
                .await?;
        }
        let mut reader = connection(&user, &password).await?;
        let (keys, columns) = mapped(&mut reader, &table).await?;
        assert_eq!(digest::codec(&columns), digest::ARROW_SHA256);
        let binding = RunBinding {
            source_id: "owned-float-growth".into(),
            schema: "codec_fixture".into(),
            table: table.clone(),
            run_generation: 1,
            input_digest: [0; 32],
        };
        let next_binding = RunBinding {
            run_generation: 2,
            ..binding.clone()
        };
        let out = std::env::temp_dir().join(format!("c5-float-growth-qualification-{tag}"));
        fs::create_dir(&out)?;
        fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
        let secret = out.join("reader-secret");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&secret)?
            .write_all(password.as_bytes())?;
        let mut config = json!({
            "max_rows":3,"max_bytes":1048576,"incremental_only":true,"boolean_tinyint1":true,
            "schema_fingerprint_codec":"mysql-full-catalog/v1","baseline_keys":keys,
            "source_id":binding.source_id,"schema":"codec_fixture","table":table,
            "audit":{"mode":"grant_enforced"},"target":{},"baseline_arrows":{},
            "connection":{"host":"127.0.0.1","port":23308,"user":user,"secret_file":secret,
                "tls":"disabled","plaintext_authorized":true},
        });
        let mut chunks = Vec::new();
        for index in 0..3 {
            let bound = ChunkBound {
                lo: (index > 0).then(|| vec![KeyValue::Int(index * 3)]),
                hi: (index < 2).then(|| vec![KeyValue::Int((index + 1) * 3)]),
            };
            let full_bound = ChunkBound {
                lo: bound.lo.clone(),
                hi: Some(vec![KeyValue::Int((index + 1) * 3)]),
            };
            let (returned, full) = chunk::read_chunk(
                reader,
                chunk::ChunkRead {
                    schema: "codec_fixture",
                    table: &table,
                    keys: &keys,
                    columns: &columns,
                    binding: &binding,
                    bound: &full_bound,
                    max_rows: 3,
                    max_bytes: 1048576,
                    deadline: Duration::from_secs(10),
                },
            )
            .await?;
            reader = returned;
            let bytes = encoded(&full)?;
            let local = oo_pipeline::native_arrow::decode_arrow_stream(&bytes)
                .map_err(|_| "float full Arrow")?;
            for row in 0..3 {
                let (f, d) = values[index as usize * 3 + row];
                assert_eq!(
                    local.columns[1][row],
                    oo_pipeline::native_arrow::NativeCell::Float64(f64::from(f).to_bits())
                );
                assert_eq!(
                    local.columns[2][row],
                    oo_pipeline::native_arrow::NativeCell::Float64(d.to_bits())
                );
            }
            let local_digest = digest::arrow_digest(&local, &columns)?;
            let (returned, captured, _, source_digest) = digest::capture(
                reader,
                chunk::ChunkRead {
                    schema: "codec_fixture",
                    table: &table,
                    keys: &keys,
                    columns: &columns,
                    binding: &next_binding,
                    bound: &bound,
                    max_rows: 3,
                    max_bytes: 1048576,
                    deadline: Duration::from_secs(10),
                },
                5000,
                digest::ARROW_SHA256,
            )
            .await?;
            reader = returned;
            assert_eq!(source_digest, local_digest, "part {index}");
            assert_eq!(
                encoded(&captured)?,
                bytes,
                "generation-only full/c5 Arrow equality"
            );
            let path = out.join(format!("part-{index}.arrow"));
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?
                .write_all(&bytes)?;
            config["baseline_arrows"][index.to_string()] = json!(path);
            chunks.push(json!({
                "chunk_id":index.to_string(),"lo":encode_bound(bound.lo.as_deref()),
                "hi":encode_bound(bound.hi.as_deref()),"membership_revision":1,"row_count":3,
                "arrow_sha256":sha256_hex(&bytes),"digest":null,
            }));
        }
        let fingerprint = full_schema_fingerprint(
            &mut reader,
            &mysql::SelectedTable {
                schema: "codec_fixture".into(),
                name: table.clone(),
                columns: vec![],
            },
        )
        .await?;
        let projection = projection_digest(&columns)?;
        let selected = json!(columns.iter().map(|c| &c.plan.name).collect::<Vec<_>>());
        let authority = json!({
            "source_id":binding.source_id,"schema":"codec_fixture","table":table,"source_generation":1,
            "schema_fingerprint":fingerprint,"chunk_topology_revision":1,"selected_columns":selected,
            "chunks":chunks,"next_chunk_index":0,"scan_id":format!("float-first-{tag}"),
            "expected_chunks":["0","1","2"],"scan_layers":[false,false,true,true],"scan_observed_at_ms":tag as u64,
            "baseline":{"source":binding.source_id,"schema_name":"codec_fixture","table_name":table,
                "source_generation":1,"selected_columns":selected,"projection_sha256":projection,
                "config_digest":"a".repeat(64)},
        });
        let input = json!({
            "protocol":"oo.jobs.native-producer/v1","method":"C5","c5_mode":"layered","source_id":binding.source_id,
            "binding":"00000000-0000-4000-8000-000000000001","pin":1,"acknowledged_source_generation":1,
            "c5_observed_at_ms":tag as u64,"c5_due":[false,false,true,true],"c5_layered":{
                "incremental_only":true,
                "policy":{"duty_per_mille":1000,"threads_running_max":32,"execution_window_utc":[0,1440]},
                "execution_timezone":"UTC","server_timeout_ms":5000,"client_timeout_ms":10000,
                "recent_column":null,"window_ms":60000,"overlap_ms":0},
        });
        let frozen_config = config.clone();
        let cold = produce(
            connection(&user, &password).await?,
            &config,
            &input,
            &authority,
            &out,
        )
        .await?;
        assert_eq!(cold["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(cold["c5_observation"]["work"]["baseline_digest_skips"], 3);
        assert_eq!(cold["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(cold["c5_completed_layers"][3], false);
        let mut confirmed = authority.clone();
        confirmed["chunks"] = cold["c5_observation"]["chunks"].clone();
        // Simulate only the authority exported AFTER target accepts the receipts.
        for entry in confirmed["chunks"].as_array_mut().unwrap() {
            entry["last_digest_scan_id"] = authority["scan_id"].clone();
        }
        confirmed["scan_id"] = json!(format!("float-next-{tag}"));
        let mut recurring = input.clone();
        recurring["c5_due"] = json!([false, false, false, true]);
        confirmed["scan_layers"] = recurring["c5_due"].clone();
        let quiet = produce(
            connection(&user, &password).await?,
            &config,
            &recurring,
            &confirmed,
            &out,
        )
        .await?;
        assert_eq!(quiet["c5_observation"]["work"]["digest_blocks"], 0);
        assert_eq!(quiet["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(quiet["c5_observation"]["chunks"], json!([]));
        assert_eq!(quiet["c5_observation"]["full_digest_scan_completed"], false);
        assert_eq!(
            quiet["c5_completed_layers"],
            json!([false, false, false, false])
        );
        assert_eq!(
            quiet["c5_observation"]["work"]["full_reconcile_deferred"],
            true
        );
        recurring["c5_due"] = json!([false, false, true, true]);
        confirmed["scan_layers"] = recurring["c5_due"].clone();
        let warm = produce(
            connection(&user, &password).await?,
            &config,
            &recurring,
            &confirmed,
            &out,
        )
        .await?;
        assert_eq!(warm["c5_observation"]["work"]["digest_blocks"], 3);
        assert_eq!(warm["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(warm["c5_observation"]["work"]["baseline_digest_blocks"], 0);
        admin
            .exec_drop(
                format!("UPDATE codec_fixture.`{table}` SET d=? WHERE id=5"),
                (f64::from_bits(values[4].1.to_bits() + 1),),
            )
            .await?;
        let changed = produce(
            connection(&user, &password).await?,
            &config,
            &recurring,
            &confirmed,
            &out,
        )
        .await?;
        assert_eq!(changed["protocol"], "oo.pipeline.native-c5-range/v1");
        assert_eq!(changed["chunk_id"], "1");
        assert_eq!(changed["row_count"], 3);
        assert_eq!(changed["c5_observation"]["work"]["captured_blocks"], 1);
        assert_eq!(changed["c5_observation"]["work"]["digest_blocks"], 2);
        let mut after_change = confirmed.clone();
        after_change["chunks"][1] = changed["c5_observation"]["chunk"].clone();
        after_change["chunks"][1]["last_digest_scan_id"] = confirmed["scan_id"].clone();
        admin
            .query_drop(format!(
                "INSERT INTO codec_fixture.`{table}` VALUES (10,0e0,0e0)"
            ))
            .await?;
        let refused = produce(
            connection(&user, &password).await?,
            &config,
            &recurring,
            &after_change,
            &out,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            refused.contains("C5_RANGE_REPARTITION_REQUIRED"),
            "{refused}"
        );
        // Growth after the aggregate is guarded independently by the same-SELECT capture.
        let tail = ChunkBound {
            lo: Some(vec![KeyValue::Int(6)]),
            hi: None,
        };
        let rejected = digest::capture(
            connection(&user, &password).await?,
            chunk::ChunkRead {
                schema: "codec_fixture",
                table: &table,
                keys: &keys,
                columns: &columns,
                binding: &next_binding,
                bound: &tail,
                max_rows: 3,
                max_bytes: 1048576,
                deadline: Duration::from_secs(10),
            },
            5000,
            digest::ARROW_SHA256,
        )
        .await;
        assert!(
            rejected
                .err()
                .ok_or("growth accepted")?
                .to_string()
                .contains("C5_RANGE_REPARTITION_REQUIRED")
        );
        let mut forbidden_override = config.clone();
        forbidden_override["range_max_rows"] = json!(4);
        let refused_override = produce(
            connection(&user, &password).await?,
            &forbidden_override,
            &recurring,
            &after_change,
            &out,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(refused_override.contains("sealed admission budget"));
        assert_eq!(config, frozen_config);
        assert_eq!(
            authority["baseline"]["config_digest"],
            after_change["baseline"]["config_digest"]
        );
        let report = json!({
            "scope":"owned producer fixture only; not Loader/overlay acceptance","mysql_version":version,
            "database":"codec_fixture","table":table,"parts":3,"baseline_rows_per_part":3,
            "source_full_gen1_c5_gen2_digest_equal":true,"float_ulps_subnormals_signed_zero":true,
            "cold_work":cold["c5_observation"]["work"],"warm_work":warm["c5_observation"]["work"],
            "l4_only_work":quiet["c5_observation"]["work"],"l4_only_full_digest_scan_completed":false,
            "changed_middle_work":changed["c5_observation"]["work"],
            "tail_rows_after_growth":4,"growth_error":refused,"capture_growth_fail_closed":true,
            "budget_override_rejected":true,"historical_config_unchanged":true,"full_refresh_fallback":false,
        });
        c5plus::save(&out.join("qualification.json"), &report)?;
        println!(
            "C5_FLOAT_GROWTH_QUALIFICATION={}",
            out.join("qualification.json").display()
        );
        reader.disconnect().await?;
        admin.disconnect().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicit owned MySQL 5.7.44 fixture on 127.0.0.1:23308 only"]
    async fn owned_mysql5744_normalization_and_key_order() -> Result<()> {
        let password = std::env::var("OO_C5_CODEC_FIXTURE_PASSWORD")?;
        let mut admin = connection("root", &password).await?;
        let version: String = admin
            .query_first("SELECT VERSION()")
            .await?
            .ok_or("version")?;
        assert_eq!(version, "5.7.44");
        admin
            .query_drop("CREATE DATABASE IF NOT EXISTS codec_fixture CHARACTER SET utf8mb4")
            .await?;
        admin.query_drop("SET SESSION sql_mode=''").await?;
        let tag = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let table = format!("norm_{tag}");
        let user = format!("norm_{tag}");
        let password = format!("owned-normalization-{tag}");
        admin
            .query_drop(format!(
                "CREATE USER '{user}'@'%' IDENTIFIED BY '{password}'"
            ))
            .await?;
        admin
            .query_drop(format!("GRANT SELECT ON codec_fixture.* TO '{user}'@'%'"))
            .await?;
        admin
            .query_drop(format!(
                "CREATE TABLE codec_fixture.`{table}` (
            a BIGINT NOT NULL,b VARCHAR(16) CHARACTER SET utf8mb4 NOT NULL,
            required_date DATE NOT NULL,required_dt DATETIME(6) NOT NULL,
            required_ts TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
            note VARCHAR(64) CHARACTER SET utf8mb4 NOT NULL,
            gbk_note VARCHAR(64) CHARACTER SET gbk NOT NULL,raw_bytes VARBINARY(16),flag BOOL,
            PRIMARY KEY(b,a)) ENGINE=InnoDB"
            ))
            .await?;
        admin
            .query_drop(format!(
                "INSERT INTO codec_fixture.`{table}` VALUES
            (1,'b','0000-00-00','2026-00-01 00:00:00','0000-00-00 00:00:00',
             CONCAT('private',CHAR(0),'tail'),CONVERT(0xD6D000CEC4 USING gbk),0x410042,1),
            (2,'a','2026-10-25','2026-10-25 12:34:56.123400','2026-10-25 12:34:56.123400',
             'clean','',NULL,0)"
            ))
            .await?;
        let mut reader = connection(&user, &password).await?;
        let (keys, mut columns) = mapped(&mut reader, &table).await?;
        assert_eq!(
            keys.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert!(columns[2..5].iter().all(|c| !c.plan.mapping.nullable));
        let policy = mysql::values::TypePolicy {
            zero_date_to_null: true,
            strip_text_nul: true,
        };
        digest::apply_type_policy(&mut columns, policy);
        assert!(
            columns[2..5]
                .iter()
                .all(|c| c.plan.mapping.nullable && !c.not_null)
        );
        let selected_table = mysql::SelectedTable {
            schema: "codec_fixture".into(),
            name: table.clone(),
            columns: vec![],
        };
        let fingerprint = full_schema_fingerprint(&mut reader, &selected_table).await?;
        let binding = RunBinding {
            source_id: "owned-normalization".into(),
            schema: "codec_fixture".into(),
            table: table.clone(),
            run_generation: 1,
            input_digest: [0; 32],
        };
        let bound = ChunkBound {
            lo: None,
            hi: Some(vec![KeyValue::Text(b"b".to_vec()), KeyValue::Int(1)]),
        };
        let (reader, full_batch) = chunk::read_chunk(
            reader,
            chunk::ChunkRead {
                schema: "codec_fixture",
                table: &table,
                keys: &keys,
                columns: &columns,
                binding: &binding,
                bound: &bound,
                max_rows: 100,
                max_bytes: 1048576,
                deadline: Duration::from_secs(10),
            },
        )
        .await?;
        assert_eq!(full_batch.rejects.len(), 5);
        let bytes = encoded(&full_batch)?;
        let decoded =
            oo_pipeline::native_arrow::decode_arrow_stream(&bytes).map_err(|_| "full Arrow")?;
        let local_digest = digest::arrow_digest(&decoded, &columns)?;
        let c5_binding = RunBinding {
            run_generation: 2,
            ..binding.clone()
        };
        let (mut reader, captured, _, captured_digest) = digest::capture(
            reader,
            chunk::ChunkRead {
                schema: "codec_fixture",
                table: &table,
                keys: &keys,
                columns: &columns,
                binding: &c5_binding,
                bound: &bound,
                max_rows: 100,
                max_bytes: 1048576,
                deadline: Duration::from_secs(10),
            },
            5000,
            digest::ARROW_SHA256,
        )
        .await?;
        assert_eq!(captured.rejects.len(), 5);
        assert_eq!(encoded(&captured)?, bytes);
        assert_eq!(captured_digest, local_digest);
        assert_eq!(
            aggregate(&mut reader, &table, &keys, &columns, "1", Params::Empty).await?,
            local_digest
        );
        assert_eq!(
            full_schema_fingerprint(&mut reader, &selected_table).await?,
            fingerprint
        );
        let out = std::env::temp_dir().join(format!("c5-normalization-qualification-{tag}"));
        fs::create_dir(&out)?;
        fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
        let archive = out.join("0.arrow");
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&archive)?
            .write_all(&bytes)?;
        let secret = out.join("owned-reader.secret");
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&secret)?
            .write_all(password.as_bytes())?;
        let config = json!({"source_id":binding.source_id,"schema":"codec_fixture","table":table,
            "max_rows":100,"max_bytes":1048576,"incremental_only":true,"boolean_tinyint1":true,
            "schema_fingerprint_codec":"mysql-full-catalog/v1","baseline_keys":keys,
            "type_policy":policy,"baseline_arrows":{"0":archive},"audit":{"mode":"grant_enforced"},"target":{},
            "connection":{"host":"127.0.0.1","port":23308,"user":user,"secret_file":secret,"tls":"disabled","plaintext_authorized":true}});
        let selected = json!(columns.iter().map(|c| &c.plan.name).collect::<Vec<_>>());
        let key_names = json!(keys.iter().map(|k| &k.name).collect::<Vec<_>>());
        let entry = json!({"chunk_id":"0","lo":null,"hi":null,"membership_revision":1,"row_count":2,
            "arrow_sha256":sha256_hex(&bytes)});
        let authority = json!({"source_id":binding.source_id,"schema":"codec_fixture","table":table,
            "source_generation":1,"schema_fingerprint":fingerprint,"chunk_topology_revision":1,
            "selected_columns":selected,"chunks":[entry],"next_chunk_index":0,
            "scan_id":format!("norm-{tag}"),"scan_layers":[false,false,true,true],
            "scan_observed_at_ms":tag as u64,"expected_chunks":["0"],
            "baseline":{"source":binding.source_id,"schema_name":"codec_fixture","table_name":table,
                "source_generation":1,"selected_columns":selected,"key_columns":key_names,"projection_sha256":projection_digest(&columns)?}});
        let input = json!({"protocol":"oo.jobs.native-producer/v1","method":"C5","c5_mode":"layered",
            "source_id":binding.source_id,"binding":"00000000-0000-4000-8000-000000000001","pin":1,
            "acknowledged_source_generation":1,"c5_observed_at_ms":tag as u64,"c5_due":[false,false,true,true],
            "c5_layered":{"incremental_only":true,"policy":{"duty_per_mille":1000,"threads_running_max":32,"execution_window_utc":[0,1440]},
                "execution_timezone":"UTC","server_timeout_ms":5000,"client_timeout_ms":10000,
                "recent_column":null,"window_ms":60000,"overlap_ms":0}});
        let cold = produce(
            connection(&user, &password).await?,
            &config,
            &input,
            &authority,
            &out,
        )
        .await?;
        assert_eq!(cold["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(cold["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(cold["key_columns"], key_names);
        assert_eq!(
            cold["c5_observation"]["chunks"][0]["type_policy"],
            json!(policy)
        );
        assert_eq!(
            cold["c5_observation"]["baseline_digest_proofs"][0]["type_policy"],
            json!(policy)
        );
        let mut confirmed = authority.clone();
        confirmed["chunks"] = cold["c5_observation"]["chunks"].clone();
        let mut warm_config = config.clone();
        warm_config
            .as_object_mut()
            .unwrap()
            .remove("baseline_arrows");
        let warm = produce(
            connection(&user, &password).await?,
            &warm_config,
            &input,
            &confirmed,
            &out,
        )
        .await?;
        assert_eq!(warm["c5_observation"]["work"]["captured_blocks"], 0);
        let mut unbound = confirmed.clone();
        unbound["chunks"][0]
            .as_object_mut()
            .unwrap()
            .remove("type_policy");
        let missing = produce(
            connection(&user, &password).await?,
            &warm_config,
            &input,
            &unbound,
            &out,
        )
        .await?;
        assert_eq!(missing["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(missing["c5_observation"]["work"]["digest_blocks"], 1);
        assert_eq!(missing["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(
            missing["c5_observation"]["work"]["baseline_digest_blocks"],
            0
        );
        admin.query_drop(format!("UPDATE codec_fixture.`{table}` SET note=CONCAT('changed',CHAR(0),'value') WHERE a=1")).await?;
        let changed = produce(
            connection(&user, &password).await?,
            &warm_config,
            &input,
            &confirmed,
            &out,
        )
        .await?;
        assert_eq!(changed["protocol"], "oo.pipeline.native-c5-range/v1");
        assert_eq!(changed["key_columns"], key_names);
        assert_eq!(
            changed["members"],
            json!([
                [{"kind":"text","value":"61"},{"kind":"int","value":2}],
                [{"kind":"text","value":"62"},{"kind":"int","value":1}],
            ])
        );
        let facts: Value = serde_json::from_slice(&fs::read(out.join("capturer.json"))?)?;
        assert_eq!(facts["key_columns"], key_names);
        assert_eq!(
            facts["normalization_rejects"],
            changed["normalization_rejects"]
        );
        let notes_bytes = fs::read(text(&changed["normalization_rejects"], "path")?)?;
        assert_eq!(
            sha256_hex(&notes_bytes),
            changed["normalization_rejects"]["sha256"]
        );
        let notes: Vec<Value> = serde_json::from_slice(&notes_bytes)?;
        assert_eq!(notes.len(), 5);
        assert!(notes.iter().all(|r| r["raw_value_recorded"] == false
            && r["column"] != "a"
            && r["column"] != "b"
            && r["column"] != "raw_bytes"));
        assert!(!String::from_utf8(notes_bytes)?.contains("changedvalue"));
        assert_eq!(
            changed["c5_observation"]["work"]["full_reconcile_deferred"],
            true
        );
        // A legacy authority export drops policy after the changed range is confirmed.
        // Its rev2 value digest is current; the retained archive is still rev1.
        let mut rev2 = confirmed.clone();
        rev2["source_generation"] = changed["source_generation"].clone();
        rev2["chunks"] = json!([changed["c5_observation"]["chunk"]]);
        rev2["chunks"][0]
            .as_object_mut()
            .unwrap()
            .remove("type_policy");
        rev2["chunks"][0]["last_digest_scan_id"] = json!("confirmed-normalized-change");
        assert_eq!(rev2["chunks"][0]["membership_revision"], 2);
        assert_ne!(rev2["chunks"][0]["arrow_sha256"], sha256_hex(&bytes));
        assert!(digest::baseline_digest(&config, &rev2["chunks"][0], &columns).is_err());
        let mut quiet = input.clone();
        quiet["acknowledged_source_generation"] = rev2["source_generation"].clone();
        quiet["c5_due"] = json!([false, false, false, false]);
        rev2["scan_layers"] = quiet["c5_due"].clone();
        let after_changed = produce(
            connection(&user, &password).await?,
            &config,
            &quiet,
            &rev2,
            &out,
        )
        .await?;
        assert_eq!(after_changed["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(after_changed["c5_observation"]["work"]["digest_blocks"], 1);
        assert_eq!(
            after_changed["c5_observation"]["work"]["captured_blocks"],
            0
        );
        assert_eq!(after_changed["c5_observation"]["work"]["captured_bytes"], 0);
        assert_eq!(
            after_changed["c5_observation"]["work"]["baseline_digest_blocks"],
            0
        );
        assert_eq!(
            after_changed["c5_observation"]["chunks"][0]["digest"],
            rev2["chunks"][0]["digest"]
        );
        assert_eq!(
            after_changed["c5_observation"]["chunks"][0]["type_policy"],
            json!(policy)
        );
        // A target-confirmed prefix without policy also rehashes instead of refusing.
        rev2["next_chunk_index"] = json!(1);
        let resumed = produce(
            connection(&user, &password).await?,
            &config,
            &quiet,
            &rev2,
            &out,
        )
        .await?;
        assert_eq!(resumed["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(resumed["c5_observation"]["work"]["digest_blocks"], 1);
        assert_eq!(resumed["c5_observation"]["work"]["captured_blocks"], 0);
        // Confirmed policy metadata restores the metadata-only shortcut.
        rev2["next_chunk_index"] = json!(0);
        rev2["chunks"] = after_changed["c5_observation"]["chunks"].clone();
        let policy_bound = produce(
            connection(&user, &password).await?,
            &config,
            &quiet,
            &rev2,
            &out,
        )
        .await?;
        assert_eq!(policy_bound["c5_observation"]["work"]["digest_blocks"], 0);
        assert_eq!(policy_bound["c5_observation"]["work"]["captured_blocks"], 0);
        // Other typed errors remain fatal, even with both normalization flags enabled.
        admin
            .query_drop(format!(
                "UPDATE codec_fixture.`{table}` SET flag=2 WHERE a=1"
            ))
            .await?;
        assert!(
            digest::capture(
                connection(&user, &password).await?,
                chunk::ChunkRead {
                    schema: "codec_fixture",
                    table: &table,
                    keys: &keys,
                    columns: &columns,
                    binding: &c5_binding,
                    bound: &bound,
                    max_rows: 100,
                    max_bytes: 1048576,
                    deadline: Duration::from_secs(10),
                },
                5000,
                digest::ARROW_SHA256
            )
            .await
            .is_err()
        );
        admin
            .query_drop(format!(
                "UPDATE codec_fixture.`{table}` SET flag=1,b=CONCAT('b',CHAR(0)) WHERE a=1"
            ))
            .await?;
        assert!(
            digest::capture(
                connection(&user, &password).await?,
                chunk::ChunkRead {
                    schema: "codec_fixture",
                    table: &table,
                    keys: &keys,
                    columns: &columns,
                    binding: &c5_binding,
                    bound: &ChunkBound { lo: None, hi: None },
                    max_rows: 100,
                    max_bytes: 1048576,
                    deadline: Duration::from_secs(10),
                },
                5000,
                digest::ARROW_SHA256
            )
            .await
            .is_err()
        );
        c5plus::save(
            &out.join("qualification.json"),
            &json!({
                "scope":"owned MySQL 5.7.44 only","type_policy":policy,"source_key_columns":key_names,
                "full_source_and_layered_digest_match":true,"source_not_null_dates_are_mapped_nullable":true,
                "cold_work":cold["c5_observation"]["work"],"warm_work":warm["c5_observation"]["work"],
                "changed_work":changed["c5_observation"]["work"],"normalization_notes":notes.len(),
                "rev2_unknown_policy_old_archive_work":after_changed["c5_observation"]["work"],
                "unknown_policy_prefix_work":resumed["c5_observation"]["work"],
                "confirmed_policy_work":policy_bound["c5_observation"]["work"],
                "other_typed_errors_refused":true,"key_normalization_refused":true,
                "capturer":out.join("capturer.json"),"normalization_rejects":changed["normalization_rejects"],
            }),
        )?;
        println!(
            "C5_NORMALIZATION_QUALIFICATION={}",
            out.join("qualification.json").display()
        );
        reader.disconnect().await?;
        admin.disconnect().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicit owned MySQL 5.7.44 fixture on 127.0.0.1:23308 only"]
    async fn owned_mysql5744_codec_qualification() -> Result<()> {
        let password = std::env::var("OO_C5_CODEC_FIXTURE_PASSWORD")?;
        let mut admin = connection("root", &password).await?;
        let facts: (String, u8) = admin
            .query_first("SELECT VERSION(),@@log_bin")
            .await?
            .ok_or("facts")?;
        assert_eq!(facts, ("5.7.44".into(), 0));
        admin
            .query_drop("CREATE DATABASE IF NOT EXISTS codec_fixture CHARACTER SET utf8mb4")
            .await?;
        let tag = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let table = format!("typed_{tag}");
        let reader_user = format!("codec_{tag}");
        let reader_password = format!("owned-codec-{tag}");
        admin
            .query_drop(format!(
                "CREATE USER '{reader_user}'@'%' IDENTIFIED BY '{reader_password}'"
            ))
            .await?;
        admin
            .query_drop(format!(
                "GRANT SELECT ON codec_fixture.* TO '{reader_user}'@'%'"
            ))
            .await?;
        let cases = [
            ("signed8", "TINYINT", ["-128", "127", "NULL", "0"]),
            ("unsigned8", "TINYINT UNSIGNED", ["255", "0", "NULL", "1"]),
            ("signed16", "SMALLINT", ["-32768", "32767", "NULL", "0"]),
            (
                "unsigned16",
                "SMALLINT UNSIGNED",
                ["65535", "0", "NULL", "1"],
            ),
            (
                "signed24",
                "MEDIUMINT",
                ["-8388608", "8388607", "NULL", "0"],
            ),
            (
                "unsigned24",
                "MEDIUMINT UNSIGNED",
                ["16777215", "0", "NULL", "1"],
            ),
            (
                "signed32",
                "INT",
                ["-2147483648", "2147483647", "NULL", "0"],
            ),
            (
                "unsigned32",
                "INT UNSIGNED",
                ["4294967295", "0", "NULL", "1"],
            ),
            (
                "signed64",
                "BIGINT",
                ["-9223372036854775808", "9223372036854775807", "NULL", "0"],
            ),
            (
                "unsigned64",
                "BIGINT UNSIGNED",
                ["18446744073709551615", "0", "NULL", "9223372036854775808"],
            ),
            ("truth", "BOOL", ["0", "1", "NULL", "0"]),
            (
                "zerofill",
                "INT(10) UNSIGNED ZEROFILL",
                ["1", "4294967295", "NULL", "0"],
            ),
            (
                "decimal38",
                "DECIMAL(38,0)",
                ["CONCAT('-',REPEAT('9',38))", "REPEAT('9',38)", "NULL", "0"],
            ),
            (
                "scaled",
                "DECIMAL(38,8)",
                ["-123.45000000", "123.00000001", "NULL", "-0.00000000"],
            ),
            (
                "decimal65",
                "DECIMAL(65,0)",
                ["CONCAT('-',REPEAT('9',65))", "REPEAT('9',65)", "NULL", "0"],
            ),
            (
                "scaled65",
                "DECIMAL(65,30)",
                [
                    "CONCAT('-',REPEAT('9',35),'.',REPEAT('9',30))",
                    "CONCAT(REPEAT('9',35),'.',REPEAT('9',30))",
                    "NULL",
                    "-0.000000000000000000000000000000",
                ],
            ),
            (
                "utf8_text",
                "VARCHAR(128) CHARACTER SET utf8mb4",
                [
                    "CONVERT(0xE4B8ADE69687F09F9880 USING utf8mb4)",
                    "'trailing  '",
                    "NULL",
                    "''",
                ],
            ),
            (
                "gbk_text",
                "VARCHAR(128) CHARACTER SET gbk",
                ["CONVERT(0xD6D0CEC4 USING gbk)", "'end  '", "NULL", "''"],
            ),
            (
                "ucs2_text",
                "VARCHAR(128) CHARACTER SET ucs2",
                ["CONVERT(0x4E2D6587 USING ucs2)", "'end  '", "NULL", "''"],
            ),
            (
                "char_text",
                "CHAR(12) CHARACTER SET utf8mb4",
                ["'padded  '", "'#'", "NULL", "''"],
            ),
            (
                "binary_value",
                "VARBINARY(32)",
                ["0x00FF2320", "0x0000", "NULL", "''"],
            ),
            (
                "fixed_binary",
                "BINARY(4)",
                ["0xFF23", "0x00", "NULL", "''"],
            ),
            (
                "date_value",
                "DATE",
                ["'1000-01-01'", "'9999-12-31'", "NULL", "'1970-01-01'"],
            ),
            (
                "datetime_value",
                "DATETIME(6)",
                [
                    "'1000-01-01 00:00:00.100000'",
                    "'9999-12-31 23:59:59.999999'",
                    "NULL",
                    "'2026-10-25 12:34:56.123400'",
                ],
            ),
            (
                "timestamp_value",
                "TIMESTAMP(6) NULL DEFAULT NULL",
                [
                    "'1970-01-01 00:00:01.100000'",
                    "'2038-01-19 03:14:07.123400'",
                    "NULL",
                    "'2026-10-25 12:34:56.000000'",
                ],
            ),
        ];
        let definition = cases
            .iter()
            .map(|(name, kind, _)| format!("`{name}` {kind}"))
            .collect::<Vec<_>>()
            .join(",");
        admin.query_drop(format!("CREATE TABLE codec_fixture.`{table}` (id BIGINT PRIMARY KEY,{definition}) ENGINE=InnoDB")).await?;
        for (row, id) in [-2, 4, 7, 11].iter().enumerate() {
            let values = cases
                .iter()
                .map(|(_, _, values)| values[row])
                .collect::<Vec<_>>()
                .join(",");
            admin
                .query_drop(format!(
                    "INSERT INTO codec_fixture.`{table}` VALUES ({id},{values})"
                ))
                .await?;
        }
        let mut reader = connection(&reader_user, &reader_password).await?;
        let (keys, all_columns) = mapped(&mut reader, &table).await?;
        let (columns, unqualified): (Vec<_>, Vec<_>) = all_columns
            .into_iter()
            .partition(|c| digest::codec(std::slice::from_ref(c)) == digest::ARROW_SHA256);
        assert_eq!(unqualified.len(), 2, "Decimal256 must remain unqualified");
        assert_eq!(digest::codec(&columns), digest::ARROW_SHA256);
        let source = aggregate(&mut reader, &table, &keys, &columns, "1", Params::Empty).await?;
        let binding = RunBinding {
            source_id: "owned-codec-fixture".into(),
            schema: "codec_fixture".into(),
            table: table.clone(),
            run_generation: 1,
            input_digest: [0; 32],
        };
        let full_bound = ChunkBound {
            lo: None,
            hi: Some(vec![KeyValue::Int(11)]),
        };
        let (reader, full_batch) = chunk::read_chunk(
            reader,
            chunk::ChunkRead {
                schema: "codec_fixture",
                table: &table,
                keys: &keys,
                columns: &columns,
                binding: &binding,
                bound: &full_bound,
                max_rows: 100,
                max_bytes: 1048576,
                deadline: Duration::from_secs(10),
            },
        )
        .await?;
        let bytes = encoded(&full_batch)?;
        let c5_binding = RunBinding {
            run_generation: 2,
            input_digest: [7; 32],
            ..binding.clone()
        };
        let bound = ChunkBound { lo: None, hi: None };
        let (mut reader, batch, grants, captured) = digest::capture(
            reader,
            chunk::ChunkRead {
                schema: "codec_fixture",
                table: &table,
                keys: &keys,
                columns: &columns,
                binding: &c5_binding,
                bound: &bound,
                max_rows: 100,
                max_bytes: 1_048_576,
                deadline: Duration::from_secs(10),
            },
            5000,
            digest::ARROW_SHA256,
        )
        .await?;
        mysql::full_read_grants(&grants)?;
        let captured_bytes = encoded(&batch)?;
        assert_eq!(full_batch.typed.run_generation, 1);
        assert_eq!(batch.typed.run_generation, 2);
        assert_eq!(
            bytes, captured_bytes,
            "current IPC must not change solely with run binding"
        );
        let full_schema =
            arrow_ipc::reader::StreamReader::try_new(std::io::Cursor::new(&bytes), None)?.schema();
        let mut metadata_bytes = Vec::new();
        let metadata_schema =
            full_schema
                .as_ref()
                .clone()
                .with_metadata(std::collections::HashMap::from([
                    ("run_generation".into(), "2".into()),
                    ("source_id".into(), "another-envelope".into()),
                ]));
        let mut writer = StreamWriter::try_new(&mut metadata_bytes, &metadata_schema)?;
        writer.write(&batch.typed.record_batch()?)?;
        writer.finish()?;
        drop(writer);
        assert_ne!(sha256_hex(&bytes), sha256_hex(&metadata_bytes));
        let decoded =
            oo_pipeline::native_arrow::decode_arrow_stream(&bytes).map_err(|_| "fixture Arrow")?;
        assert_eq!(captured, source);
        let metadata_decoded = oo_pipeline::native_arrow::decode_arrow_stream(&metadata_bytes)
            .map_err(|_| "metadata Arrow")?;
        assert_eq!(digest::arrow_digest(&metadata_decoded, &columns)?, source);
        let mut evidence = Vec::new();
        for (index, column) in columns.iter().enumerate().skip(1) {
            let projection = vec![columns[0].clone(), column.clone()];
            let local = DecodedStream {
                rows: decoded.rows,
                fields: vec![decoded.fields[0].clone(), decoded.fields[index].clone()],
                columns: vec![decoded.columns[0].clone(), decoded.columns[index].clone()],
            };
            let source =
                aggregate(&mut reader, &table, &keys, &projection, "1", Params::Empty).await?;
            assert_eq!(
                digest::arrow_digest(&local, &projection)?,
                source,
                "column {}",
                column.plan.name
            );
            evidence.push(json!({"column":column.plan.name,"source_type":column.plan.mapping.source_type,"digest":source,"match":true}));
        }
        assert_eq!(digest::arrow_digest(&decoded, &columns)?, source);
        for (row, id) in [-2i64, 4, 7, 11].iter().enumerate() {
            let local = DecodedStream {
                rows: 1,
                fields: decoded.fields.clone(),
                columns: decoded
                    .columns
                    .iter()
                    .map(|c| vec![c[row].clone()])
                    .collect(),
            };
            let source = aggregate(
                &mut reader,
                &table,
                &keys,
                &columns,
                "`id` = ?",
                Params::Positional(vec![Cell::Int(*id)]),
            )
            .await?;
            assert_eq!(digest::arrow_digest(&local, &columns)?, source, "row {id}");
        }
        let empty = DecodedStream {
            rows: 0,
            fields: decoded.fields.clone(),
            columns: columns.iter().map(|_| vec![]).collect(),
        };
        assert_eq!(
            digest::arrow_digest(&empty, &columns)?,
            aggregate(
                &mut reader,
                &table,
                &keys,
                &columns,
                "`id` > 11",
                Params::Empty
            )
            .await?
        );

        let out = std::env::temp_dir().join(format!("c5-codec-qualification-{tag}"));
        fs::create_dir(&out)?;
        fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
        let archive = out.join("0.arrow");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&archive)?
            .write_all(&bytes)?;
        let reader_secret = out.join("owned-reader.secret");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&reader_secret)?
            .write_all(reader_password.as_bytes())?;
        let config = json!({"baseline_arrows":{"0":archive},"max_rows":100,"max_bytes":1048576,
            "source_id":binding.source_id,"schema":"codec_fixture","table":table,
            "schema_fingerprint_codec":"mysql-full-catalog/v1","baseline_keys":keys,
            "incremental_only":true,"boolean_tinyint1":true,"audit":{"mode":"grant_enforced"},"target":{},
            "connection":{"host":"127.0.0.1","port":23308,"user":reader_user,"secret_file":reader_secret,
                "tls":"disabled","plaintext_authorized":true}});
        let entry = json!({"chunk_id":"0","lo":null,"hi":null,"membership_revision":1,"row_count":4,"arrow_sha256":sha256_hex(&bytes)});
        assert_eq!(
            digest::baseline_digest(&config, &entry, &columns)?,
            Some(source.clone())
        );
        let fingerprint = full_schema_fingerprint(
            &mut reader,
            &mysql::SelectedTable {
                schema: "codec_fixture".into(),
                name: table.clone(),
                columns: vec![],
            },
        )
        .await?;
        let selected = json!(columns.iter().map(|c| &c.plan.name).collect::<Vec<_>>());
        let authority = json!({"source_id":binding.source_id,"schema":"codec_fixture","table":table,
            "source_generation":1,"chunk_topology_revision":1,"schema_fingerprint":fingerprint,
            "selected_columns":selected,"chunks":[entry],"next_chunk_index":0,
            "scan_id":format!("owned-{tag}"),"expected_chunks":["0"],
            "scan_layers":[false,false,true,false],"scan_observed_at_ms":tag as u64,
            "baseline":{"source":binding.source_id,"schema_name":"codec_fixture","table_name":table,
                "source_generation":1,"selected_columns":selected,"projection_sha256":projection_digest(&columns)?}});
        let proof = digest::baseline_digest_proof(&config, &authority, &entry, &columns)?
            .ok_or("baseline proof")?;
        c5plus::save(&out.join("baseline-proof.json"), &proof)?;
        assert_eq!(proof["digest"], source);
        assert_eq!(proof["archive_sha256"], sha256_hex(&bytes));
        let input = json!({"protocol":"oo.jobs.native-producer/v1","method":"C5","c5_mode":"layered",
            "source_id":binding.source_id,"binding":"00000000-0000-4000-8000-000000000001","pin":1,
            "acknowledged_source_generation":1,"c5_observed_at_ms":tag as u64,
            "c5_due":[false,false,true,false],"c5_layered":{
                "incremental_only":true,
                "policy":{"duty_per_mille":1000,"threads_running_max":32,"execution_window_utc":[0,1440]},
                "execution_timezone":"UTC","server_timeout_ms":5000,"client_timeout_ms":10000,
                "recent_column":null,"window_ms":60000,"overlap_ms":0}});
        let cold = produce(
            connection(&reader_user, &reader_password).await?,
            &config,
            &input,
            &authority,
            &out,
        )
        .await?;
        assert_eq!(cold["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(cold["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(cold["c5_observation"]["work"]["baseline_digest_skips"], 1);
        assert_eq!(cold["c5_observation"]["baseline_digest_proofs"][0], proof);
        assert_eq!(cold["c5_observation"]["scan_id"], authority["scan_id"]);
        assert_eq!(
            cold["c5_observation"]["scan_layers"],
            authority["scan_layers"]
        );
        assert_eq!(
            cold["c5_observation"]["scan_observed_at_ms"],
            authority["scan_observed_at_ms"]
        );
        let mut initial_l4_input = input.clone();
        initial_l4_input["c5_due"] = json!([false, false, true, true]);
        let mut initial_l4_authority = authority.clone();
        initial_l4_authority["scan_id"] = json!(format!("owned-initial-l4-{tag}"));
        initial_l4_authority["scan_layers"] = initial_l4_input["c5_due"].clone();
        let initial_l4 = produce(
            connection(&reader_user, &reader_password).await?,
            &config,
            &initial_l4_input,
            &initial_l4_authority,
            &out,
        )
        .await?;
        assert_eq!(initial_l4["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(initial_l4["c5_completed_layers"][3], false);
        assert_eq!(
            initial_l4["c5_observation"]["work"]["full_reconcile_deferred"],
            true
        );
        assert!(
            initial_l4["c5_observation"]
                .get("baseline_full_read_qualification")
                .is_none()
        );
        let mut confirmed = authority.clone();
        confirmed["chunks"] = cold["c5_observation"]["chunks"].clone();
        let mut warm_config = config.clone();
        warm_config
            .as_object_mut()
            .unwrap()
            .remove("baseline_arrows");
        let warm = produce(
            connection(&reader_user, &reader_password).await?,
            &warm_config,
            &input,
            &confirmed,
            &out,
        )
        .await?;
        assert_eq!(warm["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(warm["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(warm["c5_observation"]["work"]["baseline_digest_bytes"], 0);

        // 0065 may hydrate and seed the digest before this job's first L4-due scan.
        // No retained file is supplied: the confirmed, non-null seed must suffice.
        let mut hydrated = authority.clone();
        let mut seeded_parts = Vec::new();
        for (index, start, count, lo, hi) in [
            (0, 0, 1, None, Some(-2)),
            (1, 1, 2, Some(-2), Some(7)),
            (2, 3, 1, Some(7), None),
        ] {
            let part_batch = full_batch.typed.record_batch()?.slice(start, count);
            let mut part_bytes = Vec::new();
            let mut writer = StreamWriter::try_new(&mut part_bytes, &part_batch.schema())?;
            writer.write(&part_batch)?;
            writer.finish()?;
            drop(writer);
            let part = oo_pipeline::native_arrow::decode_arrow_stream(&part_bytes)
                .map_err(|_| "seeded part Arrow")?;
            seeded_parts.push(json!({
                "chunk_id":index.to_string(),"lo":encode_bound(lo.map(|n|vec![KeyValue::Int(n)]).as_deref()),
                "hi":encode_bound(hi.map(|n|vec![KeyValue::Int(n)]).as_deref()),
                "membership_revision":1,"row_count":count,"arrow_sha256":sha256_hex(&part_bytes),
                "digest":digest::arrow_digest(&part,&columns)?,"digest_codec":digest::ARROW_SHA256,
                "projection_sha256":proof["projection_sha256"],"baseline_membership_pending":false,
            }));
        }
        hydrated["chunks"] = json!(seeded_parts);
        hydrated["expected_chunks"] = json!(["0", "1", "2"]);
        hydrated["scan_id"] = json!(format!("owned-hydrated-l4-{tag}"));
        hydrated["scan_layers"] = json!([false, false, false, true]);
        let mut hydrated_input = input.clone();
        hydrated_input["c5_due"] = hydrated["scan_layers"].clone();
        let hydrated_l4 = produce(
            connection(&reader_user, &reader_password).await?,
            &warm_config,
            &hydrated_input,
            &hydrated,
            &out,
        )
        .await?;
        assert_eq!(hydrated_l4["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(hydrated_l4["c5_observation"]["work"]["digest_blocks"], 3);
        assert_eq!(hydrated_l4["c5_observation"]["work"]["captured_blocks"], 0);
        assert_eq!(hydrated_l4["c5_observation"]["work"]["captured_bytes"], 0);
        assert_eq!(
            hydrated_l4["c5_observation"]["work"]["baseline_digest_bytes"],
            0
        );
        assert_eq!(
            hydrated_l4["c5_observation"]["work"]["full_reconcile_deferred"],
            true
        );
        assert_eq!(
            hydrated_l4["c5_observation"]["full_digest_scan_completed"],
            true
        );
        assert_eq!(
            hydrated_l4["c5_completed_layers"],
            json!([false, false, false, false])
        );
        assert!(
            hydrated_l4["c5_observation"]
                .get("baseline_full_read_qualification")
                .is_none()
        );
        assert_eq!(
            hydrated_l4["c5_observation"]["scan_layers"],
            hydrated["scan_layers"]
        );

        let mut maintenance = warm_config.clone();
        maintenance["incremental_only"] = json!(false);
        let mut maintenance_input = hydrated_input.clone();
        maintenance_input["c5_layered"]["incremental_only"] = json!(false);
        let mut maintenance_authority = confirmed.clone();
        maintenance_authority["scan_layers"] = hydrated_input["c5_due"].clone();
        let full_read = produce(
            connection(&reader_user, &reader_password).await?,
            &maintenance,
            &maintenance_input,
            &maintenance_authority,
            &out,
        )
        .await?;
        assert_eq!(full_read["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(full_read["c5_observation"]["work"]["captured_blocks"], 1);
        assert_eq!(
            full_read["c5_observation"]["work"]["full_reconcile_deferred"],
            false
        );
        assert_eq!(full_read["c5_completed_layers"][3], true);

        let mut unknown = hydrated.clone();
        unknown["chunks"][0]["digest"] = Value::Null;
        let missing_l4 = produce(
            connection(&reader_user, &reader_password).await?,
            &warm_config,
            &hydrated_input,
            &unknown,
            &out,
        )
        .await?;
        assert_eq!(missing_l4["status"], "needs_bootstrap");
        assert_eq!(missing_l4["reason"], "baseline_arrow_required");
        assert_eq!(missing_l4["work"]["full_reconcile_deferred"], true);
        let missing = produce(
            connection(&reader_user, &reader_password).await?,
            &warm_config,
            &input,
            &authority,
            &out,
        )
        .await?;
        assert_eq!(missing["status"], "needs_bootstrap");
        assert_eq!(missing["reason"], "baseline_arrow_required");

        // First-delta changes must not be overwritten with the current aggregate.
        admin
            .query_drop(format!(
                "UPDATE codec_fixture.`{table}` SET utf8_text='after-baseline' WHERE id=4"
            ))
            .await?;
        let changed = aggregate(&mut reader, &table, &keys, &columns, "1", Params::Empty).await?;
        assert_ne!(changed, proof["digest"]);
        let changed_l4 = produce(
            connection(&reader_user, &reader_password).await?,
            &warm_config,
            &hydrated_input,
            &hydrated,
            &out,
        )
        .await?;
        assert_eq!(changed_l4["protocol"], "oo.pipeline.native-c5-range/v1");
        assert_eq!(changed_l4["chunk_id"], "1");
        assert_eq!(changed_l4["row_count"], 2);
        assert_eq!(changed_l4["c5_observation"]["work"]["digest_blocks"], 2);
        assert_eq!(changed_l4["c5_observation"]["work"]["captured_blocks"], 1);
        assert_eq!(changed_l4["c5_observation"]["work"]["bootstrap_blocks"], 0);
        assert_eq!(
            changed_l4["c5_observation"]["work"]["full_reconcile_deferred"],
            true
        );
        assert_eq!(changed_l4["c5_completed_layers"][3], false);
        assert_eq!(
            changed_l4["c5_observation"]["chunk"]["digest"],
            aggregate(
                &mut reader,
                &table,
                &keys,
                &columns,
                "`id` > -2 AND `id` <= 7",
                Params::Empty
            )
            .await?
        );
        assert_eq!(
            digest::baseline_digest(&config, &entry, &columns)?,
            Some(source.clone())
        );
        assert!(needs_capture(
            &confirmed["chunks"][0],
            &changed,
            &projection_digest(&columns)?,
            false,
            digest::ARROW_SHA256
        ));
        admin.query_drop(format!("UPDATE codec_fixture.`{table}` SET utf8_text='between-probe-and-capture' WHERE id=4")).await?;
        let (capturer, changed_batch, _, captured_changed) = digest::capture(
            connection(&reader_user, &reader_password).await?,
            chunk::ChunkRead {
                schema: "codec_fixture",
                table: &table,
                keys: &keys,
                columns: &columns,
                binding: &c5_binding,
                bound: &bound,
                max_rows: 100,
                max_bytes: 1048576,
                deadline: Duration::from_secs(10),
            },
            5000,
            digest::ARROW_SHA256,
        )
        .await?;
        capturer.disconnect().await?;
        let changed_decoded =
            oo_pipeline::native_arrow::decode_arrow_stream(&encoded(&changed_batch)?)
                .map_err(|_| "changed Arrow")?;
        assert_ne!(captured_changed, changed);
        assert_eq!(
            digest::arrow_digest(&changed_decoded, &columns)?,
            captured_changed
        );

        // MySQL does not accept a negative DECIMAL scale; neither may the codec.
        assert!(
            admin
                .query_drop(format!(
                    "CREATE TABLE codec_fixture.negscale_{tag} (v DECIMAL(10,-2))"
                ))
                .await
                .is_err()
        );
        let mut negative_scale = columns.clone();
        negative_scale[1].store = mysql::map::StoreKind::Numeric {
            precision: 10,
            scale: -2,
        };
        assert_eq!(digest::codec(&negative_scale), digest::SOURCE_MD5);

        let invalid = format!("invalid_{tag}");
        admin.query_drop(format!("CREATE TABLE codec_fixture.`{invalid}` (id BIGINT PRIMARY KEY, d DATE, b BOOL) ENGINE=InnoDB")).await?;
        admin.query_drop("SET SESSION sql_mode=''").await?;
        admin
            .query_drop(format!(
                "INSERT INTO codec_fixture.`{invalid}` VALUES (1,'0000-00-00',NULL),(2,NULL,2)"
            ))
            .await?;
        let (invalid_keys, invalid_columns) = mapped(&mut reader, &invalid).await?;
        let null_bytes = super::arrow(
            &invalid_columns,
            &[vec![RawCell::Int(1), RawCell::Null, RawCell::Null]],
        );
        let null_stream = oo_pipeline::native_arrow::decode_arrow_stream(&null_bytes)
            .map_err(|_| "null Arrow")?;
        let invalid_source = aggregate(
            &mut reader,
            &invalid,
            &invalid_keys,
            &invalid_columns,
            "`id`=1",
            Params::Empty,
        )
        .await?;
        assert_ne!(
            digest::arrow_digest(&null_stream, &invalid_columns)?,
            invalid_source
        );
        let invalid_binding = RunBinding {
            table: invalid.clone(),
            ..binding.clone()
        };
        let rejected = digest::capture(
            connection(&reader_user, &reader_password).await?,
            chunk::ChunkRead {
                schema: "codec_fixture",
                table: &invalid,
                keys: &invalid_keys,
                columns: &invalid_columns,
                binding: &invalid_binding,
                bound: &bound,
                max_rows: 100,
                max_bytes: 1048576,
                deadline: Duration::from_secs(10),
            },
            5000,
            digest::ARROW_SHA256,
        )
        .await;
        assert!(
            rejected.is_err(),
            "zero dates / invalid bool must remain rejects"
        );
        let unsupported = format!("unsupported_{tag}");
        admin.query_drop(format!("CREATE TABLE codec_fixture.`{unsupported}` (id BIGINT PRIMARY KEY,f DOUBLE,j JSON,t TIME(6),d DECIMAL(65,0)) ENGINE=InnoDB")).await?;
        admin.query_drop(format!("INSERT INTO codec_fixture.`{unsupported}` VALUES (1,1.5,'{{\"k\":1}}','-12:34:56.100000',REPEAT('9',65))")).await?;
        let _ = mapped(&mut reader, &unsupported).await?;
        let unsupported_fingerprint = full_schema_fingerprint(
            &mut reader,
            &mysql::SelectedTable {
                schema: "codec_fixture".into(),
                name: unsupported.clone(),
                columns: vec![],
            },
        )
        .await?;
        for column in ["j", "t", "d"] {
            let mut config = warm_config.clone();
            config["table"] = json!(unsupported);
            let mut authority = authority.clone();
            authority["table"] = json!(unsupported);
            authority["schema_fingerprint"] = json!(unsupported_fingerprint);
            authority["selected_columns"] = json!(["id", column]);
            let refused = produce(
                connection(&reader_user, &reader_password).await?,
                &config,
                &input,
                &authority,
                &out,
            )
            .await?;
            assert_eq!(refused["status"], "needs_bootstrap", "{column}");
            assert_eq!(refused["reason"], "unsupported_columns");
            assert_eq!(refused["apply"], false);
            assert_eq!(refused["ack"], false);
        }
        let report = json!({"scope":"owned fixture only","mysql_version":facts.0,"host":"127.0.0.1","port":23308,
            "database":"codec_fixture","table":table,"codec":digest::ARROW_SHA256,
            "columns":evidence,"unqualified_columns":unqualified.iter().map(|c|json!({
                "column":c.plan.name,"source_type":c.plan.mapping.source_type,"reason":"decimal256_capture_unqualified"
            })).collect::<Vec<_>>(),"rows":4,"row_level_matches":4,"empty_range_match":true,
            "source_capture_arrow_match":true,"baseline_changed_source_detected":true,
            "full_baseline_generation":full_batch.typed.run_generation,"c5_capture_generation":batch.typed.run_generation,
            "generation_only_arrow_sha_equal":sha256_hex(&bytes)==sha256_hex(&captured_bytes),
            "full_schema_metadata":full_schema.metadata().iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),
            "metadata_only_sha_different_digest_equal":true,
            "cold_work":cold["c5_observation"]["work"],"warm_work":warm["c5_observation"]["work"],
            "initial_l4_work":initial_l4["c5_observation"]["work"],
            "initial_l4_did_not_fabricate_completion":initial_l4["c5_completed_layers"][3]==false,
            "hydrated_l4_work":hydrated_l4["c5_observation"]["work"],
            "hydrated_l4_completed_layers":hydrated_l4["c5_completed_layers"],
            "changed_l4_work":changed_l4["c5_observation"]["work"],
            "maintenance_l4_work":full_read["c5_observation"]["work"],
            "maintenance_l4_completed":full_read["c5_completed_layers"][3],
            "unknown_l4_needs_bootstrap":missing_l4["status"]=="needs_bootstrap",
            "missing_archive_needs_bootstrap":true,"unsupported_types_need_bootstrap":true,
            "capture_digest_not_stale_probe":true,
            "zero_date_distinct_from_null":true,"invalid_capture_rejected":true,
            "negative_decimal_scale_rejected":true,"baseline_proof":out.join("baseline-proof.json")});
        c5plus::save(&out.join("qualification.json"), &report)?;
        println!(
            "C5_CODEC_QUALIFICATION={}",
            out.join("qualification.json").display()
        );
        reader.disconnect().await?;
        admin.disconnect().await?;
        Ok(())
    }
}
