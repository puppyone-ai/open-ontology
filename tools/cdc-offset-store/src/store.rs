//! Read one target receipt, record it, then compare-and-swap the watermark.
//!
//! A lost control COMMIT reply is looked up before a second attempt. The
//! lookup failure does not retry. The target connection never writes.

use std::env;
use std::fs;
use std::time::Duration;

use tokio_postgres::types::ToSql;
use tokio_postgres::Client;
use tokio_postgres::Transaction;

use crate::model::{
    validate_install, validate_mirror, InstallOutput, InstallRequest, Output, Request, CONTRACT,
};
use crate::rules::{
    checkpoint, migrations_ok, receipt_closed, receipt_hash, same_batch, Checkpoint, Receipt,
    Watermark,
};
use crate::sha256::{hex, sha256};
use crate::sql;
use crate::tls;

pub fn migration_sha256() -> Option<String> {
    sha256(sql::MIGRATION_SQL.as_bytes()).map(|bytes| hex(&bytes))
}

pub async fn install(request: &InstallRequest) -> InstallOutput {
    if let Err(fault) = validate_install(request) {
        return InstallOutput::refused(fault);
    }
    let Some(hash) = migration_sha256() else {
        return InstallOutput::refused("migration");
    };
    let (password, ca) = match secrets(&request.control.password_file, &request.control.ca_file) {
        Ok(pair) => pair,
        Err(fault) => return InstallOutput::refused(fault),
    };
    let mut client = match tls::connect(&request.control, &password, &ca, false).await {
        Ok(client) => client,
        Err(()) => return InstallOutput::refused("connect"),
    };
    match install_tx(&mut client, &hash).await {
        Ok(kind) => InstallOutput {
            contract: CONTRACT,
            kind: kind.to_owned(),
            fault: None,
            sql_sha256: Some(hash),
            target_touched: false,
        },
        Err(fault) => InstallOutput::refused(fault),
    }
}

pub async fn mirror(request: &Request) -> Output {
    if let Err(fault) = validate_mirror(request) {
        return Output::refused(fault);
    }
    let target_secret = match secrets(&request.target.password_file, &request.target.ca_file) {
        Ok(pair) => pair,
        Err(fault) => return Output::refused(fault),
    };
    let control_secret = match secrets(&request.control.password_file, &request.control.ca_file) {
        Ok(pair) => pair,
        Err(fault) => return Output::refused(fault),
    };
    let mut target = match tls::connect(&request.target, &target_secret.0, &target_secret.1, false).await {
        Ok(client) => client,
        Err(()) => return Output::refused("connect"),
    };
    let read = match read_target(&mut target, request).await {
        Ok(read) => read,
        Err(fault) => return Output::refused(fault),
    };
    drop(target);
    let Some(hash) = receipt_hash(&read.receipt) else {
        return Output::refused("receipt_rule");
    };
    if !receipt_closed(&read.receipt) {
        return Output::refused("receipt_rule");
    }
    let hide = env::var("CDC_OFFSET_INJECT").ok().as_deref() == Some("hide_commit");
    let mut control = match tls::connect(&request.control, &control_secret.0, &control_secret.1, hide).await {
        Ok(client) => client,
        Err(()) => return Output::refused("connect"),
    };
    match write_once(&mut control, request, &read, &hash, true).await {
        Ok(output) => output,
        Err(()) => {
            drop(control);
            recover(request, &read, &hash, &control_secret.0, &control_secret.1).await
        }
    }
}

struct Read {
    receipt: Receipt,
    siblings: Vec<Receipt>,
}

async fn read_target(client: &mut Client, request: &Request) -> Result<Read, &'static str> {
    client
        .batch_execute("SET default_transaction_read_only = on; SET search_path = pg_catalog")
        .await
        .map_err(|_| "target_read")?;
    let tx = client.transaction().await.map_err(|_| "target_read")?;
    tx.batch_execute("SET TRANSACTION READ ONLY")
        .await
        .map_err(|_| "target_read")?;
    let migrations = tx.query(sql::READ_MIGRATIONS, &[]).await.map_err(|err| {
        if sqlstate(&err).as_deref() == Some("42501") {
            "target_privilege"
        } else {
            "migration_drift"
        }
    })?;
    let rows: Vec<(String, String, String, String)> = migrations
        .iter()
        .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
        .collect();
    if !migrations_ok(&rows) {
        return Err("migration_drift");
    }
    let tables = &request.order_binding.declared_transaction.required_table_generations;
    let found = tx
        .query(
            sql::READ_RECEIPTS,
            &[&request.source, &request.txn_or_batch_id, tables],
        )
        .await
        .map_err(|err| {
            if sqlstate(&err).as_deref() == Some("42501") {
                "target_privilege"
            } else {
                "target_read"
            }
        })?;
    let mut siblings = Vec::new();
    for row in &found {
        siblings.push(receipt_from_row(request, row)?);
    }
    let receipt = siblings
        .iter()
        .find(|row| row.table_generation == request.table_generation)
        .cloned()
        .ok_or("target_receipt_absent")?;
    tx.commit().await.map_err(|_| "target_read")?;
    Ok(Read { receipt, siblings })
}

fn receipt_from_row(request: &Request, row: &tokio_postgres::Row) -> Result<Receipt, &'static str> {
    let applied_pos = normalize_pos(&row.get::<_, String>(12)).ok_or("receipt_rule")?;
    let bitmap = decode_hex(&row.get::<_, String>(18)).ok_or("receipt_rule")?;
    let stamp = row.get::<_, String>(21);
    Ok(Receipt {
        source: request.source.clone(),
        table_generation: row.get(0),
        txn_or_batch_id: request.txn_or_batch_id.clone(),
        config_version: row.get(1),
        config_digest: row.get(2),
        type_policy_version: row.get(3),
        type_digest: row.get(4),
        hook_digest: row.get(5),
        schema_digest: row.get(6),
        payload_digest: row.get(7),
        protocol_version: row.get(8),
        position_codec: row.get(9),
        position_lineage: row.get(10),
        source_epoch: row.get(11),
        applied_pos,
        partial_txn: row.get(13),
        cross_table_consistent: row.get(14),
        txn_complete: row.get(15),
        segment_index: row.get(16),
        segment_count: row.get(17),
        segment_bitmap: bitmap,
        fence_shard: row.get(19),
        fence_generation: row.get(20),
        target_commit_at: stamp,
    })
}

async fn write_once(
    client: &mut Client,
    request: &Request,
    read: &Read,
    hash: &str,
    park: bool,
) -> Result<Output, ()> {
    let tx = client.transaction().await.map_err(|_| ())?;
    tx.batch_execute(sql::SESSION).await.map_err(|_| ())?;
    let existing = match tx
        .query_opt(
            sql::SELECT_MIRROR,
            &[&request.source, &request.table_generation, &request.txn_or_batch_id],
        )
        .await
    {
        Ok(row) => row,
        Err(err) => {
            let _ = tx.rollback().await;
            return Ok(privilege_or(err, "control_write"));
        }
    };
    let inserted = if let Some(row) = existing {
        let stored: String = row.get(0);
        if !same_batch(&stored, hash) {
            let _ = tx.rollback().await;
            return Ok(Output::refused("receipt_hash_conflict"));
        }
        false
    } else {
        match insert_receipt(&tx, &read.receipt, hash).await {
            Ok(_) => true,
            Err(err) => {
                let _ = tx.rollback().await;
                if sqlstate(&err).as_deref() == Some("23505") {
                    return Ok(Output::refused("receipt_hash_conflict"));
                }
                return Ok(privilege_or(err, "control_write"));
            }
        }
    };
    let watermark = match load_watermark(&tx, &read.receipt).await {
        Ok(mark) => mark,
        Err(err) => {
            let _ = tx.rollback().await;
            return Ok(privilege_or(err, "control_write"));
        }
    };
    let decision = match checkpoint(&read.receipt, &read.siblings, watermark.as_ref(), &request.order_binding) {
        Ok(decision) => decision,
        Err(fault) => {
            let _ = tx.rollback().await;
            return Ok(Output::refused(fault));
        }
    };
    let mut advanced = false;
    let mut hold = None;
    match decision {
        Checkpoint::Skip(reason) => {
            if reason != "already_checkpoint" {
                hold = Some(reason);
            }
        }
        Checkpoint::InsertFirst => {
            if let Err(err) = insert_watermark(&tx, request, &read.receipt).await {
                let _ = tx.rollback().await;
                return Ok(privilege_or(err, "control_write"));
            }
            advanced = true;
        }
        Checkpoint::CompareAndSwap => {
            let mark = watermark.as_ref().ok_or(())?;
            match cas_watermark(&tx, request, &read.receipt, mark).await {
                Ok(1) => advanced = true,
                Ok(0) => {
                    let now = load_watermark(&tx, &read.receipt).await.map_err(|_| ())?;
                    let same = now.as_ref().is_some_and(|row| {
                        row.applied_txn_id == read.receipt.txn_or_batch_id
                            && row.applied_pos == read.receipt.applied_pos
                    });
                    if !same {
                        let _ = tx.rollback().await;
                        return Ok(Output::refused("watermark_cas_conflict"));
                    }
                }
                Ok(_) => {
                    let _ = tx.rollback().await;
                    return Ok(Output::refused("watermark_cas_conflict"));
                }
                Err(err) => {
                    let _ = tx.rollback().await;
                    return Ok(privilege_or(err, "control_write"));
                }
            }
        }
    }
    if park && env::var("CDC_OFFSET_CHECKPOINT").ok().as_deref() == Some("before_commit") {
        let pid: i32 = tx.query_one("SELECT pg_catalog.pg_backend_pid()", &[]).await.map_err(|_| ())?.get(0);
        let path = env::var("CDC_OFFSET_MARK").map_err(|_| ())?;
        fs::write(path, pid.to_string()).map_err(|_| ())?;
        loop {
            if tx.query_one("SELECT 1", &[]).await.is_err() {
                return Err(());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    let stored = load_watermark(&tx, &read.receipt).await.map_err(|_| ())?;
    if tx.commit().await.is_err() {
        return Err(());
    }
    Ok(success(inserted, advanced, hold, hash, stored.as_ref()))
}

fn success(
    inserted: bool,
    advanced: bool,
    hold: Option<&str>,
    hash: &str,
    watermark: Option<&Watermark>,
) -> Output {
    let mut out = Output::refused("unused");
    out.kind = if inserted { "mirrored" } else { "replay" }.to_owned();
    out.fault = None;
    out.durable_control_mirror = true;
    out.checkpoint_advanced = advanced;
    out.watermark_hold = hold.map(str::to_owned);
    out.receipt_hash = Some(hash.to_owned());
    if let Some(mark) = watermark {
        out.watermark_applied_pos = Some(mark.applied_pos.clone());
        out.watermark_txn_or_batch_id = Some(mark.applied_txn_id.clone());
        out.watermark_source_epoch = Some(mark.source_epoch);
    }
    out
}

async fn recover(
    request: &Request,
    read: &Read,
    hash: &str,
    password: &str,
    ca: &[u8],
) -> Output {
    if env::var("CDC_OFFSET_RECONCILE").ok().as_deref() == Some("down") {
        return Output::unknown();
    }
    match lookup(request, read, hash, password, ca).await {
        Ok(Some(output)) => output,
        Ok(None) => {
            let mut client = match tls::connect(&request.control, password, ca, false).await {
                Ok(client) => client,
                Err(()) => return Output::unknown(),
            };
            match write_once(&mut client, request, read, hash, false).await {
                Ok(output) => output,
                Err(()) => match lookup(request, read, hash, password, ca).await {
                    Ok(Some(output)) => output,
                    _ => Output::unknown(),
                },
            }
        }
        Err(()) => Output::unknown(),
    }
}

async fn lookup(
    request: &Request,
    read: &Read,
    hash: &str,
    password: &str,
    ca: &[u8],
) -> Result<Option<Output>, ()> {
    let client = tls::connect(&request.control, password, ca, false).await?;
    let row = client
        .query_opt(
            "SELECT receipt_hash FROM oo_cdc_offset.mirrored_receipt \
             WHERE source = $1 AND table_generation = $2 AND txn_or_batch_id = $3",
            &[&request.source, &request.table_generation, &request.txn_or_batch_id],
        )
        .await
        .map_err(|_| ())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let stored: String = row.get(0);
    if !same_batch(&stored, hash) {
        return Ok(Some(Output::refused("receipt_hash_conflict")));
    }
    let watermark = load_watermark_client(&client, &read.receipt).await.map_err(|_| ())?;
    let advanced = watermark.as_ref().is_some_and(|mark| {
        mark.applied_txn_id == read.receipt.txn_or_batch_id && mark.applied_pos == read.receipt.applied_pos
    });
    Ok(Some(success(false, advanced, None, hash, watermark.as_ref())))
}

async fn insert_receipt(tx: &Transaction<'_>, receipt: &Receipt, hash: &str) -> Result<u64, tokio_postgres::Error> {
    let epoch = receipt.source_epoch;
    let partial = receipt.partial_txn;
    let cross = receipt.cross_table_consistent;
    let complete = receipt.txn_complete;
    let segment_index = receipt.segment_index;
    let segment_count = receipt.segment_count;
    let fence = receipt.fence_generation;
    let params: [&(dyn ToSql + Sync); 25] = [
        &receipt.source,
        &receipt.table_generation,
        &receipt.txn_or_batch_id,
        &receipt.config_version,
        &receipt.config_digest,
        &receipt.type_policy_version,
        &receipt.type_digest,
        &receipt.hook_digest,
        &receipt.schema_digest,
        &receipt.payload_digest,
        &receipt.protocol_version,
        &receipt.position_codec,
        &receipt.position_lineage,
        &epoch,
        &receipt.applied_pos,
        &partial,
        &cross,
        &complete,
        &segment_index,
        &segment_count,
        &receipt.segment_bitmap,
        &receipt.fence_shard,
        &fence,
        &receipt.target_commit_at,
        &hash,
    ];
    tx.execute(sql::INSERT_RECEIPT, &params).await
}

async fn insert_watermark(
    tx: &Transaction<'_>,
    request: &Request,
    receipt: &Receipt,
) -> Result<u64, tokio_postgres::Error> {
    let epoch = receipt.source_epoch;
    tx.execute(
        sql::INSERT_WATERMARK,
        &[
            &receipt.source,
            &receipt.position_codec,
            &receipt.position_lineage,
            &epoch,
            &receipt.applied_pos,
            &receipt.txn_or_batch_id,
            &request.order_binding.declared_next.txn_or_batch_id,
            &request.order_binding.declared_next.applied_pos,
        ],
    )
    .await
}

async fn cas_watermark(
    tx: &Transaction<'_>,
    request: &Request,
    receipt: &Receipt,
    mark: &Watermark,
) -> Result<u64, tokio_postgres::Error> {
    let epoch = receipt.source_epoch;
    tx.execute(
        sql::CAS_WATERMARK,
        &[
            &receipt.source,
            &receipt.position_codec,
            &receipt.position_lineage,
            &epoch,
            &receipt.applied_pos,
            &receipt.txn_or_batch_id,
            &request.order_binding.declared_next.txn_or_batch_id,
            &request.order_binding.declared_next.applied_pos,
            &mark.applied_pos,
            &mark.applied_txn_id,
        ],
    )
    .await
}

async fn load_watermark(tx: &Transaction<'_>, receipt: &Receipt) -> Result<Option<Watermark>, tokio_postgres::Error> {
    let epoch = receipt.source_epoch;
    let row = tx
        .query_opt(
            sql::SELECT_WATERMARK,
            &[
                &receipt.source,
                &receipt.position_codec,
                &receipt.position_lineage,
                &epoch,
            ],
        )
        .await?;
    Ok(row.map(|row| watermark_from_row(&row)))
}

async fn load_watermark_client(client: &Client, receipt: &Receipt) -> Result<Option<Watermark>, tokio_postgres::Error> {
    let epoch = receipt.source_epoch;
    let row = client
        .query_opt(
            "SELECT position_codec, position_lineage, source_epoch, applied_pos::text, applied_txn_id, \
             declared_next_txn, declared_next_pos::text \
             FROM oo_cdc_offset.watermark \
             WHERE source = $1 AND position_codec = $2 AND position_lineage = $3 AND source_epoch = $4",
            &[
                &receipt.source,
                &receipt.position_codec,
                &receipt.position_lineage,
                &epoch,
            ],
        )
        .await?;
    Ok(row.map(|row| watermark_from_row(&row)))
}

fn watermark_from_row(row: &tokio_postgres::Row) -> Watermark {
    Watermark {
        position_codec: row.get(0),
        position_lineage: row.get(1),
        source_epoch: row.get(2),
        applied_pos: normalize_pos(&row.get::<_, String>(3)).unwrap_or_else(|| row.get(3)),
        applied_txn_id: row.get(4),
        declared_next_txn: row.get(5),
        declared_next_pos: normalize_pos(&row.get::<_, String>(6)).unwrap_or_else(|| row.get(6)),
    }
}

fn privilege_or(err: tokio_postgres::Error, other: &str) -> Output {
    if sqlstate(&err).as_deref() == Some("42501") {
        Output::refused("control_privilege")
    } else {
        Output::refused(other)
    }
}

async fn install_tx(client: &mut Client, hash: &str) -> Result<&'static str, &'static str> {
    if !principal_ok(client).await? {
        return Err("principal");
    }
    let tx = client.transaction().await.map_err(|_| "control_write")?;
    tx.batch_execute(sql::SESSION).await.map_err(|_| "control_write")?;
    let present: bool = tx
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = 'oo_cdc_offset')",
            &[],
        )
        .await
        .map_err(|_| "control_write")?
        .get(0);
    if !present {
        tx.batch_execute(sql::MIGRATION_SQL).await.map_err(|_| "control_write")?;
        tx.execute(
            "INSERT INTO oo_cdc_offset.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)",
            &[&hash],
        )
        .await
        .map_err(|_| "control_write")?;
        catalog_ok(&tx, hash).await?;
        tx.commit().await.map_err(|_| "control_write")?;
        return Ok("installed");
    }
    catalog_ok(&tx, hash).await?;
    tx.commit().await.map_err(|_| "control_write")?;
    Ok("replay")
}

async fn principal_ok(client: &Client) -> Result<bool, &'static str> {
    let row = client
        .query_one(
            "SELECT current_user, \
                    (SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user), \
                    (SELECT rolbypassrls FROM pg_catalog.pg_roles WHERE rolname = current_user), \
                    (SELECT rolcreatedb FROM pg_catalog.pg_roles WHERE rolname = current_user), \
                    (SELECT rolcreaterole FROM pg_catalog.pg_roles WHERE rolname = current_user), \
                    (SELECT rolcanlogin FROM pg_catalog.pg_roles WHERE rolname = 'oo_cdc_offset_runtime')",
            &[],
        )
        .await
        .map_err(|_| "principal")?;
    let current: String = row.get(0);
    let superuser: bool = row.get(1);
    let bypass: bool = row.get(2);
    let createdb: bool = row.get(3);
    let createrole: bool = row.get(4);
    let runtime_login: bool = row.get(5);
    Ok(current == "oo_cdc_offset_admin" && !superuser && !bypass && !createdb && !createrole && !runtime_login)
}

async fn catalog_ok(tx: &Transaction<'_>, hash: &str) -> Result<(), &'static str> {
    let owner: String = tx
        .query_one(
            "SELECT pg_catalog.pg_get_userbyid(nspowner) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_cdc_offset'",
            &[],
        )
        .await
        .map_err(|_| "install_drift")?
        .get(0);
    if owner != "oo_cdc_offset_admin" {
        return Err("install_drift");
    }
    let stored: String = tx
        .query_one(
            "SELECT sql_sha256 FROM oo_cdc_offset.install_manifest WHERE singleton",
            &[],
        )
        .await
        .map_err(|_| "install_drift")?
        .get(0);
    if stored != hash {
        return Err("install_drift");
    }
    let tables = tx
        .query(
            "SELECT c.relname::text \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_cdc_offset' AND c.relkind = 'r' \
             ORDER BY c.relname",
            &[],
        )
        .await
        .map_err(|_| "install_drift")?;
    let names: Vec<String> = tables.iter().map(|row| row.get(0)).collect();
    if names != ["install_manifest", "mirrored_receipt", "watermark"] {
        return Err("install_drift");
    }
    let triggers = tx
        .query(
            "SELECT t.tgname::text \
             FROM pg_catalog.pg_trigger AS t \
             JOIN pg_catalog.pg_class AS c ON c.oid = t.tgrelid \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_cdc_offset' AND NOT t.tgisinternal \
             ORDER BY t.tgname",
            &[],
        )
        .await
        .map_err(|_| "install_drift")?;
    let trigger_names: Vec<String> = triggers.iter().map(|row| row.get(0)).collect();
    if trigger_names
        != [
            "mirrored_receipt_reject_truncate",
            "mirrored_receipt_reject_write",
            "watermark_reject_truncate",
            "watermark_reject_write",
        ]
    {
        return Err("install_drift");
    }
    let grants = tx
        .query(
            "SELECT c.relname::text, acl.privilege_type \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             CROSS JOIN LATERAL pg_catalog.aclexplode(c.relacl) AS acl \
             JOIN pg_catalog.pg_roles AS r ON r.oid = acl.grantee \
             WHERE n.nspname = 'oo_cdc_offset' \
               AND r.rolname = 'oo_cdc_offset_runtime' \
               AND c.relkind = 'r' \
             ORDER BY 1, 2",
            &[],
        )
        .await
        .map_err(|_| "install_drift")?;
    let grant_names: Vec<(String, String)> = grants.iter().map(|row| (row.get(0), row.get(1))).collect();
    let required = [
        ("install_manifest", "SELECT"),
        ("mirrored_receipt", "INSERT"),
        ("mirrored_receipt", "SELECT"),
        ("watermark", "INSERT"),
        ("watermark", "SELECT"),
    ];
    for want in required {
        if !grant_names.iter().any(|item| item.0 == want.0 && item.1 == want.1) {
            return Err("install_drift");
        }
    }
    for item in &grant_names {
        let column_update = item.0 == "watermark" && item.1 == "UPDATE";
        let known = required.iter().any(|want| item.0 == want.0 && item.1 == want.1);
        if !known && !column_update {
            return Err("install_drift");
        }
    }
    let columns = tx
        .query(
            "SELECT a.attname::text \
             FROM pg_catalog.pg_attribute AS a \
             JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             CROSS JOIN LATERAL pg_catalog.aclexplode(a.attacl) AS acl \
             JOIN pg_catalog.pg_roles AS r ON r.oid = acl.grantee \
             WHERE n.nspname = 'oo_cdc_offset' \
               AND c.relname = 'watermark' \
               AND r.rolname = 'oo_cdc_offset_runtime' \
               AND acl.privilege_type = 'UPDATE' \
               AND a.attnum > 0 \
               AND NOT a.attisdropped \
             ORDER BY a.attname",
            &[],
        )
        .await
        .map_err(|_| "install_drift")?;
    let column_names: Vec<String> = columns.iter().map(|row| row.get(0)).collect();
    if column_names
        != [
            "applied_pos",
            "applied_txn_id",
            "declared_next_pos",
            "declared_next_txn",
        ]
    {
        return Err("install_drift");
    }
    Ok(())
}

fn secrets(password_file: &str, ca_file: &str) -> Result<(String, Vec<u8>), &'static str> {
    let password = fs::read_to_string(password_file).map_err(|_| "endpoint")?;
    let password = password.trim().to_owned();
    if password.is_empty() {
        return Err("endpoint");
    }
    let ca = fs::read(ca_file).map_err(|_| "endpoint")?;
    if ca.is_empty() {
        return Err("endpoint");
    }
    Ok((password, ca))
}

fn normalize_pos(value: &str) -> Option<String> {
    let value = value.trim();
    let whole = match value.split_once('.') {
        Some((whole, fraction)) if fraction.chars().all(|ch| ch == '0') => whole,
        Some(_) => return None,
        None => value,
    };
    if crate::rules::pos_ok(whole) {
        Some(whole.to_owned())
    } else {
        None
    }
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let high = hex_nibble(bytes[index])?;
        let low = hex_nibble(bytes[index + 1])?;
        out.push((high << 4) | low);
        index += 2;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn sqlstate(err: &tokio_postgres::Error) -> Option<String> {
    err.code().map(|code| code.code().to_owned())
}
