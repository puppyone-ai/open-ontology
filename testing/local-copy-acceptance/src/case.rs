//! Closed input for one acceptance bundle.
//!
//! Parsing reads spool files when a table names them. It does not open a
//! database socket and it does not spawn the producer.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::fail::{fail, Fail};
use crate::json::{self, Val};
use crate::sha::hex_ok;
use crate::typed::cell_ok;

pub const PROTOCOL: &str = "oo.acceptance.local-copy/v1";
pub const PRODUCER_OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a408";
pub const CAMPAIGN_OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a407";
pub const FACT_SINGLE: &str = "repeatable_read_read_only";
pub const FACT_GUARD: &str = "verified_reader_guard";

const MAX_CASE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SPOOL_BYTES: usize = 256 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;
const MAX_SAMPLES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Fixture,
    Live,
    Archive,
}

impl Provenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Provenance::Fixture => "fixture",
            Provenance::Live => "live",
            Provenance::Archive => "archive",
        }
    }

    pub fn is_live(self) -> bool {
        matches!(self, Provenance::Live)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureClass {
    Public,
    Enterprise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Business,
    Guard,
    Monitor,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Business => "business",
            Class::Guard => "guard",
            Class::Monitor => "monitor",
        }
    }

    fn parse(text: &str) -> Result<Class, Fail> {
        match text {
            "business" => Ok(Class::Business),
            "guard" => Ok(Class::Guard),
            "monitor" => Ok(Class::Monitor),
            _ => Err(fail("config", "connection class")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Int8(i64),
    Decimal { scale: u32, text: String },
    Null,
    Time(String),
    Binary(String),
    Text(String),
    Bool(bool),
}

impl Cell {
    pub fn type_name(&self) -> &'static str {
        match self {
            Cell::Int8(_) => "int8",
            Cell::Decimal { .. } => "decimal",
            Cell::Null => "null",
            Cell::Time(_) => "time",
            Cell::Binary(_) => "binary",
            Cell::Text(_) => "text",
            Cell::Bool(_) => "bool",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub cells: BTreeMap<String, Cell>,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub schema: String,
    pub name: String,
    pub key: Vec<String>,
    pub count: u64,
    pub source_rows: Vec<Row>,
    pub target_rows: Vec<Row>,
}

#[derive(Clone, Debug)]
pub struct Conn {
    pub class: Class,
    pub usename: String,
    pub datname: String,
    pub application_name: String,
    pub backend_type: String,
    pub xact_isolation: String,
    pub read_only: bool,
    pub state: String,
    pub pid: i64,
}

#[derive(Clone, Debug)]
pub struct Fingerprint {
    pub system_identifier: String,
    pub postmaster_start: String,
    pub database: String,
}

#[derive(Clone, Debug)]
pub struct Producer {
    pub owner: String,
    pub binary_sha256: String,
    pub binary_present: bool,
    pub invoked: bool,
    pub fixed_single_connection: bool,
    pub source_socket_fact: String,
    pub source_socket_peak: Option<i64>,
    pub exit_code: Option<i64>,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub isolation: String,
    pub read_only: bool,
    pub catalog_snapshot_id: String,
    pub data_snapshot_id: String,
    pub proof: String,
    pub later_count: bool,
    pub opened_second_source_connection: bool,
}

#[derive(Clone, Debug)]
pub struct Writes {
    pub ddl: bool,
    pub dml: bool,
    pub heartbeat: bool,
    pub privilege: bool,
    pub explain: bool,
    pub count_star: bool,
}

#[derive(Clone, Debug)]
pub struct Budgets {
    pub batch_rows: i64,
    pub statement_timeout_ms: i64,
    pub lock_timeout_ms: i64,
    pub snapshot_deadline_ms: i64,
}

#[derive(Clone, Debug)]
pub struct Fault {
    pub kind: String,
    pub mirror_state: String,
    pub consistency: String,
    pub public: bool,
    pub source_backends_after: i64,
    pub target_rows_rewritten: bool,
    pub mixed_snapshot: bool,
}

#[derive(Clone, Debug)]
pub struct Audit {
    pub component: String,
    pub generation: String,
    pub target_instance: String,
    pub source_access_count: i64,
    pub query_id: String,
    pub database: String,
}

#[derive(Clone, Debug)]
pub struct AfterSeal {
    pub source_secret_in_subsequent_process: bool,
    pub public_fixture_source_blocked: bool,
    pub enterprise_stop_requested: bool,
    pub network_to_source: bool,
    pub generation: String,
    pub audits: Vec<Audit>,
}

#[derive(Clone, Debug)]
pub struct Campaign {
    pub owner: String,
    pub repeated_here: bool,
    pub reused_artifact_sha256: String,
}

#[derive(Clone, Debug)]
pub struct Case {
    pub provenance: Provenance,
    pub fixture_class: FixtureClass,
    pub producer: Producer,
    pub source: Fingerprint,
    pub target: Fingerprint,
    pub snapshot: Snapshot,
    pub writes: Writes,
    pub budgets: Budgets,
    pub samples: Vec<Vec<Conn>>,
    pub tables: Vec<Table>,
    pub faults: Vec<Fault>,
    pub after_seal: AfterSeal,
    pub campaign: Campaign,
}

pub fn parse_case(text: &str) -> Result<Case, Fail> {
    if text.len() > MAX_CASE_BYTES {
        return Err(fail("bound", "case file"));
    }
    let value = json::parse(text)?;
    let obj = object(&value)?;
    exact(
        obj,
        &[
            "protocol",
            "provenance",
            "fixture_class",
            "customer_complete",
            "producer",
            "fingerprints",
            "snapshot",
            "writes",
            "budgets",
            "samples",
            "tables",
            "faults",
            "after_seal",
            "mp02_06",
        ],
    )?;
    if req_str(obj, "protocol")? != PROTOCOL {
        return Err(fail("config", "protocol"));
    }
    if req_bool(obj, "customer_complete")? {
        return Err(fail("config", "customer_complete"));
    }
    let mut case = Case {
        provenance: provenance(req_str(obj, "provenance")?)?,
        fixture_class: fixture_class(req_str(obj, "fixture_class")?)?,
        producer: producer(req(obj, "producer")?)?,
        source: fingerprint(req(req_obj(obj, "fingerprints")?, "source")?)?,
        target: fingerprint(req(req_obj(obj, "fingerprints")?, "target")?)?,
        snapshot: snapshot(req(obj, "snapshot")?)?,
        writes: writes(req(obj, "writes")?)?,
        budgets: budgets(req(obj, "budgets")?)?,
        samples: samples(req(obj, "samples")?)?,
        tables: Vec::new(),
        faults: faults(req(obj, "faults")?)?,
        after_seal: after_seal(req(obj, "after_seal")?)?,
        campaign: campaign(req(obj, "mp02_06")?)?,
    };
    exact(object(req(obj, "fingerprints")?)?, &["source", "target"])?;
    case.tables = tables(req(obj, "tables")?)?;
    let mut source_total = 0_usize;
    let mut target_total = 0_usize;
    for table in &case.tables {
        source_total = source_total.saturating_add(table.source_rows.len());
        target_total = target_total.saturating_add(table.target_rows.len());
    }
    // The 100k ceiling is per side. A reconcile has to hold the sealed spool and the target catalog together.
    if source_total > MAX_ROWS || target_total > MAX_ROWS {
        return Err(fail("bound", "row count"));
    }
    Ok(case)
}

fn tables(value: &Val) -> Result<Vec<Table>, Fail> {
    let items = array(value)?;
    if items.len() > 32 {
        return Err(fail("bound", "tables"));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(table(item)?);
    }
    Ok(out)
}

fn table(value: &Val) -> Result<Table, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "schema",
            "name",
            "key",
            "count",
            "source_rows",
            "target_rows",
            "source_spool",
            "target_catalog",
        ],
    )?;
    let count = req_int(obj, "count")?;
    if count < 0 {
        return Err(fail("config", "count"));
    }
    let key = string_list(req(obj, "key")?)?;
    if key.is_empty() || key.len() > 16 {
        return Err(fail("config", "key"));
    }
    let mut seen = BTreeMap::new();
    for name in &key {
        if seen.insert(name.clone(), ()).is_some() {
            return Err(fail("config", "key"));
        }
    }
    let source_spool = req_str(obj, "source_spool")?;
    let target_catalog = req_str(obj, "target_catalog")?;
    let inline_source = array(req(obj, "source_rows")?)?;
    let inline_target = array(req(obj, "target_rows")?)?;
    let (source_rows, target_rows) = if source_spool.is_empty() && target_catalog.is_empty() {
        (rows(inline_source)?, rows(inline_target)?)
    } else if !source_spool.is_empty()
        && !target_catalog.is_empty()
        && inline_source.is_empty()
        && inline_target.is_empty()
    {
        (read_spool(source_spool)?, read_spool(target_catalog)?)
    } else {
        return Err(fail("config", "rows_source"));
    };
    Ok(Table {
        schema: req_str(obj, "schema")?.to_string(),
        name: req_str(obj, "name")?.to_string(),
        key,
        count: count as u64,
        source_rows,
        target_rows,
    })
}

fn rows(items: &[Val]) -> Result<Vec<Row>, Fail> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(row(item)?);
    }
    Ok(out)
}

fn row(value: &Val) -> Result<Row, Fail> {
    let obj = object(value)?;
    let mut cells = BTreeMap::new();
    for (name, cell) in obj {
        if cells.insert(name.clone(), parse_cell(cell)?).is_some() {
            return Err(fail("duplicate_key", "column"));
        }
    }
    Ok(Row { cells })
}

fn parse_cell(value: &Val) -> Result<Cell, Fail> {
    let obj = object(value)?;
    let kind = req_str(obj, "t")?;
    let cell = match kind {
        "null" => {
            exact(obj, &["t"])?;
            Cell::Null
        }
        "int8" => {
            exact(obj, &["t", "v"])?;
            let text = req_str(obj, "v")?;
            let number = text
                .parse::<i64>()
                .map_err(|_| fail("typed_cell", "int8"))?;
            if number.to_string() != text {
                return Err(fail("typed_cell", "int8"));
            }
            Cell::Int8(number)
        }
        "decimal" => {
            exact(obj, &["t", "scale", "v"])?;
            let scale = req_int(obj, "scale")?;
            if !(0..=38).contains(&scale) {
                return Err(fail("typed_cell", "decimal"));
            }
            Cell::Decimal {
                scale: scale as u32,
                text: req_str(obj, "v")?.to_string(),
            }
        }
        "time" => {
            exact(obj, &["t", "v"])?;
            Cell::Time(req_str(obj, "v")?.to_string())
        }
        "binary" => {
            exact(obj, &["t", "v"])?;
            Cell::Binary(req_str(obj, "v")?.to_string())
        }
        "text" => {
            exact(obj, &["t", "v"])?;
            Cell::Text(req_str(obj, "v")?.to_string())
        }
        "bool" => {
            exact(obj, &["t", "v"])?;
            Cell::Bool(req_bool(obj, "v")?)
        }
        _ => return Err(fail("typed_cell", "type")),
    };
    cell_ok(&cell).map_err(|detail| fail("typed_cell", detail))?;
    Ok(cell)
}

fn read_spool(path: &str) -> Result<Vec<Row>, Fail> {
    let file = Path::new(path);
    if !file.is_absolute() {
        return Err(fail("config", "spool path"));
    }
    let meta = fs::symlink_metadata(file).map_err(|_| fail("config", "spool path"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(fail("config", "spool path"));
    }
    if meta.len() > MAX_SPOOL_BYTES as u64 {
        return Err(fail("bound", "spool"));
    }
    let bytes = fs::read(file).map_err(|_| fail("config", "spool path"))?;
    if bytes.len() > MAX_SPOOL_BYTES {
        return Err(fail("bound", "spool"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| fail("spool", "utf8"))?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() || lines.iter().any(|line| line.is_empty()) {
        return Err(fail("spool", "blank line"));
    }
    if lines.len() > MAX_ROWS {
        return Err(fail("bound", "spool rows"));
    }
    let mut rows = Vec::with_capacity(lines.len());
    for line in lines {
        let value = json::parse(line).map_err(|_| fail("spool", "line"))?;
        rows.push(row(&value)?);
    }
    Ok(rows)
}

fn samples(value: &Val) -> Result<Vec<Vec<Conn>>, Fail> {
    let items = array(value)?;
    if items.len() > MAX_SAMPLES {
        return Err(fail("bound", "samples"));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let row = array(item)?;
        if row.len() > 100 {
            return Err(fail("bound", "sample width"));
        }
        let mut sample = Vec::with_capacity(row.len());
        for conn in row {
            sample.push(connection(conn)?);
        }
        out.push(sample);
    }
    Ok(out)
}

fn connection(value: &Val) -> Result<Conn, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "class",
            "usename",
            "datname",
            "application_name",
            "backend_type",
            "xact_isolation",
            "read_only",
            "state",
            "pid",
        ],
    )?;
    let pid = req_int(obj, "pid")?;
    if pid <= 0 {
        return Err(fail("config", "connection attribute"));
    }
    Ok(Conn {
        class: Class::parse(req_str(obj, "class")?)?,
        usename: req_str(obj, "usename")?.to_string(),
        datname: req_str(obj, "datname")?.to_string(),
        application_name: req_str(obj, "application_name")?.to_string(),
        backend_type: req_str(obj, "backend_type")?.to_string(),
        xact_isolation: req_str(obj, "xact_isolation")?.to_string(),
        read_only: req_bool(obj, "read_only")?,
        state: req_str(obj, "state")?.to_string(),
        pid,
    })
}

fn producer(value: &Val) -> Result<Producer, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "owner",
            "binary_sha256",
            "binary_present",
            "invoked",
            "fixed_single_connection",
            "source_socket_fact",
            "source_socket_peak",
            "exit_code",
            "argv",
        ],
    )?;
    let binary_sha256 = req_str(obj, "binary_sha256")?.to_string();
    if !binary_sha256.is_empty() && !hex_ok(&binary_sha256) {
        return Err(fail("config", "binary_sha256"));
    }
    let argv = string_list(req(obj, "argv")?)?;
    if argv.len() > 32
        || argv
            .iter()
            .any(|arg| arg.len() > 4096 || arg.contains('\n'))
    {
        return Err(fail("config", "argv"));
    }
    Ok(Producer {
        owner: req_str(obj, "owner")?.to_string(),
        binary_sha256,
        binary_present: req_bool(obj, "binary_present")?,
        invoked: req_bool(obj, "invoked")?,
        fixed_single_connection: req_bool(obj, "fixed_single_connection")?,
        source_socket_fact: req_str(obj, "source_socket_fact")?.to_string(),
        source_socket_peak: opt_int(req(obj, "source_socket_peak")?)?,
        exit_code: opt_int(req(obj, "exit_code")?)?,
        argv,
    })
}

fn fingerprint(value: &Val) -> Result<Fingerprint, Fail> {
    let obj = object(value)?;
    exact(obj, &["system_identifier", "postmaster_start", "database"])?;
    Ok(Fingerprint {
        system_identifier: req_str(obj, "system_identifier")?.to_string(),
        postmaster_start: req_str(obj, "postmaster_start")?.to_string(),
        database: req_str(obj, "database")?.to_string(),
    })
}

fn snapshot(value: &Val) -> Result<Snapshot, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "isolation",
            "read_only",
            "catalog_snapshot_id",
            "data_snapshot_id",
            "proof",
            "later_count",
            "opened_second_source_connection",
        ],
    )?;
    Ok(Snapshot {
        isolation: req_str(obj, "isolation")?.to_string(),
        read_only: req_bool(obj, "read_only")?,
        catalog_snapshot_id: req_str(obj, "catalog_snapshot_id")?.to_string(),
        data_snapshot_id: req_str(obj, "data_snapshot_id")?.to_string(),
        proof: req_str(obj, "proof")?.to_string(),
        later_count: req_bool(obj, "later_count")?,
        opened_second_source_connection: req_bool(obj, "opened_second_source_connection")?,
    })
}

fn writes(value: &Val) -> Result<Writes, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "ddl",
            "dml",
            "heartbeat",
            "privilege",
            "explain",
            "count_star",
        ],
    )?;
    Ok(Writes {
        ddl: req_bool(obj, "ddl")?,
        dml: req_bool(obj, "dml")?,
        heartbeat: req_bool(obj, "heartbeat")?,
        privilege: req_bool(obj, "privilege")?,
        explain: req_bool(obj, "explain")?,
        count_star: req_bool(obj, "count_star")?,
    })
}

fn budgets(value: &Val) -> Result<Budgets, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "batch_rows",
            "statement_timeout_ms",
            "lock_timeout_ms",
            "snapshot_deadline_ms",
        ],
    )?;
    Ok(Budgets {
        batch_rows: req_int(obj, "batch_rows")?,
        statement_timeout_ms: req_int(obj, "statement_timeout_ms")?,
        lock_timeout_ms: req_int(obj, "lock_timeout_ms")?,
        snapshot_deadline_ms: req_int(obj, "snapshot_deadline_ms")?,
    })
}

fn faults(value: &Val) -> Result<Vec<Fault>, Fail> {
    let items = array(value)?;
    if items.len() > 16 {
        return Err(fail("bound", "faults"));
    }
    let mut out = Vec::with_capacity(items.len());
    let mut seen = BTreeMap::new();
    for item in items {
        let fault = fault_item(item)?;
        if seen.insert(fault.kind.clone(), ()).is_some() {
            return Err(fail("config", "fault kind"));
        }
        out.push(fault);
    }
    Ok(out)
}

fn fault_item(value: &Val) -> Result<Fault, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "kind",
            "mirror_state",
            "consistency",
            "public",
            "source_backends_after",
            "target_rows_rewritten",
            "mixed_snapshot",
        ],
    )?;
    let kind = req_str(obj, "kind")?;
    if !matches!(
        kind,
        "partial" | "cancel" | "snapshot_disconnect" | "restart" | "idempotency" | "mixed_version"
    ) {
        return Err(fail("config", "fault kind"));
    }
    let mirror_state = req_str(obj, "mirror_state")?;
    if !matches!(mirror_state, "complete" | "incomplete" | "refused") {
        return Err(fail("config", "fault state"));
    }
    let consistency = req_str(obj, "consistency")?;
    if !matches!(consistency, "single_snapshot" | "not_a_snapshot") {
        return Err(fail("config", "fault consistency"));
    }
    Ok(Fault {
        kind: kind.to_string(),
        mirror_state: mirror_state.to_string(),
        consistency: consistency.to_string(),
        public: req_bool(obj, "public")?,
        source_backends_after: req_int(obj, "source_backends_after")?,
        target_rows_rewritten: req_bool(obj, "target_rows_rewritten")?,
        mixed_snapshot: req_bool(obj, "mixed_snapshot")?,
    })
}

fn after_seal(value: &Val) -> Result<AfterSeal, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "source_secret_in_subsequent_process",
            "public_fixture_source_blocked",
            "enterprise_stop_requested",
            "network_to_source",
            "generation",
            "audits",
        ],
    )?;
    let items = array(req(obj, "audits")?)?;
    if items.len() > 8 {
        return Err(fail("bound", "audits"));
    }
    let mut audits = Vec::with_capacity(items.len());
    let mut seen = BTreeMap::new();
    for item in items {
        let audit = audit(item)?;
        if seen.insert(audit.component.clone(), ()).is_some() {
            return Err(fail("config", "audit component"));
        }
        audits.push(audit);
    }
    Ok(AfterSeal {
        source_secret_in_subsequent_process: req_bool(obj, "source_secret_in_subsequent_process")?,
        public_fixture_source_blocked: req_bool(obj, "public_fixture_source_blocked")?,
        enterprise_stop_requested: req_bool(obj, "enterprise_stop_requested")?,
        network_to_source: req_bool(obj, "network_to_source")?,
        generation: req_str(obj, "generation")?.to_string(),
        audits,
    })
}

fn audit(value: &Val) -> Result<Audit, Fail> {
    let obj = object(value)?;
    exact(
        obj,
        &[
            "component",
            "generation",
            "target_instance",
            "source_access_count",
            "query_id",
            "database",
        ],
    )?;
    let component = req_str(obj, "component")?;
    if !matches!(component, "405" | "397") {
        return Err(fail("config", "audit component"));
    }
    Ok(Audit {
        component: component.to_string(),
        generation: req_str(obj, "generation")?.to_string(),
        target_instance: req_str(obj, "target_instance")?.to_string(),
        source_access_count: req_int(obj, "source_access_count")?,
        query_id: req_str(obj, "query_id")?.to_string(),
        database: req_str(obj, "database")?.to_string(),
    })
}

fn campaign(value: &Val) -> Result<Campaign, Fail> {
    let obj = object(value)?;
    exact(obj, &["owner", "repeated_here", "reused_artifact_sha256"])?;
    let reused = req_str(obj, "reused_artifact_sha256")?;
    if !reused.is_empty() && !hex_ok(reused) {
        return Err(fail("config", "reused hash"));
    }
    Ok(Campaign {
        owner: req_str(obj, "owner")?.to_string(),
        repeated_here: req_bool(obj, "repeated_here")?,
        reused_artifact_sha256: reused.to_string(),
    })
}

fn provenance(text: &str) -> Result<Provenance, Fail> {
    match text {
        "fixture" => Ok(Provenance::Fixture),
        "live" => Ok(Provenance::Live),
        "archive" => Ok(Provenance::Archive),
        _ => Err(fail("config", "provenance")),
    }
}

fn fixture_class(text: &str) -> Result<FixtureClass, Fail> {
    match text {
        "public" => Ok(FixtureClass::Public),
        "enterprise" => Ok(FixtureClass::Enterprise),
        _ => Err(fail("config", "fixture_class")),
    }
}

fn string_list(value: &Val) -> Result<Vec<String>, Fail> {
    let items = array(value)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Val::String(text) => out.push(text.clone()),
            _ => return Err(fail("config", "string list")),
        }
    }
    Ok(out)
}

fn object(value: &Val) -> Result<&[(String, Val)], Fail> {
    match value {
        Val::Object(pairs) => Ok(pairs),
        _ => Err(fail("malformed", "expected object")),
    }
}

fn array(value: &Val) -> Result<&[Val], Fail> {
    match value {
        Val::Array(items) => Ok(items),
        _ => Err(fail("malformed", "expected array")),
    }
}

fn exact(obj: &[(String, Val)], keys: &[&str]) -> Result<(), Fail> {
    if obj.len() != keys.len()
        || keys
            .iter()
            .any(|key| obj.iter().all(|(name, _)| name != key))
    {
        return Err(fail("config", "unknown or missing key"));
    }
    Ok(())
}

fn req<'a>(obj: &'a [(String, Val)], key: &str) -> Result<&'a Val, Fail> {
    obj.iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| fail("config", "missing key"))
}

fn req_obj<'a>(obj: &'a [(String, Val)], key: &str) -> Result<&'a [(String, Val)], Fail> {
    object(req(obj, key)?)
}

fn req_str<'a>(obj: &'a [(String, Val)], key: &str) -> Result<&'a str, Fail> {
    match req(obj, key)? {
        Val::String(text) => Ok(text),
        _ => Err(fail("config", "expected string")),
    }
}

fn req_bool(obj: &[(String, Val)], key: &str) -> Result<bool, Fail> {
    match req(obj, key)? {
        Val::Bool(value) => Ok(*value),
        _ => Err(fail("config", "expected bool")),
    }
}

fn req_int(obj: &[(String, Val)], key: &str) -> Result<i64, Fail> {
    match req(obj, key)? {
        Val::Int(value) => Ok(*value),
        _ => Err(fail("config", "expected integer")),
    }
}

fn opt_int(value: &Val) -> Result<Option<i64>, Fail> {
    match value {
        Val::Null => Ok(None),
        Val::Int(number) => Ok(Some(*number)),
        _ => Err(fail("config", "expected integer or null")),
    }
}
