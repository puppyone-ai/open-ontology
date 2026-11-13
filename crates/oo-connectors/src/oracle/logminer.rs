//! Bounded, platform-owned online-catalog LogMiner sessions.
//!
//! Mining starts at the retained-log floor, not the last commit watermark:
//! a transaction can start before the watermark and commit after it. No
//! dictionary build, source DML/DDL, continuous mining, or redo SQL execution.

use super::*;
use crate::engine::{ChangeBatch, ChangeSource, RowChange};
use std::collections::BTreeSet;

const LOGS: &str = "
SELECT name, TO_CHAR(first_change#), TO_CHAR(next_change#),
       TO_CHAR(thread#), TO_CHAR(sequence#)
FROM v$archived_log
WHERE name IS NOT NULL AND deleted='NO' AND status='A'
  AND resetlogs_change#=(SELECT resetlogs_change# FROM v$database)
UNION ALL
SELECT f.member, TO_CHAR(l.first_change#), TO_CHAR(l.next_change#),
       TO_CHAR(l.thread#), TO_CHAR(l.sequence#)
FROM v$log l JOIN v$logfile f ON f.group#=l.group#
WHERE l.status IN ('CURRENT','ACTIVE','INACTIVE')
  AND (f.status IS NULL OR f.status='STALE')";

/// One mined change: (commit SCN, SCN, RS_ID, SSN, change, row batch).
type MinedRow = (u64, u64, String, u64, RowChange, RecordBatch);

#[derive(Clone, Debug)]
struct LogFile {
    name: String,
    first: u64,
    next: u64,
    thread: u64,
    sequence: u64,
}

fn number(value: Option<&str>) -> Result<u64, SourceError> {
    value
        .and_then(|v| v.parse().ok())
        .ok_or(SourceError::protocol("logminer_number"))
}

fn scn(position: &Watermark) -> Result<u64, SourceError> {
    match position {
        Watermark::OracleScn { scn } => Ok(*scn),
        _ => Err(SourceError::config("logminer_watermark_kind")),
    }
}

fn validate_logging(mode: Option<&str>, minimal: Option<&str>) -> Result<(), SourceError> {
    if mode != Some("ARCHIVELOG") || !matches!(minimal, Some("YES" | "IMPLICIT")) {
        return Err(SourceError::catalog(
            "logminer_logging_prerequisites_rebaseline",
        ));
    }
    Ok(())
}

fn mine_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|c| {
            c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, b'_' | b'$' | b'#')
        })
}

fn coverage(files: &[LogFile], end: u64) -> Result<u64, SourceError> {
    let threads: BTreeSet<_> = files.iter().map(|f| f.thread).collect();
    if threads.is_empty() {
        return Err(SourceError::catalog("logminer_no_retained_logs"));
    }
    // V$TRANSACTION admission observes this instance, not remote RAC sessions.
    if threads.len() != 1 {
        return Err(SourceError::config("logminer_rac_unsupported"));
    }
    let mut floor = 0;
    for thread in threads {
        let mut ranges: Vec<_> = files.iter().filter(|f| f.thread == thread).collect();
        ranges.sort_by_key(|f| f.first);
        let first = ranges[0].first;
        let mut through = first;
        for file in ranges {
            if file.first > through && through <= end {
                return Err(SourceError::catalog("logminer_log_gap_rebaseline"));
            }
            through = through.max(file.next);
        }
        if through <= end {
            return Err(SourceError::catalog("logminer_log_tail_missing"));
        }
        floor = floor.max(first);
    }
    Ok(floor)
}

/// Owns a root/common-user mining session; metadata is admitted from the
/// matching PDB engine. `rebaseline_tables` identifies dictionary failures.
/// Unsupported types fail admission rather than decoding lossy row images.
pub struct OracleLogMiner {
    engine: OracleEngine,
    tables: BTreeMap<TableRef, TableMeta>,
    pdb: Option<String>,
    identity: String,
    rebaseline: BTreeSet<TableRef>,
    retained_floor: u64,
    admitted_scn: u64,
    classified_ddl: bool,
    table_positions: BTreeMap<TableRef, u64>,
    /// Largest commit-SCN span mined in one batch after a budget refusal.
    scn_window: Option<u64>,
}

impl OracleLogMiner {
    /// The coordinator must validate the native projection before and after
    /// capture. Unknown dictionary/row errors still require rebaseline.
    pub fn allow_classified_ddl(&mut self) {
        self.classified_ddl = true;
    }
    pub fn table_positions(&mut self, positions: BTreeMap<TableRef, u64>) {
        self.table_positions = positions;
    }
    /// Exclusion is legal only after the coordinator persists quarantine.
    pub fn quarantine(&mut self, tables: &BTreeSet<TableRef>) {
        self.tables.retain(|table, _| !tables.contains(table));
        self.rebaseline.retain(|table| !tables.contains(table));
    }
    /// Resume with the transaction floor captured before the aligned baseline.
    /// Persist it with DBID/resetlogs and the cursor; advancing a commit SCN
    /// does not permit advancing this floor.
    pub fn resume_from_floor(&mut self, floor: u64, from: u64) -> Result<(), SourceError> {
        let files = self.inventory()?;
        let high = self.engine.current_scn()?;
        let available = coverage(&files, high)?;
        validate_resume_floor(floor, from, available, self.retained_floor, high)?;
        self.retained_floor = floor;
        self.admitted_scn = from;
        Ok(())
    }

    /// Persist DBID/resetlogs identity with the SCN and compare on reconstruction.
    pub fn source_epoch(&self) -> &str {
        &self.identity
    }

    /// Checked before any target state is written for a cycle: a refusal for
    /// cleared database logging must leave a fresh binding untouched.
    pub fn require_logging(engine: &mut OracleEngine) -> Result<(), SourceError> {
        let rows = engine.query(
            "SELECT log_mode, supplemental_log_data_min FROM v$database",
            &[],
            1,
        )?;
        let row = rows.first();
        validate_logging(
            row.and_then(|r| r.first()).and_then(Option::as_deref),
            row.and_then(|r| r.get(1)).and_then(Option::as_deref),
        )
    }

    pub fn new(
        mut engine: OracleEngine,
        data: &mut OracleEngine,
        scope: &[TableRef],
    ) -> Result<Self, SourceError> {
        let dialect = engine.ensure_version()?;
        data.ensure_version()?;
        let identity_sql = "SELECT TO_CHAR(dbid)||':'||TO_CHAR(resetlogs_change#) FROM v$database";
        let identity = engine
            .scalar(identity_sql, &[])?
            .ok_or(SourceError::catalog("logminer_identity"))?;
        if data.scalar(identity_sql, &[])?.as_deref() != Some(&identity) {
            return Err(SourceError::config("logminer_database_mismatch"));
        }
        let cdb = dialect.multitenant != Multitenant::NonCdbOnly
            && engine.scalar("SELECT cdb FROM v$database", &[])?.as_deref() == Some("YES");
        let pdb = if cdb {
            let root = engine.scalar("SELECT SYS_CONTEXT('USERENV','CON_NAME') FROM dual", &[])?;
            let pdb = data
                .scalar("SELECT SYS_CONTEXT('USERENV','CON_NAME') FROM dual", &[])?
                .ok_or(SourceError::catalog("logminer_pdb"))?;
            if root.as_deref() != Some("CDB$ROOT")
                || !engine.session_user.starts_with("C##")
                || pdb == "CDB$ROOT"
            {
                return Err(SourceError::config("logminer_common_user_root_required"));
            }
            Some(pdb)
        } else {
            None
        };
        if scope.is_empty() || scope.len() > 128 {
            return Err(SourceError::config("logminer_empty_scope"));
        }
        let mut tables = BTreeMap::new();
        for table in scope {
            let meta = data.load_meta(table)?;
            if !mine_identifier(&table.schema)
                || !mine_identifier(&table.name)
                || meta.key_cols.is_empty()
                || meta.columns.iter().any(|c| {
                    !mine_identifier(&c.name) || !matches!(c.kind, ColKind::Number | ColKind::Char)
                })
            {
                return Err(SourceError::catalog("logminer_table_shape_unsupported"));
            }
            tables.insert(table.clone(), meta);
        }
        let mut reader = Self {
            engine,
            tables,
            pdb,
            identity,
            rebaseline: BTreeSet::new(),
            retained_floor: 0,
            admitted_scn: 0,
            classified_ddl: false,
            table_positions: BTreeMap::new(),
            scn_window: None,
        };
        reader.check_logging()?;
        let files = reader.inventory()?;
        let high = reader.engine.current_scn()?;
        reader.retained_floor = coverage(&files, high)?;
        let oldest = reader
            .engine
            .scalar("SELECT TO_CHAR(MIN(start_scn)) FROM v$transaction", &[])?;
        let oldest = oldest.as_deref().map(|v| number(Some(v))).transpose()?;
        if oldest.is_some_and(|start| start < reader.retained_floor) {
            return Err(SourceError::catalog("logminer_active_transaction_log_gap"));
        }
        // Retain the oldest active transaction, not unrelated historical logs.
        reader.retained_floor = oldest.map_or(high, |start| start.min(high));
        reader.admitted_scn = reader.engine.current_scn()?;
        Ok(reader)
    }

    pub fn rebaseline_tables(&self) -> &BTreeSet<TableRef> {
        &self.rebaseline
    }

    pub fn last_ora_code(&self) -> Option<usize> {
        self.engine.last_ora_code()
    }

    fn call(&mut self, sql: &str, binds: &[Bind]) -> Result<(), SourceError> {
        if let Some(worker) = &self.engine.worker {
            return worker.logminer_call(sql, binds).map_err(|error| {
                self.engine.last_ora = ora_code(&error);
                map_driver(&error)
            });
        }
        let values = bind_values(binds)?;
        let refs: Vec<&dyn ToDbValue> = values
            .iter()
            .map(|v| match v {
                BindValue::N(n) => n as &dyn ToDbValue,
                BindValue::S(s) => s as &dyn ToDbValue,
            })
            .collect();
        self.engine
            .conn
            .as_ref()
            .unwrap()
            .execute(sql, &refs)
            .map(|_| ())
            .map_err(|e| {
                let error = DriverError::Thin(e);
                self.engine.last_ora = ora_code(&error);
                map_driver(&error)
            })
    }

    fn check_logging(&mut self) -> Result<(), SourceError> {
        // Recreating a controlfile can preserve the tables but clear logging.
        // An empty LogMiner result in that state is not a no-change proof.
        Self::require_logging(&mut self.engine)
    }

    fn inventory(&mut self) -> Result<Vec<LogFile>, SourceError> {
        if self
            .engine
            .scalar(
                "SELECT TO_CHAR(dbid)||':'||TO_CHAR(resetlogs_change#) FROM v$database",
                &[],
            )?
            .as_deref()
            != Some(&self.identity)
        {
            return Err(SourceError::catalog("logminer_lineage_changed_rebaseline"));
        }
        let rows = self.engine.query(LOGS, &[], 4096)?;
        let mut files = BTreeMap::new();
        for row in rows {
            if row.len() != 5 {
                return Err(SourceError::protocol("logminer_log_columns"));
            }
            let file = LogFile {
                name: row[0]
                    .clone()
                    .ok_or(SourceError::protocol("logminer_log_name"))?,
                first: number(row[1].as_deref())?,
                next: number(row[2].as_deref())?,
                thread: number(row[3].as_deref())?,
                sequence: number(row[4].as_deref())?,
            };
            if file.first > 0 && file.next > file.first {
                files.entry((file.thread, file.sequence)).or_insert(file);
            }
        }
        Ok(files.into_values().collect())
    }

    fn add_files(&mut self, files: &[LogFile]) -> Result<(), SourceError> {
        for (i, file) in files.iter().enumerate() {
            let option = if i == 0 { "NEW" } else { "ADDFILE" };
            self.call(&format!(
                "BEGIN DBMS_LOGMNR.ADD_LOGFILE(LOGFILENAME=>:b1,OPTIONS=>DBMS_LOGMNR.{option}); END;"
            ), &[Bind::Text(file.name.clone())])?;
        }
        Ok(())
    }

    fn table_changes(
        &mut self,
        table: &TableRef,
        meta: &TableMeta,
        from: u64,
        floor: u64,
        budget: &ReadBudget,
    ) -> Result<Vec<MinedRow>, SourceError> {
        let mut projections = vec![
            "TO_CHAR(NVL(COMMIT_SCN,SCN))".to_owned(),
            "TO_CHAR(SCN)".into(),
            "RS_ID".into(),
            "TO_CHAR(SSN)".into(),
            "TO_CHAR(OPERATION_CODE)".into(),
            "TO_CHAR(STATUS)".into(),
            "TO_CHAR(START_SCN)".into(),
            "TO_CHAR(CSF)".into(),
        ];
        for c in &meta.columns {
            // The 21c/23ai package resolves this against each mined record.
            // PDB isolation is the SRC_CON_NAME predicate, not a 26ai-style
            // container prefix (23ai rejects that syntax with ORA-00904).
            let path = format!("{}.{}.{}", table.schema, table.name, c.name).replace('\'', "''");
            for image in ["UNDO_VALUE", "REDO_VALUE"] {
                projections.push(format!(
                    "TO_CHAR(DBMS_LOGMNR.COLUMN_PRESENT({image},'{path}'))"
                ));
                let value = format!("DBMS_LOGMNR.MINE_VALUE({image},'{path}')");
                projections.push(if c.kind == ColKind::Number {
                    format!("TO_CHAR(TO_NUMBER({value}),'TM9','NLS_NUMERIC_CHARACTERS=''.,''')")
                } else {
                    value
                });
            }
        }
        let pdb_clause = if self.pdb.is_some() {
            " AND SRC_CON_NAME=:b4"
        } else {
            ""
        };
        let data_only = if self.classified_ddl {
            " AND OPERATION_CODE IN (1,2,3)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT {} FROM v$logmnr_contents WHERE SEG_OWNER=:b1 AND TABLE_NAME=:b2 \
             AND NVL(COMMIT_SCN,SCN)>:b3{pdb_clause}{data_only} ORDER BY COMMIT_SCN,SCN,RS_ID,SSN",
            projections.join(",")
        );
        let mut binds = vec![
            Bind::Text(table.schema.clone()),
            Bind::Text(table.name.clone()),
            Bind::Number(from.to_string()),
        ];
        if let Some(pdb) = &self.pdb {
            binds.push(Bind::Text(pdb.clone()));
        }
        let invalid_operation = if self.classified_ddl {
            "STATUS<>0"
        } else {
            "OPERATION_CODE=5 OR STATUS<>0"
        };
        let invalid = self.engine.scalar(
            &format!(
                "SELECT TO_CHAR(COUNT(*)) FROM v$logmnr_contents \
             WHERE SEG_OWNER=:b1 AND TABLE_NAME=:b2 AND SCN>:b3{pdb_clause} \
             AND ({invalid_operation})"
            ),
            &binds,
        )?;
        if invalid.as_deref() != Some("0") {
            self.rebaseline.insert(table.clone());
            return Err(SourceError::catalog("logminer_table_status_rebaseline"));
        }
        let rows = self
            .engine
            .query(&sql, &binds, budget.max_chunk_rows.min(100_000) as usize)
            .map_err(|error| {
                if matches!(self.engine.last_ora, Some(904 | 1323)) {
                    self.rebaseline.insert(table.clone());
                    SourceError::catalog("logminer_dictionary_rebaseline")
                } else {
                    error
                }
            })?;
        let infos: Vec<_> = meta.columns.iter().map(ColumnInfo::from).collect();
        let schema = Arc::new(chunk_schema(table, &infos));
        let mut output = Vec::new();
        for row in rows {
            let commit = number(row[0].as_deref())?;
            let sequence = number(row[1].as_deref())?;
            let operation = number(row[4].as_deref())?;
            let refusal = if !matches!(operation, 1..=3) {
                Some("logminer_operation_rebaseline")
            } else if row[5].as_deref() != Some("0") {
                Some("logminer_row_status_rebaseline")
            } else if row[7].as_deref() != Some("0") {
                Some("logminer_continuation_rebaseline")
            } else if number(row[6].as_deref()).map_or(true, |start| start < floor) {
                Some("logminer_transaction_floor_rebaseline")
            } else {
                None
            };
            if let Some(refusal) = refusal {
                self.rebaseline.insert(table.clone());
                return Err(SourceError::catalog(refusal));
            }
            let mut before = Vec::new();
            let mut after = Vec::new();
            for (i, c) in meta.columns.iter().enumerate() {
                let offset = 8 + i * 4;
                let old_present = row[offset].as_deref() == Some("1");
                let new_present = row[offset + 2].as_deref() == Some("1");
                if (operation == 2 && !old_present)
                    || (operation != 2 && !old_present && !new_present)
                    || (operation == 3 && meta.key_cols.contains(&i) && !old_present)
                {
                    self.rebaseline.insert(table.clone());
                    return Err(SourceError::catalog("logminer_column_missing_rebaseline"));
                }
                let old = row[offset + 1]
                    .clone()
                    .map(|v| canonical(c.kind, v))
                    .transpose()?;
                let new = if new_present {
                    row[offset + 3]
                        .clone()
                        .map(|v| canonical(c.kind, v))
                        .transpose()?
                } else {
                    old.clone()
                };
                before.push(old);
                after.push(new);
            }
            let key = OracleEngine::row_key(meta, if operation == 2 { &before } else { &after })?;
            let change = match operation {
                1 => RowChange::Insert {
                    table: table.clone(),
                    key,
                },
                2 => RowChange::Delete {
                    table: table.clone(),
                    key,
                },
                _ => {
                    let old = OracleEngine::row_key(meta, &before)?;
                    let old_key = (old != key).then_some(old);
                    RowChange::Update {
                        table: table.clone(),
                        key,
                        old_key,
                    }
                }
            };
            let cells = if operation == 2 { before } else { after };
            let arrays = cells
                .into_iter()
                .zip(schema.fields())
                .map(|(cell, field)| super::super::typed::array(field, vec![cell]))
                .collect::<Result<Vec<_>, _>>()?;
            let image = RecordBatch::try_new(schema.clone(), arrays)
                .map_err(|_| SourceError::protocol("logminer_arrow"))?;
            output.push((
                commit,
                sequence,
                row[2].clone().unwrap_or_default(),
                number(row[3].as_deref())?,
                change,
                image,
            ));
        }
        Ok(output)
    }

    fn read(&mut self, from: u64, budget: &ReadBudget) -> Result<ChangeBatch, SourceError> {
        let deadline = Instant::now() + Duration::from_millis(budget.statement_timeout_ms);
        self.engine
            .set_timeout(Duration::from_millis(budget.statement_timeout_ms))?;
        self.check_logging()?;
        let current = self.engine.current_scn()?;
        if from > current {
            return Err(SourceError::config("logminer_future_watermark"));
        }
        let high = current.saturating_sub(1);
        let files = self.inventory()?;
        let floor = coverage(&files, high)?;
        // Without a durable pre-baseline floor, restart needs a new baseline.
        // Never advance the transaction floor with the commit watermark.
        if from < self.admitted_scn || floor > self.retained_floor {
            return Err(SourceError::catalog("logminer_session_rebaseline"));
        }
        if from < floor {
            return Err(SourceError::catalog("logminer_position_expired_rebaseline"));
        }
        if from == current {
            return Ok(ChangeBatch {
                position: Watermark::OracleScn { scn: from },
                changes: Vec::new(),
                images: Vec::new(),
            });
        }
        let files: Vec<_> = files
            .into_iter()
            .filter(|file| file.next > self.retained_floor && file.first <= high)
            .collect();
        // A cursor that fell behind a busy source (or a first batch after a
        // long baseline) can hold more commits than one batch may carry. Mine
        // a shorter commit-SCN span instead of refusing forever; the batch
        // position is the span's end, so every commit at or below it is whole.
        let mut end = window_end(from, high, self.scn_window);
        // Largest end known to hold no commit, smallest known over budget.
        let (mut empty, mut over) = (from, None);
        loop {
            match self.mine(from, end, &files, deadline, budget) {
                // One table's rows past the limit fail the query itself.
                Err(error)
                    if matches!(
                        error.fact(),
                        Some("logminer_batch_over_budget" | "query_row_limit")
                    ) =>
                {
                    over = Some(end);
                }
                // An empty shorter span proves nothing yet: look further.
                Ok(batch) if batch.changes.is_empty() && end < high => empty = end,
                Ok(batch) => {
                    if over.is_some() {
                        self.scn_window = Some(end - from);
                    } else if (batch.changes.len() as u64).saturating_mul(4) < budget.max_chunk_rows
                    {
                        // Grow back after a light batch so a caught-up cursor
                        // returns to whole spans.
                        self.scn_window = self.scn_window.map(|w| w.saturating_mul(2));
                    }
                    return Ok(batch);
                }
                Err(error) => return Err(error),
            }
            end = next_span(high, empty, over)
                .ok_or(SourceError::budget("logminer_batch_over_budget"))?;
        }
    }

    /// One LogMiner session over committed changes in `(from, high]`.
    fn mine(
        &mut self,
        from: u64,
        high: u64,
        files: &[LogFile],
        deadline: Instant,
        budget: &ReadBudget,
    ) -> Result<ChangeBatch, SourceError> {
        self.add_files(files)?;
        self.call(
            "BEGIN DBMS_LOGMNR.START_LOGMNR(STARTSCN=>:b1,ENDSCN=>:b2,\
             OPTIONS=>DBMS_LOGMNR.DICT_FROM_ONLINE_CATALOG+DBMS_LOGMNR.COMMITTED_DATA_ONLY); END;",
            &[
                Bind::Number(self.retained_floor.to_string()),
                Bind::Number(high.to_string()),
            ],
        )?;
        let result = (|| {
            // Missing redo is not a table event, so check independently of scope.
            let gaps = self.engine.scalar(
                "SELECT TO_CHAR(COUNT(*)) FROM v$logmnr_contents WHERE OPERATION_CODE=34",
                &[],
            )?;
            if gaps.as_deref() != Some("0") {
                return Err(SourceError::catalog("logminer_missing_scn_rebaseline"));
            }
            let mut events = Vec::new();
            let mut bytes = 0usize;
            for (table, meta) in self.tables.clone() {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or(SourceError::budget("logminer_deadline"))?;
                self.engine.set_timeout(remaining)?;
                let table_from =
                    from.max(self.table_positions.get(&table).copied().unwrap_or(from));
                let next =
                    self.table_changes(&table, &meta, table_from, self.retained_floor, budget)?;
                bytes += next
                    .iter()
                    .map(|event| event.5.get_array_memory_size())
                    .sum::<usize>();
                events.extend(next);
                if events.len() as u64 > budget.max_chunk_rows
                    || bytes as u64 > budget.max_chunk_bytes
                {
                    return Err(SourceError::budget("logminer_batch_over_budget"));
                }
            }
            events.sort_by(|a, b| (&a.0, &a.1, &a.2, &a.3).cmp(&(&b.0, &b.1, &b.2, &b.3)));
            if Instant::now() > deadline {
                return Err(SourceError::budget("logminer_deadline"));
            }
            let (changes, images) = events.into_iter().map(|e| (e.4, e.5)).unzip();
            Ok(ChangeBatch {
                position: Watermark::OracleScn { scn: high },
                changes,
                images,
            })
        })();
        let ended = self.call("BEGIN DBMS_LOGMNR.END_LOGMNR; END;", &[]);
        let batch = result?;
        ended?;
        Ok(batch)
    }
}

impl ChangeSource for OracleLogMiner {
    async fn earliest_available(&mut self) -> Result<Watermark, SourceError> {
        let files = self.inventory()?;
        let high = self.engine.current_scn()?.saturating_sub(1);
        let floor = coverage(&files, high)?;
        // ADD_LOGFILE reads the file header; catalog status alone is not proof.
        self.add_files(&files)?;
        Ok(Watermark::OracleScn { scn: floor })
    }

    async fn next_batch(
        &mut self,
        from: &Watermark,
        budget: &ReadBudget,
    ) -> Result<Option<ChangeBatch>, SourceError> {
        validate_budget(budget)?;
        if !self.rebaseline.is_empty() {
            return Err(SourceError::catalog("logminer_table_rebaseline"));
        }
        self.engine.protect().await?;
        let started = Instant::now();
        let result = self.read(scn(from)?, budget);
        self.engine.schedule_next(started.elapsed(), budget);
        self.engine.pace().await;
        result.map(Some)
    }
}

/// End of the next mined span: the whole backlog, or the learned window.
fn window_end(from: u64, high: u64, window: Option<u64>) -> u64 {
    window.map_or(high, |w| high.min(from.saturating_add(w.max(1))))
}

/// Next span end after one probe: bisect between the largest end known to
/// hold no commit and the smallest known over budget, or take the whole
/// remainder when nothing is over budget yet. `None` when one SCN alone (a
/// single oversized commit) exceeds the budget: that refusal stands.
fn next_span(high: u64, empty: u64, over: Option<u64>) -> Option<u64> {
    match over {
        Some(over) => (over.checked_sub(empty)? > 1).then(|| empty + (over - empty) / 2),
        None => (empty < high).then_some(high),
    }
}

fn validate_resume_floor(
    floor: u64,
    from: u64,
    available: u64,
    oldest_active_or_high: u64,
    high: u64,
) -> Result<(), SourceError> {
    if floor == 0
        || floor > from
        || floor < available
        || floor > oldest_active_or_high
        || from > high
    {
        return Err(SourceError::catalog("logminer_resume_floor_rebaseline"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_budget_backlog_is_mined_in_shrinking_commit_spans() {
        assert_eq!(window_end(100, 1_000, None), 1_000);
        assert_eq!(window_end(100, 1_000, Some(50)), 150);
        // The learned window never runs past the current high mark.
        assert_eq!(window_end(990, 1_000, Some(50)), 1_000);
        assert_eq!(window_end(100, 1_000, Some(0)), 101);
        // Over budget: bisect towards the cursor.
        assert_eq!(next_span(1_000, 100, Some(1_000)), Some(550));
        // An empty prefix moves the lower bound; bisect what remains.
        assert_eq!(next_span(1_000, 550, Some(1_000)), Some(775));
        // Nothing over budget yet after an empty learned window: take it all.
        assert_eq!(next_span(1_000, 150, None), Some(1_000));
        assert_eq!(next_span(1_000, 1_000, None), None);
        // One SCN that alone exceeds the budget cannot be split.
        assert_eq!(next_span(1_000, 100, Some(101)), None);
        assert_eq!(next_span(1_000, 100, Some(102)), Some(101));
        // Simulate commits at SCNs 400..=420 (one change each), budget 5.
        let commits: Vec<u64> = (400..=420).collect();
        let mined =
            |from: u64, end: u64| commits.iter().filter(|c| **c > from && **c <= end).count();
        let (from, high) = (100, 1_000);
        let (mut empty, mut over, mut end, mut probes) = (from, None, high, 0);
        loop {
            probes += 1;
            let n = mined(from, end);
            if n > 5 {
                over = Some(end);
            } else if n == 0 && end < high {
                empty = end;
            } else {
                break;
            }
            end = next_span(high, empty, over).unwrap();
        }
        let n = mined(from, end);
        assert!((1..=5).contains(&n), "{n} changes up to {end}");
        assert!(probes <= 20);
    }

    #[test]
    fn cleared_or_unknown_logging_requires_rebaseline() {
        for minimal in [Some("YES"), Some("IMPLICIT")] {
            assert!(validate_logging(Some("ARCHIVELOG"), minimal).is_ok());
        }
        for (mode, minimal) in [
            (Some("NOARCHIVELOG"), Some("YES")),
            (Some("ARCHIVELOG"), Some("NO")),
            (Some("ARCHIVELOG"), None),
            (None, Some("YES")),
        ] {
            assert!(
                validate_logging(mode, minimal)
                    .unwrap_err()
                    .to_string()
                    .contains("logminer_logging_prerequisites_rebaseline")
            );
        }
    }

    fn file(thread: u64, first: u64, next: u64) -> LogFile {
        LogFile {
            name: "fixture".into(),
            thread,
            first,
            next,
            sequence: first,
        }
    }
    #[test]
    fn coverage_rejects_gaps_and_unobserved_rac_transactions() {
        assert_eq!(
            coverage(&[file(1, 10, 20), file(1, 20, 40)], 30).unwrap(),
            10
        );
        assert!(coverage(&[file(1, 10, 40), file(2, 15, 40)], 30).is_err());
        assert!(coverage(&[file(1, 10, 20), file(1, 21, 40)], 30).is_err());
        assert!(coverage(&[file(1, 10, 20)], 20).is_err());
        assert!(coverage(&[], 20).is_err());
    }
    #[test]
    fn positions_reject_other_engines() {
        assert!(scn(&Watermark::PgLsn { lsn: 10 }).is_err());
        assert_eq!(scn(&Watermark::OracleScn { scn: 10 }).unwrap(), 10);
    }

    #[test]
    fn durable_floor_covers_retained_history_and_active_transactions() {
        assert!(validate_resume_floor(10, 20, 5, 15, 30).is_ok());
        for (floor, from, available, active, high) in [
            (0, 20, 0, 15, 30),
            (21, 20, 5, 30, 30),
            (10, 20, 11, 15, 30),
            (16, 20, 5, 15, 30),
            (10, 31, 5, 15, 30),
        ] {
            assert!(validate_resume_floor(floor, from, available, active, high).is_err());
        }
    }

    #[test]
    fn mine_paths_reject_ambiguous_and_quoted_identifiers() {
        assert!(mine_identifier("TABLE_1"));
        for name in ["", "HAS.DOT", "HAS:COLON", "CaseSensitive", "Q'"] {
            assert!(!mine_identifier(name));
        }
    }
}
