//! Metric template compiler and independent control-total check.
//!
//! `owner_authenticated`, `publication`, and `full_gates` stay false.
//! Certification requires an evaluate where G5a and G5b both passed.

#![forbid(unsafe_code)]

mod adapter;
mod decimal_cmp;
mod enums;
mod numeric;
mod probe;
mod report;
mod template;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use oo_core::oos::{AnchorName, CivilDate, Metric, MetricStatus, parse_metrics_yaml};
use oo_core::ExactDecimal;
use oo_gateway::{
    LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator, VisibleColumn,
    VisibleRelation,
};
use serde::Deserialize;

use adapter::{Bind, Cell, ExplainOut, Fetched, Session, StepError};
use probe::Kind;
use report::{ClassReport, Gate, QueryReport, Report, TestReport, seal, sha256_hex};

pub use report::Report as MetricReport;

const STDIN_MAX: usize = 2_097_152;

#[derive(Debug)]
pub struct Exit {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn argv_malformed(args: &[String]) -> Option<&'static str> {
    match args {
        [] => Some("missing_command"),
        [command] if command == "compile" => None,
        [command] if command == "evaluate" => Some("config"),
        [command, flag] if command == "evaluate" && flag == "--config" => Some("config"),
        [command, flag, path] if command == "evaluate" && flag == "--config" && path.is_empty() => Some("config"),
        [command, flag, path] if command == "evaluate" && flag == "--config" && !path.is_empty() => None,
        [command, flag] if command == "evaluate" => match flag.strip_prefix("--config=") {
            Some(path) if !path.is_empty() => None,
            _ => Some("config"),
        },
        [command, ..] if command == "compile" || command == "evaluate" => Some("extra_arg"),
        _ => Some("missing_command"),
    }
}

pub async fn dispatch(args: &[String], stdin: &[u8]) -> Exit {
    if let Some(reason) = argv_malformed(args) {
        return malformed(reason);
    }
    match args {
        [command] if command == "compile" => compile_bytes(stdin),
        [command, _, path] if command == "evaluate" => evaluate_path(stdin, Path::new(path)).await,
        [command, flag] if command == "evaluate" => {
            let path = flag.strip_prefix("--config=").unwrap_or("");
            evaluate_path(stdin, Path::new(path)).await
        }
        _ => malformed("missing_command"),
    }
}

pub fn compile_bytes(stdin: &[u8]) -> Exit {
    let request = match parse_request(stdin) {
        Ok(request) => request,
        Err(reason) => return malformed(reason),
    };
    match analyze(&request) {
        Ok(analysis) => compiled_exit(analysis),
        Err(exit_report) => refused(exit_report),
    }
}

async fn evaluate_path(stdin: &[u8], path: &Path) -> Exit {
    let request = match parse_request(stdin) {
        Ok(request) => request,
        Err(reason) => return malformed(reason),
    };
    let analysis = match analyze(&request) {
        Ok(analysis) => analysis,
        Err(exit_report) => return refused(exit_report),
    };
    let config = match read_config(path) {
        Ok(config) => config,
        Err(reason) => return malformed(reason),
    };
    let password = match read_secret(&config.password_file, 4096, true) {
        Ok(password) => password,
        Err(reason) => return malformed(reason),
    };
    let ca_pem = match read_bytes(Path::new(&config.ca_file), 1_048_576) {
        Ok(bytes) => bytes,
        Err(()) => return malformed("ca_file"),
    };
    let report = execute(analysis, &config, &password, &ca_pem).await;
    json_exit(0, &report, "")
}

fn compiled_exit(mut analysis: Analysis) -> Exit {
    analysis.report.outcome = "compiled".to_owned();
    let (status, reason) = report::fold_status(&[
        ("passed", "ok"),
        ("passed", "ok"),
        ("passed", "ok"),
        ("passed", "ok"),
        ("block", "not_executed"),
        ("block", "not_executed"),
    ]);
    analysis.report.gates = vec![
        Gate {
            gate_id: "G5a".to_owned(),
            status: status.to_owned(),
            reason: reason.to_owned(),
        },
        Gate {
            gate_id: "G5b".to_owned(),
            status: "block".to_owned(),
            reason: "not_executed".to_owned(),
        },
    ];
    analysis.report.reason = reason.to_owned();
    analysis.report.metric_certified = false;
    seal(&mut analysis.report);
    json_exit(0, &analysis.report, "")
}

fn refused(mut report: Report) -> Exit {
    let reason = report.reason.clone();
    seal(&mut report);
    json_exit(2, &report, &format!("metric-reconcile: refused {reason}\n"))
}

fn malformed(reason: &'static str) -> Exit {
    Exit {
        code: 1,
        stdout: Vec::new(),
        stderr: format!("metric-reconcile: malformed {reason}\n").into_bytes(),
    }
}

fn json_exit(code: i32, report: &Report, stderr: &str) -> Exit {
    Exit {
        code,
        stdout: serde_json::to_vec(report).expect("report serializes"),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDoc {
    metric_yaml: String,
    #[serde(default)]
    previous_metric_yaml: Option<String>,
    enums: Vec<String>,
    as_of: String,
    identity: IdentityDoc,
    anchor: AnchorDoc,
    anchor_bound: Vec<String>,
    source: String,
    parameters: BTreeMap<String, ParamDoc>,
    catalog: CatalogDoc,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityDoc {
    project: String,
    role: String,
    policy_id: String,
    knowledge_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchorDoc {
    name: String,
    closed: bool,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParamDoc {
    #[serde(rename = "type")]
    type_name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogDoc {
    relations: Vec<RelationDoc>,
    functions: Vec<FunctionDoc>,
    operators: Vec<OperatorDoc>,
    literal_types: LiteralDoc,
    null_test_allowed: bool,
    large_table_row_threshold: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationDoc {
    schema: String,
    name: String,
    physical_schema: String,
    physical_name: String,
    estimated_rows: u64,
    pending_unclassified: u32,
    columns: Vec<ColumnDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnDoc {
    name: String,
    physical_name: String,
    type_oid: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionDoc {
    oid: u32,
    schema: String,
    name: String,
    argument_type_oids: Vec<u32>,
    aggregate_star: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperatorDoc {
    oid: u32,
    procedure_oid: u32,
    schema: String,
    name: String,
    left_type_oid: u32,
    right_type_oid: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiteralDoc {
    integer: u32,
    text: u32,
    boolean: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigDoc {
    host: String,
    port: u16,
    database: String,
    user: String,
    password_file: String,
    ca_file: String,
    statement_timeout_ms: u32,
    max_result_bytes: u32,
}

struct Analysis {
    report: Report,
    metric_sql: String,
    metric_binds: Vec<Bind>,
    control_sql: String,
    control_binds: Vec<Bind>,
    metric_names: Vec<String>,
    kind_of: BTreeMap<String, Kind>,
    test_values: Vec<BTreeMap<String, String>>,
    abs: Option<ExactDecimal>,
    rel: Option<ExactDecimal>,
    role: String,
    timeout_ms: u32,
    max_bytes: usize,
}

fn parse_request(stdin: &[u8]) -> Result<RequestDoc, &'static str> {
    if stdin.is_empty() {
        return Err("empty_stdin");
    }
    if stdin.len() > STDIN_MAX {
        return Err("stdin_size");
    }
    let request: RequestDoc = serde_json::from_slice(stdin).map_err(serde_reason)?;
    if let Some(date) = &request.anchor.date {
        CivilDate::new(date.clone()).map_err(|_| "json")?;
    }
    if !plain(&request.source) || !plain(&request.as_of) {
        return Err("json");
    }
    serde_json::from_value::<AnchorName>(serde_json::Value::String(request.anchor.name.clone()))
        .map_err(|_| "json")?;
    Ok(request)
}

fn serde_reason(error: serde_json::Error) -> &'static str {
    if error.to_string().contains("unknown field") {
        "unknown_field"
    } else {
        "json"
    }
}

#[allow(clippy::result_large_err)]
fn analyze(request: &RequestDoc) -> Result<Analysis, Report> {
    let mut report = report::blank();
    report.provenance.project = request.identity.project.clone();
    report.provenance.role = request.identity.role.clone();
    report.provenance.policy_id = request.identity.policy_id.clone();
    report.provenance.knowledge_id = request.identity.knowledge_id.clone();
    report.provenance.source = request.source.clone();
    report.provenance.anchor_name = request.anchor.name.clone();
    report.provenance.anchor_date = request.anchor.date.clone();
    report.provenance.password_logged = false;
    if !plain(&request.identity.project)
        || !plain(&request.identity.policy_id)
        || !plain(&request.identity.knowledge_id)
        || !role_ok(&request.identity.role)
    {
        return Err(fail(report, "identity", false));
    }
    let metrics = parse_metrics_yaml(&request.metric_yaml).map_err(|_| ()) ;
    let metrics = match metrics {
        Ok(file) => file,
        Err(_) => return Err(fail(report, "metric_count", false)),
    };
    if metrics.metrics.len() != 1 {
        return Err(fail(report, "metric_count", false));
    }
    let metric = &metrics.metrics[0];
    fill_lifecycle(&mut report, metric);
    report.metric_query.raw_sql_sha256 = sha256_hex(metric.sql.as_str().as_bytes());
    report.semantic_diff.metric_sql_sha256 = report.metric_query.raw_sql_sha256.clone();
    let Some(control) = &metric.control_total else {
        return Err(fail(report, "control_total", false));
    };
    report.control_query.raw_sql_sha256 = sha256_hex(control.sql.as_str().as_bytes());
    if !request.anchor.closed {
        return Err(fail(report, "anchor_open", false));
    }
    let Some(anchor_date) = &request.anchor.date else {
        return Err(fail(report, "gap_anchor", false));
    };
    if request.anchor.name != control.as_of.as_str() {
        return Err(fail(report, "anchor_name", false));
    }
    if request.as_of != *anchor_date {
        return Err(fail(report, "as_of_anchor", false));
    }
    if request.source != control.source.as_str() {
        return Err(fail(report, "source_mismatch", false));
    }
    let (abs, rel) = match tolerances(control) {
        Ok(pair) => pair,
        Err(reason) => return Err(fail(report, reason, false)),
    };
    report.comparison.tolerance_abs = abs.as_ref().map(|value| value.to_plain_string());
    report.comparison.tolerance_rel = rel.as_ref().map(|value| value.to_plain_string());
    let metric_template = match template::compile_template(metric.sql.as_str()) {
        Ok(templated) => templated,
        Err(_) => return Err(fail(report, "unsupported_template", false)),
    };
    let control_template = match template::compile_template(control.sql.as_str()) {
        Ok(templated) => templated,
        Err(_) => return Err(fail(report, "unsupported_template", false)),
    };
    report.metric_query.bound_sql = metric_template.bound_sql.clone();
    report.control_query.bound_sql = control_template.bound_sql.clone();
    let integer_oid = request.catalog.literal_types.integer;
    let kind_of = match parameter_kinds(&request.parameters, integer_oid) {
        Ok(kinds) => kinds,
        Err(reason) => return Err(fail(report, reason, false)),
    };
    if let Err(reason) = check_names(request, metric, &metric_template.names, &control_template.names, anchor_date, &kind_of)
    {
        return Err(fail(report, reason, false));
    }
    let test_values = match test_values(metric, &metric_template.names, &kind_of, integer_oid) {
        Ok(values) => values,
        Err(reason) => return Err(fail(report, reason, false)),
    };
    report.tests = test_values
        .iter()
        .enumerate()
        .map(|(index, _values)| TestReport {
            index: index as u32,
            status: "not_executed".to_owned(),
            reason: "not_executed".to_owned(),
            expect_rows: metric.tests[index].expect.rows,
            actual_rows: None,
        })
        .collect();
    if collision(metric.sql.as_str())
        || collision(control.sql.as_str())
        || catalog_collision(&request.catalog)
    {
        return Err(fail(report, "probe_collision", false));
    }
    if template::negative_code(metric.sql.as_str()) || template::negative_code(control.sql.as_str()) {
        return Err(fail(report, "negative_code", false));
    }
    if template::normalize_ws(&metric_template.bound_sql) == template::normalize_ws(&control_template.bound_sql) {
        return Err(fail(report, "same_path", false));
    }
    let enum_files = match enums::parse_documents(&request.enums) {
        Ok(files) => files,
        Err(reason) => return Err(fail(report, reason, false)),
    };
    let hits = match enums::resolve(&enum_files, &metric.enum_filters, &request.as_of) {
        Ok(hits) => hits,
        Err(reason) => return Err(fail(report, reason, false)),
    };
    report.enum_resolution.certification_allowed = true;
    report.enum_resolution.classes = hits
        .into_iter()
        .map(|hit| ClassReport {
            enum_name: hit.enum_name,
            class_name: hit.class_name,
            codes: hit.codes,
        })
        .collect();
    if let Err(reason) = apply_previous(&mut report, metric, request.previous_metric_yaml.as_deref()) {
        return Err(fail(report, reason, false));
    }
    let catalog = match build_catalog(request) {
        Ok(catalog) => catalog,
        Err(reason) => return Err(fail(report, reason, false)),
    };
    let identity = PolicyIdentity {
        project: request.identity.project.clone(),
        role: request.identity.role.clone(),
        policy_id: request.identity.policy_id.clone(),
    };
    let metric_kinds = kinds_for(&metric_template.names, &kind_of);
    let control_kinds = kinds_for(&control_template.names, &kind_of);
    let metric_guard = match probe::admit(&identity, &catalog, &metric_template, &metric_kinds) {
        Ok(guarded) => guarded,
        Err(reason) => return Err(fail(report, reason, true)),
    };
    let control_guard = match probe::admit(&identity, &catalog, &control_template, &control_kinds) {
        Ok(guarded) => guarded,
        Err(reason) => return Err(fail(report, reason, true)),
    };
    if metric_guard.fingerprint == control_guard.fingerprint {
        return Err(fail(report, "same_path", false));
    }
    fill_query(&mut report.metric_query, &metric_guard);
    fill_query(&mut report.control_query, &control_guard);
    let values = request
        .parameters
        .iter()
        .map(|(name, param)| (name.clone(), param.value.clone()))
        .collect::<BTreeMap<_, _>>();
    let metric_binds = Bind::list(&metric_template.names, &kind_of, &values).map_err(|_| ()) ;
    let metric_binds = match metric_binds {
        Ok(binds) => binds,
        Err(()) => return Err(fail(report, "parameter_value", false)),
    };
    let control_binds = match Bind::list(&control_template.names, &kind_of, &values) {
        Ok(binds) => binds,
        Err(()) => return Err(fail(report, "parameter_value", false)),
    };
    Ok(Analysis {
        report,
        metric_sql: metric_guard.restored_sql,
        metric_binds,
        control_sql: control_guard.restored_sql,
        control_binds,
        metric_names: metric_template.names,
        kind_of,
        test_values,
        abs,
        rel,
        role: request.identity.role.clone(),
        timeout_ms: 0,
        max_bytes: 0,
    })
}

fn fail(mut report: Report, reason: &'static str, g5a_failed: bool) -> Report {
    report.outcome = "refused".to_owned();
    report.reason = reason.to_owned();
    report.metric_certified = false;
    report.database_used = false;
    report.gates = vec![
        Gate {
            gate_id: "G5a".to_owned(),
            status: if g5a_failed { "failed" } else { "block" }.to_owned(),
            reason: reason.to_owned(),
        },
        Gate {
            gate_id: "G5b".to_owned(),
            status: "block".to_owned(),
            reason: "not_executed".to_owned(),
        },
    ];
    report.comparison.status = "block".to_owned();
    report.comparison.reason = "not_executed".to_owned();
    report
}

fn fill_lifecycle(report: &mut Report, metric: &Metric) {
    report.lifecycle.status = Some(status_name(metric.status).to_owned());
    report.lifecycle.owner = metric.owner.as_ref().map(|owner| owner.as_str().to_owned());
    report.lifecycle.synonyms = metric.synonyms.iter().map(|item| item.as_str().to_owned()).collect();
    report.lifecycle.replaced_by = None;
    report.lifecycle.replaced_by_core_field = false;
    report.provenance.owner = report.lifecycle.owner.clone();
}

fn status_name(status: MetricStatus) -> &'static str {
    match status {
        MetricStatus::Draft => "draft",
        MetricStatus::Active => "active",
        MetricStatus::Deprecated => "deprecated",
    }
}

fn tolerances(
    control: &oo_core::oos::ControlTotal,
) -> Result<(Option<ExactDecimal>, Option<ExactDecimal>), &'static str> {
    if control.tolerance.abs.as_ref().is_some_and(|value| value.0.is_negative())
        || control.tolerance.rel.as_ref().is_some_and(|value| value.0.is_negative())
    {
        return Err("invalid_tolerance");
    }
    Ok((
        control.tolerance.abs.as_ref().map(|value| value.0.clone()),
        control.tolerance.rel.as_ref().map(|value| value.0.clone()),
    ))
}

fn parameter_kinds(parameters: &BTreeMap<String, ParamDoc>, integer_oid: u32) -> Result<BTreeMap<String, Kind>, &'static str> {
    if parameters.len() > 32 {
        return Err("unsupported_template");
    }
    let mut kinds = BTreeMap::new();
    for (name, param) in parameters {
        if !param_name_ok(name) {
            return Err("parameter_value");
        }
        if param.value.as_bytes().contains(&0) {
            return Err("parameter_value");
        }
        let kind = match param.type_name.as_str() {
            "text" => Kind::Text,
            "int" => match integer_oid {
                21 => Kind::Int2,
                23 => Kind::Int4,
                20 => Kind::Int8,
                _ => return Err("parameter_value"),
            },
            _ => return Err("unsupported_parameter_type"),
        };
        if matches!(kind, Kind::Int2 | Kind::Int4 | Kind::Int8) && parse_int(&param.value, integer_oid).is_err() {
            return Err("parameter_value");
        }
        kinds.insert(name.clone(), kind);
    }
    Ok(kinds)
}

fn check_names(
    request: &RequestDoc,
    metric: &Metric,
    metric_names: &[String],
    control_names: &[String],
    anchor_date: &str,
    kind_of: &BTreeMap<String, Kind>,
) -> Result<(), &'static str> {
    let mut needed = BTreeSet::new();
    for name in metric_names.iter().chain(control_names) {
        needed.insert(name.as_str());
    }
    let provided: BTreeSet<&str> = request.parameters.keys().map(String::as_str).collect();
    if needed.iter().any(|name| !provided.contains(name)) {
        return Err("missing_parameter");
    }
    if provided.iter().any(|name| !needed.contains(name)) {
        return Err("unused_parameter");
    }
    if request.anchor_bound.len() > 32 {
        return Err("parameter_value");
    }
    for name in &request.anchor_bound {
        if !needed.contains(name.as_str()) {
            return Err("missing_parameter");
        }
        if kind_of.get(name) != Some(&Kind::Text) {
            return Err("parameter_value");
        }
        if request.parameters.get(name).map(|param| param.value.as_str()) != Some(anchor_date) {
            return Err("parameter_value");
        }
    }
    if metric.tests.iter().any(|test| test.expect.rows.is_none()) {
        return Err("missing_parameter");
    }
    Ok(())
}

fn test_values(
    metric: &Metric,
    metric_names: &[String],
    kind_of: &BTreeMap<String, Kind>,
    integer_oid: u32,
) -> Result<Vec<BTreeMap<String, String>>, &'static str> {
    let needed: BTreeSet<&str> = metric_names.iter().map(String::as_str).collect();
    let mut tests = Vec::new();
    for test in &metric.tests {
        let mut values = BTreeMap::new();
        for (key, value) in test.params.iter() {
            if values.insert(key.as_str().to_owned(), value.as_str().to_owned()).is_some() {
                return Err("parameter_value");
            }
        }
        let keys: BTreeSet<&str> = values.keys().map(String::as_str).collect();
        if keys != needed {
            return if keys.iter().any(|key| !needed.contains(key)) {
                Err("unused_parameter")
            } else {
                Err("missing_parameter")
            };
        }
        for (name, text) in &values {
            if text.as_bytes().contains(&0) {
                return Err("parameter_value");
            }
            if let Some(kind) = kind_of.get(name)
                && matches!(kind, Kind::Int2 | Kind::Int4 | Kind::Int8)
                && parse_int(text, integer_oid).is_err()
            {
                return Err("parameter_value");
            }
        }
        tests.push(values);
    }
    Ok(tests)
}

fn collision(sql: &str) -> bool {
    sql.contains("oo_mr_probe_") || sql.contains("810316")
}

fn catalog_collision(catalog: &CatalogDoc) -> bool {
    catalog.relations.iter().any(|relation| {
        collision(&relation.schema)
            || collision(&relation.name)
            || collision(&relation.physical_schema)
            || collision(&relation.physical_name)
            || relation
                .columns
                .iter()
                .any(|column| collision(&column.name) || collision(&column.physical_name))
    })
}

fn apply_previous(report: &mut Report, metric: &Metric, previous: Option<&str>) -> Result<(), &'static str> {
    let Some(previous) = previous else {
        report.semantic_diff.owner_review_required = false;
        return Ok(());
    };
    let parsed = parse_metrics_yaml(previous).map_err(|_| "previous_metric")?;
    if parsed.metrics.len() != 1 {
        return Err("previous_metric");
    }
    let prior = &parsed.metrics[0];
    report.semantic_diff.previous_metric_sql_sha256 = Some(sha256_hex(prior.sql.as_str().as_bytes()));
    let mut changed = Vec::new();
    push(&mut changed, "grain", !labels_eq(&metric.grain, &prior.grain));
    push(&mut changed, "enum_filters", !filters_eq(&metric.enum_filters, &prior.enum_filters));
    push(&mut changed, "sql", metric.sql.as_str() != prior.sql.as_str());
    push(
        &mut changed,
        "time_dimension",
        metric.time_dimension.as_ref().map(|path| path.as_str()) != prior.time_dimension.as_ref().map(|path| path.as_str()),
    );
    push(
        &mut changed,
        "default_anchor",
        metric.default_anchor.map(|anchor| anchor.as_str()) != prior.default_anchor.map(|anchor| anchor.as_str()),
    );
    let left = metric.control_total.as_ref();
    let right = prior.control_total.as_ref();
    push(&mut changed, "control_as_of", left.map(|item| item.as_of) != right.map(|item| item.as_of));
    push(
        &mut changed,
        "control_source",
        left.map(|item| item.source.as_str()) != right.map(|item| item.source.as_str()),
    );
    push(
        &mut changed,
        "control_sql",
        left.map(|item| item.sql.as_str()) != right.map(|item| item.sql.as_str()),
    );
    push(&mut changed, "control_tolerance", !tolerance_eq(left.map(|item| &item.tolerance), right.map(|item| &item.tolerance)));
    push(&mut changed, "status", metric.status != prior.status);
    push(
        &mut changed,
        "owner",
        metric.owner.as_ref().map(|item| item.as_str()) != prior.owner.as_ref().map(|item| item.as_str()),
    );
    push(&mut changed, "synonyms", !labels_eq(&metric.synonyms, &prior.synonyms));
    report.semantic_diff.owner_review_required = !changed.is_empty();
    report.semantic_diff.changed = changed;
    Ok(())
}

fn push(changed: &mut Vec<String>, name: &str, different: bool) {
    if different {
        changed.push(name.to_owned());
    }
}

fn labels_eq(left: &[oo_core::text::Label], right: &[oo_core::text::Label]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| left.as_str() == right.as_str())
}

fn filters_eq(left: &[oo_core::oos::EnumFilter], right: &[oo_core::oos::EnumFilter]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.column.as_str() == right.column.as_str()
                && left.enum_name.as_str() == right.enum_name.as_str()
                && left.classes.len() == right.classes.len()
                && left
                    .classes
                    .iter()
                    .zip(&right.classes)
                    .all(|(left, right)| left.as_str() == right.as_str())
        })
}

fn tolerance_eq(left: Option<&oo_core::oos::Tolerance>, right: Option<&oo_core::oos::Tolerance>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            decimal_opt(left.abs.as_ref(), right.abs.as_ref()) && decimal_opt(left.rel.as_ref(), right.rel.as_ref())
        }
        _ => false,
    }
}

fn decimal_opt(left: Option<&oo_core::FlexDecimal>, right: Option<&oo_core::FlexDecimal>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => decimal_cmp::numeric_eq(&left.0, &right.0),
        _ => false,
    }
}

fn build_catalog(request: &RequestDoc) -> Result<PolicyCatalog, &'static str> {
    if request.catalog.relations.len() > 32 {
        return Err("catalog");
    }
    if request.catalog.relations.iter().any(|relation| relation.columns.len() > 64 || relation.columns.is_empty()) {
        return Err("catalog");
    }
    let literals = &request.catalog.literal_types;
    if !matches!(literals.integer, 20 | 21 | 23) || literals.text != 25 || literals.boolean != 16 {
        return Err("catalog");
    }
    Ok(PolicyCatalog {
        identity: PolicyIdentity {
            project: request.identity.project.clone(),
            role: request.identity.role.clone(),
            policy_id: request.identity.policy_id.clone(),
        },
        relations: request
            .catalog
            .relations
            .iter()
            .map(|relation| VisibleRelation {
                schema: relation.schema.clone(),
                name: relation.name.clone(),
                physical_schema: relation.physical_schema.clone(),
                physical_name: relation.physical_name.clone(),
                columns: relation
                    .columns
                    .iter()
                    .map(|column| VisibleColumn {
                        name: column.name.clone(),
                        physical_name: column.physical_name.clone(),
                        type_oid: column.type_oid,
                    })
                    .collect(),
                pending_unclassified: relation.pending_unclassified,
                estimated_rows: relation.estimated_rows,
            })
            .collect(),
        functions: request
            .catalog
            .functions
            .iter()
            .map(|function| ResolvedFunction {
                oid: function.oid,
                schema: function.schema.clone(),
                name: function.name.clone(),
                argument_type_oids: function.argument_type_oids.clone(),
                aggregate_star: function.aggregate_star,
            })
            .collect(),
        operators: request
            .catalog
            .operators
            .iter()
            .map(|operator| ResolvedOperator {
                oid: operator.oid,
                procedure_oid: operator.procedure_oid,
                schema: operator.schema.clone(),
                name: operator.name.clone(),
                left_type_oid: operator.left_type_oid,
                right_type_oid: operator.right_type_oid,
            })
            .collect(),
        literal_types: LiteralTypes {
            integer: Some(literals.integer),
            text: Some(literals.text),
            boolean: Some(literals.boolean),
        },
        null_test_allowed: request.catalog.null_test_allowed,
        large_table_row_threshold: request.catalog.large_table_row_threshold,
    })
}

fn kinds_for(names: &[String], kind_of: &BTreeMap<String, Kind>) -> Vec<Kind> {
    names.iter().map(|name| kind_of[name]).collect()
}

fn fill_query(slot: &mut QueryReport, guarded: &probe::GuardedSql) {
    slot.bound_sql = guarded.bound_sql.clone();
    slot.guard_fingerprint = guarded.fingerprint.clone();
    slot.executed_sql = guarded.restored_sql.clone();
    slot.executed_sql_sha256 = guarded.executed_sha256.clone();
}

impl Bind {
    fn list(names: &[String], kind_of: &BTreeMap<String, Kind>, values: &BTreeMap<String, String>) -> Result<Vec<Self>, ()> {
        let mut binds = Vec::with_capacity(names.len());
        for name in names {
            let kind = kind_of.get(name).ok_or(())?;
            let text = values.get(name).ok_or(())?;
            binds.push(Self::from_kind(*kind, text)?);
        }
        Ok(binds)
    }
}

fn plain(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(|character| character == '\0' || character.is_control())
}

fn role_ok(role: &str) -> bool {
    let Some(rest) = role.strip_prefix("oo_r_") else {
        return false;
    };
    let bytes = rest.as_bytes();
    if bytes.is_empty() || bytes.len() > 55 {
        return false;
    }
    bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn param_name_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..].iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn parse_int(text: &str, oid: u32) -> Result<i64, ()> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    let value = text.parse::<i64>().map_err(|_| ())?;
    let fits = match oid {
        21 => (i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(&value),
        23 => (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&value),
        20 => true,
        _ => false,
    };
    if fits { Ok(value) } else { Err(()) }
}

fn read_config(path: &Path) -> Result<ConfigDoc, &'static str> {
    let bytes = read_bytes(path, 1_048_576).map_err(|_| "config")?;
    let config: ConfigDoc = serde_json::from_slice(&bytes).map_err(|_| "config")?;
    if !(1..=30_000).contains(&config.statement_timeout_ms) || !(1..=65_536).contains(&config.max_result_bytes) {
        return Err("config");
    }
    if config.host.is_empty() || config.database.is_empty() || config.user.is_empty() {
        return Err("config");
    }
    if !plain(&config.host) || !plain(&config.database) || !plain(&config.user) {
        return Err("config");
    }
    Ok(config)
}

fn read_secret(path: &str, max: usize, password: bool) -> Result<String, &'static str> {
    let reason = if password { "password_file" } else { "config" };
    let file = Path::new(path);
    let meta = std::fs::metadata(file).map_err(|_| reason)?;
    let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions());
    if !meta.is_file() || meta.len() > max as u64 || mode & 0o077 != 0 {
        return Err(reason);
    }
    let mut bytes = std::fs::read(file).map_err(|_| reason)?;
    if bytes.len() > max || bytes.contains(&0) {
        return Err(reason);
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(reason);
    }
    String::from_utf8(bytes).map_err(|_| reason)
}

fn read_bytes(path: &Path, max: usize) -> Result<Vec<u8>, ()> {
    let meta = std::fs::metadata(path).map_err(|_| ())?;
    if !meta.is_file() || meta.len() > max as u64 {
        return Err(());
    }
    let bytes = std::fs::read(path).map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() > max {
        Err(())
    } else {
        Ok(bytes)
    }
}

async fn execute(mut analysis: Analysis, config: &ConfigDoc, password: &str, ca_pem: &[u8]) -> Report {
    analysis.timeout_ms = config.statement_timeout_ms;
    analysis.max_bytes = config.max_result_bytes as usize;
    analysis.report.outcome = "evaluated".to_owned();
    analysis.report.provenance.host = Some(config.host.clone());
    analysis.report.provenance.database = Some(config.database.clone());
    analysis.report.provenance.user = Some(config.user.clone());
    let endpoint = oo_gateway::TlsEndpoint {
        host: config.host.clone(),
        port: config.port,
        database: config.database.clone(),
        user: config.user.clone(),
    };
    let mut session = match adapter::connect(&endpoint, password, ca_pem).await {
        Ok(session) => session,
        Err(reason) => {
            analysis.report.reason = reason.to_owned();
            set_gates(&mut analysis.report, "block", reason, "block", reason);
            analysis.report.comparison.status = "block".to_owned();
            analysis.report.comparison.reason = reason.to_owned();
            analysis.report.database_used = false;
            analysis.report.metric_certified = false;
            seal(&mut analysis.report);
            return analysis.report;
        }
    };
    let mut test_failed = false;
    let mut test_block: Option<&'static str> = None;
    for (index, values) in analysis.test_values.iter().enumerate() {
        if session.dead() {
            mark_test(&mut analysis.report.tests[index], "block", "rollback_uncertain", None);
            test_block.get_or_insert("rollback_uncertain");
            continue;
        }
        let binds = match Bind::list(&analysis.metric_names, &analysis.kind_of, values) {
            Ok(binds) => binds,
            Err(()) => {
                mark_test(&mut analysis.report.tests[index], "block", "parameter_value", None);
                test_block.get_or_insert("parameter_value");
                continue;
            }
        };
        let expect = analysis.report.tests[index].expect_rows.unwrap_or(0);
        let fetch = i32::try_from(expect.saturating_add(1).min(65)).unwrap_or(65);
        match session
            .count(&analysis.metric_sql, &binds, fetch, analysis.timeout_ms, analysis.max_bytes, &analysis.role)
            .await
        {
            Ok(Fetched::Rows(got)) => {
                analysis.report.tests[index].actual_rows = Some(got);
                if expect >= 65 && got == 65 {
                    mark_test(&mut analysis.report.tests[index], "block", "truncated", Some(got));
                    test_block.get_or_insert("truncated");
                } else if got == expect {
                    mark_test(&mut analysis.report.tests[index], "passed", "ok", Some(got));
                } else {
                    mark_test(&mut analysis.report.tests[index], "failed", "test_rows", Some(got));
                    test_failed = true;
                }
            }
            Ok(Fetched::Truncated) => {
                mark_test(&mut analysis.report.tests[index], "block", "truncated", None);
                test_block.get_or_insert("truncated");
            }
            Err(StepError::Uncertain) => {
                mark_test(&mut analysis.report.tests[index], "block", "rollback_uncertain", None);
                test_block.get_or_insert("rollback_uncertain");
            }
            Err(StepError::Block(reason)) => {
                mark_test(&mut analysis.report.tests[index], "block", reason, None);
                test_block.get_or_insert(reason);
            }
        }
    }
    let explain_state = if session.dead() {
        ("block", "rollback_uncertain")
    } else {
        match session
            .explain(
                &analysis.metric_sql,
                &analysis.metric_binds,
                analysis.timeout_ms,
                analysis.max_bytes,
                &analysis.role,
            )
            .await
        {
            Ok(ExplainOut::Ready { rows, text_sha256 }) => {
                analysis.report.explain.rows = rows;
                analysis.report.explain.text_sha256 = text_sha256;
                ("passed", "ok")
            }
            Ok(ExplainOut::Truncated) => ("block", "truncated"),
            Ok(ExplainOut::Shape) => ("block", "shape"),
            Err(StepError::Uncertain) => ("block", "rollback_uncertain"),
            Err(StepError::Block(reason)) => ("block", reason),
        }
    };
    analysis.report.explain.status = explain_state.0.to_owned();
    analysis.report.explain.reason = explain_state.1.to_owned();
    let metric_cell = take_cell(&mut session, &analysis.metric_sql, &analysis.metric_binds, analysis.timeout_ms, analysis.max_bytes, &analysis.role).await;
    let control_cell = take_cell(&mut session, &analysis.control_sql, &analysis.control_binds, analysis.timeout_ms, analysis.max_bytes, &analysis.role).await;
    analysis.report.database_used = session.database_used;
    let g5b = compare_cells(&mut analysis, metric_cell, control_cell);
    let tests_state = if analysis.report.tests.is_empty() {
        ("passed", "ok")
    } else if test_failed {
        ("failed", "test_rows")
    } else if let Some(reason) = test_block {
        ("block", reason)
    } else {
        ("passed", "ok")
    };
    let (g5a_status, g5a_reason) = report::fold_status(&[
        ("passed", "ok"),
        ("passed", "ok"),
        ("passed", "ok"),
        ("passed", "ok"),
        tests_state,
        explain_state,
    ]);
    set_gates(&mut analysis.report, g5a_status, g5a_reason, g5b.0, g5b.1);
    analysis.report.metric_certified = g5a_status == "passed"
        && g5b.0 == "passed"
        && !analysis.report.semantic_diff.owner_review_required
        && analysis.report.enum_resolution.certification_allowed;
    analysis.report.reason = if g5a_status == "failed" {
        g5a_reason.to_owned()
    } else if g5b.0 == "failed" {
        g5b.1.to_owned()
    } else if g5a_status == "block" {
        g5a_reason.to_owned()
    } else if g5b.0 == "block" {
        g5b.1.to_owned()
    } else if analysis.report.semantic_diff.owner_review_required {
        "owner_review_required".to_owned()
    } else {
        "ok".to_owned()
    };
    seal(&mut analysis.report);
    analysis.report
}

async fn take_cell(session: &mut Session, sql: &str, binds: &[Bind], timeout_ms: u32, max_bytes: usize, role: &str) -> Result<Cell, &'static str> {
    if session.dead() {
        return Err("rollback_uncertain");
    }
    match session.value(sql, binds, timeout_ms, max_bytes, role).await {
        Ok(cell) => Ok(cell),
        Err(StepError::Uncertain) => Err("rollback_uncertain"),
        Err(StepError::Block(reason)) => Err(reason),
    }
}

fn compare_cells(analysis: &mut Analysis, metric_cell: Result<Cell, &'static str>, control_cell: Result<Cell, &'static str>) -> (&'static str, &'static str) {
    let metric_value = match metric_cell {
        Ok(Cell::Decimal(value)) => Some(value),
        Ok(Cell::Null) => {
            analysis.report.values.metric = None;
            return block_comparison(&mut analysis.report, "null_value");
        }
        Ok(Cell::Shape) => return block_comparison(&mut analysis.report, "shape"),
        Ok(Cell::Truncated) => return block_comparison(&mut analysis.report, "truncated"),
        Err(reason) => return block_comparison(&mut analysis.report, reason),
    };
    let control_value = match control_cell {
        Ok(Cell::Decimal(value)) => Some(value),
        Ok(Cell::Null) => {
            if let Some(value) = &metric_value {
                analysis.report.values.metric = Some(value.to_plain_string());
            }
            return block_comparison(&mut analysis.report, "null_value");
        }
        Ok(Cell::Shape) => return block_comparison(&mut analysis.report, "shape"),
        Ok(Cell::Truncated) => return block_comparison(&mut analysis.report, "truncated"),
        Err(reason) => return block_comparison(&mut analysis.report, reason),
    };
    let (Some(metric_value), Some(control_value)) = (metric_value, control_value) else {
        return block_comparison(&mut analysis.report, "shape");
    };
    analysis.report.values.metric = Some(metric_value.to_plain_string());
    analysis.report.values.control = Some(control_value.to_plain_string());
    match decimal_cmp::compare(&metric_value, &control_value, analysis.abs.as_ref(), analysis.rel.as_ref()) {
        Ok(decimal_cmp::Verdict::Passed { difference }) => {
            analysis.report.comparison.status = "passed".to_owned();
            analysis.report.comparison.reason = "ok".to_owned();
            analysis.report.comparison.difference = Some(difference.to_plain_string());
            ("passed", "ok")
        }
        Ok(decimal_cmp::Verdict::Failed { difference }) => {
            analysis.report.comparison.status = "failed".to_owned();
            analysis.report.comparison.reason = "tolerance".to_owned();
            analysis.report.comparison.difference = Some(difference.to_plain_string());
            ("failed", "tolerance")
        }
        Err(()) => block_comparison(&mut analysis.report, "decimal_budget"),
    }
}

fn block_comparison(report: &mut Report, reason: &'static str) -> (&'static str, &'static str) {
    report.comparison.status = "block".to_owned();
    report.comparison.reason = reason.to_owned();
    ("block", reason)
}

fn set_gates(report: &mut Report, g5a_status: &str, g5a_reason: &str, g5b_status: &str, g5b_reason: &str) {
    report.gates = vec![
        Gate {
            gate_id: "G5a".to_owned(),
            status: g5a_status.to_owned(),
            reason: g5a_reason.to_owned(),
        },
        Gate {
            gate_id: "G5b".to_owned(),
            status: g5b_status.to_owned(),
            reason: g5b_reason.to_owned(),
        },
    ];
}

fn mark_test(test: &mut TestReport, status: &str, reason: &str, actual: Option<u64>) {
    test.status = status.to_owned();
    test.reason = reason.to_owned();
    test.actual_rows = actual;
}

#[cfg(test)]
mod behavior;

#[cfg(test)]
mod live;

#[cfg(test)]
mod lock_subset;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_request_stays_empty() {
        let exit = compile_bytes(br#"{"expected":1}"#);
        assert_eq!(exit.code, 1);
        assert!(exit.stdout.is_empty());
        assert_eq!(exit.stderr, b"metric-reconcile: malformed unknown_field\n");
        let exit = compile_bytes(b"");
        assert_eq!(exit.code, 1);
        assert!(exit.stderr.windows(9).any(|bytes| bytes == b"malformed"));
        assert_eq!(argv_malformed(&["compile".to_owned(), "extra".to_owned()]), Some("extra_arg"));
        assert_eq!(argv_malformed(&["evaluate".to_owned()]), Some("config"));
    }
}
