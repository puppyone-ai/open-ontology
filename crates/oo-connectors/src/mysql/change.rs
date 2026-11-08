//! Opt-in, read-only ROW/FULL CDC. Each batch is one complete transaction.
//! No cursor is acknowledged here: callers resume only from durable receipts.
use std::{future::poll_fn, pin::Pin};

use futures_util::Stream;
use mysql_async::binlog::{
    events::{EventData, RowsEventData},
    row::BinlogRow,
};
use mysql_async::{BinlogStream, BinlogStreamRequest};

use super::*;
use crate::engine::{ChangeBatch, ChangeSource, Requirement, RequirementKind, RowChange};
use crate::mysql::values::{self, RawCell};
use crate::types::assemble_batch;

/// A separate replication reader for a fixed set of tables on one source.
/// Row images contain exactly one row per change (the before image for deletes).
/// This makes alignment unambiguous even for interleaved tables and key changes.
pub struct MysqlChanges {
    engine: MysqlEngine,
    server_id: u32,
    source_uuid: String,
    source_epoch: String,
    mariadb_anchor: Option<Vec<(u32, u32, u64)>>,
    classified_alter: BTreeMap<TableRef, std::collections::BTreeSet<String>>,
    quarantined: std::collections::BTreeSet<TableRef>,
    table_positions: BTreeMap<TableRef, Watermark>,
    projections: BTreeMap<TableRef, Vec<String>>,
    previous_columns: BTreeMap<TableRef, Vec<String>>,
}

/// Actual retained history, measured from the oldest readable binlog event.
#[derive(Debug)]
pub struct BinlogRetention {
    pub earliest: Watermark,
    pub oldest_event_unix: Option<u32>,
    pub server_unix: u64,
    pub required_seconds: u64,
    pub rebaseline: Option<Requirement>,
}

impl BinlogRetention {
    pub fn require_sufficient(self) -> std::result::Result<Self, Requirement> {
        match &self.rebaseline {
            Some(requirement) => Err(requirement.clone()),
            None => Ok(self),
        }
    }
}

impl MysqlEngine {
    /// Opens no source objects and writes no source data. The replication client
    /// id must be unique among concurrently connected replication clients.
    pub async fn changes(&mut self, tables: &[TableRef], server_id: u32) -> Result<MysqlChanges> {
        match self.family {
            EngineFamily::Tidb => return Err(SourceError::config("tidb_c1_unsupported_use_c5")),
            EngineFamily::Oceanbase => {
                return Err(SourceError::config("oceanbase_c1_unsupported_use_c5"));
            }
            _ => {}
        }
        if tables.is_empty() || server_id == 0 {
            return Err(SourceError::config("binlog_subscription"));
        }
        let mut engine = MysqlEngine::new_family(
            self.family,
            self.config.clone(),
            self.password.clone(),
            self.options.clone(),
        );
        engine.ensure_basics().await?;
        engine.check_replica_commit_order().await?;
        let maria = engine.family == EngineFamily::Mariadb;
        let settings = engine.exec(if maria {
            "SELECT CAST(@@GLOBAL.server_id AS CHAR), @@GLOBAL.server_id, @@GLOBAL.binlog_format, @@GLOBAL.binlog_row_image, @@GLOBAL.log_bin, @@GLOBAL.gtid_binlog_pos"
        } else {
            "SELECT @@GLOBAL.server_uuid, @@GLOBAL.server_id, @@GLOBAL.binlog_format, @@GLOBAL.binlog_row_image, @@GLOBAL.log_bin"
        }, vec![]).await?;
        let row = settings
            .first()
            .ok_or_else(|| SourceError::protocol("binlog_settings"))?;
        if row_u64(row, 1) == Some(u64::from(server_id))
            || row_text(row, 2).as_deref() != Some("ROW")
            || row_text(row, 3).as_deref() != Some("FULL")
            || !on_off(row_text(row, 4))
        {
            return Err(SourceError::config(
                "binlog_row_full_unique_server_id_required",
            ));
        }
        let source_uuid = row_text(row, 0).ok_or_else(|| SourceError::protocol("server_uuid"))?;
        let mariadb_anchor = if maria {
            Some(crate::mysql::mariadb::parse_gtid(
                &row_text(row, 5).ok_or_else(rebaseline)?,
            )?)
        } else {
            None
        };
        for table in tables {
            engine.meta(table).await?;
            let meta = engine.meta_ref(table)?;
            if meta.key.columns.is_empty() {
                return Err(SourceError::config("binlog_requires_key"));
            }
            // Snapshot SQL transcodes these encodings; binlog bytes do not.
            if meta.columns.iter().any(|c| {
                c.plan
                    .mapping
                    .metadata
                    .iter()
                    .any(|(k, _)| k == "mysql_source_charset")
            }) {
                return Err(SourceError::config("binlog_charset_unsupported"));
            }
        }
        let source_epoch = engine.source_epoch().await?;
        Ok(MysqlChanges {
            engine,
            server_id,
            source_uuid,
            source_epoch,
            mariadb_anchor,
            classified_alter: Default::default(),
            quarantined: Default::default(),
            table_positions: BTreeMap::new(),
            projections: BTreeMap::new(),
            previous_columns: BTreeMap::new(),
        })
    }
}

fn rebaseline() -> SourceError {
    SourceError::protocol("mysql_lineage_or_purge_new_generation_rebaseline")
}

fn stream_error(error: mysql_async::Error) -> SourceError {
    match error {
        mysql_async::Error::Server(s) if s.code == 1236 => rebaseline(),
        mysql_async::Error::Server(s) if matches!(s.code, 1044 | 1045 | 1142 | 1227) => {
            SourceError::privilege("binlog_denied")
        }
        _ => SourceError::protocol("binlog_stream_failed"),
    }
}

fn uuid(sid: [u8; 16]) -> String {
    let hex = hex(&sid);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn raw(value: Value) -> RawCell {
    match value {
        Value::NULL => RawCell::Null,
        Value::Int(n) => RawCell::Int(n),
        Value::UInt(n) => RawCell::Uint(n),
        Value::Float(n) => RawCell::Float(n),
        Value::Double(n) => RawCell::Double(n),
        Value::Bytes(b) => RawCell::Bytes(b),
        Value::Date(year, month, day, hour, minute, second, micros) => RawCell::Date {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        },
        Value::Time(negative, days, hours, minutes, seconds, micros) => RawCell::Time {
            negative,
            days,
            hours,
            minutes,
            seconds,
            micros,
        },
    }
}

async fn image(
    engine: &mut MysqlEngine,
    meta: &TableMeta,
    row: &BinlogRow,
    indexes: &[usize],
) -> Result<(Vec<KeyValue>, RecordBatch)> {
    if indexes.len() != meta.columns.len() || indexes.iter().any(|i| *i >= row.len()) {
        return Err(SourceError::protocol(
            "binlog_schema_new_generation_rebaseline",
        ));
    }
    let mut cells = Vec::with_capacity(meta.columns.len());
    let mut key = Vec::with_capacity(meta.key_positions.len());
    let mut raw_values = Vec::with_capacity(meta.columns.len());
    for (i, column) in meta.columns.iter().enumerate() {
        let value = row
            .as_ref(indexes[i])
            .ok_or_else(|| SourceError::protocol("binlog_partial_image"))?;
        let mut value = Value::try_from(value.clone())
            .map_err(|_| SourceError::protocol("binlog_partial_image"))?;
        // mysql_common adds 1900 to YEAR's zero byte as well. MySQL's zero
        // YEAR is 0, while 1900 itself is outside the type's valid range.
        if column
            .plan
            .mapping
            .source_type
            .to_ascii_lowercase()
            .starts_with("year")
            && value == Value::Bytes(b"1900".to_vec())
        {
            value = Value::Int(0);
        }
        if matches!(value, Value::Bytes(_))
            && matches!(column.store, super::super::map::StoreKind::Json)
        {
            // The driver emits compact JSON, but snapshot JSON preserves the
            // server's spelling. Format the logged value, never reread a row
            // that could already belong to a later transaction.
            let rows = engine
                .exec(
                    "SELECT CAST(CAST(? AS JSON) AS CHAR CHARACTER SET utf8mb4)",
                    vec![value],
                )
                .await?;
            value = row_value(
                rows.first()
                    .ok_or_else(|| SourceError::protocol("binlog_json"))?,
                0,
            )?;
        }
        if !matches!(value, Value::NULL)
            && column
                .plan
                .mapping
                .source_type
                .to_ascii_lowercase()
                .starts_with("timestamp")
        {
            if matches!(&value, Value::Bytes(b) if b == b"0" || b == b"0.000000")
                || value == Value::Int(0)
            {
                return Err(SourceError::protocol("binlog_zero_timestamp"));
            }
            let rows = engine
                .exec("SELECT CAST(FROM_UNIXTIME(?) AS DATETIME(6))", vec![value])
                .await?;
            value = row_value(
                rows.first()
                    .ok_or_else(|| SourceError::protocol("binlog_timestamp"))?,
                0,
            )?;
            if matches!(value, Value::NULL) {
                return Err(SourceError::protocol("binlog_timestamp_range"));
            }
        }
        let cell = values::normalize_binlog_integer(&column.plan.mapping.source_type, raw(value))
            .map_err(mysql_err)?;
        let (decoded, reject) = values::decode_mapped(column, &cell).map_err(mysql_err)?;
        if reject.is_some() {
            return Err(SourceError::protocol("binlog_conversion_reject"));
        }
        raw_values.push(cell);
        cells.push(decoded);
    }
    for (col, &index) in meta.key.columns.iter().zip(&meta.key_positions) {
        let value = match &raw_values[index] {
            RawCell::Int(n) => Value::Int(*n),
            RawCell::Uint(n) => Value::UInt(*n),
            RawCell::Bytes(b) => Value::Bytes(b.clone()),
            _ => return Err(SourceError::protocol("binlog_key_type")),
        };
        key.push(key_value(col.kind, &value)?);
    }
    let keys = meta
        .columns
        .iter()
        .filter(|c| c.key)
        .map(|c| c.plan.attnum)
        .collect::<Vec<_>>();
    let batch = assemble_batch(
        &meta.binding,
        meta.columns.iter().map(|c| c.plan.clone()).collect(),
        &keys,
        vec![cells],
    )?
    .record_batch()?;
    Ok((key, batch))
}

impl MysqlChanges {
    /// Only a coordinator that checked native facts against the sealed
    /// projection may admit these DDL boundaries. Row decoding stays strict.
    pub fn allow_classified_alter(
        &mut self,
        clauses: BTreeMap<TableRef, std::collections::BTreeSet<String>>,
        quarantined: std::collections::BTreeSet<TableRef>,
    ) {
        self.classified_alter = clauses;
        self.quarantined = quarantined;
    }
    /// File positions must be aligned SHOW BINARY LOG STATUS boundaries, never
    /// offsets inside a transaction. Repaired history needs no current-schema decode.
    pub fn table_positions(&mut self, positions: BTreeMap<TableRef, Watermark>) {
        self.table_positions = positions;
    }
    pub fn projections(
        &mut self,
        projections: BTreeMap<TableRef, Vec<String>>,
        previous_columns: BTreeMap<TableRef, Vec<String>>,
    ) {
        self.projections = projections;
        self.previous_columns = previous_columns;
    }

    fn projected_meta(
        &self,
        table: &TableRef,
        mut meta: TableMeta,
        width: usize,
    ) -> Result<(TableMeta, Vec<usize>)> {
        let Some(names) = self.projections.get(table) else {
            if width != meta.columns.len() {
                return Err(SourceError::protocol(
                    "binlog_schema_new_generation_rebaseline",
                ));
            }
            return Ok((meta, (0..width).collect()));
        };
        let indexes = projection_indexes(
            names,
            &meta.names,
            self.previous_columns.get(table).map(Vec::as_slice),
            width,
        )?;
        meta.columns = names
            .iter()
            .map(|name| {
                meta.columns
                    .iter()
                    .find(|c| &c.plan.name == name)
                    .cloned()
                    .ok_or_else(|| SourceError::protocol("binlog_projection"))
            })
            .collect::<Result<_>>()?;
        meta.names = names.clone();
        meta.key_positions = meta
            .key
            .columns
            .iter()
            .map(|key| {
                names
                    .iter()
                    .position(|name| name == &key.name)
                    .ok_or_else(|| SourceError::protocol("binlog_projection_key"))
            })
            .collect::<Result<_>>()?;
        Ok((meta, indexes))
    }
    /// Persist with the durable cursor; compare on reconstruction before replay.
    pub fn source_epoch(&self) -> &str {
        &self.source_epoch
    }

    pub async fn close(&mut self) {
        self.engine.close().await;
    }

    /// Same oldest-event measurement as the preview retention helper, now
    /// available to connector callers without depending on a preview tool.
    pub async fn retention(
        &mut self,
        longest_cycle_seconds: u64,
        recovery_seconds: u64,
    ) -> Result<BinlogRetention> {
        let required_seconds = longest_cycle_seconds
            .checked_add(recovery_seconds)
            .ok_or_else(|| SourceError::config("retention_interval"))?;
        let earliest = self.earliest_available().await?;
        self.validate_position(&earliest).await?;
        let Watermark::MysqlFilePos { file, position } = &earliest else {
            unreachable!()
        };
        let rows = self.engine.exec("SELECT UNIX_TIMESTAMP()", vec![]).await?;
        let server_unix = rows
            .first()
            .and_then(|r| row_u64(r, 0))
            .ok_or_else(|| SourceError::protocol("retention_server_time"))?;
        let conn = super::super::connect(&self.engine.config, &self.engine.password)
            .await
            .map_err(mysql_err)?;
        let mut stream = Box::pin(
            conn.get_binlog_stream(
                BinlogStreamRequest::new(self.server_id)
                    .with_filename(file.as_bytes())
                    .with_pos(*position)
                    .with_non_blocking(),
            )
            .await
            .map_err(stream_error)?,
        );
        let measured = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
                let timestamp = event.map_err(stream_error)?.header().timestamp();
                if timestamp > 0 {
                    return Ok::<_, SourceError>(Some(timestamp));
                }
            }
            Ok(None)
        })
        .await;
        let _ = tokio::time::timeout(Duration::from_secs(2), Pin::into_inner(stream).close()).await;
        let oldest_event_unix =
            measured.map_err(|_| SourceError::budget("retention_timeout"))??;
        if self.earliest_available().await? != earliest {
            return Err(SourceError::protocol("retention_inventory_changed_retry"));
        }
        self.validate_position(&earliest).await?;
        let age = oldest_event_unix.and_then(|old| server_unix.checked_sub(u64::from(old)));
        let rebaseline = age
            .filter(|age| *age >= required_seconds)
            .is_none()
            .then(|| Requirement {
                kind: RequirementKind::Retention,
                name: "actual_binlog_history_new_generation_rebaseline".into(),
                observed: age.map(|age| age.to_string()),
                required: format!("{required_seconds} seconds (longest cycle + recovery)"),
                dba_statement: None,
            });
        Ok(BinlogRetention {
            earliest,
            oldest_event_unix,
            server_unix,
            required_seconds,
            rebaseline,
        })
    }

    pub(super) async fn validate_position(&mut self, from: &Watermark) -> Result<()> {
        self.engine.ensure_basics().await?;
        self.engine.check_replica_commit_order().await?;
        if self.engine.source_epoch().await? != self.source_epoch {
            return Err(rebaseline());
        }
        let rows = self
            .engine
            .exec(
                if self.mariadb_anchor.is_some() {
                    "SELECT CAST(@@GLOBAL.server_id AS CHAR), @@GLOBAL.gtid_binlog_pos, ''"
                } else {
                    "SELECT @@GLOBAL.server_uuid, @@GLOBAL.gtid_executed, @@GLOBAL.gtid_purged"
                },
                vec![],
            )
            .await?;
        let row = rows.first().ok_or_else(rebaseline)?;
        if row_text(row, 0).as_deref() != Some(&self.source_uuid) {
            return Err(rebaseline());
        }
        if let Some(anchor) = &self.mariadb_anchor {
            if !matches!(from, Watermark::MysqlFilePos { .. }) {
                return Err(SourceError::config("mariadb_file_position_required"));
            }
            let current =
                crate::mysql::mariadb::parse_gtid(&row_text(row, 1).ok_or_else(rebaseline)?)?;
            if !crate::mysql::mariadb::gtid_follows(anchor, &current) {
                return Err(rebaseline());
            }
            self.mariadb_anchor = Some(current);
        }
        match from {
            Watermark::MysqlGtidSet { gtid_set } => {
                let start = GtidSet::parse(gtid_set)?;
                let executed = GtidSet::parse(&row_text(row, 1).ok_or_else(rebaseline)?)?;
                let purged = GtidSet::parse(&row_text(row, 2).ok_or_else(rebaseline)?)?;
                if !executed.contains(&start) || !start.contains(&purged) {
                    return Err(rebaseline());
                }
            }
            Watermark::MysqlFilePos { file, position } => {
                let logs = self.engine.exec("SHOW BINARY LOGS", vec![]).await?;
                let size = logs
                    .iter()
                    .find(|r| row_text(r, 0).as_deref() == Some(file))
                    .and_then(|r| row_u64(r, 1));
                if *position < 4 || size.is_none_or(|n| *position > n) {
                    return Err(rebaseline());
                }
            }
            _ => return Err(SourceError::config("mysql_watermark_required")),
        }
        Ok(())
    }

    async fn read(&mut self, from: &Watermark, budget: &ReadBudget) -> Result<Option<ChangeBatch>> {
        self.validate_position(from).await?;
        let mut set = match from {
            Watermark::MysqlGtidSet { gtid_set } => Some(GtidSet::parse(gtid_set)?),
            _ => None,
        };
        let conn = super::super::connect(&self.engine.config, &self.engine.password)
            .await
            .map_err(mysql_err)?;
        let request = BinlogStreamRequest::new(self.server_id).with_non_blocking();
        let request = match from {
            Watermark::MysqlGtidSet { .. } => request
                .with_gtid()
                .with_gtid_set(set.as_ref().expect("gtid").wire()?),
            Watermark::MysqlFilePos { file, position } => {
                request.with_filename(file.as_bytes()).with_pos(*position)
            }
            _ => unreachable!(),
        };
        let mut stream = Box::pin(
            conn.get_binlog_stream(request)
                .await
                .map_err(stream_error)?,
        );
        let result = self
            .transaction(stream.as_mut(), from, &mut set, budget)
            .await;
        // Closing is best effort; a read error never yields a cursor to commit.
        let _ = tokio::time::timeout(Duration::from_secs(2), Pin::into_inner(stream).close()).await;
        result
    }

    async fn transaction(
        &mut self,
        mut stream: Pin<&mut BinlogStream>,
        from: &Watermark,
        set: &mut Option<GtidSet>,
        budget: &ReadBudget,
    ) -> Result<Option<ChangeBatch>> {
        let mut file = match from {
            Watermark::MysqlFilePos { file, .. } => file.clone(),
            _ => String::new(),
        };
        let mut pending = false;
        let mut gtid = None;
        let mut changes = Vec::new();
        let mut images = Vec::new();
        let mut bytes = 0u64;
        while let Some(event) = poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            let event = event.map_err(stream_error)?;
            if let Some(checksum) = event.checksum()
                && event.footer().get_checksum_enabled()
                && let Some(alg) = event
                    .footer()
                    .get_checksum_alg()
                    .map_err(|_| SourceError::protocol("binlog_checksum"))?
                && event.calc_checksum(alg).to_le_bytes() != checksum
            {
                return Err(SourceError::protocol("binlog_checksum"));
            }
            let position = u64::from(event.header().log_pos());
            bytes = bytes.saturating_add(u64::from(event.header().event_size()));
            if bytes > budget.max_chunk_bytes {
                return Err(SourceError::budget("binlog_transaction_bytes"));
            }
            let data = event
                .read_data()
                .map_err(|_| SourceError::protocol("binlog_event"))?
                .ok_or_else(|| SourceError::protocol("binlog_unknown_event"))?;
            let mut commit = false;
            match data {
                EventData::RotateEvent(e) => {
                    // MariaDB's synthetic initial rotate precedes the format
                    // descriptor; the driver can include its CRC in the name.
                    // Keep the already inventory-validated requested filename.
                    if !(self.mariadb_anchor.is_some()
                        && event.header().timestamp() == 0
                        && event.header().log_pos() == 0)
                    {
                        file = e.name().into_owned();
                    }
                }
                EventData::GtidEvent(e) => {
                    if pending {
                        return Err(SourceError::protocol("binlog_unclosed_transaction"));
                    }
                    pending = true;
                    gtid = Some((
                        uuid(e.sid()),
                        e.tag().map(ToString::to_string).unwrap_or_default(),
                        e.gno(),
                    ));
                }
                EventData::AnonymousGtidEvent(_) => {
                    if set.is_some() || pending {
                        return Err(rebaseline());
                    }
                    pending = true;
                }
                EventData::QueryEvent(e) => {
                    let sql = e.query().trim().to_ascii_uppercase();
                    match sql.as_str() {
                        // MariaDB's legacy-slave protocol replaces row annotations
                        // (type 160) with a known truncated comment, not arbitrary SQL.
                        _ if self.mariadb_anchor.is_some()
                            && pending
                            && crate::mysql::mariadb::annotation_noop(&sql) => {}
                        "BEGIN" => {
                            pending = true;
                        }
                        "COMMIT" => {
                            commit = true;
                        }
                        "ROLLBACK" => {
                            changes.clear();
                            images.clear();
                            commit = true;
                        }
                        // Account management is its own implicitly committed
                        // transaction and cannot change rows or table shape.
                        // Access is rechecked table by table on every cycle.
                        _ if account_statement(&sql) => {
                            if !changes.is_empty() {
                                return Err(SourceError::protocol(
                                    "binlog_statement_new_generation_rebaseline",
                                ));
                            }
                            commit = true;
                        }
                        // DDL or statement DML may invalidate the cached schema. Fail closed.
                        _ => {
                            let table = alter_table(e.query().as_ref(), e.schema().as_ref())
                                .ok_or_else(|| {
                                    SourceError::protocol(
                                        "binlog_statement_new_generation_rebaseline",
                                    )
                                })?;
                            let covered = self
                                .table_positions
                                .get(&table)
                                .map(|floor| covered_file_event(&file, position, floor))
                                .transpose()?
                                .unwrap_or(false);
                            let proven = alter_clause(e.query().as_ref()).is_some_and(|clause| {
                                self.classified_alter
                                    .get(&table)
                                    .is_some_and(|clauses| clauses.contains(&clause))
                            });
                            if !covered && !proven && !self.quarantined.contains(&table) {
                                return Err(SourceError::protocol(
                                    "binlog_statement_new_generation_rebaseline",
                                ));
                            }
                            commit = true;
                        }
                    }
                }
                EventData::RowsEvent(rows) => {
                    if !pending {
                        return Err(SourceError::protocol("binlog_rows_outside_transaction"));
                    }
                    if matches!(rows, RowsEventData::PartialUpdateRowsEvent(_)) {
                        return Err(SourceError::protocol("binlog_partial_image"));
                    }
                    let tme = stream
                        .get_tme(rows.table_id())
                        .ok_or_else(|| SourceError::protocol("binlog_table_map"))?;
                    let table = TableRef {
                        schema: tme.database_name().into_owned(),
                        name: tme.table_name().into_owned(),
                    };
                    if let Some(floor) = self.table_positions.get(&table)
                        && covered_file_event(&file, position, floor)?
                    {
                        continue;
                    }
                    if let Some(meta) = self.engine.tables.get(&table).cloned() {
                        let (meta, indexes) =
                            self.projected_meta(&table, meta, tme.columns_count() as usize)?;
                        for pair in rows.rows(tme) {
                            let (before, after) =
                                pair.map_err(|_| SourceError::protocol("binlog_row"))?;
                            let before = match before.as_ref() {
                                Some(row) => {
                                    Some(image(&mut self.engine, &meta, row, &indexes).await?)
                                }
                                None => None,
                            };
                            let after = match after.as_ref() {
                                Some(row) => {
                                    Some(image(&mut self.engine, &meta, row, &indexes).await?)
                                }
                                None => None,
                            };
                            let (change, batch) = match (before, after) {
                                (None, Some((key, batch))) => (
                                    RowChange::Insert {
                                        table: table.clone(),
                                        key,
                                    },
                                    batch,
                                ),
                                (Some((key, batch)), None) => (
                                    RowChange::Delete {
                                        table: table.clone(),
                                        key,
                                    },
                                    batch,
                                ),
                                (Some((old, _)), Some((key, batch))) => {
                                    let old_key = (old != key).then_some(old);
                                    (
                                        RowChange::Update {
                                            table: table.clone(),
                                            key,
                                            old_key,
                                        },
                                        batch,
                                    )
                                }
                                _ => return Err(SourceError::protocol("binlog_row_pair")),
                            };
                            bytes = bytes.saturating_add(batch.get_array_memory_size() as u64);
                            changes.push(change);
                            images.push(batch);
                            if changes.len() as u64 > budget.max_chunk_rows
                                || bytes > budget.max_chunk_bytes
                            {
                                return Err(SourceError::budget("binlog_transaction_budget"));
                            }
                        }
                    }
                }
                EventData::XidEvent(_) => {
                    commit = true;
                }
                EventData::TableMapEvent(_)
                | EventData::FormatDescriptionEvent(_)
                | EventData::PreviousGtidsEvent(_)
                | EventData::RowsQueryEvent(_)
                | EventData::HeartbeatEvent
                | EventData::StopEvent => {}
                _ => return Err(SourceError::protocol("binlog_unsupported_event")),
            }
            if commit {
                if !pending {
                    return Err(SourceError::protocol("binlog_commit_without_transaction"));
                }
                let position = if let Some(set) = set {
                    let (uuid, tag, n) = gtid.ok_or_else(rebaseline)?;
                    set.advance(&uuid, &tag, n)?;
                    Watermark::MysqlGtidSet {
                        gtid_set: set.to_string(),
                    }
                } else {
                    Watermark::MysqlFilePos { file, position }
                };
                return Ok(Some(ChangeBatch {
                    position,
                    changes,
                    images,
                }));
            }
        }
        if pending {
            return Err(SourceError::protocol("binlog_incomplete_transaction"));
        }
        Ok(None)
    }
}

/// `sql` is the trimmed, upper-cased query text. A leading comment or any
/// other statement is not admitted.
fn account_statement(sql: &str) -> bool {
    let words: Vec<_> = sql.split_whitespace().take(3).collect();
    matches!(
        words.as_slice(),
        ["GRANT" | "REVOKE", ..]
            | ["CREATE" | "ALTER" | "DROP" | "RENAME", "USER" | "ROLE", ..]
            | ["SET", "PASSWORD", ..]
            | ["SET", "DEFAULT", "ROLE"]
            | ["FLUSH", "PRIVILEGES", ..]
    )
}

fn alter_table(sql: &str, database: &str) -> Option<TableRef> {
    let sql = sql.trim();
    let prefix = sql.get(..12)?;
    if !prefix.eq_ignore_ascii_case("ALTER TABLE ") {
        return None;
    }
    let rest = sql.get(12..)?.trim_start();
    let end = rest.find(char::is_whitespace)?;
    let name = &rest[..end];
    let parts: Vec<_> = name.split('.').collect();
    let identifier = |s: &str| {
        let s = s
            .strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .unwrap_or(s);
        (!s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
            .then(|| s.to_owned())
    };
    match parts.as_slice() {
        [name] => Some(TableRef {
            schema: identifier(database)?,
            name: identifier(name)?,
        }),
        [schema, name] => Some(TableRef {
            schema: identifier(schema)?,
            name: identifier(name)?,
        }),
        _ => None,
    }
}

fn alter_clause(sql: &str) -> Option<String> {
    let rest = sql.trim().get(12..)?.trim_start();
    let end = rest.find(char::is_whitespace)?;
    Some(
        rest[end..]
            .trim()
            .trim_end_matches(';')
            .replace('`', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase(),
    )
}

fn covered_file_event(file: &str, position: u64, floor: &Watermark) -> Result<bool> {
    let Watermark::MysqlFilePos {
        file: end,
        position: high,
    } = floor
    else {
        return Err(SourceError::config(
            "binlog_table_floor_requires_file_position",
        ));
    };
    let split = |name: &str| -> Result<(String, u64)> {
        let (prefix, number) = name.rsplit_once('.').ok_or_else(rebaseline)?;
        Ok((prefix.into(), number.parse().map_err(|_| rebaseline())?))
    };
    let (prefix, sequence) = split(file)?;
    let (end_prefix, end_sequence) = split(end)?;
    if prefix != end_prefix {
        return Err(rebaseline());
    }
    Ok((sequence, position) <= (end_sequence, *high))
}

fn projection_indexes(
    names: &[String],
    current: &[String],
    previous: Option<&[String]>,
    width: usize,
) -> Result<Vec<usize>> {
    let indexes = |columns: &[String]| {
        names
            .iter()
            .map(|name| {
                columns
                    .iter()
                    .position(|n| n == name)
                    .ok_or_else(|| SourceError::protocol("binlog_projection"))
            })
            .collect::<Result<Vec<_>>>()
    };
    let live = (current.len() == width)
        .then(|| indexes(current))
        .transpose()?;
    let old = previous
        .filter(|p| p.len() == width)
        .map(indexes)
        .transpose()?;
    match (live, old) {
        (Some(a), Some(b)) if a != b => Err(SourceError::protocol(
            "binlog_ambiguous_projection_rebaseline",
        )),
        (Some(a), _) | (_, Some(a)) => Ok(a),
        _ => Err(SourceError::protocol(
            "binlog_schema_new_generation_rebaseline",
        )),
    }
}

impl ChangeSource for MysqlChanges {
    async fn earliest_available(&mut self) -> Result<Watermark> {
        self.engine.earliest_binlog().await
    }

    async fn next_batch(
        &mut self,
        from: &Watermark,
        budget: &ReadBudget,
    ) -> Result<Option<ChangeBatch>> {
        self.engine.apply_budget(budget).await?;
        self.engine.gate().await?;
        let started = Instant::now();
        let deadline = Duration::from_millis(budget.statement_timeout_ms.clamp(1, 600_000));
        let result = match tokio::time::timeout(deadline, self.read(from, budget)).await {
            Ok(result) => result,
            Err(_) => {
                self.engine.drop_conn().await;
                Err(SourceError::budget("binlog_read_timeout"))
            }
        };
        self.engine.finish_op(started);
        result
    }
}

#[cfg(test)]
#[path = "change_tests.rs"]
mod tests;
