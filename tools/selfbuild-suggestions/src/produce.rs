//! Deterministic S6–S8 candidates and the S9 queue they feed.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use crate::json::{candidate_id, canonical_sha256};

const SOURCE_PROTOCOL: &str = "oo.source.pgsql/v1";
const PROJECTION_SCHEMA: &str = "oo.selfbuild-suggestions.catalog/v1";
const COMMAND_SCHEMA: &str = "oo.selfbuild-suggestions.command/v1";
const RELATION_SCHEMA: &str = "oo.selfbuild-suggestions.relationships/v1";
const PROFILE_SCHEMA: &str = "oo.selfbuild-suggestions.profile/v1";
const FEEDBACK_SCHEMA: &str = "oo.selfbuild-suggestions.feedback/v1";
const OUTCOME_SCHEMA: &str = "oo.selfbuild-suggestions.outcome/v1";
const SEMANTICS: &[&str] = &[
    "identifier",
    "money",
    "quantity",
    "time",
    "boolean_flag",
    "phone",
    "person_name",
    "text",
    "unknown",
];

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
    Refused {
        reason: &'static str,
        outcome: Value,
    },
}

pub struct Spawned {
    pub exit: Option<i32>,
    pub stdout: Vec<u8>,
    pub timed_out: bool,
    pub started: bool,
}

impl Spawned {
    pub fn not_started() -> Self {
        Self {
            exit: None,
            stdout: Vec::new(),
            timed_out: false,
            started: false,
        }
    }
}

pub trait SelectRunner {
    fn run_select(&mut self, sql: &str) -> Spawned;
}

pub struct Gateway<'a> {
    pub probe_sql: &'a str,
    pub max_validated: usize,
    pub runner: &'a mut dyn SelectRunner,
}

enum Stop {
    Malformed(&'static str),
    Refused {
        reason: &'static str,
        outcome: Value,
    },
}

struct Scope {
    project: String,
    role: String,
    evidence: String,
    source_id: String,
    schema_version: String,
    model_scope: String,
}

struct Col {
    name: String,
    type_name: String,
    pk: bool,
    semantic: String,
    strong: bool,
}

struct Table {
    schema: String,
    name: String,
    qualified: String,
    columns: Vec<Col>,
    reltuples: Option<String>,
}

struct Edge {
    name: String,
    from: usize,
    to: usize,
    column: String,
    ref_column: String,
}

struct RawFk {
    name: String,
    schema: String,
    table: String,
    column: String,
    ref_schema: String,
    ref_table: String,
    ref_column: String,
}

struct RawTable {
    schema: String,
    name: String,
    columns: Vec<(String, String)>,
    primary_key: Vec<String>,
}

struct Catalog {
    schema_hash: String,
    tables: Vec<Table>,
    edges: Vec<Edge>,
}

struct Fb {
    kind: String,
    target: String,
    text: String,
}

struct Curated {
    key: String,
    fact_sha256: String,
}

struct Inputs {
    scope: Scope,
    schema_hash: String,
    tables: Vec<Table>,
    edges: Vec<Edge>,
    rel_collapsed: u64,
    rel_rejected: u64,
    rel_drops: u64,
    feedback: Vec<Fb>,
    fb_collapsed: u64,
    fb_drops: u64,
    negatives: Vec<(String, String)>,
    neg_collapsed: u64,
    curated: Vec<Curated>,
    prior: BTreeSet<String>,
}

struct Entity {
    candidate_id: String,
    key: String,
    members: Vec<String>,
    primary: String,
    domain: String,
    provenance: &'static str,
    confidence: &'static str,
    strong_key_status: &'static str,
    weak_columns: Vec<String>,
    profile_reltuples: Vec<Value>,
    fact_sha256: String,
    edged: bool,
}

struct DomainOut {
    domain_id: String,
    members: Vec<String>,
}

struct Metric {
    candidate_id: String,
    key: String,
    column: String,
    table: String,
    grain: Vec<&'static str>,
    time_dimension: Option<String>,
    time_anchor: Option<String>,
    time_anchor_closed: Option<bool>,
    time_anchor_needed: bool,
    sql: String,
    fact_sha256: String,
}

struct Example {
    candidate_id: String,
    key: String,
    constraint: String,
    question: String,
    sql: String,
    validation: &'static str,
    admitted: bool,
}

struct Gw {
    configured: bool,
    probe: &'static str,
    probe_exit: Option<i64>,
    admitted: u64,
    spawned: u64,
}

impl Gw {
    fn idle(configured: bool) -> Self {
        Self {
            configured,
            probe: "not_run",
            probe_exit: None,
            admitted: 0,
            spawned: 0,
        }
    }
}

struct Built {
    entities: Vec<Entity>,
    domains: Vec<DomainOut>,
    metrics: Vec<Metric>,
    examples: Vec<Example>,
    stales: Vec<(String, String)>,
    gateway: Gw,
}

pub fn produce(document: &Value) -> Result<Value, Error> {
    produce_with(document, None)
}

pub fn produce_with(document: &Value, gateway: Option<Gateway<'_>>) -> Result<Value, Error> {
    match try_produce(document, gateway) {
        Ok(outcome) => Ok(outcome),
        Err(Stop::Malformed(reason)) => Err(Error::Malformed(reason)),
        Err(Stop::Refused { reason, outcome }) => Err(Error::Refused { reason, outcome }),
    }
}

fn try_produce(document: &Value, mut gateway: Option<Gateway<'_>>) -> Result<Value, Stop> {
    let configured = gateway.is_some();
    let inputs = read_inputs(document, configured)?;
    let mut built = build_candidates(&inputs);
    built.gateway = Gw::idle(configured);
    if let Some(gateway) = gateway.as_mut() {
        match run_gateway(gateway, &mut built.examples) {
            Ok(state) => built.gateway = state,
            Err(state) => {
                built.gateway = state;
                return Err(Stop::Refused {
                    reason: "probe",
                    outcome: render(&inputs, &built),
                });
            }
        }
    }
    Ok(render(&inputs, &built))
}

fn read_inputs(document: &Value, configured: bool) -> Result<Inputs, Stop> {
    let root = object(document)?;
    only(
        root,
        &[
            "schema",
            "scope",
            "authorization",
            "catalog",
            "relationships",
            "profile",
            "feedback",
            "negative_pairs",
            "curated_overrides",
            "prior_candidate_ids",
        ],
    )?;
    if required_str(root, "schema")? != COMMAND_SCHEMA {
        return Err(Stop::Malformed("type"));
    }
    let scope = read_scope(root)?;
    if let Some(reason) = scope_problem(&scope) {
        return Err(deny(&scope, None, configured, reason));
    }
    let auth = object(required(root, "authorization")?)?;
    only(
        auth,
        &[
            "authorized",
            "catalog_sha256",
            "relationships_sha256",
            "profile_sha256",
            "feedback_sha256",
        ],
    )?;
    match required(auth, "authorized")?.as_bool() {
        Some(true) => {}
        Some(false) => return Err(deny(&scope, None, configured, "authorization")),
        None => return Err(Stop::Malformed("type")),
    }
    let catalog_value = required(root, "catalog")?;
    let relationships = required(root, "relationships")?;
    let profile = required(root, "profile")?;
    let feedback = required(root, "feedback")?;
    let catalog_sha = required_str(auth, "catalog_sha256")?;
    let relationships_sha = required_str(auth, "relationships_sha256")?;
    let profile_sha = required_str(auth, "profile_sha256")?;
    let feedback_sha = required_str(auth, "feedback_sha256")?;
    if scope.evidence != catalog_sha
        || catalog_sha != canonical_sha256(catalog_value)
        || relationships_sha != canonical_sha256(relationships)
        || profile_sha != canonical_sha256(profile)
        || feedback_sha != canonical_sha256(feedback)
    {
        return Err(deny(&scope, None, configured, "seal"));
    }
    let catalog = load_catalog(catalog_value, &scope, configured)?;
    let hash = catalog.schema_hash.clone();
    let (edges, rel_collapsed, rel_rejected, rel_drops) =
        load_relationships(relationships, &scope, &catalog, &hash, configured)?;
    let tables = apply_profile(profile, &scope, &hash, catalog.tables, configured)?;
    let (feedback_items, fb_collapsed, fb_drops) =
        load_feedback(feedback, &scope, &hash, configured)?;
    let (negatives, neg_collapsed) = load_negatives(root, &scope, &hash, &tables, configured)?;
    let curated = load_curated(root, &scope, &hash, configured)?;
    let prior = load_prior(root, &scope, &hash, configured)?;
    Ok(Inputs {
        scope,
        schema_hash: hash,
        tables,
        edges,
        rel_collapsed,
        rel_rejected,
        rel_drops,
        feedback: feedback_items,
        fb_collapsed,
        fb_drops,
        negatives,
        neg_collapsed,
        curated,
        prior,
    })
}

fn read_scope(root: &Map<String, Value>) -> Result<Scope, Stop> {
    let map = object(required(root, "scope")?)?;
    only(
        map,
        &[
            "project",
            "role",
            "evidence_sha256",
            "source_id",
            "schema_version",
            "model_scope",
        ],
    )?;
    Ok(Scope {
        project: required_str(map, "project")?.to_string(),
        role: required_str(map, "role")?.to_string(),
        evidence: required_str(map, "evidence_sha256")?.to_string(),
        source_id: required_str(map, "source_id")?.to_string(),
        schema_version: required_str(map, "schema_version")?.to_string(),
        model_scope: required_str(map, "model_scope")?.to_string(),
    })
}

fn scope_problem(scope: &Scope) -> Option<&'static str> {
    if !project_ok(&scope.project) || !project_ok(&scope.source_id) || !role_ok(&scope.role) {
        return Some("scope");
    }
    if scope.schema_version != "oos/v1" || scope.model_scope != "none" {
        return Some("scope");
    }
    None
}

fn load_catalog(value: &Value, scope: &Scope, configured: bool) -> Result<Catalog, Stop> {
    let map = object(value)?;
    if map.get("protocol").and_then(Value::as_str) == Some(SOURCE_PROTOCOL) {
        load_source(map, scope, configured)
    } else if map.get("schema").and_then(Value::as_str) == Some(PROJECTION_SCHEMA) {
        load_projection(map, scope, configured)
    } else {
        Err(Stop::Malformed("type"))
    }
}

fn load_source(map: &Map<String, Value>, scope: &Scope, configured: bool) -> Result<Catalog, Stop> {
    let source_id = required_str(map, "source_id")?;
    if source_id != scope.source_id {
        return Err(deny(scope, None, configured, "scope"));
    }
    let schema_hash = required_str(map, "schema_hash")?;
    if !hex_ok(schema_hash, 64) {
        return Err(deny(scope, None, configured, "catalog"));
    }
    let tables_value = required(map, "tables")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    if tables_value.len() > 64 {
        return Err(deny(scope, Some(schema_hash), configured, "catalog"));
    }
    let mut tables = Vec::new();
    let mut raw_fks = Vec::new();
    for table_value in tables_value {
        let table = object(table_value)?;
        let schema = required_str(table, "schema")?;
        let name = required_str(table, "name")?;
        ident(schema)?;
        ident(name)?;
        let columns_value = required(table, "columns")?
            .as_array()
            .ok_or(Stop::Malformed("type"))?;
        if columns_value.len() > 64 {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        }
        let mut columns = Vec::new();
        for column_value in columns_value {
            let column = object(column_value)?;
            let column_name = required_str(column, "name")?;
            let type_name = required_str(column, "type_name")?;
            ident(column_name)?;
            ident(type_name)?;
            if columns.iter().any(|(seen, _)| seen == column_name) {
                return Err(deny(scope, Some(schema_hash), configured, "catalog"));
            }
            columns.push((column_name.to_string(), type_name.to_string()));
        }
        let pk_value = required(table, "primary_key")?
            .as_array()
            .ok_or(Stop::Malformed("type"))?;
        let mut primary = Vec::new();
        for item in pk_value {
            let pk_name = object(item)?
                .get("name")
                .and_then(Value::as_str)
                .ok_or(Stop::Malformed("type"))?;
            ident(pk_name)?;
            if !columns.iter().any(|(column, _)| column == pk_name)
                || primary.iter().any(|seen| seen == pk_name)
            {
                return Err(deny(scope, Some(schema_hash), configured, "catalog"));
            }
            primary.push(pk_name.to_string());
        }
        let fk_value = required(table, "foreign_keys")?
            .as_array()
            .ok_or(Stop::Malformed("type"))?;
        for fk_value in fk_value {
            let fk = object(fk_value)?;
            let fk_name = required_str(fk, "name")?;
            ident(fk_name)?;
            let pairs = required(fk, "columns")?
                .as_array()
                .ok_or(Stop::Malformed("type"))?;
            if pairs.len() != 1 {
                return Err(deny(scope, Some(schema_hash), configured, "catalog"));
            }
            let pair = object(&pairs[0])?;
            let column = required_str(pair, "name")?;
            let ref_schema = required_str(pair, "ref_schema")?;
            let ref_table = required_str(pair, "ref_table")?;
            let ref_column = required_str(pair, "ref_name")?;
            ident(column)?;
            ident(ref_schema)?;
            ident(ref_table)?;
            ident(ref_column)?;
            raw_fks.push(RawFk {
                name: fk_name.to_string(),
                schema: schema.to_string(),
                table: name.to_string(),
                column: column.to_string(),
                ref_schema: ref_schema.to_string(),
                ref_table: ref_table.to_string(),
                ref_column: ref_column.to_string(),
            });
        }
        tables.push(RawTable {
            schema: schema.to_string(),
            name: name.to_string(),
            columns,
            primary_key: primary,
        });
    }
    finish_catalog(scope, configured, schema_hash, tables, raw_fks)
}

fn load_projection(
    map: &Map<String, Value>,
    scope: &Scope,
    configured: bool,
) -> Result<Catalog, Stop> {
    only(
        map,
        &[
            "schema",
            "source_id",
            "schema_hash",
            "tables",
            "foreign_keys",
        ],
    )?;
    let source_id = required_str(map, "source_id")?;
    if source_id != scope.source_id {
        return Err(deny(scope, None, configured, "scope"));
    }
    let schema_hash = required_str(map, "schema_hash")?;
    if schema_hash != "unsealed" && !hex_ok(schema_hash, 64) {
        return Err(deny(scope, None, configured, "catalog"));
    }
    let tables_value = required(map, "tables")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    if tables_value.len() > 64 {
        return Err(deny(scope, Some(schema_hash), configured, "catalog"));
    }
    let mut tables = Vec::new();
    for table_value in tables_value {
        let table = object(table_value)?;
        only(table, &["schema", "name", "primary_key", "columns"])?;
        let schema = required_str(table, "schema")?;
        let name = required_str(table, "name")?;
        ident(schema)?;
        ident(name)?;
        let columns_value = required(table, "columns")?
            .as_array()
            .ok_or(Stop::Malformed("type"))?;
        if columns_value.len() > 64 {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        }
        let mut columns = Vec::new();
        for column_value in columns_value {
            let column = object(column_value)?;
            only(column, &["name", "type_name"])?;
            let column_name = required_str(column, "name")?;
            let type_name = required_str(column, "type_name")?;
            ident(column_name)?;
            ident(type_name)?;
            if columns.iter().any(|(seen, _)| seen == column_name) {
                return Err(deny(scope, Some(schema_hash), configured, "catalog"));
            }
            columns.push((column_name.to_string(), type_name.to_string()));
        }
        let pk_value = required(table, "primary_key")?
            .as_array()
            .ok_or(Stop::Malformed("type"))?;
        let mut primary = Vec::new();
        for item in pk_value {
            let pk_name = item.as_str().ok_or(Stop::Malformed("type"))?;
            ident(pk_name)?;
            if !columns.iter().any(|(column, _)| column == pk_name)
                || primary.iter().any(|seen| seen == pk_name)
            {
                return Err(deny(scope, Some(schema_hash), configured, "catalog"));
            }
            primary.push(pk_name.to_string());
        }
        tables.push(RawTable {
            schema: schema.to_string(),
            name: name.to_string(),
            columns,
            primary_key: primary,
        });
    }
    let fk_value = required(map, "foreign_keys")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    let mut raw_fks = Vec::new();
    for item in fk_value {
        let fk = object(item)?;
        only(
            fk,
            &[
                "name",
                "schema",
                "table",
                "columns",
                "ref_schema",
                "ref_table",
                "ref_columns",
            ],
        )?;
        let name = required_str(fk, "name")?;
        let schema = required_str(fk, "schema")?;
        let table = required_str(fk, "table")?;
        let ref_schema = required_str(fk, "ref_schema")?;
        let ref_table = required_str(fk, "ref_table")?;
        ident(name)?;
        ident(schema)?;
        ident(table)?;
        ident(ref_schema)?;
        ident(ref_table)?;
        let columns = string_list(required(fk, "columns")?)?;
        let ref_columns = string_list(required(fk, "ref_columns")?)?;
        if columns.len() != 1 || ref_columns.len() != 1 {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        }
        raw_fks.push(RawFk {
            name: name.to_string(),
            schema: schema.to_string(),
            table: table.to_string(),
            column: columns[0].clone(),
            ref_schema: ref_schema.to_string(),
            ref_table: ref_table.to_string(),
            ref_column: ref_columns[0].clone(),
        });
    }
    finish_catalog(scope, configured, schema_hash, tables, raw_fks)
}

fn finish_catalog(
    scope: &Scope,
    configured: bool,
    schema_hash: &str,
    raw_tables: Vec<RawTable>,
    raw_fks: Vec<RawFk>,
) -> Result<Catalog, Stop> {
    if raw_fks.len() > 128 {
        return Err(deny(scope, Some(schema_hash), configured, "catalog"));
    }
    let mut seen_tables = BTreeSet::new();
    let mut tables = Vec::new();
    let mut index = BTreeMap::new();
    let mut types = BTreeMap::new();
    let mut primary = BTreeMap::new();
    for RawTable {
        schema,
        name,
        columns,
        primary_key: pk,
    } in raw_tables
    {
        let qualified = format!("{schema}.{name}");
        if !seen_tables.insert(qualified.clone()) {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        }
        let mut cols = Vec::new();
        for (column_name, type_name) in &columns {
            types.insert((qualified.clone(), column_name.clone()), type_name.clone());
            cols.push(Col {
                name: column_name.clone(),
                type_name: type_name.clone(),
                pk: pk.iter().any(|item| item == column_name),
                semantic: String::new(),
                strong: false,
            });
        }
        primary.insert(qualified.clone(), pk);
        index.insert(qualified.clone(), tables.len());
        tables.push(Table {
            schema,
            name,
            qualified,
            columns: cols,
            reltuples: None,
        });
    }
    let mut seen_fk = BTreeSet::new();
    let mut edges = Vec::new();
    for fk in raw_fks {
        let from_name = format!("{}.{}", fk.schema, fk.table);
        let to_name = format!("{}.{}", fk.ref_schema, fk.ref_table);
        let Some(&from) = index.get(&from_name) else {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        };
        let Some(&to) = index.get(&to_name) else {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        };
        let Some(child_type) = types.get(&(from_name, fk.column.clone())) else {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        };
        let Some(parent_type) = types.get(&(to_name.clone(), fk.ref_column.clone())) else {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        };
        let is_pk = primary
            .get(&to_name)
            .is_some_and(|items| items.iter().any(|item| item == &fk.ref_column));
        if !is_pk || !types_ok(child_type, parent_type) {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        }
        if !seen_fk.insert((fk.schema.clone(), fk.name.clone())) {
            return Err(deny(scope, Some(schema_hash), configured, "catalog"));
        }
        edges.push(Edge {
            name: fk.name,
            from,
            to,
            column: fk.column,
            ref_column: fk.ref_column,
        });
    }
    Ok(Catalog {
        schema_hash: schema_hash.to_string(),
        tables,
        edges,
    })
}

fn load_relationships(
    value: &Value,
    scope: &Scope,
    catalog: &Catalog,
    _hash: &str,
    _configured: bool,
) -> Result<(Vec<Edge>, u64, u64, u64), Stop> {
    let map = object(value)?;
    only(map, &["schema", "items"])?;
    if required_str(map, "schema")? != RELATION_SCHEMA {
        return Err(Stop::Malformed("type"));
    }
    let items = required(map, "items")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    let mut confirmed = Vec::new();
    let mut seen = BTreeSet::new();
    let mut collapsed = 0_u64;
    let mut rejected = 0_u64;
    let mut drops = 0_u64;
    for item in items {
        let row = object(item)?;
        only(
            row,
            &[
                "project",
                "provenance",
                "review_state",
                "name",
                "schema",
                "table",
                "columns",
                "ref_schema",
                "ref_table",
                "ref_columns",
            ],
        )?;
        let project = required_str(row, "project")?;
        if !project_ok(project) {
            return Err(Stop::Malformed("type"));
        }
        let provenance = required_str(row, "provenance")?;
        let review_state = required_str(row, "review_state")?;
        let name = required_str(row, "name")?;
        let schema = required_str(row, "schema")?;
        let table = required_str(row, "table")?;
        let ref_schema = required_str(row, "ref_schema")?;
        let ref_table = required_str(row, "ref_table")?;
        ident(name)?;
        ident(schema)?;
        ident(table)?;
        ident(ref_schema)?;
        ident(ref_table)?;
        let columns = string_list(required(row, "columns")?)?;
        let ref_columns = string_list(required(row, "ref_columns")?)?;
        if project != scope.project {
            drops += 1;
            continue;
        }
        let matched = if provenance == "declared_fk"
            && review_state == "approved"
            && columns.len() == 1
            && ref_columns.len() == 1
        {
            catalog.edges.iter().find(|edge| {
                let from = &catalog.tables[edge.from];
                let to = &catalog.tables[edge.to];
                edge.name == name
                    && from.schema == schema
                    && from.name == table
                    && edge.column == columns[0]
                    && to.schema == ref_schema
                    && to.name == ref_table
                    && edge.ref_column == ref_columns[0]
            })
        } else {
            None
        };
        let Some(edge) = matched else {
            rejected += 1;
            continue;
        };
        let key = (
            name.to_string(),
            schema.to_string(),
            table.to_string(),
            columns[0].clone(),
            ref_columns[0].clone(),
        );
        if !seen.insert(key) {
            collapsed += 1;
            continue;
        }
        confirmed.push(Edge {
            name: edge.name.clone(),
            from: edge.from,
            to: edge.to,
            column: edge.column.clone(),
            ref_column: edge.ref_column.clone(),
        });
    }
    Ok((confirmed, collapsed, rejected, drops))
}

fn apply_profile(
    value: &Value,
    scope: &Scope,
    hash: &str,
    mut tables: Vec<Table>,
    configured: bool,
) -> Result<Vec<Table>, Stop> {
    let map = object(value)?;
    only(map, &["schema", "sample_policy", "tables"])?;
    if required_str(map, "schema")? != PROFILE_SCHEMA {
        return Err(Stop::Malformed("type"));
    }
    if required_str(map, "sample_policy")? != "none" {
        return Err(deny(scope, Some(hash), configured, "profile"));
    }
    let listed = required(map, "tables")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    let mut seen_tables = BTreeSet::new();
    for item in listed {
        let table = object(item)?;
        only(table, &["schema", "name", "reltuples", "columns"])?;
        let schema = required_str(table, "schema")?;
        let name = required_str(table, "name")?;
        ident(schema)?;
        ident(name)?;
        let qualified = format!("{schema}.{name}");
        if !seen_tables.insert(qualified.clone()) {
            return Err(deny(scope, Some(hash), configured, "profile"));
        }
        let Some(slot) = tables.iter_mut().find(|item| item.qualified == qualified) else {
            return Err(deny(scope, Some(hash), configured, "profile"));
        };
        if let Some(reltuples) = table.get("reltuples") {
            let text = reltuples.as_str().ok_or(Stop::Malformed("type"))?;
            if !reltuples_ok(text) {
                return Err(Stop::Malformed("type"));
            }
            slot.reltuples = Some(text.to_string());
        }
        let columns = required(table, "columns")?
            .as_array()
            .ok_or(Stop::Malformed("type"))?;
        let mut seen_columns = BTreeSet::new();
        for column_value in columns {
            let column = object(column_value)?;
            only(column, &["name", "semantic", "strong_key"])?;
            let column_name = required_str(column, "name")?;
            ident(column_name)?;
            if !seen_columns.insert(column_name.to_string()) {
                return Err(deny(scope, Some(hash), configured, "profile"));
            }
            let Some(slot_column) = slot
                .columns
                .iter_mut()
                .find(|item| item.name == column_name)
            else {
                return Err(deny(scope, Some(hash), configured, "profile"));
            };
            if let Some(semantic) = column.get("semantic") {
                let semantic = semantic.as_str().ok_or(Stop::Malformed("type"))?;
                if !SEMANTICS.contains(&semantic) {
                    return Err(Stop::Malformed("type"));
                }
                slot_column.semantic = semantic.to_string();
            }
            if let Some(strong) = column.get("strong_key") {
                if strong.as_str() != Some("validated") {
                    return Err(Stop::Malformed("type"));
                }
                slot_column.strong = true;
            }
        }
    }
    for table in &mut tables {
        for column in &mut table.columns {
            if column.semantic.is_empty() {
                column.semantic =
                    infer_semantic(&column.name, &column.type_name, column.pk).to_string();
            }
        }
    }
    Ok(tables)
}

fn load_feedback(
    value: &Value,
    scope: &Scope,
    hash: &str,
    configured: bool,
) -> Result<(Vec<Fb>, u64, u64), Stop> {
    let map = object(value)?;
    only(map, &["schema", "items"])?;
    if required_str(map, "schema")? != FEEDBACK_SCHEMA {
        return Err(Stop::Malformed("type"));
    }
    let items = required(map, "items")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    if items.len() > 32 {
        return Err(deny(scope, Some(hash), configured, "scope"));
    }
    let mut kept = Vec::new();
    let mut seen = BTreeSet::new();
    let mut collapsed = 0_u64;
    let mut drops = 0_u64;
    for item in items {
        let row = object(item)?;
        only(row, &["project", "kind", "text", "target"])?;
        let project = required_str(row, "project")?;
        if !project_ok(project) {
            return Err(Stop::Malformed("type"));
        }
        let kind = required_str(row, "kind")?;
        if !matches!(kind, "gap" | "reject" | "failure" | "comment") {
            return Err(Stop::Malformed("type"));
        }
        let text = required_str(row, "text")?;
        let target = required_str(row, "target")?;
        if !text_ok(text) || !target_ok(target) {
            return Err(Stop::Malformed("type"));
        }
        if project != scope.project {
            drops += 1;
            continue;
        }
        let key = (kind.to_string(), target.to_string(), text.to_string());
        if !seen.insert(key) {
            collapsed += 1;
            continue;
        }
        kept.push(Fb {
            kind: kind.to_string(),
            target: target.to_string(),
            text: text.to_string(),
        });
    }
    Ok((kept, collapsed, drops))
}

fn load_negatives(
    root: &Map<String, Value>,
    scope: &Scope,
    hash: &str,
    tables: &[Table],
    configured: bool,
) -> Result<(Vec<(String, String)>, u64), Stop> {
    let pairs = required(root, "negative_pairs")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    if pairs.len() > 128 {
        return Err(deny(scope, Some(hash), configured, "scope"));
    }
    let known: BTreeSet<&str> = tables
        .iter()
        .map(|table| table.qualified.as_str())
        .collect();
    let mut kept = Vec::new();
    let mut seen = BTreeSet::new();
    let mut collapsed = 0_u64;
    for pair in pairs {
        let items = pair.as_array().ok_or(Stop::Malformed("type"))?;
        if items.len() != 2 {
            return Err(Stop::Malformed("type"));
        }
        let left = qualified_name(items[0].as_str().ok_or(Stop::Malformed("type"))?)?;
        let right = qualified_name(items[1].as_str().ok_or(Stop::Malformed("type"))?)?;
        if left == right || !known.contains(left.as_str()) || !known.contains(right.as_str()) {
            return Err(deny(scope, Some(hash), configured, "catalog"));
        }
        let ordered = ordered_pair(&left, &right);
        if !seen.insert(ordered.clone()) {
            collapsed += 1;
            continue;
        }
        kept.push(ordered);
    }
    kept.sort();
    Ok((kept, collapsed))
}

fn load_curated(
    root: &Map<String, Value>,
    scope: &Scope,
    hash: &str,
    configured: bool,
) -> Result<Vec<Curated>, Stop> {
    let items = required(root, "curated_overrides")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    if items.len() > 128 {
        return Err(deny(scope, Some(hash), configured, "scope"));
    }
    let mut kept = Vec::new();
    let mut seen = BTreeSet::new();
    for item in items {
        let row = object(item)?;
        only(row, &["key", "fact_sha256"])?;
        let key = required_str(row, "key")?;
        let fact = required_str(row, "fact_sha256")?;
        if key.chars().count() == 0
            || key.chars().count() > 200
            || key.chars().any(|ch| ch < ' ')
            || !hex_ok(fact, 64)
        {
            return Err(Stop::Malformed("type"));
        }
        if !seen.insert(key.to_string()) {
            return Err(deny(scope, Some(hash), configured, "scope"));
        }
        kept.push(Curated {
            key: key.to_string(),
            fact_sha256: fact.to_string(),
        });
    }
    Ok(kept)
}

fn load_prior(
    root: &Map<String, Value>,
    scope: &Scope,
    hash: &str,
    configured: bool,
) -> Result<BTreeSet<String>, Stop> {
    let items = required(root, "prior_candidate_ids")?
        .as_array()
        .ok_or(Stop::Malformed("type"))?;
    if items.len() > 256 {
        return Err(deny(scope, Some(hash), configured, "scope"));
    }
    let mut prior = BTreeSet::new();
    for item in items {
        let text = item.as_str().ok_or(Stop::Malformed("type"))?;
        if !prior_ok(text) {
            return Err(Stop::Malformed("type"));
        }
        prior.insert(text.to_string());
    }
    Ok(prior)
}

fn build_candidates(inputs: &Inputs) -> Built {
    let edges: Vec<&Edge> = inputs
        .edges
        .iter()
        .filter(|edge| {
            let from = inputs.tables[edge.from].qualified.as_str();
            let to = inputs.tables[edge.to].qualified.as_str();
            !inputs
                .negatives
                .iter()
                .any(|(left, right)| (left == from && right == to) || (left == to && right == from))
        })
        .collect();
    let count = inputs.tables.len();
    let mut parent: Vec<usize> = (0..count).collect();
    for edge in &edges {
        unite(&mut parent, edge.from, edge.to);
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..count {
        let root = find(&mut parent, index);
        groups.entry(root).or_default().push(index);
    }
    let mut components: Vec<Vec<usize>> = groups.into_values().collect();
    for component in &mut components {
        component.sort_by(|&left, &right| {
            inputs.tables[left]
                .qualified
                .cmp(&inputs.tables[right].qualified)
        });
    }
    components.sort_by(|left, right| {
        inputs.tables[left[0]]
            .qualified
            .cmp(&inputs.tables[right[0]].qualified)
    });
    let mut outgoing = vec![0_u32; count];
    let mut incoming = vec![0_u32; count];
    for edge in &edges {
        outgoing[edge.from] += 1;
        incoming[edge.to] += 1;
    }

    let mut entities = Vec::new();
    let mut domain_members: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for component in &components {
        let members: Vec<String> = component
            .iter()
            .map(|&index| inputs.tables[index].qualified.clone())
            .collect();
        let primary = inputs.tables[primary_index(component, &outgoing, &incoming)]
            .qualified
            .clone();
        let mut schemas = BTreeSet::new();
        let mut weak = Vec::new();
        let mut strong = false;
        let mut reltuples = Vec::new();
        for &index in component {
            let table = &inputs.tables[index];
            schemas.insert(table.schema.clone());
            if let Some(text) = &table.reltuples {
                reltuples.push(json!({"reltuples": text, "table": table.qualified}));
            }
            for column in &table.columns {
                if column.semantic == "phone" || column.semantic == "person_name" {
                    weak.push(format!("{}.{}", table.qualified, column.name));
                }
                if column.strong {
                    strong = true;
                }
            }
        }
        weak.sort();
        reltuples.sort_by(|left, right| left["table"].as_str().cmp(&right["table"].as_str()));
        let domain = domain_id(&schemas);
        domain_members
            .entry(domain.clone())
            .or_default()
            .extend(members.iter().cloned());
        let edged = edges
            .iter()
            .any(|edge| component.contains(&edge.from) && component.contains(&edge.to));
        let provenance = if component.len() == 1 {
            "isolated"
        } else {
            "declared_fk"
        };
        let confidence = if component.len() == 1 {
            "lower"
        } else {
            "graph"
        };
        let strong_key_status = if !weak.is_empty() {
            "weak"
        } else if strong {
            "validated_unmerged"
        } else {
            "unknown"
        };
        let key = format!("entity:{}", members.join("|"));
        let fact = json!({
            "auto_merge": false,
            "confidence": confidence,
            "domain": domain,
            "members": members.clone(),
            "primary": primary,
            "provenance": provenance,
            "strong_key_status": strong_key_status,
            "weak_columns": weak.clone(),
        });
        entities.push(Entity {
            candidate_id: candidate_id(&inputs.scope.project, "entity", &key),
            key,
            members,
            primary,
            domain,
            provenance,
            confidence,
            strong_key_status,
            weak_columns: weak,
            profile_reltuples: reltuples,
            fact_sha256: canonical_sha256(&fact),
            edged,
        });
    }

    let mut metrics = Vec::new();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&left, &right| {
        inputs.tables[left]
            .qualified
            .cmp(&inputs.tables[right].qualified)
    });
    for index in order {
        let table = &inputs.tables[index];
        let time = table
            .columns
            .iter()
            .filter(|column| column.semantic == "time")
            .min_by(|left, right| left.name.cmp(&right.name))
            .map(|column| column.name.clone());
        let mut measures: Vec<&Col> = table
            .columns
            .iter()
            .filter(|column| column.semantic == "money" || column.semantic == "quantity")
            .collect();
        measures.sort_by(|left, right| left.name.cmp(&right.name));
        for column in measures {
            let (grain, time_dimension, time_anchor, time_anchor_closed, time_anchor_needed) =
                if let Some(time_name) = time.clone() {
                    (
                        vec!["month"],
                        Some(time_name),
                        Some("prev_month_start".to_string()),
                        Some(false),
                        false,
                    )
                } else {
                    (vec!["row"], None, None, None, true)
                };
            let sql = format!(
                "SELECT sum({}) FROM {}.{}",
                column.name, table.schema, table.name
            );
            let fact = json!({
                "certified": false,
                "column": column.name,
                "control_total_needed": true,
                "grain": grain,
                "owner_needed": true,
                "sql_template": sql,
                "status": "draft",
                "table": table.qualified,
                "time_anchor": time_anchor,
                "time_anchor_closed": time_anchor_closed,
                "time_anchor_needed": time_anchor_needed,
                "time_dimension": time_dimension,
            });
            let id_key = format!("{}.{}", table.qualified, column.name);
            metrics.push(Metric {
                candidate_id: candidate_id(&inputs.scope.project, "metric", &id_key),
                key: format!("metric:{id_key}"),
                column: column.name.clone(),
                table: table.qualified.clone(),
                grain,
                time_dimension,
                time_anchor,
                time_anchor_closed,
                time_anchor_needed,
                sql,
                fact_sha256: canonical_sha256(&fact),
            });
        }
    }

    let mut example_edges = edges;
    example_edges.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(
                inputs.tables[left.from]
                    .qualified
                    .cmp(&inputs.tables[right.from].qualified),
            )
            .then(left.column.cmp(&right.column))
    });
    let mut examples = Vec::new();
    for edge in example_edges {
        let table = &inputs.tables[edge.from];
        let sql = format!(
            "SELECT {column} FROM {schema}.{name} WHERE {column} IS NOT NULL LIMIT 1",
            column = edge.column,
            schema = table.schema,
            name = table.name
        );
        let question = format!(
            "Which values of {schema}.{name}.{column} does {constraint} relate?",
            schema = table.schema,
            name = table.name,
            column = edge.column,
            constraint = edge.name
        );
        let id_key = format!(
            "{}.{}.{}:{}",
            table.schema, table.name, edge.column, edge.name
        );
        examples.push(Example {
            candidate_id: candidate_id(&inputs.scope.project, "example", &id_key),
            key: format!("example:{id_key}"),
            constraint: edge.name.clone(),
            question,
            sql,
            validation: "not_run",
            admitted: false,
        });
    }

    let mut stales = Vec::new();
    for curated in &inputs.curated {
        let fact = entities
            .iter()
            .find(|item| item.key == curated.key)
            .map(|item| item.fact_sha256.clone())
            .or_else(|| {
                metrics
                    .iter()
                    .find(|item| item.key == curated.key)
                    .map(|item| item.fact_sha256.clone())
            });
        if let Some(fact) = fact
            && fact != curated.fact_sha256
        {
            stales.push((
                candidate_id(&inputs.scope.project, "stale", &curated.key),
                curated.key.clone(),
            ));
        }
    }
    stales.sort();
    let domains = domain_members
        .into_iter()
        .map(|(domain_id, members)| DomainOut {
            domain_id,
            members: members.into_iter().collect(),
        })
        .collect();
    Built {
        entities,
        domains,
        metrics,
        examples,
        stales,
        gateway: Gw::idle(false),
    }
}

fn run_gateway(gateway: &mut Gateway<'_>, examples: &mut [Example]) -> Result<Gw, Gw> {
    let probe = gateway.runner.run_select(gateway.probe_sql);
    let probe_exit = probe.exit.map(i64::from);
    if probe.timed_out || !probe.started || probe.exit != Some(6) || !probe.stdout.is_empty() {
        return Err(Gw {
            configured: true,
            probe: "not_refused",
            probe_exit,
            admitted: 0,
            spawned: 1,
        });
    }
    let mut admitted = 0_u64;
    let mut spawned = 1_u64;
    let limit = gateway.max_validated.min(examples.len());
    for example in examples.iter_mut().take(limit) {
        let result = gateway.runner.run_select(&example.sql);
        spawned += 1;
        classify(example, &result);
        if example.admitted {
            admitted += 1;
        }
    }
    Ok(Gw {
        configured: true,
        probe: "refused",
        probe_exit: Some(6),
        admitted,
        spawned,
    })
}

fn classify(example: &mut Example, result: &Spawned) {
    if !result.started || result.timed_out {
        example.validation = "error";
        return;
    }
    match result.exit {
        Some(6) => example.validation = "guard_refused",
        Some(0) => match summary(result) {
            Some((rows, true)) if rows >= 1 => {
                example.validation = "admitted";
                example.admitted = true;
            }
            Some((0, _)) => example.validation = "empty",
            _ => example.validation = "error",
        },
        _ => example.validation = "error",
    }
}

fn summary(result: &Spawned) -> Option<(u64, bool)> {
    let text = std::str::from_utf8(&result.stdout).ok()?;
    let mut found = None;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("record").and_then(Value::as_str) != Some("summary") {
            continue;
        }
        if let Some(rows) = value.get("returned_rows").and_then(Value::as_u64) {
            let succeeded = value.get("status").and_then(Value::as_str) == Some("succeeded");
            found = Some((rows, succeeded));
        }
    }
    found
}

fn render(inputs: &Inputs, built: &Built) -> Value {
    let mut ids = Vec::new();
    ids.extend(built.entities.iter().map(|item| item.candidate_id.clone()));
    ids.extend(built.metrics.iter().map(|item| item.candidate_id.clone()));
    ids.extend(built.examples.iter().map(|item| item.candidate_id.clone()));
    let feedback: Vec<(String, &Fb)> = inputs
        .feedback
        .iter()
        .map(|item| {
            let key = format!("{}|{}|{}", item.kind, item.target, item.text);
            (candidate_id(&inputs.scope.project, "feedback", &key), item)
        })
        .collect();
    ids.extend(feedback.iter().map(|(id, _)| id.clone()));
    let negatives: Vec<(String, String)> = inputs
        .negatives
        .iter()
        .map(|pair| {
            let key = format!("{}|{}", pair.0, pair.1);
            (candidate_id(&inputs.scope.project, "negative", &key), key)
        })
        .collect();
    ids.extend(negatives.iter().map(|(id, _)| id.clone()));
    ids.extend(built.stales.iter().map(|(id, _)| id.clone()));
    let overlaps: Vec<String> = ids
        .into_iter()
        .filter(|id| inputs.prior.contains(id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let overlap = |id: &str| overlaps.iter().any(|item| item == id);

    let entities = built
        .entities
        .iter()
        .map(|item| {
            json!({
                "auto_merge": false,
                "candidate_id": item.candidate_id,
                "confidence": item.confidence,
                "domain": item.domain,
                "fact_sha256": item.fact_sha256,
                "key": item.key,
                "knowledge_verified": false,
                "members": item.members,
                "primary": item.primary,
                "prior_overlap": overlap(&item.candidate_id),
                "profile_reltuples": item.profile_reltuples,
                "provenance": item.provenance,
                "schema_hash": inputs.schema_hash,
                "strong_key_status": item.strong_key_status,
                "weak_columns": item.weak_columns,
            })
        })
        .collect::<Vec<_>>();
    let domains = built
        .domains
        .iter()
        .map(|item| {
            json!({
                "domain_id": item.domain_id,
                "knowledge_verified": false,
                "members": item.members,
                "schema_hash": inputs.schema_hash,
            })
        })
        .collect::<Vec<_>>();
    let metrics = built
        .metrics
        .iter()
        .map(|item| {
            json!({
                "candidate_id": item.candidate_id,
                "certified": false,
                "column": item.column,
                "control_total_needed": true,
                "fact_sha256": item.fact_sha256,
                "gateway_admitted": false,
                "grain": item.grain,
                "key": item.key,
                "knowledge_verified": false,
                "owner_needed": true,
                "prior_overlap": overlap(&item.candidate_id),
                "sql_template": item.sql,
                "status": "draft",
                "table": item.table,
                "time_anchor": item.time_anchor,
                "time_anchor_closed": item.time_anchor_closed,
                "time_anchor_needed": item.time_anchor_needed,
                "time_dimension": item.time_dimension,
                "validation": "not_applicable",
            })
        })
        .collect::<Vec<_>>();
    let examples = built
        .examples
        .iter()
        .map(|item| {
            json!({
                "candidate_id": item.candidate_id,
                "constraint": item.constraint,
                "gateway_admitted": item.admitted,
                "key": item.key,
                "knowledge_verified": false,
                "prior_overlap": overlap(&item.candidate_id),
                "question": item.question,
                "sql_template": item.sql,
                "validation": item.validation,
            })
        })
        .collect::<Vec<_>>();
    let mut feedback_json: Vec<Value> = feedback
        .iter()
        .map(|(id, item)| {
            json!({
                "candidate_id": id,
                "kind": item.kind,
                "knowledge_verified": false,
                "prior_overlap": overlap(id),
                "target": item.target,
                "text": item.text,
                "trusted_metric": false,
                "untrusted": true,
            })
        })
        .collect();
    feedback_json.sort_by(|left, right| {
        left["candidate_id"]
            .as_str()
            .cmp(&right["candidate_id"].as_str())
    });

    let mut queue = Vec::new();
    for (id, key) in &built.stales {
        queue.push(queue_item(id, "stale_override", 9, 800, overlap(id), key));
    }
    for item in &built.metrics {
        queue.push(queue_item(
            &item.candidate_id,
            "metric_hypothesis",
            6,
            700,
            overlap(&item.candidate_id),
            &item.key,
        ));
    }
    for item in &built.examples {
        let uncertainty = if item.admitted { 400 } else { 900 };
        queue.push(queue_item(
            &item.candidate_id,
            "example",
            4,
            uncertainty,
            overlap(&item.candidate_id),
            &item.key,
        ));
    }
    for (id, key) in &negatives {
        queue.push(queue_item(id, "negative_entity", 5, 600, overlap(id), key));
    }
    for (id, item) in &feedback {
        let key = format!("feedback:{}:{}", item.kind, item.target);
        queue.push(queue_item(
            id,
            "untrusted_feedback",
            2,
            1000,
            overlap(id),
            &key,
        ));
    }
    for item in &built.entities {
        if item.edged {
            queue.push(queue_item(
                &item.candidate_id,
                "entity",
                8,
                200,
                overlap(&item.candidate_id),
                &item.key,
            ));
        } else {
            queue.push(queue_item(
                &item.candidate_id,
                "entity",
                3,
                500,
                overlap(&item.candidate_id),
                &item.key,
            ));
        }
    }
    queue.sort_by(|left, right| {
        right["score"]
            .as_u64()
            .cmp(&left["score"].as_u64())
            .then(left["queue_id"].as_str().cmp(&right["queue_id"].as_str()))
    });
    queue.truncate(128);

    json!({
        "schema": OUTCOME_SCHEMA,
        "scope": {
            "evidence_sha256": inputs.scope.evidence,
            "model_scope": inputs.scope.model_scope,
            "project": inputs.scope.project,
            "role": inputs.scope.role,
            "schema_hash": inputs.schema_hash,
            "schema_version": inputs.scope.schema_version,
            "source_id": inputs.scope.source_id,
        },
        "effects": effects(),
        "gateway": {
            "admitted": built.gateway.admitted,
            "configured": built.gateway.configured,
            "probe": built.gateway.probe,
            "probe_exit": built.gateway.probe_exit,
            "spawned": built.gateway.spawned,
        },
        "dedup": {
            "feedback_collapsed": inputs.fb_collapsed,
            "negative_collapsed": inputs.neg_collapsed,
            "prior_overlaps": overlaps,
            "relationships_collapsed": inputs.rel_collapsed,
            "relationships_rejected": inputs.rel_rejected,
        },
        "scope_drops": {
            "feedback": inputs.fb_drops,
            "relationships": inputs.rel_drops,
        },
        "entities": entities,
        "domains": domains,
        "metrics": metrics,
        "examples": examples,
        "feedback": feedback_json,
        "queue": queue,
    })
}

fn queue_item(
    id: &str,
    channel: &str,
    impact: u64,
    uncertainty: u64,
    overlap: bool,
    key: &str,
) -> Value {
    json!({
        "candidate_id": id,
        "channel": channel,
        "impact": impact,
        "key": key,
        "knowledge_verified": false,
        "prior_overlap": overlap,
        "queue_id": id,
        "score": impact * uncertainty,
        "trusted_metric": false,
        "uncertainty_millis": uncertainty,
    })
}

fn empty_outcome(scope: &Scope, schema_hash: Option<&str>, configured: bool) -> Value {
    let gateway = Gw::idle(configured);
    json!({
        "schema": OUTCOME_SCHEMA,
        "scope": {
            "evidence_sha256": scope.evidence,
            "model_scope": scope.model_scope,
            "project": scope.project,
            "role": scope.role,
            "schema_hash": schema_hash,
            "schema_version": scope.schema_version,
            "source_id": scope.source_id,
        },
        "effects": effects(),
        "gateway": {
            "admitted": gateway.admitted,
            "configured": gateway.configured,
            "probe": gateway.probe,
            "probe_exit": gateway.probe_exit,
            "spawned": gateway.spawned,
        },
        "dedup": {
            "feedback_collapsed": 0,
            "negative_collapsed": 0,
            "prior_overlaps": [],
            "relationships_collapsed": 0,
            "relationships_rejected": 0,
        },
        "scope_drops": {"feedback": 0, "relationships": 0},
        "entities": [],
        "domains": [],
        "metrics": [],
        "examples": [],
        "feedback": [],
        "queue": [],
    })
}

fn effects() -> Value {
    json!({
        "auto_merge": false,
        "curated_written": false,
        "grants_issued": false,
        "knowledge_verified_count": 0,
        "metrics_certified": false,
        "model_called": false,
        "published": false,
        "roles_changed": false,
        "samples_retained": false,
        "source_queried": false,
    })
}

fn deny(scope: &Scope, schema_hash: Option<&str>, configured: bool, reason: &'static str) -> Stop {
    Stop::Refused {
        reason,
        outcome: empty_outcome(scope, schema_hash, configured),
    }
}

fn primary_index(members: &[usize], outgoing: &[u32], incoming: &[u32]) -> usize {
    let mut best: Option<usize> = None;
    for &index in members {
        if outgoing[index] != 0 {
            continue;
        }
        best = Some(match best {
            None => index,
            Some(current) if incoming[index] > incoming[current] => index,
            Some(current) => current,
        });
    }
    best.unwrap_or(members[0])
}

fn domain_id(schemas: &BTreeSet<String>) -> String {
    if schemas.len() != 1 {
        return "cross_source".to_string();
    }
    match schemas.iter().next().map(String::as_str).unwrap_or("") {
        "shop" => "commerce".to_string(),
        "logistics" | "tms" => "transport".to_string(),
        "finance" | "erp" => "finance".to_string(),
        "wms" | "warehouse" => "warehouse".to_string(),
        other => other.to_string(),
    }
}

fn infer_semantic(name: &str, type_name: &str, pk: bool) -> &'static str {
    if type_name == "bool" {
        return "boolean_flag";
    }
    if matches!(type_name, "date" | "timestamp" | "timestamptz")
        || name.ends_with("_at")
        || name.ends_with("_on")
        || name.ends_with("_rq")
    {
        return "time";
    }
    if matches!(name, "amount" | "je" | "money") {
        return "money";
    }
    if matches!(name, "qty" | "quantity" | "sl") {
        return "quantity";
    }
    if matches!(name, "phone" | "mobile" | "lxdh") {
        return "phone";
    }
    if matches!(name, "name" | "display_name" | "khmc") {
        return "person_name";
    }
    if pk {
        return "identifier";
    }
    "unknown"
}

fn types_ok(child: &str, parent: &str) -> bool {
    const INTS: &[&str] = &["int2", "int4", "int8"];
    child == parent || (INTS.contains(&child) && INTS.contains(&parent))
}

fn find(parent: &mut [usize], mut index: usize) -> usize {
    let mut root = index;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[index] != root {
        let next = parent[index];
        parent[index] = root;
        index = next;
    }
    root
}

fn unite(parent: &mut [usize], left: usize, right: usize) {
    let left_root = find(parent, left);
    let right_root = find(parent, right);
    if left_root != right_root {
        parent[right_root] = left_root;
    }
}

fn ordered_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_string(), right.to_string())
    } else {
        (right.to_string(), left.to_string())
    }
}

fn qualified_name(value: &str) -> Result<String, Stop> {
    let Some((schema, name)) = value.split_once('.') else {
        return Err(Stop::Malformed("type"));
    };
    if value.matches('.').count() != 1 {
        return Err(Stop::Malformed("type"));
    }
    ident(schema)?;
    ident(name)?;
    Ok(value.to_string())
}

fn object(value: &Value) -> Result<&Map<String, Value>, Stop> {
    value.as_object().ok_or(Stop::Malformed("type"))
}

fn required<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, Stop> {
    map.get(key).ok_or(Stop::Malformed("type"))
}

fn required_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, Stop> {
    required(map, key)?.as_str().ok_or(Stop::Malformed("type"))
}

fn only(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), Stop> {
    if map
        .keys()
        .any(|key| !allowed.iter().any(|item| item == key))
    {
        return Err(Stop::Malformed("unknown_field"));
    }
    Ok(())
}

fn ident(value: &str) -> Result<(), Stop> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return Err(Stop::Malformed("type"));
    }
    let first = bytes[0];
    if !(first == b'_' || first.is_ascii_lowercase()) {
        return Err(Stop::Malformed("type"));
    }
    if bytes
        .iter()
        .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_'))
    {
        return Err(Stop::Malformed("type"));
    }
    Ok(())
}

fn string_list(value: &Value) -> Result<Vec<String>, Stop> {
    let items = value.as_array().ok_or(Stop::Malformed("type"))?;
    let mut names = Vec::new();
    for item in items {
        let text = item.as_str().ok_or(Stop::Malformed("type"))?;
        ident(text)?;
        names.push(text.to_string());
    }
    Ok(names)
}

fn project_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn role_ok(value: &str) -> bool {
    value
        .strip_prefix("oo_r_")
        .is_some_and(|rest| project_ok(rest) && rest.len() <= 31)
}

fn hex_ok(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn prior_ok(value: &str) -> bool {
    value
        .strip_prefix("cand_")
        .is_some_and(|rest| hex_ok(rest, 32))
}

fn text_ok(value: &str) -> bool {
    let count = value.chars().count();
    (1..=500).contains(&count) && value.chars().all(|ch| ch >= ' ')
}

fn target_ok(value: &str) -> bool {
    value.chars().count() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '.')
}

fn reltuples_ok(text: &str) -> bool {
    if text.is_empty() || text.len() > 32 {
        return false;
    }
    let mut parts = text.split('.');
    let Some(whole) = parts.next() else {
        return false;
    };
    let fraction = parts.next();
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    if whole.len() > 1 && whole.starts_with('0') {
        return false;
    }
    match fraction {
        None => true,
        Some(fraction) => {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        }
    }
}
