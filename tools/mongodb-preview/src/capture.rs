//! Explicit capture gate for this MongoDB source.
//!
//! Standalone topology refuses change-stream CDC. C5 is admitted only when the
//! request names it and discloses grade B, reconcile deletes, and the double
//! `weight` column. This module does not import, call a model, or write a pack.

use serde_json::{Value, json};

/// Approved shipment fields. The order is the customer allowlist.
pub const APPROVED_FIELDS: &[&str] = &[
    "_id",
    "code",
    "flag",
    "units",
    "price",
    "created",
    "uid",
    "blob",
    "blob_presence",
    "weight",
    "warehouse",
    "whCode",
    "lane",
    "memo",
    "memo_presence",
    "marks",
    "marks_presence",
    "pack",
    "pack_presence",
];

/// Customer row budget. A sync request cannot raise or lower it.
pub const APPROVED_BUDGET_ROWS: u64 = 16;

// Each flag is an independent disclosure. Folding them into one enum would hide a missing check.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRequest {
    /// `C1` or `C5`. Other methods stay blocked.
    pub method: String,
    /// Configured detect period. Zero is invalid.
    pub every_ms: u64,
    /// `native` for C1, `reconcile` for C5.
    pub delete_mode: String,
    /// Disclosed grade. C5 without a snapshot token is `B`.
    pub consistency: String,
    /// `Total` for a C1 clusterTime claim, `None` for C5.
    pub token_order: String,
    /// Quality flags the request is willing to show.
    pub quality: Vec<String>,
    /// Hard row budget. Must stay the approved 16.
    pub budget_rows: u64,
    /// Fields the customer approved.
    pub approved_fields: Vec<String>,
    /// Fields the plan would project. Must equal the approved list.
    pub projected_fields: Vec<String>,
    /// `secret://` name. The value is never accepted here.
    pub secret_ref: String,
    /// True only when the named secret file exists. The planner does not open it.
    pub secret_path_present: bool,
    /// `standalone`, `replica`, or `sharded`.
    pub topology: String,
    /// Server-side change-stream privilege observed for the read role.
    pub change_stream_privilege: bool,
    /// Server code from a refused or accepted change-stream probe.
    pub change_stream_server_code: Option<i64>,
    /// Resume token support was checked.
    pub resume_supported: bool,
    /// Oplog window was checked.
    pub oplog_window_known: bool,
    /// A delete event was actually observed.
    pub delete_event_observed: bool,
    /// Keys present on that delete event.
    pub delete_event_keys: Vec<String>,
    /// Transaction metadata (`lsid` / `txnNumber`) was observed.
    pub transaction_metadata: bool,
    /// `R0` for a standalone full diff, `R1` for a DBA-built change stream.
    pub write_class: String,
    /// The request asked for a complete `doc` jsonb column.
    pub fulldoc_requested: bool,
    /// The collection contains a BSON double.
    pub fulldoc_contains_double: bool,
    /// The request claims that jsonb document is complete.
    pub fulldoc_claim_complete: bool,
    /// Field names observed on the collection, including unapproved ones.
    pub observed_fields: Vec<String>,
    /// BSON type names per field. Values are not carried.
    pub field_types: Vec<(String, Vec<String>)>,
    /// Rows the probe counted.
    pub row_count: u64,
    /// Snapshot token. C5 on this source has none.
    pub snapshot_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Finding {
    code: &'static str,
    owner: &'static str,
    interface: &'static str,
    detail: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenGap {
    code: &'static str,
    owner: &'static str,
    also_owner: &'static str,
    interface: &'static str,
    detail: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HiddenField {
    field: String,
    mixed: bool,
}

/// Planner result. `source_acceptance` stays false.
///
/// Each flag is reported on its own so a later reader can see which claim was refused.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncDecision {
    /// `c1_admitted`, `c5_admitted`, `cdc_refused`, or `blocked`.
    pub outcome: String,
    /// Detect period from the request. Admission does not rewrite it.
    pub every_ms: u64,
    /// Admitted method. Refusal and block leave this empty.
    pub capture_method: Option<String>,
    /// True only when outcome is `c1_admitted`.
    pub cdc_claimed: bool,
    /// True only when a change-stream apply may start.
    pub cdc_read_allowed: bool,
    /// True only when an admitted read may start.
    pub read_allowed: bool,
    /// Grade the decision is willing to disclose.
    pub consistency: Option<String>,
    /// Delete mode the decision is willing to disclose.
    pub delete_detection: Option<String>,
    /// Quality flags copied from an admission.
    pub quality: Vec<String>,
    /// This command does not accept the source.
    pub source_acceptance: bool,
    /// This command does not call a model.
    pub model_called: bool,
    /// This command does not call import.
    pub import_invoked: bool,
    /// This command does not create a PostgreSQL generation.
    pub generation_created: bool,
    /// A jsonb document was not written.
    pub fulldoc_produced: bool,
    /// A complete document was not claimed.
    pub fulldoc_complete: bool,
    /// `weight` stayed on the approved float64 projection.
    pub typed_weight_retained: bool,
    /// Approved field count after the equality check.
    pub approved_field_count: usize,
    blocks: Vec<Finding>,
    gaps: Vec<OpenGap>,
    unclassified: Vec<HiddenField>,
}

/// One captured row. `canonical` is the caller’s stable field text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapRow {
    /// Primary key text.
    pub key: String,
    /// Stable projection text.
    pub canonical: String,
}

/// Key diff between two full reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatchUp {
    /// Keys present only in the new read.
    pub inserts: Vec<String>,
    /// Keys present in both reads whose canonical text changed.
    pub updates: Vec<String>,
    /// Keys present only in the previous read.
    pub deletes: Vec<String>,
    /// Keys published from the new read.
    pub published: Vec<String>,
    /// True when the published keys are exactly the new read.
    pub converged: bool,
}

impl SyncDecision {
    /// JSON body written by `--plan-sync`.
    pub fn to_json(&self) -> Value {
        json!({
            "protocol": "oo.mongodb-preview.sync-decision/v1",
            "outcome": self.outcome,
            "every_ms": self.every_ms,
            "capture_method": self.capture_method,
            "cdc_claimed": self.cdc_claimed,
            "cdc_read_allowed": self.cdc_read_allowed,
            "read_allowed": self.read_allowed,
            "consistency": self.consistency,
            "delete_detection": self.delete_detection,
            "quality": self.quality,
            "source_acceptance": self.source_acceptance,
            "model_called": self.model_called,
            "import_invoked": self.import_invoked,
            "generation_created": self.generation_created,
            "fulldoc_produced": self.fulldoc_produced,
            "fulldoc_complete": self.fulldoc_complete,
            "typed_weight_retained": self.typed_weight_retained,
            "approved_field_count": self.approved_field_count,
            "blocks": self.blocks.iter().map(|item| json!({
                "code": item.code,
                "owner": item.owner,
                "interface": item.interface,
                "detail": item.detail,
            })).collect::<Vec<_>>(),
            "gaps": self.gaps.iter().map(|item| json!({
                "code": item.code,
                "owner": item.owner,
                "also_owner": item.also_owner,
                "interface": item.interface,
                "detail": item.detail,
            })).collect::<Vec<_>>(),
            "unclassified": self.unclassified.iter().map(|item| json!({
                "field": item.field,
                "status": "unclassified",
                "visible": false,
                "projected": false,
                "raw_doc_authorized": false,
                "mixed": item.mixed,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Two configured periods are distinct and both positive.
pub fn periods_are_distinct(first_ms: u64, second_ms: u64) -> bool {
    first_ms > 0 && second_ms > 0 && first_ms != second_ms
}

/// Parse a sync request. Missing keys are errors. No capability is defaulted on.
pub fn parse_request(value: &Value) -> Result<SyncRequest, &'static str> {
    if value.get("protocol").and_then(Value::as_str) != Some("oo.mongodb-preview.sync-plan/v1") {
        return Err("protocol");
    }
    let snapshot = match value.get("snapshot_token") {
        None => return Err("snapshot_token"),
        Some(Value::Null) => None,
        Some(Value::String(token)) if !token.is_empty() => Some(token.clone()),
        Some(Value::String(_)) => None,
        _ => return Err("snapshot_token"),
    };
    Ok(SyncRequest {
        method: required_string(value, "method")?,
        every_ms: required_u64(value, "every_ms")?,
        delete_mode: required_string(value, "delete_mode")?,
        consistency: required_string(value, "consistency")?,
        token_order: required_string(value, "token_order")?,
        quality: required_strings(value, "quality")?,
        budget_rows: required_u64(value, "budget_rows")?,
        approved_fields: required_strings(value, "approved_fields")?,
        projected_fields: required_strings(value, "projected_fields")?,
        secret_ref: required_string(value, "secret_ref")?,
        secret_path_present: required_bool(value, "secret_path_present")?,
        topology: required_string(value, "topology")?,
        change_stream_privilege: required_bool(value, "change_stream_privilege")?,
        change_stream_server_code: optional_i64(value, "change_stream_server_code")?,
        resume_supported: required_bool(value, "resume_supported")?,
        oplog_window_known: required_bool(value, "oplog_window_known")?,
        delete_event_observed: required_bool(value, "delete_event_observed")?,
        delete_event_keys: required_strings(value, "delete_event_keys")?,
        transaction_metadata: required_bool(value, "transaction_metadata")?,
        write_class: required_string(value, "write_class")?,
        fulldoc_requested: required_bool(value, "fulldoc_requested")?,
        fulldoc_contains_double: required_bool(value, "fulldoc_contains_double")?,
        fulldoc_claim_complete: required_bool(value, "fulldoc_claim_complete")?,
        observed_fields: required_strings(value, "observed_fields")?,
        field_types: field_types(value)?,
        row_count: required_u64(value, "row_count")?,
        snapshot_token: snapshot,
    })
}

/// Decide the capture method. Explicit C1 is never rewritten as C5.
pub fn plan(request: &SyncRequest) -> SyncDecision {
    let mut blocks = Vec::new();
    let weight_retained = request
        .approved_fields
        .iter()
        .any(|field| field == "weight")
        && request
            .projected_fields
            .iter()
            .any(|field| field == "weight")
        && request.approved_fields == approved_owned()
        && request.projected_fields == approved_owned();
    structural(request, &mut blocks);
    let unclassified = classify(request, &mut blocks);
    fulldoc(request, &mut blocks);
    if !blocks.is_empty() {
        return finish(
            request,
            refused(blocks, unclassified, weight_retained, "blocked"),
        );
    }
    match request.method.as_str() {
        "C1" => plan_c1(request, unclassified, weight_retained),
        "C5" => plan_c5(request, unclassified, weight_retained),
        _ => {
            blocks.push(local("METHOD_UNKNOWN", "method is not C1 or C5"));
            finish(
                request,
                refused(blocks, unclassified, weight_retained, "blocked"),
            )
        }
    }
}

/// Diff two full reads. Published keys are the new read and nothing from the old read is filled back.
pub fn converge(
    before: &[SnapRow],
    source_now: &[SnapRow],
    tombstones: &[String],
) -> Result<CatchUp, &'static str> {
    let previous = index_rows(before)?;
    let current = index_rows(source_now)?;
    for key in tombstones {
        if current.contains_key(key) {
            return Err("resurrected");
        }
    }
    let mut inserts = Vec::new();
    let mut updates = Vec::new();
    let mut deletes = Vec::new();
    let current_keys: Vec<String> = current.keys().cloned().collect();
    for key in &current_keys {
        match previous.get(key) {
            None => inserts.push(key.clone()),
            Some(old) if old != &current[key] => updates.push(key.clone()),
            Some(_) => {}
        }
    }
    for key in previous.keys() {
        if !current.contains_key(key) {
            deletes.push(key.clone());
        }
    }
    inserts.sort();
    updates.sort();
    deletes.sort();
    let mut published: Vec<String> = current.keys().cloned().collect();
    published.sort();
    let leaked = deletes
        .iter()
        .any(|key| published.binary_search(key).is_ok());
    if leaked {
        return Err("resurrected");
    }
    Ok(CatchUp {
        inserts,
        updates,
        deletes,
        published,
        converged: true,
    })
}

/// JSON for `--diff-sync`.
pub fn catch_up_json(caught: &CatchUp) -> Value {
    json!({
        "protocol": "oo.mongodb-preview.sync-diff/v1",
        "inserts": caught.inserts,
        "updates": caught.updates,
        "deletes": caught.deletes,
        "published": caught.published,
        "converged": caught.converged,
        "resurrected": [],
        "import_invoked": false,
        "generation_created": false,
    })
}

fn plan_c1(
    request: &SyncRequest,
    unclassified: Vec<HiddenField>,
    weight_retained: bool,
) -> SyncDecision {
    let mut blocks = Vec::new();
    if request.write_class != "R1" {
        blocks.push(local(
            "WRITE_CLASS",
            "C1 requires disclosed DBA write class R1",
        ));
    }
    if request.every_ms == 0 {
        blocks.push(local("PERIOD_INVALID", "period must be greater than zero"));
    }
    if request.delete_mode != "native" {
        blocks.push(local("DELETE_MODE", "C1 delete mode is native"));
    }
    if request.consistency != "T" || request.token_order != "Total" {
        blocks.push(local(
            "CONSISTENCY_OVERCLAIM",
            "C1 admission here requires grade T and token order Total",
        ));
    }
    if request.quality.iter().any(|item| item == "DELETES_LAGGED") {
        blocks.push(local(
            "QUALITY_MISMATCH",
            "native delete does not use DELETES_LAGGED",
        ));
    }
    if !blocks.is_empty() {
        return finish(
            request,
            refused(blocks, unclassified, weight_retained, "blocked"),
        );
    }
    let code = c1_capability(request);
    if let Some((code, detail)) = code {
        blocks.push(Finding {
            code,
            owner: "DL01-W1-41",
            interface: "mongodb hello + aggregate $changeStream",
            detail,
        });
        return finish(
            request,
            refused(blocks, unclassified, weight_retained, "cdc_refused"),
        );
    }
    if request.fulldoc_contains_double {
        let gap = double_gap();
        return finish(
            request,
            admitted(
                "c1_admitted",
                "C1",
                true,
                vec![gap],
                unclassified,
                weight_retained,
            ),
        );
    }
    finish(
        request,
        admitted(
            "c1_admitted",
            "C1",
            true,
            Vec::new(),
            unclassified,
            weight_retained,
        ),
    )
}

fn c1_capability(request: &SyncRequest) -> Option<(&'static str, &'static str)> {
    if request.topology == "standalone" {
        return Some((
            "CDC_TOPOLOGY_UNSUPPORTED",
            "standalone mongod has no change stream",
        ));
    }
    if request.topology != "replica" && request.topology != "sharded" {
        return Some(("TOPOLOGY_UNKNOWN", "topology is not replica or sharded"));
    }
    if !request.change_stream_privilege {
        return Some((
            "CDC_PRIVILEGE",
            "read role changeStream privilege was not granted",
        ));
    }
    if !request.resume_supported {
        return Some((
            "CDC_RESUME_UNCHECKED",
            "resume token support was not checked",
        ));
    }
    if !request.oplog_window_known {
        return Some(("CDC_OPLOG_UNCHECKED", "oplog window was not checked"));
    }
    if !request.delete_event_observed || request.delete_event_keys.is_empty() {
        return Some((
            "CDC_DELETE_SHAPE_UNCHECKED",
            "delete event shape was not observed",
        ));
    }
    if !request.transaction_metadata {
        return Some((
            "CDC_TRANSACTION_METADATA_ABSENT",
            "transaction metadata was not observed; grade T is not claimed and C5 is not substituted",
        ));
    }
    None
}

fn plan_c5(
    request: &SyncRequest,
    unclassified: Vec<HiddenField>,
    weight_retained: bool,
) -> SyncDecision {
    let mut blocks = Vec::new();
    if request.write_class != "R0" {
        blocks.push(local("WRITE_CLASS", "this C5 path is R0"));
    }
    if request.every_ms == 0 {
        blocks.push(local("PERIOD_INVALID", "period must be greater than zero"));
    }
    if request.delete_mode != "reconcile" {
        blocks.push(local("DELETE_MODE", "C5 delete mode is reconcile"));
    }
    if request.consistency != "B"
        || request.token_order != "None"
        || request.snapshot_token.is_some()
    {
        blocks.push(local(
            "CONSISTENCY_OVERCLAIM",
            "C5 without a snapshot token discloses grade B and token order None",
        ));
    }
    if !request.quality.iter().any(|item| item == "DELETES_LAGGED") {
        blocks.push(local(
            "QUALITY_MISMATCH",
            "reconcile deletes disclose DELETES_LAGGED",
        ));
    }
    if !blocks.is_empty() {
        return finish(request, refused(blocks, unclassified, false, "blocked"));
    }
    let mut gaps = Vec::new();
    if request.fulldoc_contains_double {
        gaps.push(double_gap());
    }
    finish(
        request,
        admitted(
            "c5_admitted",
            "C5",
            false,
            gaps,
            unclassified,
            weight_retained,
        ),
    )
}

fn structural(request: &SyncRequest, blocks: &mut Vec<Finding>) {
    if !request.secret_ref.starts_with("secret://") || request.secret_ref.contains("mongodb://") {
        blocks.push(local("SECRET_REF", "secret ref is not a secret:// name"));
    }
    if !request.secret_path_present {
        blocks.push(local(
            "SECRET_PATH_MISSING",
            "secret path is absent; read stays closed",
        ));
    }
    if request.budget_rows != APPROVED_BUDGET_ROWS {
        blocks.push(local(
            "BUDGET_NOT_APPROVED",
            "row budget is not the approved 16",
        ));
    }
    if request.row_count > request.budget_rows {
        blocks.push(local(
            "SOURCE_BUDGET_EXHAUSTED",
            "row count is above the hard budget",
        ));
    }
    if request.approved_fields != approved_owned() || request.projected_fields != approved_owned() {
        blocks.push(local(
            "FIELD_SET_CHANGED",
            "approved and projected fields must stay the original 19 in order",
        ));
    }
    if request.topology != "standalone"
        && request.topology != "replica"
        && request.topology != "sharded"
    {
        blocks.push(local("TOPOLOGY_UNKNOWN", "topology was not observed"));
    }
}

fn classify(request: &SyncRequest, blocks: &mut Vec<Finding>) -> Vec<HiddenField> {
    let mut hidden = Vec::new();
    for field in &request.observed_fields {
        if request
            .approved_fields
            .iter()
            .any(|approved| approved == field)
        {
            continue;
        }
        let mixed = request
            .field_types
            .iter()
            .find(|(name, _)| name == field)
            .is_some_and(|(_, types)| types.len() > 1);
        hidden.push(HiddenField {
            field: field.clone(),
            mixed,
        });
    }
    for (field, types) in &request.field_types {
        let approved = request.approved_fields.iter().any(|item| item == field);
        if !approved {
            if request.projected_fields.iter().any(|item| item == field) {
                blocks.push(local(
                    "UNCLASSIFIED_PROJECTED",
                    "an unapproved field was projected",
                ));
            }
            continue;
        }
        if !stable_type(field, types) {
            blocks.push(local("TYPE_DRIFT", "an approved field changed type"));
            if types.len() > 1 && request.projected_fields.iter().any(|item| item == field) {
                blocks.push(local(
                    "MIXED_TYPE_PROJECTED",
                    "a mixed field was projected as one scalar",
                ));
            }
        }
    }
    hidden.sort_by(|left, right| left.field.cmp(&right.field));
    hidden.dedup_by(|left, right| left.field == right.field);
    hidden
}

fn stable_type(field: &str, types: &[String]) -> bool {
    if types.is_empty() {
        return true;
    }
    if field == "lane" {
        return types
            .iter()
            .all(|item| item == "string" || item == "long" || item == "int");
    }
    if field == "marks" || field == "pack" || field == "memo" || field == "blob" {
        return true;
    }
    types.len() == 1
}

fn fulldoc(request: &SyncRequest, blocks: &mut Vec<Finding>) {
    if request
        .approved_fields
        .iter()
        .any(|field| field == "weight")
        && !request.fulldoc_contains_double
    {
        blocks.push(shared_block(
            "DOUBLE_CONCEALED",
            "weight is a BSON double and was not disclosed",
        ));
    }
    if request.fulldoc_claim_complete {
        blocks.push(shared_block(
            "FULLDOC_NOT_PRODUCED",
            "this command does not produce a complete jsonb document",
        ));
        if request.fulldoc_contains_double {
            blocks.push(shared_block(
                "PRECISION_LOSS_CONCEALED",
                "a complete jsonb claim would drop or round BSON double weight",
            ));
        }
    }
}

fn double_gap() -> OpenGap {
    OpenGap {
        code: "JSONB_DOUBLE_UNENCODED",
        owner: "DL01-W1-05",
        also_owner: "DL01-W1-06",
        interface: "pg-builder document-profile and oo-pipeline import-native doc jsonb",
        detail: "docs 02 §6.4 keeps the whole document in jsonb and forbids passing Int64 or Decimal128 through float. It gives no exact jsonb encoding for BSON double. weight stays on the float64 column. fulldoc_produced is false and fulldoc_complete is false.",
    }
}

struct Draft {
    outcome: &'static str,
    capture_method: Option<String>,
    cdc_claimed: bool,
    read_allowed: bool,
    blocks: Vec<Finding>,
    gaps: Vec<OpenGap>,
    unclassified: Vec<HiddenField>,
    weight_retained: bool,
}

fn refused(
    blocks: Vec<Finding>,
    unclassified: Vec<HiddenField>,
    weight_retained: bool,
    outcome: &'static str,
) -> Draft {
    Draft {
        outcome,
        capture_method: None,
        cdc_claimed: false,
        read_allowed: false,
        blocks,
        gaps: Vec::new(),
        unclassified,
        weight_retained,
    }
}

fn admitted(
    outcome: &'static str,
    method: &str,
    cdc_claimed: bool,
    gaps: Vec<OpenGap>,
    unclassified: Vec<HiddenField>,
    weight_retained: bool,
) -> Draft {
    Draft {
        outcome,
        capture_method: Some(method.to_string()),
        cdc_claimed,
        read_allowed: true,
        blocks: Vec::new(),
        gaps,
        unclassified,
        weight_retained,
    }
}

fn finish(request: &SyncRequest, draft: Draft) -> SyncDecision {
    let admitted = draft.outcome == "c5_admitted" || draft.outcome == "c1_admitted";
    SyncDecision {
        outcome: draft.outcome.to_string(),
        every_ms: request.every_ms,
        capture_method: draft.capture_method,
        cdc_claimed: draft.cdc_claimed,
        cdc_read_allowed: draft.cdc_claimed,
        read_allowed: draft.read_allowed && draft.blocks.is_empty(),
        consistency: admitted.then(|| request.consistency.clone()),
        delete_detection: admitted.then(|| request.delete_mode.clone()),
        quality: if admitted {
            request.quality.clone()
        } else {
            Vec::new()
        },
        source_acceptance: false,
        model_called: false,
        import_invoked: false,
        generation_created: false,
        fulldoc_produced: false,
        fulldoc_complete: false,
        typed_weight_retained: draft.weight_retained && draft.outcome != "blocked",
        approved_field_count: request.approved_fields.len(),
        blocks: draft.blocks,
        gaps: draft.gaps,
        unclassified: draft.unclassified,
    }
}

fn local(code: &'static str, detail: &'static str) -> Finding {
    Finding {
        code,
        owner: "DL01-W1-41",
        interface: "mongodb-preview --plan-sync",
        detail,
    }
}

fn shared_block(code: &'static str, detail: &'static str) -> Finding {
    Finding {
        code,
        owner: "DL01-W1-05",
        interface: "pg-builder document-profile / oo-pipeline import-native",
        detail,
    }
}

fn approved_owned() -> Vec<String> {
    APPROVED_FIELDS
        .iter()
        .map(|field| (*field).to_string())
        .collect()
}

fn index_rows(
    rows: &[SnapRow],
) -> Result<std::collections::BTreeMap<String, String>, &'static str> {
    let mut map = std::collections::BTreeMap::new();
    for row in rows {
        if row.key.is_empty() || row.canonical.is_empty() {
            return Err("row");
        }
        if map.insert(row.key.clone(), row.canonical.clone()).is_some() {
            return Err("duplicate_key");
        }
    }
    Ok(map)
}

fn required_string(value: &Value, key: &str) -> Result<String, &'static str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or("request")
}

fn required_u64(value: &Value, key: &str) -> Result<u64, &'static str> {
    value.get(key).and_then(Value::as_u64).ok_or("request")
}

fn required_bool(value: &Value, key: &str) -> Result<bool, &'static str> {
    value.get(key).and_then(Value::as_bool).ok_or("request")
}

fn optional_i64(value: &Value, key: &str) -> Result<Option<i64>, &'static str> {
    match value.get(key) {
        None => Err("request"),
        Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_i64().map(Some).ok_or("request"),
        _ => Err("request"),
    }
}

fn required_strings(value: &Value, key: &str) -> Result<Vec<String>, &'static str> {
    let items = value.get(key).and_then(Value::as_array).ok_or("request")?;
    let mut names = Vec::new();
    for item in items {
        names.push(item.as_str().ok_or("request")?.to_string());
    }
    Ok(names)
}

fn field_types(value: &Value) -> Result<Vec<(String, Vec<String>)>, &'static str> {
    let object = value
        .get("field_types")
        .and_then(Value::as_object)
        .ok_or("request")?;
    let mut pairs = Vec::new();
    for (field, types) in object {
        let list = types.as_array().ok_or("request")?;
        let mut names = Vec::new();
        for item in list {
            names.push(item.as_str().ok_or("request")?.to_string());
        }
        names.sort();
        names.dedup();
        pairs.push((field.clone(), names));
    }
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(pairs)
}
