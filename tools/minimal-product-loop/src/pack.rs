//! Draft knowledge folder. LLM text stays unreviewed. Nothing here is published.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::source::SourceCounts;

pub fn write_draft(
    dir: &Path,
    counts: &SourceCounts,
    configured: &[(String, String)],
    profile: &Value,
    generation: Option<&Value>,
) -> Result<(), &'static str> {
    // lib.rs does not re-export write_sealed_pack. 411 owns that file.
    // Naming the function here keeps it reachable for the non-test build.
    let _reachable = std::hint::black_box(write_sealed_pack as fn(&Path, &Value) -> Result<Value, &'static str>);
    if configured.is_empty() || configured.len() > 16 || configured.len() != counts.rows.len() {
        return Err("pack");
    }
    let tables = dir.join("tables");
    let ontology = dir.join("ontology");
    fs::create_dir_all(&tables).map_err(|_| "pack")?;
    fs::create_dir_all(ontology.join("reviews")).map_err(|_| "pack")?;
    let banner = "status: draft\nreview_state: unreviewed\npublication: not_published\nmode: operator-development\nruntime: local-process-not-a-sandbox-certificate\n";
    let mut counted = String::new();
    for (item, count) in configured.iter().zip(&counts.rows) {
        counted.push_str(&format!("{}.{} rows: {count}\n", item.0, item.1));
    }
    let schemas = schema_list(configured.iter().map(|item| item.0.as_str()));
    let name = if configured.iter().all(|item| item.0 == configured[0].0) {
        configured[0].0.as_str()
    } else {
        "configured"
    };
    let readme = format!(
        "# DRAFT — unreviewed — not published\n\n\
This folder is an operator development preview. It is not an active production pack.\n\
LLM descriptions are unreviewed. Structural counts come from the connected database.\n\
Production authentication and Git publication remain TODO.\n\n\
{banner}\n\
source_sha256: {sha}\n\
server_version: {version}\n\
{counted}",
        sha = counts.source_sha256,
        version = counts.version,
    );
    fs::write(dir.join("README.md"), readme).map_err(|_| "pack")?;
    let profile_name = profile.get("protocol").and_then(Value::as_str).unwrap_or("absent");
    for (item, count) in configured.iter().zip(&counts.rows) {
        let qualified = format!("{}.{}", item.0, item.1);
        let body = format!(
            "# {qualified}\n\n{banner}\ndataset: {qualified}\nrow_count: {count}\nprovenance: source_count\nllm_review_state: unreviewed\n"
        );
        fs::write(tables.join(format!("{qualified}.md")), body).map_err(|_| "pack")?;
        let generated = ontology.join("generated").join("datasets").join(&item.0);
        fs::create_dir_all(&generated).map_err(|_| "pack")?;
        let yaml = format!(
            "dataset: {qualified}\nrow_count: {count}\nreview_state: unreviewed\npublication: not_published\nprovenance: source_count\nprofile_present: {profile_name}\n"
        );
        fs::write(generated.join(format!("{}.yaml", item.1)), yaml).map_err(|_| "pack")?;
    }
    let llm = match generation {
        None => "absent".to_owned(),
        Some(value) => format!(
            "present\nmodel_id: {model}\nreview_state: unreviewed\n",
            model = value.get("model_id").and_then(Value::as_str).unwrap_or("unreviewed")
        ),
    };
    let ontology_yaml = format!(
        "spec_version: oos/v1\nname: {name}-development-preview\nlocale: en-US\ninclude_schemas: [{schemas}]\npublication: not_published\nllm_review: unreviewed\ndevelopment_preview: true\n{banner}\nllm_generation: {llm}\n"
    );
    fs::write(ontology.join("ontology.yaml"), ontology_yaml).map_err(|_| "pack")?;
    fs::write(
        ontology.join("reviews").join("README.md"),
        "No authenticated review. No Git publication. LLM descriptions are unreviewed drafts.\n",
    )
    .map_err(|_| "pack")?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct PackColumn {
    pub name: String,
    pub type_name: String,
    pub admitted: bool,
}

#[derive(Clone, Debug)]
pub struct PackTable {
    pub logical_schema: String,
    pub logical_table: String,
    pub physical_schema: String,
    pub physical_table: String,
    pub rows: i64,
    pub columns: Vec<PackColumn>,
}

pub fn write_bulk_draft(
    dir: &Path,
    counts: &SourceCounts,
    tables: &[PackTable],
    import_hash: &str,
    snapshot_id: &str,
    generation: Option<&Value>,
) -> Result<(), &'static str> {
    if tables.is_empty() || tables.len() > 16 {
        return Err("pack");
    }
    let tables_dir = dir.join("tables");
    let ontology = dir.join("ontology");
    fs::create_dir_all(&tables_dir).map_err(|_| "pack")?;
    fs::create_dir_all(ontology.join("reviews")).map_err(|_| "pack")?;
    let banner = "status: draft\nreview_state: unreviewed\npublication: not_published\nmode: operator-development\nruntime: local-process-not-a-sandbox-certificate\n";
    let mut binding = String::from("# DRAFT — unreviewed — not published\n\ndeclared_foreign_key: not_declared\n");
    binding.push_str(&format!("import_hash: {import_hash}\nsnapshot_id: {snapshot_id}\n"));
    binding.push_str("gateway: schema-qualified names are admitted; aliases, natural joins, and subqueries are rejected\n");
    let mut counted = String::new();
    for table in tables {
        let qualified = format!("{}.{}", table.logical_schema, table.logical_table);
        counted.push_str(&format!("{qualified} rows: {}\n", table.rows));
        binding.push_str(&format!(
            "{qualified} physical: {}.{}\n",
            table.physical_schema, table.physical_table
        ));
        let admitted: Vec<String> = table
            .columns
            .iter()
            .filter(|column| column.admitted)
            .map(|column| format!("{} {}", column.name, column.type_name))
            .collect();
        let omitted: Vec<String> = table
            .columns
            .iter()
            .filter(|column| !column.admitted)
            .map(|column| format!("{} {}", column.name, column.type_name))
            .collect();
        let body = format!(
            "# {qualified}\n\n{banner}dataset: {qualified}\nrow_count: {rows}\nphysical: {physical_schema}.{physical}\ncolumns: {admitted}\nomitted: {omitted}\ndeclared_foreign_key: not_declared\nprovenance: sealed_manifest\nllm_review_state: unreviewed\n",
            rows = table.rows,
            physical_schema = table.physical_schema,
            physical = table.physical_table,
            admitted = if admitted.is_empty() { "none".to_owned() } else { admitted.join(", ") },
            omitted = if omitted.is_empty() { "none".to_owned() } else { omitted.join(", ") },
        );
        write_checked(&tables_dir.join(format!("{qualified}.md")), &body)?;
        let generated = ontology.join("generated").join("datasets").join(&table.logical_schema);
        fs::create_dir_all(&generated).map_err(|_| "pack")?;
        let yaml = format!(
            "dataset: {qualified}\nrow_count: {rows}\nreview_state: unreviewed\npublication: not_published\nprovenance: sealed_manifest\ncolumns: {admitted}\nomitted: {omitted}\n",
            rows = table.rows,
            admitted = if admitted.is_empty() { "none".to_owned() } else { admitted.join(", ") },
            omitted = if omitted.is_empty() { "none".to_owned() } else { omitted.join(", ") },
        );
        write_checked(&generated.join(format!("{}.yaml", table.logical_table)), &yaml)?;
    }
    let schemas = schema_list(tables.iter().map(|table| table.logical_schema.as_str()));
    let name = if tables.iter().all(|table| table.logical_schema == tables[0].logical_schema) {
        tables[0].logical_schema.as_str()
    } else {
        "configured"
    };
    write_checked(&dir.join("MANIFEST.md"), &binding)?;
    let readme = format!(
        "# DRAFT — unreviewed — not published\n\n\
This folder is an operator development preview of a sealed local copy. It is not an active production pack.\n\
LLM descriptions are unreviewed. Structural counts come from the sealed local database.\n\
No foreign key is declared. Production authentication and Git publication remain TODO.\n\n\
{banner}\n\
source_sha256: {sha}\n\
server_version: {version}\n\
import_hash: {import_hash}\n\
snapshot_id: {snapshot_id}\n\
{counted}",
        sha = counts.source_sha256,
        version = counts.version,
    );
    write_checked(&dir.join("README.md"), &readme)?;
    let llm = match generation {
        None => "absent".to_owned(),
        Some(value) => format!(
            "present\nmodel_id: {model}\nreview_state: unreviewed\n",
            model = value.get("model_id").and_then(Value::as_str).unwrap_or("unreviewed")
        ),
    };
    let ontology_yaml = format!(
        "spec_version: oos/v1\nname: {name}-development-preview\nlocale: en-US\ninclude_schemas: [{schemas}]\npublication: not_published\nllm_review: unreviewed\ndevelopment_preview: true\n{banner}llm_generation: {llm}\n"
    );
    write_checked(&ontology.join("ontology.yaml"), &ontology_yaml)?;
    write_checked(
        &ontology.join("reviews").join("README.md"),
        "No authenticated review. No Git publication. LLM descriptions are unreviewed drafts.\n",
    )?;
    Ok(())
}

const PACK_INPUT_PROTOCOL: &str = "oo.minimal-product-loop.pack-input/v1";
const PACK_RECEIPT_PROTOCOL: &str = "oo.minimal-product-loop.pack-receipt/v1";
const CANDIDATE_PROTOCOL: &str = "oo.minimal-product-loop.draft-candidate/v1";
const FOREIGN_KEY_PROTOCOL: &str = "oo.minimal-product-loop.local-copy-foreign-keys/v1";

const SHARED_KEYS: &[&str] = &[
    "card_sha256",
    "caveats",
    "confidence",
    "dataset",
    "description",
    "grain",
    "label",
    "open_questions",
];

/// Standard unreviewed draft. `input` is `oo.minimal-product-loop.pack-input/v1`.
/// The returned object is `oo.minimal-product-loop.pack-receipt/v1`.
/// Model text is copied from the validated generation. This function does not invent it.
pub fn write_sealed_pack(dir: &Path, input: &Value) -> Result<Value, &'static str> {
    let plan = prepare(input)?;
    let preserved = snapshot_preserved(dir)?;
    let unexpected = unexpected_datasets(dir, &plan)?;
    reject_symlink_chain(dir)?;
    write_plan(dir, &plan)?;
    if snapshot_preserved(dir)? != preserved {
        return Err("pack.curated");
    }
    receipt(dir, &plan, unexpected)
}

struct Binding {
    import_hash: String,
    snapshot_id: String,
    source_id: String,
    import_generation: u64,
    manifest_sha256: String,
    target_config_sha256: String,
}

struct CatalogTable {
    schema: String,
    name: String,
    columns: Vec<CatalogColumn>,
    primary_key: Vec<String>,
    foreign_keys: Vec<ForeignKey>,
}

struct CatalogColumn {
    name: String,
    attnum: i64,
    type_name: String,
    typmod: i32,
}

#[derive(Clone)]
struct ForeignKey {
    name: String,
    columns: Vec<ForeignColumn>,
}

#[derive(Clone)]
struct ForeignColumn {
    attnum: i64,
    name: String,
    ref_schema: String,
    ref_table: String,
    ref_attnum: i64,
    ref_name: String,
}

struct RequestedTable {
    schema: String,
    name: String,
    rows: u64,
    physical_schema: Option<String>,
    physical_table: Option<String>,
    columns: Option<Vec<RequestedColumn>>,
}

struct RequestedColumn {
    name: String,
    admitted: bool,
    type_name: Option<String>,
}

#[derive(Clone)]
struct GeneratedTable {
    label: String,
    description: String,
    grain: String,
    confidence: String,
    caveats: Vec<String>,
    open_questions: Vec<String>,
    effect: String,
    card_sha256: String,
    fields: Vec<GeneratedField>,
}

#[derive(Clone)]
struct GeneratedField {
    name: String,
    attnum: i64,
    label: String,
    description: String,
    confidence: String,
    caveats: Vec<String>,
    open_questions: Vec<String>,
    card_sha256: String,
}

struct Plan {
    binding: Binding,
    schema_hash: String,
    candidate_sha256: String,
    tables: Vec<PlannedTable>,
    warnings: Vec<String>,
}

struct PlannedTable {
    schema: String,
    name: String,
    rows: u64,
    reltuples: u64,
    physical_schema: Option<String>,
    physical_table: Option<String>,
    primary_key: Vec<String>,
    foreign_keys: Vec<ForeignKey>,
    generated: GeneratedTable,
    columns: Vec<PlannedColumn>,
}

struct PlannedColumn {
    name: String,
    attnum: i64,
    type_name: String,
    admitted: bool,
    label: String,
    description: String,
    confidence: String,
    caveats: Vec<String>,
    open_questions: Vec<String>,
    card_sha256: String,
}

fn prepare(input: &Value) -> Result<Plan, &'static str> {
    let root = as_map(input, "pack.input")?;
    exact_keys(
        root,
        &["binding", "catalog", "generation", "profile", "protocol", "tables"],
        "pack.input",
    )?;
    if root.get("protocol").and_then(Value::as_str) != Some(PACK_INPUT_PROTOCOL) {
        return Err("pack.input");
    }
    let binding = read_binding(root.get("binding").ok_or("pack.input")?)?;
    let requested = read_requests(root.get("tables").ok_or("pack.input")?)?;
    let catalog = read_catalog(root.get("catalog").ok_or("pack.input")?, &binding)?;
    let profiles = read_profile(root.get("profile").ok_or("pack.input")?)?;
    let generation = read_generation(
        root.get("generation").ok_or("pack.input")?,
        &binding,
        &catalog,
    )?;
    assemble(binding, requested, catalog, profiles, generation)
}

fn assemble(
    binding: Binding,
    requested: Vec<RequestedTable>,
    catalog: Catalog,
    profiles: BTreeMap<(String, String), u64>,
    generation: Generation,
) -> Result<Plan, &'static str> {
    let mut tables = Vec::new();
    let mut warnings = Vec::new();
    for catalog_table in &catalog.tables {
        let Some(request) = requested.iter().find(|item| {
            item.schema == catalog_table.schema && item.name == catalog_table.name
        }) else {
            continue;
        };
        let dataset = format!("{}.{}", catalog_table.schema, catalog_table.name);
        let generated = generation.tables.get(&dataset).ok_or("pack.generation")?;
        let reltuples = profiles
            .get(&(catalog_table.schema.clone(), catalog_table.name.clone()))
            .copied()
            .ok_or("pack.catalog")?;
        if request.rows != reltuples {
            warnings.push(format!("profile_reltuples_differs:{dataset}"));
        }
        if request.physical_schema.is_none() {
            warnings.push(format!("physical_not_supplied:{dataset}"));
        }
        let columns = plan_columns(catalog_table, request, generated)?;
        if columns.iter().all(|column| !column.admitted) {
            return Err("pack.catalog");
        }
        tables.push(PlannedTable {
            schema: catalog_table.schema.clone(),
            name: catalog_table.name.clone(),
            rows: request.rows,
            reltuples,
            physical_schema: request.physical_schema.clone(),
            physical_table: request.physical_table.clone(),
            primary_key: catalog_table.primary_key.clone(),
            foreign_keys: catalog_table.foreign_keys.clone(),
            generated: generated.clone(),
            columns,
        });
    }
    if tables.len() != requested.len() {
        return Err("pack.catalog");
    }
    warnings.sort();
    Ok(Plan {
        binding,
        schema_hash: catalog.schema_hash,
        candidate_sha256: generation.candidate_sha256,
        tables,
        warnings,
    })
}

fn plan_columns(
    catalog_table: &CatalogTable,
    request: &RequestedTable,
    generated: &GeneratedTable,
) -> Result<Vec<PlannedColumn>, &'static str> {
    if let Some(specified) = &request.columns {
        for item in specified {
            if !catalog_table.columns.iter().any(|column| column.name == item.name) {
                return Err("pack.catalog");
            }
        }
    }
    let mut columns = Vec::new();
    for (catalog_column, field) in catalog_table.columns.iter().zip(&generated.fields) {
        if field.name != catalog_column.name || field.attnum != catalog_column.attnum {
            return Err("pack.generation");
        }
        let admitted = admitted_column(catalog_column, request)?;
        columns.push(PlannedColumn {
            name: catalog_column.name.clone(),
            attnum: catalog_column.attnum,
            type_name: catalog_column.type_name.clone(),
            admitted,
            label: field.label.clone(),
            description: field.description.clone(),
            confidence: field.confidence.clone(),
            caveats: field.caveats.clone(),
            open_questions: field.open_questions.clone(),
            card_sha256: field.card_sha256.clone(),
        });
    }
    if columns.len() != generated.fields.len() {
        return Err("pack.generation");
    }
    Ok(columns)
}

fn admitted_column(column: &CatalogColumn, request: &RequestedTable) -> Result<bool, &'static str> {
    let gate = crate::bind::grantable(&column.type_name, column.typmod);
    let Some(specified) = request.columns.as_ref() else {
        return Ok(gate);
    };
    let Some(item) = specified.iter().find(|item| item.name == column.name) else {
        return Ok(gate);
    };
    if item.type_name.as_ref().is_some_and(|type_name| type_name != &column.type_name) {
        return Err("pack.input");
    }
    if item.admitted != gate {
        return Err("pack.input");
    }
    Ok(gate)
}

struct Catalog {
    schema_hash: String,
    tables: Vec<CatalogTable>,
}

struct Generation {
    candidate_sha256: String,
    tables: BTreeMap<String, GeneratedTable>,
}

fn read_binding(value: &Value) -> Result<Binding, &'static str> {
    let map = as_map(value, "pack.input")?;
    exact_keys(
        map,
        &[
            "import_generation",
            "import_hash",
            "manifest_sha256",
            "snapshot_id",
            "source_id",
            "target_config_sha256",
        ],
        "pack.input",
    )?;
    let import_generation = map.get("import_generation").and_then(Value::as_u64).ok_or("pack.input")?;
    if import_generation == 0 {
        return Err("pack.input");
    }
    let import_hash = required_hex(map, "import_hash")?;
    let manifest_sha256 = required_hex(map, "manifest_sha256")?;
    let target_config_sha256 = required_hex(map, "target_config_sha256")?;
    let snapshot_id = required_plain(map, "snapshot_id")?;
    let source_id = required_plain(map, "source_id")?;
    if !is_plugin_name(&source_id) {
        return Err("pack.identifier");
    }
    Ok(Binding {
        import_hash,
        snapshot_id,
        source_id,
        import_generation,
        manifest_sha256,
        target_config_sha256,
    })
}

fn read_requests(value: &Value) -> Result<Vec<RequestedTable>, &'static str> {
    let rows = value.as_array().ok_or("pack.input")?;
    if rows.is_empty() || rows.len() > 16 {
        return Err("pack.input");
    }
    let mut out = Vec::new();
    let mut paths = BTreeSet::new();
    for row in rows {
        let table = read_request(row)?;
        let path = format!(
            "{}/{}.yaml",
            table.schema.to_ascii_lowercase(),
            table.name.to_ascii_lowercase()
        );
        if !paths.insert(path) {
            return Err("pack.collision");
        }
        out.push(table);
    }
    Ok(out)
}

fn read_request(value: &Value) -> Result<RequestedTable, &'static str> {
    let map = as_map(value, "pack.input")?;
    for key in map.keys() {
        if !matches!(
            key.as_str(),
            "schema" | "name" | "rows" | "physical_schema" | "physical_table" | "columns"
        ) {
            return Err("pack.input");
        }
    }
    let schema = identifier_str(map, "schema", is_source_name)?;
    let name = identifier_str(map, "name", is_table_name)?;
    let rows = map.get("rows").and_then(Value::as_u64).ok_or("pack.input")?;
    let physical_schema = optional_physical(map, "physical_schema")?;
    let physical_table = optional_physical(map, "physical_table")?;
    if physical_schema.is_some() != physical_table.is_some() {
        return Err("pack.input");
    }
    let columns = match map.get("columns") {
        None => None,
        Some(columns) => Some(read_requested_columns(columns)?),
    };
    Ok(RequestedTable {
        schema,
        name,
        rows,
        physical_schema,
        physical_table,
        columns,
    })
}

fn read_requested_columns(value: &Value) -> Result<Vec<RequestedColumn>, &'static str> {
    let rows = value.as_array().ok_or("pack.input")?;
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        let map = as_map(row, "pack.input")?;
        for key in map.keys() {
            if !matches!(key.as_str(), "name" | "admitted" | "type_name") {
                return Err("pack.input");
            }
        }
        let name = identifier_str(map, "name", is_table_name)?;
        if !seen.insert(name.clone()) {
            return Err("pack.collision");
        }
        let admitted = map.get("admitted").and_then(Value::as_bool).ok_or("pack.input")?;
        let type_name = match map.get("type_name") {
            None => None,
            Some(type_name) => {
                let text = type_name.as_str().ok_or("pack.input")?;
                if !is_plain(text) {
                    return Err("pack.input");
                }
                Some(text.to_owned())
            }
        };
        out.push(RequestedColumn { name, admitted, type_name });
    }
    Ok(out)
}

fn optional_physical(map: &Map<String, Value>, key: &str) -> Result<Option<String>, &'static str> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => {
            let text = value.as_str().ok_or("pack.input")?;
            if !is_plain(text) {
                return Err("pack.input");
            }
            if !is_table_name(text) {
                return Err("pack.identifier");
            }
            Ok(Some(text.to_owned()))
        }
    }
}

fn read_catalog(value: &Value, binding: &Binding) -> Result<Catalog, &'static str> {
    let map = as_map(value, "pack.input")?;
    if map.get("protocol").and_then(Value::as_str) != Some("oo.source.pgsql/v1") {
        return Err("pack.input");
    }
    let source_id = map.get("source_id").and_then(Value::as_str).ok_or("pack.catalog")?;
    if source_id != binding.source_id {
        return Err("pack.binding");
    }
    let schema_hash = map.get("schema_hash").and_then(Value::as_str).ok_or("pack.catalog")?;
    if !is_hex64(schema_hash) {
        return Err("pack.catalog");
    }
    let rows = map.get("tables").and_then(Value::as_array).ok_or("pack.catalog")?;
    if rows.is_empty() || rows.len() > 16 {
        return Err("pack.catalog");
    }
    let mut tables = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        let table = read_catalog_table(row)?;
        let key = format!("{}/{}", table.schema.to_ascii_lowercase(), table.name.to_ascii_lowercase());
        if !seen.insert(key) {
            return Err("pack.collision");
        }
        tables.push(table);
    }
    Ok(Catalog { schema_hash: schema_hash.to_owned(), tables })
}

fn read_catalog_table(value: &Value) -> Result<CatalogTable, &'static str> {
    let map = as_map(value, "pack.catalog")?;
    let schema = catalog_name(map, "schema", "schema_ident", is_source_name)?;
    let name = catalog_name(map, "name", "name_ident", is_table_name)?;
    if map.get("relkind").and_then(Value::as_str) != Some("r") {
        return Err("pack.catalog");
    }
    let columns = read_catalog_columns(map.get("columns").ok_or("pack.catalog")?)?;
    let primary_key = read_primary_key(map.get("primary_key").ok_or("pack.catalog")?, &columns)?;
    let foreign_keys = read_foreign_keys(map.get("foreign_keys").ok_or("pack.catalog")?)?;
    Ok(CatalogTable { schema, name, columns, primary_key, foreign_keys })
}

fn read_catalog_columns(value: &Value) -> Result<Vec<CatalogColumn>, &'static str> {
    let rows = value.as_array().ok_or("pack.catalog")?;
    if rows.is_empty() {
        return Err("pack.catalog");
    }
    let mut columns = Vec::new();
    let mut names = BTreeSet::new();
    let mut attnums = BTreeSet::new();
    for row in rows {
        let map = as_map(row, "pack.catalog")?;
        let name = catalog_name(map, "name", "name_ident", is_table_name)?;
        let attnum = map.get("attnum").and_then(Value::as_i64).ok_or("pack.catalog")?;
        let type_name = map.get("type_name").and_then(Value::as_str).ok_or("pack.catalog")?;
        if !is_plain(type_name) {
            return Err("pack.catalog");
        }
        let typmod = read_i32(map.get("typmod").ok_or("pack.catalog")?)?;
        if !names.insert(name.clone()) || !attnums.insert(attnum) {
            return Err("pack.collision");
        }
        columns.push(CatalogColumn { name, attnum, type_name: type_name.to_owned(), typmod });
    }
    columns.sort_by_key(|column| column.attnum);
    Ok(columns)
}

fn read_primary_key(value: &Value, columns: &[CatalogColumn]) -> Result<Vec<String>, &'static str> {
    let rows = value.as_array().ok_or("pack.catalog")?;
    let mut names = Vec::new();
    for row in rows {
        let map = as_map(row, "pack.catalog")?;
        let name = map.get("name").and_then(Value::as_str).ok_or("pack.catalog")?;
        if !is_plain(name) || !columns.iter().any(|column| column.name == name) {
            return Err("pack.catalog");
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

fn read_foreign_keys(value: &Value) -> Result<Vec<ForeignKey>, &'static str> {
    let rows = value.as_array().ok_or("pack.catalog")?;
    let mut keys = Vec::new();
    for row in rows {
        keys.push(read_foreign_key(row)?);
    }
    Ok(keys)
}

fn read_foreign_key(value: &Value) -> Result<ForeignKey, &'static str> {
    let map = as_map(value, "pack.catalog")?;
    let name = map.get("name").and_then(Value::as_str).ok_or("pack.catalog")?;
    if !is_plain(name) {
        return Err("pack.catalog");
    }
    let rows = map.get("columns").and_then(Value::as_array).ok_or("pack.catalog")?;
    if rows.is_empty() {
        return Err("pack.catalog");
    }
    let mut columns = Vec::new();
    for row in rows {
        columns.push(read_foreign_column(row)?);
    }
    Ok(ForeignKey { name: name.to_owned(), columns })
}

fn read_foreign_column(value: &Value) -> Result<ForeignColumn, &'static str> {
    let map = as_map(value, "pack.catalog")?;
    Ok(ForeignColumn {
        attnum: map.get("attnum").and_then(Value::as_i64).ok_or("pack.catalog")?,
        name: required_plain(map, "name").map_err(|_| "pack.catalog")?,
        ref_schema: required_plain(map, "ref_schema").map_err(|_| "pack.catalog")?,
        ref_table: required_plain(map, "ref_table").map_err(|_| "pack.catalog")?,
        ref_attnum: map.get("ref_attnum").and_then(Value::as_i64).ok_or("pack.catalog")?,
        ref_name: required_plain(map, "ref_name").map_err(|_| "pack.catalog")?,
    })
}

fn read_profile(value: &Value) -> Result<BTreeMap<(String, String), u64>, &'static str> {
    let map = as_map(value, "pack.input")?;
    if map.get("protocol").and_then(Value::as_str) != Some("oo.pg-builder.profile/v1") {
        return Err("pack.input");
    }
    if map.get("sample_policy").and_then(Value::as_str) != Some("none") {
        return Err("pack.input");
    }
    let rows = map.get("tables").and_then(Value::as_array).ok_or("pack.catalog")?;
    let mut out = BTreeMap::new();
    for row in rows {
        let table = as_map(row, "pack.catalog")?;
        let schema = table.get("schema").and_then(Value::as_str).ok_or("pack.catalog")?;
        let name = table.get("name").and_then(Value::as_str).ok_or("pack.catalog")?;
        let reltuples = table.get("reltuples").and_then(Value::as_str).ok_or("pack.catalog")?;
        if !is_source_name(schema) || !is_table_name(name) || !is_row_count(reltuples) {
            return Err("pack.catalog");
        }
        let count = reltuples.parse::<u64>().map_err(|_| "pack.catalog")?;
        if out.insert((schema.to_owned(), name.to_owned()), count).is_some() {
            return Err("pack.collision");
        }
    }
    Ok(out)
}

fn read_generation(value: &Value, binding: &Binding, catalog: &Catalog) -> Result<Generation, &'static str> {
    let map = as_map(value, "pack.input")?;
    if map.get("protocol").and_then(Value::as_str) != Some("oo.pg-builder.generation/v1") {
        return Err("pack.input");
    }
    if map.get("outcome").and_then(Value::as_str) != Some("validated") {
        return Err("pack.input");
    }
    if map.get("transport").and_then(|item| item.get("model_id")).and_then(Value::as_str) != Some("grok-4.7") {
        return Err("pack.input");
    }
    check_cost(map)?;
    check_generation_shell(map)?;
    let calls = map.get("calls").and_then(Value::as_array).ok_or("pack.generation")?;
    let call_count = u64::try_from(calls.len()).map_err(|_| "pack.generation")?;
    // One call is one column batch. Eight tables of 64 columns are 64 batches.
    if calls.is_empty() || calls.len() > 64 || map.get("call_count").and_then(Value::as_u64) != Some(call_count) {
        return Err("pack.generation");
    }
    let candidates = map.get("candidates").ok_or("pack.generation")?;
    let candidate_sha256 = map.get("candidate_sha256").and_then(Value::as_str).ok_or("pack.generation")?;
    // pg-builder does not export rebind_generation. Hash, statement, seam, and captured-body equality are the checks available here.
    if !is_hex64(candidate_sha256) || candidate_sha256 != pg_builder::canonical_sha256(candidates) {
        return Err("pack.generation");
    }
    let mut field_rows = Vec::new();
    let mut table_rows = Vec::new();
    let mut pieces = Vec::new();
    for call in calls {
        let parsed = read_call(call, binding, catalog)?;
        field_rows.extend(parsed.field_values.iter().cloned());
        if let Some(table) = &parsed.table_value {
            table_rows.push(table.clone());
        }
        pieces.push(parsed);
    }
    let rebuilt = serde_json::json!({"field": field_rows, "table": table_rows});
    if &rebuilt != candidates {
        return Err("pack.generation");
    }
    let tables = merge_column_batches(pieces, catalog)?;
    Ok(Generation { candidate_sha256: candidate_sha256.to_owned(), tables })
}

fn check_generation_shell(map: &Map<String, Value>) -> Result<(), &'static str> {
    if map.get("compiler").and_then(Value::as_str) != Some("oo.pg-builder/v1") {
        return Err("pack.generation");
    }
    if map.get("repair").and_then(Value::as_str) != Some("not_performed") {
        return Err("pack.generation");
    }
    if !matches!(map.get("cache").and_then(Value::as_str), Some("miss" | "hit" | "partial")) {
        return Err("pack.generation");
    }
    if map.get("complete_command") != Some(&Value::Null) {
        return Err("pack.generation");
    }
    let statements = expected_statements();
    let seam = pg_builder::field_seam();
    if map.get("statements") != Some(&statements) || map.get("field_publication_seam") != Some(&seam) {
        return Err("pack.generation");
    }
    Ok(())
}

fn check_cost(map: &Map<String, Value>) -> Result<(), &'static str> {
    let Some(transport) = map.get("transport") else {
        return Err("pack.input");
    };
    let Some(cost) = transport.get("declared_cost") else {
        return Ok(());
    };
    let cost = as_map(cost, "pack.input")?;
    match cost.get("status") {
        None => {}
        Some(status) if status.as_str() == Some("unknown") => {}
        Some(_) => return Err("pack.input"),
    }
    if cost.get("billing_certificate") == Some(&Value::Bool(true)) {
        return Err("pack.generation");
    }
    Ok(())
}

struct CallPiece {
    dataset: String,
    index: u64,
    count: u64,
    from: u64,
    to: u64,
    fields: Vec<GeneratedField>,
    field_values: Vec<Value>,
    table: Option<GeneratedTable>,
    table_value: Option<Value>,
}

fn read_call(value: &Value, binding: &Binding, catalog: &Catalog) -> Result<CallPiece, &'static str> {
    let map = as_map(value, "pack.generation")?;
    let dataset = map.get("dataset").and_then(Value::as_str).ok_or("pack.generation")?;
    let catalog_table = catalog
        .tables
        .iter()
        .find(|table| format!("{}.{}", table.schema, table.name) == dataset)
        .ok_or("pack.generation")?;
    let (index, count, from, to) = check_batch(map, catalog_table.columns.len())?;
    check_call_shell(map, index > 0)?;
    let candidates = map.get("candidates").ok_or("pack.generation")?;
    let hash = map.get("candidate_sha256").and_then(Value::as_str).ok_or("pack.generation")?;
    if !is_hex64(hash) || hash != pg_builder::canonical_sha256(candidates) {
        return Err("pack.generation");
    }
    let candidate_map = as_map(candidates, "pack.generation")?;
    exact_keys(candidate_map, &["field", "table"], "pack.generation")?;
    let table_values = candidate_map.get("table").and_then(Value::as_array).ok_or("pack.generation")?;
    let field_values = candidate_map.get("field").and_then(Value::as_array).ok_or("pack.generation")?;
    let start = usize::try_from(from).map_err(|_| "pack.generation")?;
    let end = usize::try_from(to).map_err(|_| "pack.generation")?;
    if field_values.len() != end - start {
        return Err("pack.generation");
    }
    let mut parsed_fields = Vec::new();
    for field in field_values {
        parsed_fields.push(read_field(field, dataset)?);
    }
    let mut ordered = Vec::new();
    for column in &catalog_table.columns[start..end] {
        let Some(field) = parsed_fields.iter().find(|field| field.name == column.name && field.attnum == column.attnum) else {
            return Err("pack.generation");
        };
        ordered.push(field.clone());
    }
    if index == 0 {
        if table_values.len() != 1 {
            return Err("pack.generation");
        }
        let outcome_table = outcome_table(map, dataset, &catalog.schema_hash, &binding.source_id)?;
        let body_table = captured_table(map, dataset, &catalog.schema_hash, &binding.source_id)?;
        same_shared(&table_values[0], &outcome_table)?;
        same_shared(&table_values[0], &body_table)?;
        let generated = read_table_card(&table_values[0], dataset)?;
        return Ok(CallPiece {
            dataset: dataset.to_owned(),
            index,
            count,
            from,
            to,
            fields: ordered,
            field_values: field_values.clone(),
            table: Some(generated),
            table_value: Some(table_values[0].clone()),
        });
    }
    if !table_values.is_empty() || map.get("table_outcome").is_some_and(|item| !item.is_null()) {
        return Err("pack.generation");
    }
    Ok(CallPiece {
        dataset: dataset.to_owned(),
        index,
        count,
        from,
        to,
        fields: ordered,
        field_values: field_values.clone(),
        table: None,
        table_value: None,
    })
}

fn merge_column_batches(pieces: Vec<CallPiece>, catalog: &Catalog) -> Result<BTreeMap<String, GeneratedTable>, &'static str> {
    let mut grouped: BTreeMap<String, Vec<CallPiece>> = BTreeMap::new();
    for piece in pieces {
        grouped.entry(piece.dataset.clone()).or_default().push(piece);
    }
    let mut tables = BTreeMap::new();
    for (dataset, mut group) in grouped {
        let catalog_table = catalog
            .tables
            .iter()
            .find(|table| format!("{}.{}", table.schema, table.name) == dataset)
            .ok_or("pack.generation")?;
        group.sort_by_key(|piece| piece.index);
        let spans: Vec<(u64, u64, u64, u64)> = group.iter().map(|piece| (piece.index, piece.count, piece.from, piece.to)).collect();
        let column_count = u64::try_from(catalog_table.columns.len()).map_err(|_| "pack.generation")?;
        cover_column_batches(&spans, column_count)?;
        let mut generated = None;
        let mut fields = Vec::new();
        for piece in &group {
            if piece.index == 0 {
                generated = piece.table.clone();
            } else if piece.table.is_some() {
                return Err("pack.generation");
            }
            fields.extend(piece.fields.iter().cloned());
        }
        let mut generated = generated.ok_or("pack.generation")?;
        if fields.len() != catalog_table.columns.len() {
            return Err("pack.generation");
        }
        for (field, column) in fields.iter().zip(&catalog_table.columns) {
            if field.name != column.name || field.attnum != column.attnum {
                return Err("pack.generation");
            }
        }
        generated.fields = fields;
        if tables.insert(dataset, generated).is_some() {
            return Err("pack.collision");
        }
    }
    Ok(tables)
}

fn cover_column_batches(spans: &[(u64, u64, u64, u64)], column_count: u64) -> Result<(), &'static str> {
    if spans.is_empty() || column_count == 0 {
        return Err("pack.generation");
    }
    let count = spans[0].1;
    if spans.len() != usize::try_from(count).unwrap_or(usize::MAX) {
        return Err("pack.generation");
    }
    let mut covered = 0u64;
    for (expected, span) in spans.iter().enumerate() {
        let (index, span_count, from, to) = *span;
        if span_count != count || index != u64::try_from(expected).map_err(|_| "pack.generation")? || from != covered || to <= from {
            return Err("pack.generation");
        }
        covered = to;
    }
    if covered == column_count { Ok(()) } else { Err("pack.generation") }
}

fn check_call_shell(map: &Map<String, Value>, continuation: bool) -> Result<(), &'static str> {
    if map.get("protocol").and_then(Value::as_str) != Some("oo.pg-builder.generation/v1") {
        return Err("pack.generation");
    }
    if map.get("compiler").and_then(Value::as_str) != Some("oo.pg-builder/v1") {
        return Err("pack.generation");
    }
    if map.get("outcome").and_then(Value::as_str) != Some("validated") {
        return Err("pack.generation");
    }
    if map.get("field_outcome").and_then(Value::as_str) != Some("validated") {
        return Err("pack.generation");
    }
    if map.get("transport").and_then(|item| item.get("model_id")).and_then(Value::as_str) != Some("grok-4.7") {
        return Err("pack.generation");
    }
    let statements = expected_statements();
    let seam = pg_builder::field_seam();
    if map.get("statements") != Some(&statements) || map.get("field_publication_seam") != Some(&seam) {
        return Err("pack.generation");
    }
    if continuation {
        if !map.get("complete_command").is_some_and(Value::is_null) {
            return Err("pack.generation");
        }
    } else if map.get("complete_command").and_then(Value::as_str).is_none() {
        return Err("pack.generation");
    }
    Ok(())
}

fn check_batch(map: &Map<String, Value>, column_count: usize) -> Result<(u64, u64, u64, u64), &'static str> {
    let batch = map.get("batch").ok_or("pack.generation")?;
    let index = batch.get("index").and_then(Value::as_u64).ok_or("pack.generation")?;
    let count = batch.get("count").and_then(Value::as_u64).ok_or("pack.generation")?;
    let from = batch.get("column_from").and_then(Value::as_u64).ok_or("pack.generation")?;
    let to = batch.get("column_to").and_then(Value::as_u64).ok_or("pack.generation")?;
    let width = u64::try_from(column_count).map_err(|_| "pack.generation")?;
    if !(1..=64).contains(&count) || index >= count || from >= to || to > width {
        return Err("pack.generation");
    }
    Ok((index, count, from, to))
}

fn outcome_table(
    map: &Map<String, Value>,
    dataset: &str,
    schema_hash: &str,
    source_id: &str,
) -> Result<Value, &'static str> {
    let outcome = map.get("table_outcome").ok_or("pack.generation")?;
    if outcome.get("protocol").and_then(Value::as_str) != Some("oo.builder.description-candidate/v1")
        || outcome.get("outcome").and_then(Value::as_str) != Some("candidates")
        || outcome.get("compiler").and_then(Value::as_str) != Some("oo-builder/description-candidate/v1")
    {
        return Err("pack.generation");
    }
    check_outcome_statements(outcome.get("statements").ok_or("pack.generation")?)?;
    let draft = outcome.get("structural_draft").ok_or("pack.generation")?;
    if draft.get("protocol").and_then(Value::as_str) != Some("oo.builder.structure-draft/v1")
        || draft.get("schema_hash").and_then(Value::as_str) != Some(schema_hash)
        || draft.get("source_id").and_then(Value::as_str) != Some(source_id)
    {
        return Err("pack.generation");
    }
    let selected = outcome.get("selected_datasets").and_then(Value::as_array).ok_or("pack.generation")?;
    if selected.len() != 1 || selected[0].as_str() != Some(dataset) {
        return Err("pack.generation");
    }
    let candidates = outcome.get("candidates").and_then(Value::as_array).ok_or("pack.generation")?;
    if candidates.len() != 1 {
        return Err("pack.generation");
    }
    let card = as_map(&candidates[0], "pack.generation")?;
    exact_keys(card, &table_keys(), "pack.generation")?;
    if card.get("effect").and_then(Value::as_str) != Some("review_only") {
        return Err("pack.generation");
    }
    Ok(candidates[0].clone())
}

fn check_outcome_statements(value: &Value) -> Result<(), &'static str> {
    let map = as_map(value, "pack.generation")?;
    let expected = [
        ("publication", "unavailable"),
        ("samples", "not_included"),
        ("pii_classification", "not_performed"),
        ("auto_accept", "not_authorized"),
        ("grants", "not_authorized"),
        ("curated", "not_written"),
        ("effective", "not_written"),
    ];
    for (key, want) in expected {
        if map.get(key).and_then(Value::as_str) != Some(want) {
            return Err("pack.generation");
        }
    }
    Ok(())
}

fn captured_table(
    map: &Map<String, Value>,
    dataset: &str,
    schema_hash: &str,
    source_id: &str,
) -> Result<Value, &'static str> {
    let command_text = map.get("complete_command").and_then(Value::as_str).ok_or("pack.generation")?;
    let command: Value = serde_json::from_str(command_text).map_err(|_| "pack.generation")?;
    let captured = command.get("captured_response").ok_or("pack.generation")?;
    if captured.get("status").and_then(Value::as_str) != Some("complete") {
        return Err("pack.generation");
    }
    let body_text = captured.get("body").and_then(Value::as_str).ok_or("pack.generation")?;
    let byte_len = captured.get("byte_len").and_then(Value::as_u64).ok_or("pack.generation")?;
    let body_len = u64::try_from(body_text.len()).map_err(|_| "pack.generation")?;
    if byte_len != body_len {
        return Err("pack.generation");
    }
    let body: Value = serde_json::from_str(body_text).map_err(|_| "pack.generation")?;
    if body.get("protocol").and_then(Value::as_str) != Some("oo.builder.description-response/v1")
        || body.get("complete") != Some(&Value::Bool(true))
        || body.get("model_id").and_then(Value::as_str) != Some("grok-4.7")
        || body.get("source_id").and_then(Value::as_str) != Some(source_id)
        || body.get("schema_hash").and_then(Value::as_str) != Some(schema_hash)
    {
        return Err("pack.generation");
    }
    let candidates = body.get("candidates").and_then(Value::as_array).ok_or("pack.generation")?;
    if candidates.len() != 1 {
        return Err("pack.generation");
    }
    let card = as_map(&candidates[0], "pack.generation")?;
    exact_keys(card, SHARED_KEYS, "pack.generation")?;
    if card.get("dataset").and_then(Value::as_str) != Some(dataset) {
        return Err("pack.generation");
    }
    Ok(candidates[0].clone())
}

fn read_table_card(value: &Value, dataset: &str) -> Result<GeneratedTable, &'static str> {
    let map = as_map(value, "pack.generation")?;
    exact_keys(map, &table_keys(), "pack.generation")?;
    if map.get("effect").and_then(Value::as_str) != Some("review_only") {
        return Err("pack.generation");
    }
    if map.get("dataset").and_then(Value::as_str) != Some(dataset) {
        return Err("pack.generation");
    }
    Ok(GeneratedTable {
        label: model_string(map, "label", 256)?,
        description: model_string(map, "description", 4000)?,
        grain: model_string(map, "grain", 256)?,
        confidence: confidence_string(map)?,
        caveats: text_list(map.get("caveats").ok_or("pack.generation")?)?,
        open_questions: text_list(map.get("open_questions").ok_or("pack.generation")?)?,
        effect: "review_only".to_owned(),
        card_sha256: hex_string(map, "card_sha256")?,
        fields: Vec::new(),
    })
}

fn read_field(value: &Value, dataset: &str) -> Result<GeneratedField, &'static str> {
    let map = as_map(value, "pack.generation")?;
    exact_keys(
        map,
        &[
            "attnum",
            "card_sha256",
            "caveats",
            "column",
            "confidence",
            "dataset",
            "description",
            "label",
            "open_questions",
        ],
        "pack.generation",
    )?;
    if map.get("dataset").and_then(Value::as_str) != Some(dataset) {
        return Err("pack.generation");
    }
    let name = map.get("column").and_then(Value::as_str).ok_or("pack.generation")?;
    if !is_table_name(name) {
        return Err("pack.identifier");
    }
    Ok(GeneratedField {
        name: name.to_owned(),
        attnum: map.get("attnum").and_then(Value::as_i64).ok_or("pack.generation")?,
        label: model_string(map, "label", 256)?,
        description: model_string(map, "description", 4000)?,
        confidence: confidence_string(map)?,
        caveats: text_list(map.get("caveats").ok_or("pack.generation")?)?,
        open_questions: text_list(map.get("open_questions").ok_or("pack.generation")?)?,
        card_sha256: hex_string(map, "card_sha256")?,
    })
}

fn table_keys() -> Vec<&'static str> {
    let mut keys = SHARED_KEYS.to_vec();
    keys.push("effect");
    keys
}

fn same_shared(left: &Value, right: &Value) -> Result<(), &'static str> {
    for key in SHARED_KEYS {
        if left.get(*key) != right.get(*key) {
            return Err("pack.generation");
        }
    }
    Ok(())
}

fn expected_statements() -> Value {
    serde_json::json!({
        "authentication": "not_claimed",
        "authorization": "not_claimed",
        "parent_auth": "required",
        "reviewer": "declared_only",
        "grants": "not_authorized",
        "publication": "unavailable",
        "pii_classification": "not_performed",
        "semantic_classification": "not_performed",
        "curated": "not_written",
        "effective": "not_written",
        "pack": "not_compiled",
        "samples": "not_included",
        "llm_decides_review": false,
        "billing_certificate": false,
        "repair": "not_performed"
    })
}

fn write_plan(dir: &Path, plan: &Plan) -> Result<(), &'static str> {
    let files = rendered_files(plan)?;
    for (relative, _) in &files {
        reject_owned(relative)?;
        let path = dir.join(relative);
        reject_symlink_chain(&path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| "pack")?;
        }
    }
    for (relative, body) in files {
        fs::write(dir.join(&relative), body).map_err(|_| "pack")?;
    }
    Ok(())
}

fn rendered_files(plan: &Plan) -> Result<Vec<(String, String)>, &'static str> {
    let mut files = vec![
        ("README.md".to_owned(), render_readme(plan)),
        ("ontology/ontology.yaml".to_owned(), render_ontology(plan)?),
        (
            "ontology/reviews/README.md".to_owned(),
            "No authenticated review. No Git publication. LLM descriptions are unreviewed drafts.\n".to_owned(),
        ),
        (
            "ontology/generated/evidence/foreign-keys.json".to_owned(),
            render_foreign_keys(plan)?,
        ),
    ];
    for table in &plan.tables {
        let qualified = format!("{}.{}", table.schema, table.name);
        files.push((format!("tables/{qualified}.md"), render_table_md(table)));
        files.push((
            format!("ontology/generated/datasets/{}/{}.yaml", table.schema, table.name),
            render_dataset(table)?,
        ));
        files.push((
            format!("ontology/generated/candidates/{}/{}.json", table.schema, table.name),
            render_candidate(plan, table)?,
        ));
    }
    Ok(files)
}

fn render_ontology(plan: &Plan) -> Result<String, &'static str> {
    let mut schemas = Vec::new();
    for table in &plan.tables {
        if !schemas.iter().any(|schema| schema == &table.schema) {
            schemas.push(table.schema.clone());
        }
    }
    let mut body = String::new();
    body.push_str("spec_version: oos/v1\n");
    body.push_str(&format!("name: {}\n", yaml_plain(&plan.binding.source_id)));
    body.push_str("locale: en-US\n");
    body.push_str(&format!("include_schemas: {}\n", flow_plain(&schemas)));
    body.push_str("default_tier_preference: [raw]\n");
    body.push_str("publish:\n  include_unreviewed: []\n  require_eval: []\n");
    body.push_str("classification:\n  auto_accept: not_authorized\n");
    body.push_str("lifecycle:\n  deprecation_min_releases: 1\n");
    body.push_str("compile:\n  per_role: true\n  budgets:\n    readme_tokens: 2000\n    table_file_tokens: 3000\n");
    body.push_str("  sample_values:\n    default: none\n    max_per_column: 0\n");
    Ok(body)
}

fn render_dataset(table: &PlannedTable) -> Result<String, &'static str> {
    let qualified = format!("{}.{}", table.schema, table.name);
    let mut body = String::new();
    body.push_str("schema_version: oos/v1\n");
    body.push_str(&format!("dataset: {}\n", yaml_plain(&qualified)));
    body.push_str(&format!("label: {}\n", yaml_quote(&table.generated.label)));
    body.push_str(&format!("description: {}\n", yaml_quote(&table.generated.description)));
    body.push_str("tier: raw\n");
    body.push_str("recommended: false\n");
    body.push_str(&format!("grain: {}\n", yaml_quote(&table.generated.grain)));
    if !table.primary_key.is_empty() {
        body.push_str(&format!("primary_key: {}\n", flow_plain(&table.primary_key)));
    }
    body.push_str(&format!("row_count_estimate: {}\n", table.reltuples));
    push_caveats(&mut body, &table.generated.caveats);
    body.push_str("columns:\n");
    for column in &table.columns {
        body.push_str(&format!("  - name: {}\n", yaml_quote(&column.name)));
        body.push_str(&format!("    label: {}\n", yaml_quote(&column.label)));
        body.push_str(&format!("    data_type: {}\n", yaml_plain(&column.type_name)));
        body.push_str(&format!("    description: {}\n", yaml_quote(&column.description)));
        body.push_str("    provenance: generated\n");
    }
    Ok(body)
}

fn push_caveats(body: &mut String, caveats: &[String]) {
    if caveats.is_empty() {
        return;
    }
    body.push_str("caveats:\n");
    for caveat in caveats {
        body.push_str(&format!("  - {}\n", yaml_quote(caveat)));
    }
}

fn render_readme(plan: &Plan) -> String {
    let mut body = String::from(
        "# DRAFT — unreviewed — not published\n\n\
This folder is an operator development preview. It is not an active production pack.\n\
LLM descriptions are unreviewed. No human review and no Git publication are recorded here.\n\n\
Read this file first. Then read the unreviewed draft files listed below.\n\
Write your own statement. Cite query_id from the gateway response.\n\n\
review: unreviewed\n\
publication: not_published\n\
human_approval: false\n",
    );
    body.push_str(&format!("source_id: {}\n", plan.binding.source_id));
    body.push_str(&format!("import_hash: {}\n", plan.binding.import_hash));
    body.push_str(&format!("snapshot_id: {}\n", plan.binding.snapshot_id));
    body.push_str(&format!("import_generation: {}\n", plan.binding.import_generation));
    body.push_str(&format!("schema_hash: {}\n", plan.schema_hash));
    body.push_str("model_id: grok-4.7\n");
    body.push_str(&format!("local_copy_foreign_keys: {}\n", foreign_key_count(plan)));
    body.push_str("original_source_metadata: unknown\n\nDraft files:\n");
    body.push_str("- ontology/ontology.yaml\n");
    body.push_str("- ontology/generated/evidence/foreign-keys.json\n");
    body.push_str("- ontology/reviews/README.md\n");
    for table in &plan.tables {
        let qualified = format!("{}.{}", table.schema, table.name);
        body.push_str(&format!("- ontology/generated/datasets/{}/{}.yaml\n", table.schema, table.name));
        body.push_str(&format!("- ontology/generated/candidates/{}/{}.json\n", table.schema, table.name));
        body.push_str(&format!("- tables/{qualified}.md\n"));
    }
    body
}

fn render_table_md(table: &PlannedTable) -> String {
    let qualified = format!("{}.{}", table.schema, table.name);
    let physical = match (&table.physical_schema, &table.physical_table) {
        (Some(schema), Some(name)) => format!("{schema}.{name}"),
        _ => "not_supplied".to_owned(),
    };
    let mut body = format!(
        "# {qualified}\n\n\
# DRAFT — unreviewed — not published\n\n\
review: unreviewed\n\
publication: not_published\n\
human_approval: false\n\
dataset: {qualified}\n\
row_count_estimate: {reltuples}\n\
sealed_rows: {rows}\n\
physical: {physical}\n\
label: {label}\n\
description: {description}\n\
grain: {grain}\n\
local_copy_primary_key: {primary}\n\
original_source_key_metadata: unknown\n\
admitted: {admitted}\n\
omitted: {omitted}\n",
        reltuples = table.reltuples,
        rows = table.rows,
        label = yaml_quote(&table.generated.label),
        description = yaml_quote(&table.generated.description),
        grain = yaml_quote(&table.generated.grain),
        primary = key_list(&table.primary_key),
        admitted = column_list(table, true),
        omitted = column_list(table, false),
    );
    body.push_str(&render_markdown_keys(&table.foreign_keys));
    body.push_str(&format!(
        "\nStandard draft: ontology/generated/datasets/{}/{}.yaml\n\
Candidate: ontology/generated/candidates/{}/{}.json\n\
LLM descriptions are unreviewed. This file is not a review and not a publication.\n",
        table.schema, table.name, table.schema, table.name
    ));
    body
}

fn render_markdown_keys(keys: &[ForeignKey]) -> String {
    if keys.is_empty() {
        return "local_copy_foreign_keys: none\noriginal_source_metadata: unknown\n".to_owned();
    }
    let mut body = String::from("local_copy_foreign_keys:\n");
    for key in keys {
        body.push_str(&format!("- name: {}\n", yaml_quote(&key.name)));
        for column in &key.columns {
            body.push_str(&format!(
                "  column: {} {} -> {} {} {}\n",
                column.attnum,
                yaml_quote(&column.name),
                yaml_quote(&column.ref_schema),
                yaml_quote(&column.ref_table),
                yaml_quote(&column.ref_name)
            ));
        }
    }
    body.push_str("original_source_metadata: unknown\n");
    body
}

fn render_candidate(plan: &Plan, table: &PlannedTable) -> Result<String, &'static str> {
    let mut fields = Vec::new();
    for column in &table.columns {
        fields.push(serde_json::json!({
            "admission": if column.admitted { "type_supported" } else { "unsupported_type" },
            "admitted": column.admitted,
            "attnum": column.attnum,
            "card_sha256": column.card_sha256,
            "caveats": column.caveats,
            "column": column.name,
            "confidence": column.confidence,
            "data_type": column.type_name,
            "description": column.description,
            "label": column.label,
            "open_questions": column.open_questions
        }));
    }
    let value = serde_json::json!({
        "binding": binding_value(&plan.binding),
        "dataset": format!("{}.{}", table.schema, table.name),
        "fields": fields,
        "generation_candidate_sha256": plan.candidate_sha256,
        "human_approval": false,
        "model_id": "grok-4.7",
        "protocol": CANDIDATE_PROTOCOL,
        "publication": "not_published",
        "review": "unreviewed",
        "schema_hash": plan.schema_hash,
        "table": {
            "card_sha256": table.generated.card_sha256,
            "caveats": table.generated.caveats,
            "confidence": table.generated.confidence,
            "description": table.generated.description,
            "effect": table.generated.effect,
            "grain": table.generated.grain,
            "label": table.generated.label,
            "open_questions": table.generated.open_questions
        }
    });
    pretty_json(&value)
}

fn render_foreign_keys(plan: &Plan) -> Result<String, &'static str> {
    let mut tables = Vec::new();
    for table in &plan.tables {
        let mut keys = Vec::new();
        for key in &table.foreign_keys {
            let mut columns = Vec::new();
            for column in &key.columns {
                columns.push(serde_json::json!({
                    "attnum": column.attnum,
                    "name": column.name,
                    "ref_attnum": column.ref_attnum,
                    "ref_name": column.ref_name,
                    "ref_schema": column.ref_schema,
                    "ref_table": column.ref_table
                }));
            }
            keys.push(serde_json::json!({"columns": columns, "name": key.name}));
        }
        tables.push(serde_json::json!({
            "dataset": format!("{}.{}", table.schema, table.name),
            "local_copy_foreign_keys": keys,
            "local_copy_primary_key": table.primary_key,
            "original_source_metadata": "unknown"
        }));
    }
    pretty_json(&serde_json::json!({
        "local_copy_count": foreign_key_count(plan),
        "original_source_metadata": "unknown",
        "protocol": FOREIGN_KEY_PROTOCOL,
        "tables": tables
    }))
}

fn receipt(dir: &Path, plan: &Plan, mut warnings: Vec<String>) -> Result<Value, &'static str> {
    warnings.extend(plan.warnings.iter().cloned());
    warnings.sort();
    warnings.dedup();
    let mut generated = Vec::new();
    let mut candidates = Vec::new();
    let mut markdown = vec![
        hash_file(dir, "README.md")?,
        hash_file(dir, "ontology/reviews/README.md")?,
    ];
    for table in &plan.tables {
        let qualified = format!("{}.{}", table.schema, table.name);
        markdown.push(hash_file(dir, &format!("tables/{qualified}.md"))?);
        generated.push(hash_file(
            dir,
            &format!("ontology/generated/datasets/{}/{}.yaml", table.schema, table.name),
        )?);
        candidates.push(hash_file(
            dir,
            &format!("ontology/generated/candidates/{}/{}.json", table.schema, table.name),
        )?);
    }
    markdown.sort_by(|left, right| json_path(left).cmp(json_path(right)));
    Ok(serde_json::json!({
        "binding": binding_value(&plan.binding),
        "files": {
            "candidates": candidates,
            "evidence": [hash_file(dir, "ontology/generated/evidence/foreign-keys.json")?],
            "generated": generated,
            "manifest": hash_file(dir, "ontology/ontology.yaml")?,
            "markdown": markdown
        },
        "foreign_keys": {
            "local_copy_count": foreign_key_count(plan),
            "original_source_metadata": "unknown"
        },
        "generation_candidate_sha256": plan.candidate_sha256,
        "git_publication": "not_published",
        "human_approval": false,
        "model_id": "grok-4.7",
        "protocol": PACK_RECEIPT_PROTOCOL,
        "publication": "not_published",
        "review": "unreviewed",
        "schema_hash": plan.schema_hash,
        "warnings": warnings
    }))
}

fn binding_value(binding: &Binding) -> Value {
    serde_json::json!({
        "import_generation": binding.import_generation,
        "import_hash": binding.import_hash,
        "manifest_sha256": binding.manifest_sha256,
        "snapshot_id": binding.snapshot_id,
        "source_id": binding.source_id,
        "target_config_sha256": binding.target_config_sha256
    })
}

fn json_path(value: &Value) -> &str {
    value.get("path").and_then(Value::as_str).unwrap_or_default()
}

fn hash_file(dir: &Path, relative: &str) -> Result<Value, &'static str> {
    let bytes = fs::read(dir.join(relative)).map_err(|_| "pack")?;
    Ok(serde_json::json!({
        "path": relative,
        "sha256": pg_builder::sha256_hex(&bytes)
    }))
}

fn pretty_json(value: &Value) -> Result<String, &'static str> {
    let mut text = serde_json::to_string_pretty(value).map_err(|_| "pack")?;
    text.push('\n');
    Ok(text)
}

fn foreign_key_count(plan: &Plan) -> usize {
    plan.tables.iter().map(|table| table.foreign_keys.len()).sum()
}

fn column_list(table: &PlannedTable, admitted: bool) -> String {
    let items: Vec<String> = table
        .columns
        .iter()
        .filter(|column| column.admitted == admitted)
        .map(|column| format!("{} {}", column.name, column.type_name))
        .collect();
    if items.is_empty() { "none".to_owned() } else { items.join(", ") }
}

fn key_list(names: &[String]) -> String {
    if names.is_empty() { "none".to_owned() } else { names.join(", ") }
}

fn flow_plain(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_owned();
    }
    let mut body = String::from("[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            body.push_str(", ");
        }
        body.push_str(&yaml_plain(item));
    }
    body.push(']');
    body
}

fn yaml_plain(text: &str) -> String {
    if stays_plain(text) { text.to_owned() } else { yaml_quote(text) }
}

fn stays_plain(text: &str) -> bool {
    if text.is_empty() || matches!(text, "true" | "false" | "null" | "Null" | "NULL" | "~") {
        return false;
    }
    if text.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    text.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.'))
}

fn yaml_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            ch if (u32::from(ch)) < 0x20 || ch == '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn snapshot_preserved(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, &'static str> {
    let mut out = BTreeMap::new();
    for relative in ["ontology/curated", "ontology/guides", "ontology/reviews/decisions.yaml"] {
        snapshot_one(dir, relative, &mut out)?;
    }
    Ok(out)
}

fn snapshot_one(dir: &Path, relative: &str, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), &'static str> {
    let path = dir.join(relative);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("pack"),
    };
    if meta.file_type().is_symlink() {
        return Err("pack.curated");
    }
    if meta.is_file() {
        out.insert(relative.to_owned(), fs::read(&path).map_err(|_| "pack")?);
        return Ok(());
    }
    if meta.is_dir() {
        walk_files(&path, relative, out)?;
        return Ok(());
    }
    Err("pack.curated")
}

fn walk_files(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), &'static str> {
    let entries = fs::read_dir(dir).map_err(|_| "pack")?;
    for entry in entries {
        let entry = entry.map_err(|_| "pack")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err("pack");
        };
        if name.contains('/') || name.contains('\\') {
            return Err("pack");
        }
        let relative = format!("{prefix}/{name}");
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| "pack")?;
        if meta.file_type().is_symlink() {
            return Err("pack.curated");
        }
        if meta.is_dir() {
            walk_files(&entry.path(), &relative, out)?;
        } else if meta.is_file() {
            out.insert(relative, fs::read(entry.path()).map_err(|_| "pack")?);
        } else {
            return Err("pack.curated");
        }
    }
    Ok(())
}

fn unexpected_datasets(dir: &Path, plan: &Plan) -> Result<Vec<String>, &'static str> {
    let root = dir.join("ontology/generated/datasets");
    let meta = match fs::symlink_metadata(&root) {
        Ok(meta) => meta,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("pack"),
    };
    if meta.file_type().is_symlink() {
        return Err("pack");
    }
    if !meta.is_dir() {
        return Err("pack");
    }
    let mut found = Vec::new();
    list_files(&root, "ontology/generated/datasets", &mut found)?;
    let owned: BTreeSet<String> = plan
        .tables
        .iter()
        .map(|table| format!("ontology/generated/datasets/{}/{}.yaml", table.schema, table.name))
        .collect();
    let mut warnings = Vec::new();
    for relative in found {
        if !owned.contains(&relative) {
            warnings.push(format!("unexpected_generated_dataset:{relative}"));
        }
    }
    Ok(warnings)
}

fn list_files(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), &'static str> {
    let entries = fs::read_dir(dir).map_err(|_| "pack")?;
    for entry in entries {
        let entry = entry.map_err(|_| "pack")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err("pack");
        };
        let relative = format!("{prefix}/{name}");
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| "pack")?;
        if meta.file_type().is_symlink() {
            return Err("pack");
        }
        if meta.is_dir() {
            list_files(&entry.path(), &relative, out)?;
        } else if meta.is_file() {
            out.push(relative);
        } else {
            return Err("pack");
        }
    }
    Ok(())
}

fn reject_symlink_chain(path: &Path) -> Result<(), &'static str> {
    let mut acc = PathBuf::new();
    for component in path.components() {
        acc.push(component);
        match fs::symlink_metadata(&acc) {
            Ok(meta) if meta.file_type().is_symlink() => return Err("pack"),
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("pack"),
        }
    }
    Ok(())
}

fn reject_owned(relative: &str) -> Result<(), &'static str> {
    if relative.starts_with("ontology/curated/")
        || relative.starts_with("ontology/guides/")
        || relative == "ontology/reviews/decisions.yaml"
    {
        return Err("pack.curated");
    }
    Ok(())
}

fn as_map<'a>(value: &'a Value, err: &'static str) -> Result<&'a Map<String, Value>, &'static str> {
    value.as_object().ok_or(err)
}

fn exact_keys(map: &Map<String, Value>, keys: &[&str], err: &'static str) -> Result<(), &'static str> {
    if map.len() == keys.len() && keys.iter().all(|key| map.contains_key(*key)) {
        Ok(())
    } else {
        Err(err)
    }
}

fn required_hex(map: &Map<String, Value>, key: &str) -> Result<String, &'static str> {
    let text = map.get(key).and_then(Value::as_str).ok_or("pack.input")?;
    if is_hex64(text) { Ok(text.to_owned()) } else { Err("pack.input") }
}

fn required_plain(map: &Map<String, Value>, key: &str) -> Result<String, &'static str> {
    let text = map.get(key).and_then(Value::as_str).ok_or("pack.input")?;
    if is_plain(text) { Ok(text.to_owned()) } else { Err("pack.input") }
}

fn identifier_str(
    map: &Map<String, Value>,
    key: &str,
    pattern: fn(&str) -> bool,
) -> Result<String, &'static str> {
    let text = map.get(key).and_then(Value::as_str).ok_or("pack.input")?;
    if !is_plain(text) {
        return Err("pack.input");
    }
    if pattern(text) { Ok(text.to_owned()) } else { Err("pack.identifier") }
}

fn catalog_name(
    map: &Map<String, Value>,
    key: &str,
    ident_key: &str,
    pattern: fn(&str) -> bool,
) -> Result<String, &'static str> {
    let text = map.get(key).and_then(Value::as_str).ok_or("pack.catalog")?;
    let ident = map.get(ident_key).and_then(Value::as_str).ok_or("pack.catalog")?;
    if text != ident {
        return Err("pack.identifier");
    }
    if !is_plain(text) {
        return Err("pack.catalog");
    }
    if pattern(text) { Ok(text.to_owned()) } else { Err("pack.identifier") }
}

fn model_string(map: &Map<String, Value>, key: &str, max_chars: usize) -> Result<String, &'static str> {
    let text = map.get(key).and_then(Value::as_str).ok_or("pack.generation")?;
    if text_ok(text, max_chars) && !sentinel(text) {
        Ok(text.to_owned())
    } else {
        Err("pack.generation")
    }
}

fn confidence_string(map: &Map<String, Value>) -> Result<String, &'static str> {
    let text = map.get("confidence").and_then(Value::as_str).ok_or("pack.generation")?;
    if confidence_ok(text) && !sentinel(text) { Ok(text.to_owned()) } else { Err("pack.generation") }
}

fn hex_string(map: &Map<String, Value>, key: &str) -> Result<String, &'static str> {
    let text = map.get(key).and_then(Value::as_str).ok_or("pack.generation")?;
    if is_hex64(text) { Ok(text.to_owned()) } else { Err("pack.generation") }
}

fn text_list(value: &Value) -> Result<Vec<String>, &'static str> {
    let items = value.as_array().ok_or("pack.generation")?;
    if items.len() > 8 {
        return Err("pack.generation");
    }
    let mut out = Vec::new();
    for item in items {
        let text = item.as_str().ok_or("pack.generation")?;
        if !text_ok(text, 2000) || sentinel(text) {
            return Err("pack.generation");
        }
        out.push(text.to_owned());
    }
    Ok(out)
}

fn read_i32(value: &Value) -> Result<i32, &'static str> {
    let number = value.as_i64().ok_or("pack.catalog")?;
    i32::try_from(number).map_err(|_| "pack.catalog")
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_plain(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_control)
}

fn is_row_count(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.iter().all(|byte| byte.is_ascii_digit())
        && (bytes.len() == 1 || bytes[0] != b'0')
}

fn is_source_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn is_table_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..].iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn is_plugin_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

fn text_ok(text: &str, max_chars: usize) -> bool {
    let count = text.chars().count();
    if count == 0 || count > max_chars {
        return false;
    }
    if text.chars().any(|ch| {
        let code = u32::from(ch);
        ch == '\0' || (code < 0x20 && ch != '\t' && ch != '\n') || ch == '\u{7f}'
    }) {
        return false;
    }
    !(text.contains('`')
        || text.contains("{{")
        || text.contains("}}")
        || text.contains("${")
        || text.contains("$(")
        || text.contains("%{"))
}

fn confidence_ok(text: &str) -> bool {
    if text == "0" || text == "1" {
        return true;
    }
    let Some((whole, frac)) = text.split_once('.') else {
        return false;
    };
    if frac.is_empty() || frac.len() > 4 || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    whole == "0" || (whole == "1" && frac.bytes().all(|byte| byte == b'0'))
}

fn sentinel(text: &str) -> bool {
    text.contains("SELECT")
        || text.contains("1234567890123456789012345678901234")
        || text.contains("0001ff")
        || text.contains("9223372036854775807")
}

fn schema_list<'a>(schemas: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    let mut seen: Vec<&str> = Vec::new();
    for schema in schemas {
        if seen.contains(&schema) {
            continue;
        }
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(schema);
        seen.push(schema);
    }
    out
}

fn write_checked(path: &Path, body: &str) -> Result<(), &'static str> {
    if body.contains("SELECT")
        || body.contains("1234567890123456789012345678901234")
        || body.contains("0001ff")
        || body.contains("9223372036854775807")
    {
        return Err("pack");
    }
    fs::write(path, body).map_err(|_| "pack")
}

#[cfg(test)]
mod batch_tests {
    use super::cover_column_batches;

    #[test]
    fn wide_tables_cover_every_column_batch() {
        assert!(cover_column_batches(&[(0, 1, 0, 8)], 8).is_ok());
        assert!(cover_column_batches(&[(0, 2, 0, 8), (1, 2, 8, 10)], 10).is_ok());
        assert!(cover_column_batches(&[(0, 2, 0, 8)], 10).is_err());
        assert!(cover_column_batches(&[(0, 2, 0, 8), (1, 2, 7, 10)], 10).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::write_draft;
    use crate::source::SourceCounts;
    use serde_json::json;
    use std::env;
    use std::fs;

    #[test]
    fn draft_pack_is_labeled_unreviewed() {
        let dir = env::temp_dir().join(format!("oo-a405-pack-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let counts = SourceCounts {
            rows: vec![2000, 8000, 90000],
            version: "17.10".to_owned(),
            source_sha256: "abc".to_owned(),
        };
        let configured = vec![
            ("shop".to_owned(), "party".to_owned()),
            ("shop".to_owned(), "account".to_owned()),
            ("shop".to_owned(), "entry".to_owned()),
        ];
        write_draft(&dir, &counts, &configured, &json!({"protocol": "absent"}), None).expect("draft");
        let readme = fs::read_to_string(dir.join("README.md")).expect("readme");
        assert!(readme.starts_with("# DRAFT — unreviewed — not published"));
        assert!(readme.contains("shop.party rows: 2000"));
        assert!(readme.contains("shop.entry rows: 90000"));
        assert!(readme.contains("review_state: unreviewed"));
        assert!(readme.contains("publication: not_published"));
        assert!(readme.contains("local-process-not-a-sandbox-certificate"));
        let ontology = fs::read_to_string(dir.join("ontology/ontology.yaml")).expect("ontology");
        assert!(ontology.contains("publication: not_published"));
        assert!(ontology.contains("llm_review: unreviewed"));
        let review = fs::read_to_string(dir.join("ontology/reviews/README.md")).expect("review");
        assert!(review.contains("unreviewed"));
        assert!(review.contains("No Git publication"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bulk_pack_binds_the_manifest_and_hides_gold_cells() {
        use super::{write_bulk_draft, PackColumn, PackTable};
        let dir = env::temp_dir().join(format!("oo-a411-pack-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let counts = SourceCounts {
            rows: vec![2000, 8000, 90000],
            version: "17.10".to_owned(),
            source_sha256: "abc".to_owned(),
        };
        let tables = vec![
            PackTable {
                logical_schema: "public".to_owned(),
                logical_table: "batch_a".to_owned(),
                physical_schema: "_oo".to_owned(),
                physical_table: "pt_a".to_owned(),
                rows: 2000,
                columns: vec![PackColumn { name: "id".to_owned(), type_name: "int8".to_owned(), admitted: true }],
            },
            PackTable {
                logical_schema: "public".to_owned(),
                logical_table: "batch_b".to_owned(),
                physical_schema: "_oo".to_owned(),
                physical_table: "pt_b".to_owned(),
                rows: 8000,
                columns: vec![
                    PackColumn { name: "id".to_owned(), type_name: "int8".to_owned(), admitted: true },
                    PackColumn { name: "blob".to_owned(), type_name: "bytea".to_owned(), admitted: false },
                ],
            },
            PackTable {
                logical_schema: "public".to_owned(),
                logical_table: "batch_c".to_owned(),
                physical_schema: "_oo".to_owned(),
                physical_table: "pt_c".to_owned(),
                rows: 90000,
                columns: vec![PackColumn { name: "qty".to_owned(), type_name: "int8".to_owned(), admitted: true }],
            },
        ];
        write_bulk_draft(&dir, &counts, &tables, "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73", "00000004-00000013-1", None).expect("draft");
        let manifest = fs::read_to_string(dir.join("MANIFEST.md")).expect("manifest");
        assert!(manifest.contains("declared_foreign_key: not_declared"));
        assert!(manifest.contains("_oo.pt_c"));
        assert!(!manifest.contains("SELECT"));
        assert!(!manifest.contains("0001ff"));
        let batch_b = fs::read_to_string(dir.join("tables/public.batch_b.md")).expect("b");
        assert!(batch_b.contains("omitted: blob bytea"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn draft_pack_accepts_tables_outside_the_shop_and_bulk_fixtures() {
        use super::write_draft;
        let dir = env::temp_dir().join(format!("oo-a411-pack-other-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let counts = SourceCounts {
            rows: vec![3, 5],
            version: "17.10".to_owned(),
            source_sha256: "abc".to_owned(),
        };
        let configured = vec![
            ("finance".to_owned(), "ledger".to_owned()),
            ("hr".to_owned(), "person".to_owned()),
        ];
        write_draft(&dir, &counts, &configured, &json!({"protocol": "absent"}), None).expect("draft");
        let readme = fs::read_to_string(dir.join("README.md")).expect("readme");
        assert!(readme.contains("finance.ledger rows: 3"));
        assert!(readme.contains("hr.person rows: 5"));
        let ontology = fs::read_to_string(dir.join("ontology/ontology.yaml")).expect("ontology");
        assert!(ontology.contains("name: configured-development-preview"));
        assert!(ontology.contains("include_schemas: [finance, hr]"));
        assert!(dir.join("ontology/generated/datasets/hr/person.yaml").is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
#[path = "../../../testing/standard-draft-pack-acceptance/behavior.rs"]
mod standard_draft_behavior;
