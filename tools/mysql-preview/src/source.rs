//! One read-only MySQL business connection. No SET and no DML.

use arrow_ipc::writer::StreamWriter;
use std::time::{Duration, Instant};
use std::{fs::File, path::Path};


use mysql_async::Conn;
use oo_connectors::PG17_RESERVED;
use oo_connectors::types::{RunBinding, TypedBatch};
use oo_pipeline::catalog_import::{push_schema_column, push_schema_table};
use oo_pipeline::copy_encoder::{
    CopyBudgets, CopyColumn, CopyEncoder, CopyPiece, ExactDecimal, MapDecision, UnavailablePolicy,
    arrow_dimensions,
};
use oo_pipeline::copy_sink::hash_batch;
use oo_pipeline::full_import::{QualityReject, hook_digest, quality_preimage};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value as JsonValue, json};


use crate::config::{PreviewConfig, SelectedTable};
use crate::error::PreviewError;
use crate::map::{
    MappedColumn, MysqlColumn, map_table, session_utc,
};


pub use oo_connectors::mysql::StatementNote;
use oo_connectors::mysql::{query_drop, current_user_is, keep_requested};
#[derive(Clone, Debug)]
pub struct TableBatch {
    pub index: u32,
    pub schema: String,
    pub name: String,
    pub logical_schema: String,
    pub logical_table: String,
    pub columns: Vec<MappedColumn>,
    pub batches: Vec<EncodedBatch>,
    pub rejects: Vec<JsonValue>,
}

#[derive(Clone, Debug)]
pub struct EncodedBatch {
    pub batch_id: String,
    pub rows: u64,
    pub payload_digest: String,
    pub applied_pos: String,
    pub row_hashes: Vec<u8>,
    pub copy_bytes: Vec<u8>,
    pub quality_rejects: Vec<QualityReject>,
}

pub struct SourceRead {
    pub version: String,
    pub version_comment: String,
    pub sql_mode: String,
    pub time_zone: String,
    pub ssl_cipher: String,
    pub current_user: String,
    pub statements: Vec<StatementNote>,
    pub tables: Vec<TableBatch>,
    pub config_digest: String,
    pub type_digest: String,
    pub input_digest: String,
    pub input_bytes: [u8; 32],
    pub schema_digest: String,
    pub hook_digest: String,
}

pub(crate) async fn open_mysql(
    config: &PreviewConfig,
    password: &str,
) -> Result<Conn, PreviewError> {
    oo_connectors::mysql::connect(&config.mysql_source()?, password).await.map_err(Into::into)
}

pub async fn read_source(
    config: &PreviewConfig,
    password: &str,
    config_bytes: &[u8],
) -> Result<SourceRead, PreviewError> {
    read_source_export(config, password, config_bytes, None).await
}

pub async fn read_source_export(
    config: &PreviewConfig,
    password: &str,
    config_bytes: &[u8],
    export_dir: Option<&Path>,
) -> Result<SourceRead, PreviewError> {
    let started = Instant::now();
    let mut conn = open_mysql(config, password).await?;
    let mut statements = Vec::new();
    let access=oo_connectors::mysql::inspect(&mut conn,&config.mysql_user,&mut statements).await?;
    let oo_connectors::mysql::Access {version,version_comment:comment,sql_mode,time_zone,ssl_cipher:cipher,current_user}=access;
    let reserved: std::collections::BTreeSet<String> = PG17_RESERVED
        .iter()
        .map(|word| (*word).to_string())
        .collect();
    let utc = session_utc(&time_zone);
    let mut tables = Vec::new();
    for (index, selected) in config.tables.iter().enumerate() {
        let columns = catalog_columns(&mut conn, &mut statements, selected).await?;
        let mapped = keep_requested(
            map_table(&columns, config.boolean_tinyint1, utc, &reserved)?,
            &selected.columns,
        )?;
        let logical_schema = oo_connectors::stable_ident(&selected.schema, &reserved)
            .map_err(|_| PreviewError::new("ident", "ident"))?;
        let logical_table = oo_connectors::stable_ident(&selected.name, &reserved)
            .map_err(|_| PreviewError::new("ident", "ident"))?;
        tables.push(TableBatch {
            index: u32::try_from(index).map_err(|_| PreviewError::new("config", "tables"))?,
            schema: selected.schema.clone(),
            name: selected.name.clone(),
            logical_schema,
            logical_table,
            columns: mapped,
            batches: Vec::new(),
            rejects: Vec::new(),
        });
    }
    let digests = digests_of(config, config_bytes, &tables);
    if config.stop != crate::config::Stop::Access {
        query_drop(
            &mut conn,
            &mut statements,
            "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
        )
        .await?;
        let mut reader_connection=oo_connectors::mysql::ReadConnection::new(conn,config.mysql_source()?,zeroize::Zeroizing::new(password.to_owned()));
        for table in &mut tables {
            if matches!(config.stop, crate::config::Stop::PartialAfterFirst) && table.index > 0 {
                continue;
            }
            read_table(
                config,
                &mut reader_connection,
                &mut statements,
                table,
                &digests,
                export_dir,
                started,
            )
            .await?;
        }
        conn=reader_connection.into_connection()?;
        query_drop(&mut conn, &mut statements, "COMMIT").await?;
    }
    drop(conn);
    past_snapshot(config, started)?;
    Ok(SourceRead {
        version,
        version_comment: comment,
        sql_mode,
        time_zone,
        ssl_cipher: cipher,
        current_user,
        statements,
        tables,
        config_digest: digests.config_digest,
        type_digest: digests.type_digest,
        input_digest: digests.input_digest,
        input_bytes: digests.input_bytes,
        schema_digest: digests.schema_digest,
        hook_digest: digests.hook_digest,
    })
}

struct Digests {
    config_digest: String,
    type_digest: String,
    input_digest: String,
    input_bytes: [u8; 32],
    schema_digest: String,
    hook_digest: String,
}

fn past_snapshot(config: &PreviewConfig, started: Instant) -> Result<(), PreviewError> {
    if started.elapsed() > Duration::from_millis(config.snapshot_deadline_ms) {
        return Err(PreviewError::new("budget", "snapshot_deadline"));
    }
    Ok(())
}

pub(crate) fn sealed_identity(
    source: &SourceRead,
    config: &PreviewConfig,
) -> Result<(), PreviewError> {
    if !current_user_is(&source.current_user, &config.mysql_user) {
        return Err(PreviewError::new("privilege", "current_user"));
    }
    let rendered = source
        .input_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if rendered != source.input_digest {
        return Err(PreviewError::new("target", "input_digest"));
    }
    Ok(())
}

fn digests_of(config: &PreviewConfig, config_bytes: &[u8], tables: &[TableBatch]) -> Digests {
    let config_digest = sha256_hex(config_bytes);
    let mut schema = String::new();
    for table in tables {
        push_schema_table(&mut schema, &table.logical_schema, &table.logical_table);
        for column in &table.columns {
            push_schema_column(
                &mut schema,
                &column.logical_name,
                column.oid,
                column.typmod,
                column.caveat,
            );
        }
    }
    let schema_digest = sha256_hex(schema.as_bytes());
    let type_digest = sha256_hex(b"oo.mysql-preview.types/v1");
    let input_material = format!(
        "{schema_digest}\n{}\n{}",
        config.generation, config.source_id
    );
    let input_digest = sha256_hex(input_material.as_bytes());
    let mut input_bytes = [0u8; 32];
    for (index, byte) in input_digest.as_bytes().chunks(2).enumerate() {
        let hex = std::str::from_utf8(byte).unwrap_or("00");
        input_bytes[index] = u8::from_str_radix(hex, 16).unwrap_or(0);
    }
    Digests {
        config_digest,
        type_digest,
        input_digest,
        input_bytes,
        schema_digest,
        hook_digest: hook_digest(),
    }
}

pub(crate) async fn catalog_columns(conn: &mut Conn, notes: &mut Vec<StatementNote>, selected: &SelectedTable) -> Result<Vec<MysqlColumn>,PreviewError> {
    oo_connectors::mysql::catalog_columns(conn,notes,selected).await.map_err(Into::into)
}

async fn read_table(
    config: &PreviewConfig, conn: &mut oo_connectors::mysql::ReadConnection, notes: &mut Vec<StatementNote>,
    table: &mut TableBatch, digests: &Digests, export_dir: Option<&Path>, started:Instant,
) -> Result<(),PreviewError> {
    let export_path=export_dir.map(|dir|dir.join(format!("table-{}.arrow",table.index)));
    let mut writer=None;
    let binding=RunBinding {source_id:config.source_id.clone(),schema:table.schema.clone(),table:table.name.clone(),run_generation:config.generation,input_digest:digests.input_bytes};
    past_snapshot(config,started)?;
    let remaining=config.snapshot_deadline_ms.saturating_sub(started.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
    let mut reader=oo_connectors::mysql::TableReader::new(&table.schema,&table.name,table.columns.clone(),binding,
        config.batch_rows,config.max_rows,config.max_bytes,remaining.max(1),config.statement_timeout_ms)?;
    let cancel=oo_connectors::mysql::Cancel::default();
    let mut index=0;
    while let Some(batch)=conn.read_chunk(&mut reader,&cancel).await? {
        notes.push(batch.statement.clone());
        let encoded=encode_page(config,table,digests,&batch,index,export_path.as_deref(),&mut writer)?;
        table.batches.push(encoded);index+=1;
    }
    if let Some(writer)=writer.as_mut(){writer.finish().map_err(|_|PreviewError::new("batch","arrow_finish"))?;}
    Ok(())
}

fn encode_page(
    config: &PreviewConfig,
    table: &mut TableBatch,
    digests: &Digests,
    batch: &oo_connectors::mysql::Batch,
    batch_index: u64,
    export_path: Option<&Path>,
    arrow_writer: &mut Option<StreamWriter<File>>,
) -> Result<EncodedBatch, PreviewError> {
    let typed=&batch.typed;
    let mut quality_rejects=Vec::with_capacity(batch.rejects.len());
    let mut pending_rejects=Vec::with_capacity(batch.rejects.len());
    for (row_index,column_index,note) in &batch.rejects {
        let column=&table.columns[*column_index];
        let value_len=i64::try_from(note.value_len).map_err(|_|PreviewError::new("value","value_len"))?;
        let row_number=i64::try_from(*row_index).map_err(|_|PreviewError::new("budget","max_rows"))?;
        pending_rejects.push((column.logical_name.clone(),note.reason,column.plan.attnum,row_number,value_len));
        quality_rejects.push(QualityReject{reason:note.reason.to_owned(),value_len,attnum:column.plan.attnum,row_index:row_number,key_digest:String::new()});
    }
    if let Some(path) = export_path {
        let batch = typed
            .record_batch()
            .map_err(|_| PreviewError::new("batch", "arrow"))?;
        if arrow_writer.is_none() {
            let file = File::create(path).map_err(|_| PreviewError::new("batch", "arrow_file"))?;
            *arrow_writer = Some(
                StreamWriter::try_new(file, &batch.schema())
                    .map_err(|_| PreviewError::new("batch", "arrow_schema"))?,
            );
        }
        if let Some(writer) = arrow_writer.as_mut() {
            writer
                .write(&batch)
                .map_err(|_| PreviewError::new("batch", "arrow_write"))?;
        }
    }
    fill_key_digests(typed, &mut quality_rejects);
    for (pending, quality) in pending_rejects.iter().zip(quality_rejects.iter()) {
        table.rejects.push(json!({
            "attnum": pending.2,
            "column": pending.0,
            "key_digest": quality.key_digest,
            "reason": pending.1,
            "row": pending.3,
            "table": table.logical_table,
            "value_len": pending.4
        }));
    }
    let (rows_n, cols_n) =
        arrow_dimensions(typed).map_err(|_| PreviewError::new("batch", "arrow"))?;
    if rows_n != typed.rows.len() || cols_n != table.columns.len() {
        return Err(PreviewError::new("batch", "arrow"));
    }
    encode_typed(table, digests, typed, batch_index, config, quality_rejects)
}

fn encode_typed(
    table: &TableBatch,
    digests: &Digests,
    typed: &TypedBatch,
    batch_index: u64,
    config: &PreviewConfig,
    quality_rejects: Vec<QualityReject>,
) -> Result<EncodedBatch, PreviewError> {
    let start = table.batches.iter().map(|batch| batch.rows).sum::<u64>();
    let mut positions = Vec::with_capacity(typed.rows.len());
    for offset in 0..typed.rows.len() {
        let index = start + offset as u64 + 1;
        positions.push(
            ExactDecimal::new(false, index.to_string(), 0)
                .map_err(|_| PreviewError::new("batch", "position"))?,
        );
    }
    let mut columns = Vec::with_capacity(typed.plans.len() + 1);
    columns.push(CopyColumn::SystemPosition);
    for column in &typed.plans {
        if column.mapping.decision == MapDecision::Lossless {
            columns.push(CopyColumn::User {
                attnum: column.attnum,
            });
        }
    }
    let budgets = copy_budgets(config)?;
    let first = pull_bytes(typed, &columns, &positions, budgets)?;
    let second = pull_bytes(typed, &columns, &positions, budgets)?;
    if first != second {
        return Err(PreviewError::new("batch", "encode"));
    }
    let hashed = hash_batch(typed, &columns, &positions, budgets)
        .map_err(|_| PreviewError::new("batch", "encode"))?;
    if hashed.rows != typed.rows.len() as u64 {
        return Err(PreviewError::new("batch", "encode"));
    }
    let batch_id = format!("t{}.b{batch_index}", table.index);
    let applied_pos = positions
        .last()
        .map(ExactDecimal::to_plain_string)
        .unwrap_or_else(|| "0".to_string());
    let rejected = u64::try_from(quality_rejects.len())
        .map_err(|_| PreviewError::new("budget", "max_rows"))?;
    let quality = quality_preimage(
        &batch_id,
        typed.rows.len() as u64,
        rejected,
        &digests.input_digest,
        &quality_rejects,
    );
    let mut mixed = first.clone();
    mixed.extend(quality);
    let mut row_hashes = Vec::with_capacity(typed.rows.len() * 32);
    for row in &typed.rows {
        row_hashes.extend_from_slice(&row.canonical_hash);
    }
    Ok(EncodedBatch {
        batch_id,
        rows: typed.rows.len() as u64,
        payload_digest: sha256_hex(&mixed),
        applied_pos,
        row_hashes,
        copy_bytes: first,
        quality_rejects,
    })
}

fn fill_key_digests(typed: &TypedBatch, rejects: &mut [QualityReject]) {
    for reject in rejects {
        let Ok(index) = usize::try_from(reject.row_index) else {
            continue;
        };
        let Some(row) = typed.rows.get(index) else {
            continue;
        };
        let Some(digest) = row.key_digest else {
            continue;
        };
        reject.key_digest = hex32(&digest);
    }
}

fn hex32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Access document for one captured read. It has no physical names.
pub fn access_document(source: &SourceRead) -> JsonValue {
    json!({
        "protocol": "oo.mysql-preview/v1",
        "state": "access",
        "mysql_version": source.version,
        "version_comment": source.version_comment,
        "sql_mode": source.sql_mode,
        "time_zone": source.time_zone,
        "ssl_cipher_present": !source.ssl_cipher.is_empty(),
        "config_digest": source.config_digest,
        "statements": source.statements.iter().map(|note| json!({"verb": note.verb, "sha256": note.sha256})).collect::<Vec<_>>(),
        "tables": source.tables.iter().map(|table| json!({
            "schema": table.logical_schema,
            "table": table.logical_table,
            "columns": table.columns.iter().map(|column| json!({
                "name": column.logical_name,
                "oid": column.oid,
                "typmod": column.typmod,
                "caveat": column.caveat
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })
}

fn pull_bytes(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    budgets: CopyBudgets,
) -> Result<Vec<u8>, PreviewError> {
    let mut encoder = CopyEncoder::open(
        batch,
        columns,
        positions,
        budgets,
        UnavailablePolicy::Refuse,
    )
    .map_err(|_| PreviewError::new("batch", "encode"))?;
    let mut out = Vec::new();
    loop {
        match encoder
            .pull()
            .map_err(|_| PreviewError::new("batch", "encode"))?
        {
            CopyPiece::Done => break,
            CopyPiece::Header { bytes }
            | CopyPiece::Trailer { bytes }
            | CopyPiece::Rows { bytes, .. } => {
                out.extend_from_slice(&bytes);
            }
        }
    }
    Ok(out)
}

fn copy_budgets(config: &PreviewConfig) -> Result<CopyBudgets, PreviewError> {
    let max_value =
        u32::try_from(config.max_bytes).map_err(|_| PreviewError::new("budget", "max_bytes"))?;
    let max_row = max_value
        .checked_mul(2)
        .ok_or_else(|| PreviewError::new("budget", "max_bytes"))?;
    let max_chunk = max_row
        .checked_mul(2)
        .ok_or_else(|| PreviewError::new("budget", "max_bytes"))?;
    let batch_rows =
        u32::try_from(config.batch_rows).map_err(|_| PreviewError::new("budget", "batch_rows"))?;
    Ok(CopyBudgets {
        max_value_bytes: max_value,
        max_row_bytes: max_row.max(64),
        max_chunk_bytes: max_chunk.max(64),
        max_rows_per_chunk: batch_rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_quality_preimage_matches_the_historical_line() {
        let bytes = quality_preimage("t0.b0", 5, 0, "abc", &[]);
        assert_eq!(bytes, b"full-import/v1 quality\nbatch\tt0.b0\t5\t0\tabc\n");
    }

    #[test]
    fn a_masked_reject_is_a_quality_line_without_the_value() {
        let bytes = quality_preimage(
            "t0.b0",
            5,
            1,
            "abc",
            &[QualityReject {
                reason: "zero_date".to_string(),
                value_len: 10,
                attnum: 12,
                row_index: 1,
                key_digest: String::new(),
            }],
        );
        let text = String::from_utf8(bytes).expect("utf8");
        assert_eq!(
            text,
            "full-import/v1 quality\nbatch\tt0.b0\t5\t1\tabc\nreject\tzero_date\t10\t12\t1\t\tabc\n"
        );
        assert!(!text.contains("0000-00-00"));
    }

    #[test]
    fn key_digest_text_is_lowercase_hex() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xab;
        bytes[31] = 0x0f;
        let text = hex32(&bytes);
        assert_eq!(text.len(), 64);
        assert!(
            text.chars()
                .all(|char| char.is_ascii_hexdigit() && !char.is_ascii_uppercase())
        );
        assert!(text.starts_with("ab"));
        assert!(text.ends_with("0f"));
    }
}
