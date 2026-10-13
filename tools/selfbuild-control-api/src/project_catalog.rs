//! Build an `oo.source.pgsql/v1` catalog from a capture, sealed type basis, and audited DDL.
//!
//! The capture file is not retagged. `seal_request` is the only writer of a seal.
//! A statement that is not a supported CREATE, DROP, or GRANT stops the projection.
//! CHECK text stays out of the catalog hash. Bytea is not remapped.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::canonical::sha256_hex;

pub struct Projected {
    pub catalog: Vec<u8>,
    pub profile: Vec<u8>,
    pub note: Value,
}

pub fn project(catalog_bytes: &[u8], profile_bytes: &[u8], bases: &[&[u8]], audits: &[&[u8]]) -> Result<Projected, &'static str> {
    if audits.is_empty() {
        return Err("schema audits are empty");
    }
    if bases.is_empty() {
        return Err("sealed type basis is empty");
    }
    let (wanted, rows_compared) = read_capture(catalog_bytes, profile_bytes)?;
    let basis = load_basis(bases)?;
    let statements = logged_statements(audits)?;
    let mut live = BTreeMap::new();
    let mut parsed_creates = 0_u64;
    for statement in &statements {
        apply_statement(statement, &mut live, &mut parsed_creates)?;
    }
    let aligned = align_capture(&wanted, &live, &basis)?;
    emit(aligned, &basis.source_id, rows_compared, parsed_creates, audits)
}

fn read_capture(catalog_bytes: &[u8], profile_bytes: &[u8]) -> Result<(Vec<Wanted>, u64), &'static str> {
    let capture = object(catalog_bytes)?;
    let profile = object(profile_bytes)?;
    if capture.get("protocol").and_then(Value::as_str) != Some(crate::adapt::CAPTURE_CATALOG) {
        return Err("the capture protocol is not the selected catalog");
    }
    if profile.get("protocol").and_then(Value::as_str) != Some(crate::adapt::CAPTURE_COMPARE) {
        return Err("the compare protocol is not the selected profile");
    }
    if profile.get("sampled") != Some(&Value::Bool(false)) {
        return Err("the compare profile is sampled");
    }
    if let Some(samples) = profile.get("samples")
        && samples.as_array().is_none_or(|rows| !rows.is_empty())
    {
        return Err("the compare profile has samples");
    }
    let rows_compared = profile.get("rows_compared").and_then(Value::as_u64).ok_or("the compare profile has no row count")?;
    let wanted = capture_tables(capture.get("tables").ok_or("the capture has no tables")?)?;
    if wanted.is_empty() || wanted.len() > 16 {
        return Err("the capture has no tables");
    }
    Ok((wanted, rows_compared))
}

fn logged_statements(audits: &[&[u8]]) -> Result<Vec<Logged>, &'static str> {
    let mut statements = Vec::new();
    for (index, audit) in audits.iter().enumerate() {
        let text = std::str::from_utf8(audit).map_err(|_| "schema audit is not utf-8")?;
        for row in csv_records(text).map_err(|_| "schema audit is not a csv log")? {
            if row.len() <= 13 {
                continue;
            }
            let field = row[13].trim();
            let Some(body) = field.strip_prefix("statement:") else {
                continue;
            };
            let body = body.trim();
            if statement_ineligible(body) {
                return Err("an audited statement is not eligible");
            }
            let at = captured_at(row[0].trim()).map_err(|_| "captured_at is not a utc timestamp")?;
            let user = simple_name(row[1].trim()).map_err(|_| "ddl session user is not a catalog name")?;
            let database = simple_name(row[2].trim()).map_err(|_| "schema audits disagree on the database")?;
            for sql in split_statements(body).map_err(|_| "an audited statement is not a supported catalog fact")? {
                statements.push(Logged {
                    at: at.clone(),
                    user: user.clone(),
                    database: database.clone(),
                    sql,
                    index,
                });
            }
        }
    }
    statements.sort_by(|left, right| (&left.at, left.index).cmp(&(&right.at, right.index)));
    Ok(statements)
}

struct Aligned {
    tables: Vec<BuiltTable>,
    times: BTreeSet<String>,
    select_grants: Vec<String>,
}

fn align_capture(wanted: &[Wanted], live: &BTreeMap<(String, String), LiveTable>, basis: &Basis) -> Result<Aligned, &'static str> {
    let mut tables = Vec::new();
    let mut times = BTreeSet::new();
    let mut select_grants = Vec::new();
    for want in wanted {
        let (table, captured_at) = one_table(want, live, basis)?;
        times.insert(captured_at);
        if table.select {
            select_grants.push(format!("{}.{}", table.schema, table.name));
        }
        tables.push(table);
    }
    Ok(Aligned { tables, times, select_grants })
}

fn one_table(want: &Wanted, live: &BTreeMap<(String, String), LiveTable>, basis: &Basis) -> Result<(BuiltTable, String), &'static str> {
    let live_table = live.get(&(want.schema.clone(), want.name.clone())).ok_or("a capture table has no audited create")?;
    if live_table.columns.iter().map(|column| column.name.as_str()).ne(want.columns.iter().map(String::as_str)) {
        return Err("a capture column is not in the audited create");
    }
    let order = basis.order.get(&(want.schema.clone(), want.name.clone())).ok_or("a capture table has no sealed manifest")?;
    if order != &want.columns {
        return Err("a column type does not match the sealed manifest");
    }
    let pk_names: Vec<&str> = live_table.primary_key.iter().map(|column| column.1.as_str()).collect();
    if pk_names.join(",") != want.pk {
        return Err("a capture primary key is not the audited primary key");
    }
    let mut columns = Vec::new();
    for (offset, column) in live_table.columns.iter().enumerate() {
        let attnum = i32::try_from(offset + 1).map_err(|_| "a capture column is not in the audited create")?;
        let (oid, typmod) = basis
            .columns
            .get(&(want.schema.clone(), want.name.clone(), column.name.clone()))
            .copied()
            .ok_or("a column type does not match the sealed manifest")?;
        if column.phys.oid != oid || column.phys.typmod != typmod {
            return Err("a column type does not match the sealed manifest");
        }
        columns.push(built_column(attnum, column, live)?);
    }
    let mut foreign = Vec::new();
    for column in &columns {
        let Some(reference) = &column.reference else {
            continue;
        };
        let name = format!("{}_{}_fkey", want.name, column.name);
        if name.len() > 63 || name == "primary_key" {
            return Err("a catalog name is not a simple identifier");
        }
        foreign.push(BuiltFk {
            name,
            columns: vec![reference.clone()],
        });
    }
    let select = live_table.grants.iter().any(|grant| grant.privilege == "SELECT");
    let insert = live_table.grants.iter().any(|grant| grant.privilege == "INSERT");
    let update = live_table.grants.iter().any(|grant| grant.privilege == "UPDATE");
    let table = BuiltTable {
        schema: want.schema.clone(),
        name: want.name.clone(),
        replica: want.replica.clone(),
        owner: live_table.owner.clone(),
        database: live_table.database.clone(),
        columns,
        primary_key: live_table.primary_key.clone(),
        primary_key_nulls_not_distinct: live_table.primary_key_nulls_not_distinct,
        foreign,
        grants: live_table.grants.clone(),
        select,
        insert,
        update,
    };
    Ok((table, live_table.captured_at.clone()))
}

fn built_column(attnum: i32, column: &ParsedColumn, live: &BTreeMap<(String, String), LiveTable>) -> Result<BuiltColumn, &'static str> {
    let mut reference = None;
    if let Some(target) = &column.reference {
        let parent = live.get(&(target.schema.clone(), target.table.clone())).ok_or("a foreign key target is not in the audited create")?;
        let ref_attnum = parent
            .columns
            .iter()
            .position(|item| item.name == target.column)
            .ok_or("a foreign key target is not in the audited create")?;
        let ref_attnum = i32::try_from(ref_attnum + 1).map_err(|_| "a foreign key target is not in the audited create")?;
        reference = Some(FkCol {
            attnum,
            name: column.name.clone(),
            ref_schema: target.schema.clone(),
            ref_table: target.table.clone(),
            ref_attnum,
            ref_name: target.column.clone(),
        });
    }
    Ok(BuiltColumn {
        attnum,
        name: column.name.clone(),
        phys: column.phys,
        not_null: column.not_null,
        reference,
    })
}

fn emit(aligned: Aligned, source_id: &str, rows_compared: u64, parsed_creates: u64, audits: &[&[u8]]) -> Result<Projected, &'static str> {
    let Aligned { mut tables, times, mut select_grants } = aligned;
    let mut database = String::new();
    for table in &tables {
        if database.is_empty() {
            database.clone_from(&table.database);
        } else if database != table.database {
            return Err("schema audits disagree on the database");
        }
    }
    if database.is_empty() {
        return Err("schema audits disagree on the database");
    }
    tables.sort_by(|left, right| (&left.schema, &left.name).cmp(&(&right.schema, &right.name)));
    select_grants.sort();
    let captured_at = times.iter().next_back().cloned().ok_or("captured_at is not a utc timestamp")?;
    let preimage = schema_preimage(&tables)?;
    let schema_hash = sha256_hex(preimage.as_bytes());
    let catalog = catalog_document(&tables, source_id, &database, &captured_at, &schema_hash)?;
    let profile_out = cards_profile(&tables)?;
    let mut audit_sha256 = Vec::new();
    for audit in audits {
        audit_sha256.push(Value::String(sha256_hex(audit)));
    }
    let note = json!({
        "audit_sha256": audit_sha256,
        "captured_at": captured_at,
        "captured_at_rule": "latest_ddl_statement",
        "collation_rule": "typcollation_when_ddl_has_no_collate",
        "ddl_statement_times": times.into_iter().collect::<Vec<_>>(),
        "emitted_tables": tables.len(),
        "fk_name_rule": "table_column_fkey",
        "input_protocol": crate::adapt::CAPTURE_CATALOG,
        "owner_source": "ddl_session_user",
        "parsed_create_count": parsed_creates,
        "privilege_source": "audited_grant",
        "rows_compared": rows_compared,
        "schema_hash": schema_hash,
        "schema_usage_is_table_privilege": false,
        "select_grants": select_grants,
        "source_id": source_id,
        "table_checks_omitted": true
    });
    Ok(Projected {
        catalog: serde_json::to_vec(&catalog).map_err(|_| "the catalog is not json")?,
        profile: serde_json::to_vec(&profile_out).map_err(|_| "the catalog is not json")?,
        note,
    })
}

struct Wanted {
    schema: String,
    name: String,
    columns: Vec<String>,
    pk: String,
    replica: String,
}

struct Logged {
    at: String,
    user: String,
    database: String,
    sql: String,
    index: usize,
}

struct LiveTable {
    owner: String,
    database: String,
    captured_at: String,
    columns: Vec<ParsedColumn>,
    primary_key: Vec<(i32, String)>,
    primary_key_nulls_not_distinct: Option<bool>,
    grants: Vec<Grant>,
}

struct ParsedColumn {
    name: String,
    phys: Phys,
    not_null: bool,
    primary: bool,
    nulls_not_distinct: bool,
    reference: Option<Reference>,
}

struct Reference {
    schema: String,
    table: String,
    column: String,
}

#[derive(Clone, Copy)]
struct Phys {
    oid: i64,
    typmod: i32,
    type_name: &'static str,
    precision: Option<i32>,
    scale: Option<i32>,
    collation_oid: i64,
    collation_name: Option<&'static str>,
}

#[derive(Clone)]
struct Grant {
    grantee: String,
    privilege: String,
    grantable: bool,
}

struct Basis {
    source_id: String,
    columns: BTreeMap<(String, String, String), (i64, i32)>,
    order: BTreeMap<(String, String), Vec<String>>,
}

struct BuiltTable {
    schema: String,
    name: String,
    replica: String,
    owner: String,
    database: String,
    columns: Vec<BuiltColumn>,
    primary_key: Vec<(i32, String)>,
    primary_key_nulls_not_distinct: Option<bool>,
    foreign: Vec<BuiltFk>,
    grants: Vec<Grant>,
    select: bool,
    insert: bool,
    update: bool,
}

struct BuiltColumn {
    attnum: i32,
    name: String,
    phys: Phys,
    not_null: bool,
    reference: Option<FkCol>,
}

#[derive(Clone)]
struct FkCol {
    attnum: i32,
    name: String,
    ref_schema: String,
    ref_table: String,
    ref_attnum: i32,
    ref_name: String,
}

struct BuiltFk {
    name: String,
    columns: Vec<FkCol>,
}

fn apply_statement(statement: &Logged, live: &mut BTreeMap<(String, String), LiveTable>, parsed_creates: &mut u64) -> Result<(), &'static str> {
    let verb = leading_verb(&statement.sql).ok_or("an audited statement is not a supported catalog fact")?;
    if matches!(verb.as_str(), "select" | "insert" | "update" | "delete" | "begin" | "commit" | "rollback" | "start" | "set" | "copy" | "show") {
        return Ok(());
    }
    let tokens = tokenize(&statement.sql).map_err(|_| "an audited statement is not a supported catalog fact")?;
    match verb.as_str() {
        "create" => apply_create(&tokens, statement, live, parsed_creates),
        "drop" => apply_drop(&tokens, live),
        "grant" => apply_grant(&tokens, live),
        _ => Err("an audited statement is not a supported catalog fact"),
    }
}

fn leading_verb(sql: &str) -> Option<String> {
    let mut word = String::new();
    for ch in sql.chars().skip_while(|ch| ch.is_whitespace()) {
        if ch.is_ascii_alphabetic() || ch == '_' {
            word.push(ch.to_ascii_lowercase());
        } else {
            break;
        }
    }
    (!word.is_empty()).then_some(word)
}

fn apply_create(
    tokens: &[Tok],
    statement: &Logged,
    live: &mut BTreeMap<(String, String), LiveTable>,
    parsed_creates: &mut u64,
) -> Result<(), &'static str> {
    let second = word_at(tokens, 1).ok_or("an audited statement is not a supported catalog fact")?;
    if second.eq_ignore_ascii_case("schema") {
        return Ok(());
    }
    if !second.eq_ignore_ascii_case("table") {
        return Err("an audited statement is not a supported catalog fact");
    }
    if ddl_unsupported(&statement.sql) {
        return Err("ddl contains default collation identity or row security");
    }
    let mut index = 2;
    let schema = take_ident(tokens, &mut index)?;
    if !eat_punct(tokens, &mut index, '.') {
        return Err("a catalog name is not a simple identifier");
    }
    let name = take_ident(tokens, &mut index)?;
    if !eat_punct(tokens, &mut index, '(') {
        return Err("an audited statement is not a supported catalog fact");
    }
    let (columns, primary, nulls_not_distinct) = take_columns(tokens, &mut index)?;
    if index != tokens.len() {
        return Err("an audited statement is not a supported catalog fact");
    }
    if columns.is_empty() || columns.len() > 64 {
        return Err("a capture column is not in the audited create");
    }
    if let Some(existing) = live.get(&(schema.clone(), name.clone())) {
        if existing.database != statement.database {
            return Err("schema audits disagree on the database");
        }
        if existing.owner != statement.user {
            return Err("schema audits disagree on a table owner");
        }
    } else if live.values().any(|table| table.database != statement.database) {
        return Err("schema audits disagree on the database");
    }
    let primary_key_nulls_not_distinct = if primary.is_empty() { None } else { Some(nulls_not_distinct) };
    *parsed_creates += 1;
    live.insert(
        (schema, name),
        LiveTable {
            owner: statement.user.clone(),
            database: statement.database.clone(),
            captured_at: statement.at.clone(),
            columns,
            primary_key: primary,
            primary_key_nulls_not_distinct,
            grants: Vec::new(),
        },
    );
    Ok(())
}

fn ddl_unsupported(sql: &str) -> bool {
    let low = sql.to_ascii_lowercase();
    low.contains(" default ") || low.contains(" collate ") || low.contains(" generated ") || low.contains(" identity ") || low.contains("row level")
}

type ParsedColumns = (Vec<ParsedColumn>, Vec<(i32, String)>, bool);

fn take_columns(tokens: &[Tok], index: &mut usize) -> Result<ParsedColumns, &'static str> {
    let mut columns = Vec::new();
    let mut primary = Vec::new();
    let mut nulls_not_distinct = false;
    loop {
        if peek_punct(tokens, *index) == Some(')') {
            *index += 1;
            break;
        }
        let (item, ended) = next_item(tokens, index)?;
        if item.is_empty() {
            return Err("an audited statement is not a supported catalog fact");
        }
        if first_word(&item).is_some_and(|word| word.eq_ignore_ascii_case("constraint")) {
            accept_check(&item)?;
            if ended {
                break;
            }
            continue;
        }
        let column = parse_column(&item)?;
        if column.primary {
            remember_primary(&columns, &mut primary, &mut nulls_not_distinct, &column)?;
        }
        columns.push(column);
        if ended {
            break;
        }
    }
    Ok((columns, primary, nulls_not_distinct))
}

fn accept_check(item: &[Tok]) -> Result<(), &'static str> {
    let check = word_at(item, 2).ok_or("an audited statement is not a supported catalog fact")?;
    if check.eq_ignore_ascii_case("check") {
        Ok(())
    } else {
        Err("an audited statement is not a supported catalog fact")
    }
}

fn remember_primary(columns: &[ParsedColumn], primary: &mut Vec<(i32, String)>, nulls_not_distinct: &mut bool, column: &ParsedColumn) -> Result<(), &'static str> {
    let attnum = i32::try_from(columns.len() + 1).map_err(|_| "a capture column is not in the audited create")?;
    if primary.is_empty() {
        *nulls_not_distinct = column.nulls_not_distinct;
    } else if *nulls_not_distinct != column.nulls_not_distinct {
        return Err("an audited statement is not a supported catalog fact");
    }
    primary.push((attnum, column.name.clone()));
    Ok(())
}

fn parse_column(tokens: &[Tok]) -> Result<ParsedColumn, &'static str> {
    let mut index = 0;
    let name = take_ident(tokens, &mut index)?;
    let phys = parse_type(tokens, &mut index)?;
    let mut not_null = false;
    let mut primary = false;
    let mut nulls_not_distinct = false;
    let mut reference = None;
    while index < tokens.len() {
        let word = take_word(tokens, &mut index)?;
        if word.eq_ignore_ascii_case("not") {
            let null = take_word(tokens, &mut index)?;
            if !null.eq_ignore_ascii_case("null") {
                return Err("an audited statement is not a supported catalog fact");
            }
            not_null = true;
        } else if word.eq_ignore_ascii_case("null") {
            not_null = false;
        } else if word.eq_ignore_ascii_case("primary") {
            let key = take_word(tokens, &mut index)?;
            if !key.eq_ignore_ascii_case("key") {
                return Err("an audited statement is not a supported catalog fact");
            }
            primary = true;
            not_null = true;
            if peek_word(tokens, index).is_some_and(|item| item.eq_ignore_ascii_case("nulls")) {
                index += 1;
                let marker = take_word(tokens, &mut index)?;
                if marker.eq_ignore_ascii_case("not") {
                    let distinct = take_word(tokens, &mut index)?;
                    if !distinct.eq_ignore_ascii_case("distinct") {
                        return Err("an audited statement is not a supported catalog fact");
                    }
                    nulls_not_distinct = true;
                } else if !marker.eq_ignore_ascii_case("distinct") {
                    return Err("an audited statement is not a supported catalog fact");
                }
            }
        } else if word.eq_ignore_ascii_case("references") {
            let schema = take_ident(tokens, &mut index)?;
            if !eat_punct(tokens, &mut index, '.') {
                return Err("a foreign key target is not in the audited create");
            }
            let table = take_ident(tokens, &mut index)?;
            if !eat_punct(tokens, &mut index, '(') {
                return Err("a foreign key target is not in the audited create");
            }
            let column = take_ident(tokens, &mut index)?;
            if !eat_punct(tokens, &mut index, ')') {
                return Err("a foreign key target is not in the audited create");
            }
            reference = Some(Reference { schema, table, column });
        } else {
            return Err("an audited statement is not a supported catalog fact");
        }
    }
    Ok(ParsedColumn {
        name,
        phys,
        not_null,
        primary,
        nulls_not_distinct,
        reference,
    })
}

fn parse_type(tokens: &[Tok], index: &mut usize) -> Result<Phys, &'static str> {
    let word = take_word(tokens, index)?;
    if word.eq_ignore_ascii_case("timestamp") {
        for expected in ["with", "time", "zone"] {
            let next = take_word(tokens, index)?;
            if !next.eq_ignore_ascii_case(expected) {
                return Err("a column type does not match the sealed manifest");
            }
        }
        return Ok(plain_type("timestamptz", 1184));
    }
    if word.eq_ignore_ascii_case("numeric") {
        if !eat_punct(tokens, index, '(') {
            return Err("a column type does not match the sealed manifest");
        }
        let precision = take_number(tokens, index)?;
        if !eat_punct(tokens, index, ',') {
            return Err("a column type does not match the sealed manifest");
        }
        let scale = take_number(tokens, index)?;
        if !eat_punct(tokens, index, ')') {
            return Err("a column type does not match the sealed manifest");
        }
        let typmod = numeric_typmod(precision, scale).ok_or("a column type does not match the sealed manifest")?;
        return Ok(Phys {
            oid: 1700,
            typmod,
            type_name: "numeric",
            precision: Some(precision),
            scale: Some(scale),
            collation_oid: 0,
            collation_name: None,
        });
    }
    match word.to_ascii_lowercase().as_str() {
        "bigint" => Ok(plain_type("int8", 20)),
        "uuid" => Ok(plain_type("uuid", 2950)),
        "text" => Ok(Phys {
            oid: 25,
            typmod: -1,
            type_name: "text",
            precision: None,
            scale: None,
            collation_oid: 100,
            collation_name: Some("default"),
        }),
        "bytea" => Ok(plain_type("bytea", 17)),
        "jsonb" => Ok(plain_type("jsonb", 3802)),
        _ => Err("a column type does not match the sealed manifest"),
    }
}

fn plain_type(type_name: &'static str, oid: i64) -> Phys {
    Phys {
        oid,
        typmod: -1,
        type_name,
        precision: None,
        scale: None,
        collation_oid: 0,
        collation_name: None,
    }
}

fn numeric_typmod(precision: i32, scale: i32) -> Option<i32> {
    if !(1..=1000).contains(&precision) || scale < 0 || scale > precision {
        return None;
    }
    let shifted = precision.checked_shl(16)?;
    (shifted | scale).checked_add(4)
}

fn apply_drop(tokens: &[Tok], live: &mut BTreeMap<(String, String), LiveTable>) -> Result<(), &'static str> {
    let second = word_at(tokens, 1).ok_or("an audited statement is not a supported catalog fact")?;
    if !second.eq_ignore_ascii_case("table") {
        return Err("an audited statement is not a supported catalog fact");
    }
    let mut index = 2;
    if peek_word(tokens, index).is_some_and(|word| word.eq_ignore_ascii_case("if")) {
        index += 1;
        let exists = take_word(tokens, &mut index)?;
        if !exists.eq_ignore_ascii_case("exists") {
            return Err("an audited statement is not a supported catalog fact");
        }
        let schema = take_ident(tokens, &mut index)?;
        if !eat_punct(tokens, &mut index, '.') {
            return Err("a catalog name is not a simple identifier");
        }
        let name = take_ident(tokens, &mut index)?;
        if index != tokens.len() {
            return Err("an audited statement is not a supported catalog fact");
        }
        live.remove(&(schema, name));
        return Ok(());
    }
    let schema = take_ident(tokens, &mut index)?;
    if !eat_punct(tokens, &mut index, '.') {
        return Err("a catalog name is not a simple identifier");
    }
    let name = take_ident(tokens, &mut index)?;
    if index != tokens.len() || !live.contains_key(&(schema.clone(), name.clone())) {
        return Err("an audited statement is not a supported catalog fact");
    }
    live.remove(&(schema, name));
    Ok(())
}

fn apply_grant(tokens: &[Tok], live: &mut BTreeMap<(String, String), LiveTable>) -> Result<(), &'static str> {
    let privilege = word_at(tokens, 1).ok_or("an audited statement is not a supported catalog fact")?;
    let on = word_at(tokens, 2).ok_or("an audited statement is not a supported catalog fact")?;
    let target = word_at(tokens, 3).ok_or("an audited statement is not a supported catalog fact")?;
    if !on.eq_ignore_ascii_case("on") {
        return Err("an audited statement is not a supported catalog fact");
    }
    if target.eq_ignore_ascii_case("schema") {
        return Ok(());
    }
    if !target.eq_ignore_ascii_case("table") {
        return Err("an audited statement is not a supported catalog fact");
    }
    let privilege = match privilege.to_ascii_uppercase().as_str() {
        "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "TRUNCATE" | "REFERENCES" | "TRIGGER" => privilege.to_ascii_uppercase(),
        _ => return Err("an audited statement is not a supported catalog fact"),
    };
    let mut index = 4;
    let mut tables = Vec::new();
    loop {
        let schema = take_ident(tokens, &mut index)?;
        if !eat_punct(tokens, &mut index, '.') {
            return Err("a catalog name is not a simple identifier");
        }
        let name = take_ident(tokens, &mut index)?;
        tables.push((schema, name));
        if eat_punct(tokens, &mut index, ',') {
            continue;
        }
        break;
    }
    let to = take_word(tokens, &mut index)?;
    if !to.eq_ignore_ascii_case("to") {
        return Err("an audited statement is not a supported catalog fact");
    }
    let grantee = take_ident(tokens, &mut index)?;
    let mut grantable = false;
    if index != tokens.len() {
        let with = take_word(tokens, &mut index)?;
        let grant = take_word(tokens, &mut index)?;
        let option = take_word(tokens, &mut index)?;
        if !with.eq_ignore_ascii_case("with") || !grant.eq_ignore_ascii_case("grant") || !option.eq_ignore_ascii_case("option") || index != tokens.len() {
            return Err("an audited statement is not a supported catalog fact");
        }
        grantable = true;
    }
    for key in tables {
        if let Some(table) = live.get_mut(&key) {
            table.grants.push(Grant {
                grantee: grantee.clone(),
                privilege: privilege.clone(),
                grantable,
            });
        }
    }
    Ok(())
}

fn next_item(tokens: &[Tok], index: &mut usize) -> Result<(Vec<Tok>, bool), &'static str> {
    let mut item = Vec::new();
    let mut depth = 0_i32;
    while *index < tokens.len() {
        let token = tokens[*index].clone();
        *index += 1;
        match &token {
            Tok::Punct('(') => {
                depth += 1;
                item.push(token);
            }
            Tok::Punct(')') if depth == 0 => return Ok((item, true)),
            Tok::Punct(')') => {
                depth -= 1;
                item.push(token);
            }
            Tok::Punct(',') if depth == 0 => return Ok((item, false)),
            _ => item.push(token),
        }
    }
    Err("an audited statement is not a supported catalog fact")
}

fn capture_tables(value: &Value) -> Result<Vec<Wanted>, &'static str> {
    let rows = value.as_array().ok_or("the capture has no tables")?;
    let mut out = Vec::new();
    for row in rows {
        let schema = simple_name(row.get("schema").and_then(Value::as_str).unwrap_or(""))?;
        let name = simple_name(row.get("table").and_then(Value::as_str).unwrap_or(""))?;
        let pk = simple_name(row.get("pk").and_then(Value::as_str).unwrap_or(""))?;
        let replica = row.get("replica").and_then(Value::as_str).unwrap_or("");
        if replica.len() != 1 || !replica.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return Err("a capture table has no audited create");
        }
        let excluded = row.get("excluded").and_then(Value::as_array).ok_or("a capture table excludes a column")?;
        if !excluded.is_empty() {
            return Err("a capture table excludes a column");
        }
        let columns = row.get("columns").and_then(Value::as_array).ok_or("a capture column is not in the audited create")?;
        let mut names = Vec::new();
        for column in columns {
            names.push(simple_name(column.as_str().unwrap_or(""))?);
        }
        out.push(Wanted {
            schema,
            name,
            columns: names,
            pk,
            replica: replica.to_owned(),
        });
    }
    Ok(out)
}

fn load_basis(bases: &[&[u8]]) -> Result<Basis, &'static str> {
    let mut columns = BTreeMap::new();
    let mut order = BTreeMap::new();
    let mut source_id = String::new();
    for bytes in bases {
        let document = object(bytes)?;
        if document.get("protocol").and_then(Value::as_str) != Some(crate::adapt::CATALOG_IMPORT) {
            return Err("sealed manifests disagree on the source id");
        }
        let top = simple_source(document.get("source_id").and_then(Value::as_str).unwrap_or(""))?;
        let manifest = document.get("manifest").ok_or("sealed manifests disagree on the source id")?;
        if manifest.get("protocol").and_then(Value::as_str) != Some(crate::adapt::SEALED_MANIFEST) {
            return Err("sealed manifests disagree on the source id");
        }
        let inner = simple_source(manifest.get("source_id").and_then(Value::as_str).unwrap_or(""))?;
        if top != inner {
            return Err("sealed manifests disagree on the source id");
        }
        if source_id.is_empty() {
            source_id = top;
        } else if source_id != top {
            return Err("sealed manifests disagree on the source id");
        }
        let tables = manifest.get("tables").and_then(Value::as_array).ok_or("a capture table has no sealed manifest")?;
        for table in tables {
            let schema = simple_name(table.get("logical_schema").and_then(Value::as_str).unwrap_or(""))?;
            let name = simple_name(table.get("logical_table").and_then(Value::as_str).unwrap_or(""))?;
            let listed = table.get("columns").and_then(Value::as_array).ok_or("a column type does not match the sealed manifest")?;
            let mut names = Vec::new();
            for column in listed {
                if column.get("data_type").is_some() {
                    return Err("a column type does not match the sealed manifest");
                }
                let logical = simple_name(column.get("logical_name").and_then(Value::as_str).unwrap_or(""))?;
                let oid = column.get("oid").and_then(Value::as_i64).ok_or("a column type does not match the sealed manifest")?;
                let typmod = column.get("typmod").and_then(Value::as_i64).ok_or("a column type does not match the sealed manifest")?;
                let typmod = i32::try_from(typmod).map_err(|_| "a column type does not match the sealed manifest")?;
                let key = (schema.clone(), name.clone(), logical.clone());
                if columns.get(&key).is_some_and(|previous| *previous != (oid, typmod)) {
                    return Err("a column type does not match the sealed manifest");
                }
                columns.insert(key, (oid, typmod));
                names.push(logical);
            }
            if let Some(previous) = order.get(&(schema.clone(), name.clone())) {
                if previous != &names {
                    return Err("a column type does not match the sealed manifest");
                }
            } else {
                order.insert((schema, name), names);
            }
        }
    }
    if source_id.is_empty() {
        return Err("sealed manifests disagree on the source id");
    }
    Ok(Basis { source_id, columns, order })
}

fn schema_preimage(tables: &[BuiltTable]) -> Result<String, &'static str> {
    let mut ordered: Vec<&BuiltTable> = tables.iter().collect();
    ordered.sort_by(|left, right| (&left.schema, &left.name).cmp(&(&right.schema, &right.name)));
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_table(&mut buf, table)?;
    }
    buf.push_str("]}");
    Ok(buf)
}

fn write_table(buf: &mut String, table: &BuiltTable) -> Result<(), &'static str> {
    buf.push('{');
    field_str(buf, "schema", &table.schema)?;
    buf.push(',');
    field_str(buf, "name", &table.name)?;
    buf.push(',');
    field_str(buf, "relkind", "r")?;
    buf.push(',');
    field_str(buf, "replica_identity", &table.replica)?;
    buf.push(',');
    field_bool(buf, "rls", false)?;
    buf.push(',');
    field_bool(buf, "force_rls", false)?;
    buf.push_str(",\"columns\":[");
    let mut columns = table.columns.iter().collect::<Vec<_>>();
    columns.sort_by_key(|column| column.attnum);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_column(buf, column)?;
    }
    buf.push_str("],\"primary_key\":[");
    for (index, (attnum, name)) in table.primary_key.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        field_i32(buf, "attnum", *attnum)?;
        buf.push(',');
        field_str(buf, "name", name)?;
        buf.push('}');
    }
    buf.push_str("],\"primary_key_nulls_not_distinct\":");
    match table.primary_key_nulls_not_distinct {
        Some(true) => buf.push_str("true"),
        Some(false) => buf.push_str("false"),
        None => buf.push_str("null"),
    }
    buf.push_str(",\"unique_keys\":[],\"foreign_keys\":[");
    let mut foreign = table.foreign.iter().collect::<Vec<_>>();
    foreign.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, key) in foreign.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        field_str(buf, "name", &key.name)?;
        buf.push_str(",\"columns\":[");
        for (col_index, column) in key.columns.iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push('{');
            field_i32(buf, "attnum", column.attnum)?;
            buf.push(',');
            field_str(buf, "name", &column.name)?;
            buf.push(',');
            field_str(buf, "ref_schema", &column.ref_schema)?;
            buf.push(',');
            field_str(buf, "ref_table", &column.ref_table)?;
            buf.push(',');
            field_i32(buf, "ref_attnum", column.ref_attnum)?;
            buf.push(',');
            field_str(buf, "ref_name", &column.ref_name)?;
            buf.push('}');
        }
        buf.push_str("]}");
    }
    buf.push_str("],\"policies\":[]}");
    Ok(())
}

fn write_column(buf: &mut String, column: &BuiltColumn) -> Result<(), &'static str> {
    buf.push('{');
    field_i32(buf, "attnum", column.attnum)?;
    buf.push(',');
    field_str(buf, "name", &column.name)?;
    buf.push(',');
    field_i64(buf, "type_oid", column.phys.oid)?;
    buf.push(',');
    field_i32(buf, "typmod", column.phys.typmod)?;
    buf.push(',');
    field_bool(buf, "not_null", column.not_null)?;
    buf.push(',');
    field_i64(buf, "collation_oid", column.phys.collation_oid)?;
    buf.push(',');
    field_str(buf, "typtype", "b")?;
    buf.push_str(",\"base_type_oid\":null,");
    field_str(buf, "identity", "")?;
    buf.push(',');
    field_str(buf, "generated", "")?;
    buf.push_str(",\"default_sha256\":null,\"enum_labels\":[],\"domain_checks\":[]}");
    Ok(())
}

fn field_str(buf: &mut String, name: &str, value: &str) -> Result<(), &'static str> {
    push_plain(buf, name)?;
    buf.push(':');
    push_plain(buf, value)
}

fn field_bool(buf: &mut String, name: &str, value: bool) -> Result<(), &'static str> {
    push_plain(buf, name)?;
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
    Ok(())
}

fn field_i32(buf: &mut String, name: &str, value: i32) -> Result<(), &'static str> {
    push_plain(buf, name)?;
    buf.push(':');
    buf.push_str(&value.to_string());
    Ok(())
}

fn field_i64(buf: &mut String, name: &str, value: i64) -> Result<(), &'static str> {
    push_plain(buf, name)?;
    buf.push(':');
    buf.push_str(&value.to_string());
    Ok(())
}

fn push_plain(buf: &mut String, value: &str) -> Result<(), &'static str> {
    if value.chars().any(|ch| ch == '"' || ch == '\\' || (ch as u32) < 0x20) {
        return Err("a catalog name is not a simple identifier");
    }
    buf.push('"');
    buf.push_str(value);
    buf.push('"');
    Ok(())
}

fn catalog_document(tables: &[BuiltTable], source_id: &str, database: &str, captured_at: &str, schema_hash: &str) -> Result<Value, &'static str> {
    let mut listed = Vec::new();
    for table in tables {
        let mut columns = Vec::new();
        for column in &table.columns {
            let reason = type_reason(column.phys.type_name, column.phys.typmod);
            columns.push(json!({
                "attnum": column.attnum,
                "base_type_oid": null,
                "collation_name": column.phys.collation_name,
                "collation_oid": column.phys.collation_oid,
                "comment": null,
                "current_privileges": {"insert": table.insert, "select": table.select, "update": table.update},
                "default_expr": null,
                "domain_checks": [],
                "enum_labels": [],
                "generated": "",
                "identity": "",
                "name": column.name,
                "name_ident": column.name,
                "not_null": column.not_null,
                "numeric_precision": column.phys.precision,
                "numeric_scale": column.phys.scale,
                "type_name": column.phys.type_name,
                "type_oid": column.phys.oid,
                "type_schema": "pg_catalog",
                "type_support": {"converted": false, "reason": reason},
                "typmod": column.phys.typmod,
                "typtype": "b"
            }));
        }
        let primary_key: Vec<Value> = table
            .primary_key
            .iter()
            .map(|(attnum, name)| json!({"attnum": attnum, "name": name}))
            .collect();
        let mut foreign = Vec::new();
        for key in &table.foreign {
            let pairs: Vec<Value> = key
                .columns
                .iter()
                .map(|column| {
                    json!({
                        "attnum": column.attnum,
                        "name": column.name,
                        "ref_attnum": column.ref_attnum,
                        "ref_name": column.ref_name,
                        "ref_schema": column.ref_schema,
                        "ref_table": column.ref_table
                    })
                })
                .collect();
            foreign.push(json!({"columns": pairs, "name": key.name}));
        }
        let privileges: Vec<Value> = table
            .grants
            .iter()
            .map(|grant| json!({"grantable": grant.grantable, "grantee": grant.grantee, "privilege": grant.privilege}))
            .collect();
        listed.push(json!({
            "columns": columns,
            "comment": null,
            "foreign_keys": foreign,
            "name": table.name,
            "name_ident": table.name,
            "owner": table.owner,
            "policies": [],
            "primary_key": primary_key,
            "primary_key_nulls_not_distinct": table.primary_key_nulls_not_distinct,
            "privileges": privileges,
            "relkind": "r",
            "replica_identity": table.replica,
            "rls_enabled": false,
            "rls_forced": false,
            "schema": table.schema,
            "schema_ident": table.schema,
            "unique_keys": []
        }));
    }
    Ok(json!({
        "captured_at": captured_at,
        "core_contract": "oo.config/v1",
        "database": database,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "protocol": "oo.source.pgsql/v1",
        "schema_hash": schema_hash,
        "source_id": source_id,
        "tables": listed,
        "type_conversion": "unsupported"
    }))
}

fn cards_profile(tables: &[BuiltTable]) -> Result<Value, &'static str> {
    let mut listed = Vec::new();
    for table in tables {
        let columns: Vec<Value> = table.columns.iter().map(|column| json!({"name": column.name})).collect();
        listed.push(json!({"columns": columns, "name": table.name, "schema": table.schema}));
    }
    Ok(json!({
        "protocol": "oo.selfbuild-control-api.cards-profile/v1",
        "sample_policy": "none",
        "tables": listed
    }))
}

fn type_reason(type_name: &str, typmod: i32) -> &'static str {
    if type_name == "numeric" {
        if typmod < 0 {
            return "unbounded_or_wide_numeric";
        }
        let precision = ((typmod - 4) >> 16) & 0xFFFF;
        if precision > 38 {
            return "unbounded_or_wide_numeric";
        }
    }
    "known_builtin_not_converted"
}

fn object(bytes: &[u8]) -> Result<Value, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the catalog is not utf-8")?;
    let value: Value = serde_json::from_str(text).map_err(|_| "the catalog is not json")?;
    if !value.is_object() {
        return Err("the catalog is not json");
    }
    Ok(value)
}

fn statement_ineligible(sql: &str) -> bool {
    let low = sql.to_ascii_lowercase();
    ["password", "bearer", "dba_checklist", "secret"].iter().any(|word| low.contains(word))
}

fn captured_at(raw: &str) -> Result<String, ()> {
    let Some(rest) = raw.strip_suffix(" UTC") else {
        return Err(());
    };
    let Some((date, time)) = rest.split_once(' ') else {
        return Err(());
    };
    let Some((clock, fraction)) = time.split_once('.') else {
        return Err(());
    };
    if date.len() != 10 || clock.len() != 8 || fraction.is_empty() || fraction.len() > 6 {
        return Err(());
    }
    let stamp = format!("{date}T{clock}.{fraction:0<6}Z");
    if stamp.len() != 27 {
        return Err(());
    }
    let bytes = stamp.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' || bytes[19] != b'.' || bytes[26] != b'Z' {
        return Err(());
    }
    if !bytes.iter().enumerate().all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 26) || byte.is_ascii_digit()) {
        return Err(());
    }
    Ok(stamp)
}

fn simple_name(value: &str) -> Result<String, &'static str> {
    if simple_ident(value) {
        Ok(value.to_owned())
    } else {
        Err("a catalog name is not a simple identifier")
    }
}

fn simple_source(value: &str) -> Result<String, &'static str> {
    let bytes = value.as_bytes();
    if (1..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-')
    {
        Ok(value.to_owned())
    } else {
        Err("sealed manifests disagree on the source id")
    }
}

fn simple_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=63).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn split_statements(sql: &str) -> Result<Vec<String>, ()> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if quoted {
            current.push(ch);
            if ch == '\'' {
                if chars.peek() == Some(&'\'') {
                    current.push(chars.next().ok_or(())?);
                } else {
                    quoted = false;
                }
            }
            continue;
        }
        if ch == '\'' {
            quoted = true;
            current.push(ch);
            continue;
        }
        if ch == ';' {
            if !current.trim().is_empty() {
                out.push(current.trim().to_owned());
            }
            current.clear();
            continue;
        }
        current.push(ch);
    }
    if quoted {
        return Err(());
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    Ok(out)
}

fn csv_records(text: &str) -> Result<Vec<Vec<String>>, ()> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {}
            _ => field.push(ch),
        }
    }
    if quoted {
        return Err(());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

#[derive(Clone)]
enum Tok {
    Word(String),
    Number(i32),
    Punct(char),
}

fn tokenize(sql: &str) -> Result<Vec<Tok>, ()> {
    let mut out = Vec::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '\'' {
            loop {
                let next = chars.next().ok_or(())?;
                if next == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        continue;
                    }
                    break;
                }
            }
            out.push(Tok::Word(String::new()));
            continue;
        }
        if ch == '"' {
            return Err(());
        }
        if matches!(ch, '(' | ')' | ',' | '.') {
            out.push(Tok::Punct(ch));
            continue;
        }
        if ch.is_ascii_digit() {
            let mut number = String::new();
            number.push(ch);
            while chars.peek().is_some_and(|item| item.is_ascii_digit()) {
                number.push(chars.next().ok_or(())?);
            }
            out.push(Tok::Number(number.parse().map_err(|_| ())?));
            continue;
        }
        if ch == '_' || ch.is_ascii_alphabetic() {
            let mut word = String::new();
            word.push(ch);
            while chars.peek().is_some_and(|item| item.is_ascii_alphanumeric() || *item == '_') {
                word.push(chars.next().ok_or(())?);
            }
            out.push(Tok::Word(word));
            continue;
        }
        return Err(());
    }
    Ok(out)
}

fn first_word(tokens: &[Tok]) -> Option<&str> {
    match tokens.first() {
        Some(Tok::Word(word)) if !word.is_empty() => Some(word.as_str()),
        _ => None,
    }
}

fn word_at(tokens: &[Tok], index: usize) -> Option<&str> {
    match tokens.get(index) {
        Some(Tok::Word(word)) if !word.is_empty() => Some(word.as_str()),
        _ => None,
    }
}

fn peek_word(tokens: &[Tok], index: usize) -> Option<&str> {
    word_at(tokens, index)
}

fn peek_punct(tokens: &[Tok], index: usize) -> Option<char> {
    match tokens.get(index) {
        Some(Tok::Punct(ch)) => Some(*ch),
        _ => None,
    }
}

fn take_word(tokens: &[Tok], index: &mut usize) -> Result<String, &'static str> {
    let word = word_at(tokens, *index).ok_or("an audited statement is not a supported catalog fact")?.to_owned();
    *index += 1;
    Ok(word)
}

fn take_ident(tokens: &[Tok], index: &mut usize) -> Result<String, &'static str> {
    let word = take_word(tokens, index)?.to_ascii_lowercase();
    if simple_ident(&word) {
        Ok(word)
    } else {
        Err("a catalog name is not a simple identifier")
    }
}

fn take_number(tokens: &[Tok], index: &mut usize) -> Result<i32, &'static str> {
    match tokens.get(*index) {
        Some(Tok::Number(number)) => {
            *index += 1;
            Ok(*number)
        }
        _ => Err("a column type does not match the sealed manifest"),
    }
}

fn eat_punct(tokens: &[Tok], index: &mut usize, expected: char) -> bool {
    if peek_punct(tokens, *index) == Some(expected) {
        *index += 1;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{csv_records, schema_preimage, BuiltColumn, BuiltTable, Phys};

    fn customer() -> BuiltTable {
        BuiltTable {
            schema: "public".to_owned(),
            name: "customer".to_owned(),
            replica: "d".to_owned(),
            owner: "owner".to_owned(),
            database: "src".to_owned(),
            columns: vec![BuiltColumn {
                attnum: 1,
                name: "id".to_owned(),
                phys: Phys {
                    oid: 23,
                    typmod: -1,
                    type_name: "int4",
                    precision: None,
                    scale: None,
                    collation_oid: 0,
                    collation_name: None,
                },
                not_null: true,
                reference: None,
            }],
            primary_key: vec![(1, "id".to_owned())],
            primary_key_nulls_not_distinct: Some(false),
            foreign: Vec::new(),
            grants: Vec::new(),
            select: false,
            insert: false,
            update: false,
        }
    }

    #[test]
    fn empty_and_customer_preimage_match_pg_catalog_hash() {
        let empty = schema_preimage(&[]).expect("empty");
        assert_eq!(empty, r#"{"tables":[]}"#);
        assert_eq!(
            crate::canonical::sha256_hex(empty.as_bytes()),
            "cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc"
        );
        let expected = r#"{"tables":[{"schema":"public","name":"customer","relkind":"r","replica_identity":"d","rls":false,"force_rls":false,"columns":[{"attnum":1,"name":"id","type_oid":23,"typmod":-1,"not_null":true,"collation_oid":0,"typtype":"b","base_type_oid":null,"identity":"","generated":"","default_sha256":null,"enum_labels":[],"domain_checks":[]}],"primary_key":[{"attnum":1,"name":"id"}],"primary_key_nulls_not_distinct":false,"unique_keys":[],"foreign_keys":[],"policies":[]}]}"#;
        let actual = schema_preimage(&[customer()]).expect("customer");
        assert_eq!(actual, expected);
        assert_eq!(
            crate::canonical::sha256_hex(actual.as_bytes()),
            "60f7939195cc539644d11132ae4a604c3b025167f1fe657d121167feb8463499"
        );
    }

    #[test]
    fn csv_keeps_a_quoted_newline() {
        let rows = csv_records("a,\"b\nc\",d\n").expect("csv");
        assert_eq!(rows, vec![vec!["a".to_owned(), "b\nc".to_owned(), "d".to_owned()]]);
    }

    #[test]
    fn captured_at_pads_fraction_without_truncation() {
        assert_eq!(
            super::captured_at("2026-10-11 14:19:33.582 UTC").expect("stamp"),
            "2026-10-11T14:19:33.582000Z"
        );
        assert!(super::captured_at("2026-10-11 14:19:33.5820001 UTC").is_err());
    }
}
