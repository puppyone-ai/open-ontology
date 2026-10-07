//! Column, relation, enum, and expert-card rules from sealed JSON.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::canonical::canonical_sha256;
use crate::scan::parse_json;
use crate::sha256::digest_hex;
use crate::{Fail, CARDS_COMMIT, CATALOG_COMMIT, MAX_INPUT_BYTES, REVIEW_PROTOCOL};

const SCALE: i128 = 100_000_000;
const SENSITIVE: &[&str] = &["national_id", "id_card", "sfzh", "ssn", "passport", "password", "secret"];
const PHONE: &[&str] = &["phone", "mobile", "tel", "lxdh", "telephone"];

struct Table {
    schema: String,
    name: String,
    pk: Vec<String>,
    unique: Vec<Vec<String>>,
    fks: Vec<Fk>,
    foreign_keys: Value,
    cols: Vec<Col>,
    reltuples: Option<String>,
    profile_present: bool,
}

struct Col {
    attnum: i32,
    name: String,
    type_name: String,
    not_null: bool,
    n_distinct: Option<String>,
}

struct Fk {
    name: String,
    pairs: Vec<Pair>,
}

struct Pair {
    attnum: i32,
    name: String,
    ref_schema: String,
    ref_table: String,
    ref_attnum: i32,
    ref_name: String,
}

struct Decision {
    semantic: &'static str,
    pii: &'static str,
    confidence: &'static str,
    uncertainty: &'static str,
    rules: &'static [&'static str],
}

struct OutCol {
    stable_id: String,
    dataset: String,
    column: String,
    attnum: i32,
    decision: Decision,
    state: &'static str,
    profile: &'static str,
}

struct Rel {
    stable_id: String,
    provenance: &'static str,
    cardinality: &'static str,
    conflict: bool,
    confidence: &'static str,
    uncertainty: &'static str,
    type_compatible: bool,
    from_label: String,
    to_label: String,
    group: String,
    schema: String,
    table: String,
    column: String,
    attnum: i32,
    name_match: &'static str,
    constraint: Option<String>,
    profile: &'static str,
    rules: &'static [&'static str],
    bridge: Option<String>,
}

struct ConflictOut {
    stable_id: String,
    dataset: String,
    column: String,
    attnum: i32,
}

pub fn produce(catalog: &[u8], profile: &[u8], cards: &[u8], auto_accept: &str) -> Result<Value, Fail> {
    for bytes in [catalog, profile, cards] {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(Fail::new("bound.input"));
        }
    }
    if !matches!(auto_accept, "rule_high_confidence" | "none") {
        return Err(Fail::new("schema"));
    }
    let catalog_value = parse_json(catalog)?;
    let profile_value = parse_json(profile)?;
    let cards_value = parse_json(cards)?;
    let tables = bind(&catalog_value, &profile_value, &cards_value)?;
    let mut columns = Vec::new();
    for table in &tables {
        for col in &table.cols {
            let decision = decide(&tables, table, col);
            let state = decide_state(auto_accept, decision.pii, decision.confidence, decision.uncertainty, decision.semantic);
            columns.push(OutCol {
                stable_id: format!("col/{}.{}.{}#{}", table.schema, table.name, col.name, col.attnum),
                dataset: format!("{}.{}", table.schema, table.name),
                column: col.name.clone(),
                attnum: col.attnum,
                decision,
                state,
                profile: if table.profile_present { "present" } else { "absent" },
            });
        }
    }
    if columns.iter().any(|col| {
        (col.decision.pii == "personal" || col.decision.pii == "sensitive") && col.state == "auto_accepted"
    }) {
        return Err(Fail::new("pii.leak"));
    }
    let (relations, conflicts) = relations(&tables);
    let enums = enums(&tables, &columns);
    let dictionaries = dictionaries(&tables);
    columns.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    let schema_hash = req_str(object(&catalog_value)?, "schema_hash")?.to_owned();
    let evidence_hash = req_str(object(&cards_value)?, "evidence_hash")?.to_owned();
    let catalog_sha = digest_hex(catalog).ok_or(Fail::new("json"))?;
    let profile_sha = digest_hex(profile).ok_or(Fail::new("json"))?;
    let cards_sha = digest_hex(cards).ok_or(Fail::new("json"))?;
    Ok(assemble(
        Stamp {
            auto_accept,
            schema_hash: &schema_hash,
            evidence_hash: &evidence_hash,
            catalog_sha: &catalog_sha,
            profile_sha: &profile_sha,
            cards_sha: &cards_sha,
        },
        &columns,
        &relations,
        &enums,
        &dictionaries,
        &conflicts,
    ))
}

pub fn seal_card(card: &mut Value) -> Result<(), Fail> {
    let Some(map) = card.as_object_mut() else {
        return Err(Fail::new("schema"));
    };
    map.remove("card_sha256");
    let hash = canonical_sha256(card).ok_or(Fail::new("json"))?;
    let Some(map) = card.as_object_mut() else {
        return Err(Fail::new("schema"));
    };
    map.insert("card_sha256".to_owned(), Value::String(hash));
    Ok(())
}

struct Stamp<'a> {
    auto_accept: &'a str,
    schema_hash: &'a str,
    evidence_hash: &'a str,
    catalog_sha: &'a str,
    profile_sha: &'a str,
    cards_sha: &'a str,
}

fn assemble(
    stamp: Stamp<'_>,
    columns: &[OutCol],
    relations: &[Rel],
    enums: &[Value],
    dictionaries: &[Value],
    conflicts: &[ConflictOut],
) -> Value {
    let experts = expert_cards(columns, relations, enums, conflicts);
    serde_json::json!({
        "protocol": REVIEW_PROTOCOL,
        "schema_version": REVIEW_PROTOCOL,
        "effect": "review_only",
        "authenticated": false,
        "parent_auth": "required",
        "publication": "unavailable",
        "grants": "not_authorized",
        "certification": "none",
        "entity_merge": "not_performed",
        "metric_certification": "not_performed",
        "knowledge_graph": "not_emitted",
        "sample_policy": "none",
        "comment_disclosure": "withheld",
        "source_pins": {"catalog_commit": CATALOG_COMMIT, "cards_commit": CARDS_COMMIT},
        "identity": {
            "schema_hash": stamp.schema_hash,
            "evidence_hash": stamp.evidence_hash,
            "catalog_sha256": stamp.catalog_sha,
            "profile_sha256": stamp.profile_sha,
            "cards_sha256": stamp.cards_sha
        },
        "auto_accept": stamp.auto_accept,
        "columns": columns.iter().map(column_value).collect::<Vec<_>>(),
        "relations": relations.iter().map(relation_value).collect::<Vec<_>>(),
        "enums": enums,
        "dictionaries": dictionaries,
        "conflicts": conflicts.iter().map(conflict_value).collect::<Vec<_>>(),
        "expert_cards": experts,
        "pii_leak_count": 0
    })
}

fn expert_cards(columns: &[OutCol], relations: &[Rel], enums: &[Value], conflicts: &[ConflictOut]) -> Vec<Value> {
    let mut experts = Vec::new();
    for col in columns {
        if col.state != "pending" {
            continue;
        }
        let kind = match col.decision.pii {
            "personal" => "personal",
            "sensitive" => "sensitive",
            _ => "column",
        };
        let question = match kind {
            "personal" => format!(
                "Confirm the PII class of {dataset}.{column}. The rule marked it personal and did not grant access.",
                dataset = col.dataset,
                column = col.column
            ),
            "sensitive" => format!(
                "Confirm the PII class of {dataset}.{column}. The rule marked it sensitive and did not grant access.",
                dataset = col.dataset,
                column = col.column
            ),
            _ => format!("What is the semantic type and PII class of {dataset}.{column}?", dataset = col.dataset, column = col.column),
        };
        experts.push(expert(kind, &col.stable_id, question, col.decision.uncertainty, column_evidence(col)));
    }
    for item in enums {
        let id = item["stable_id"].as_str().unwrap_or("");
        let dataset = item["dataset"].as_str().unwrap_or("");
        let column = item["column"].as_str().unwrap_or("");
        let question = format!(
            "What do the distinct values of {dataset}.{column} mean? Values are withheld. The signature is undetermined and not approved."
        );
        experts.push(expert(
            "enum",
            id,
            question,
            "1",
            serde_json::json!({"rule_ids": ["enum.low_cardinality"], "values": "withheld"}),
        ));
    }
    for rel in relations {
        if rel.uncertainty == "0" && !rel.conflict {
            continue;
        }
        if rel.provenance == "inferred" {
            let bridge = rel.bridge.clone().unwrap_or_default();
            let question = format!(
                "Do {left} and {right} form a many-to-many relation through {bridge}? This candidate does not merge entities.",
                left = rel.from_label,
                right = rel.to_label
            );
            experts.push(expert("bridge", &rel.stable_id, question, rel.uncertainty, relation_evidence(rel)));
        } else {
            let compat = if rel.type_compatible { "true" } else { "false" };
            let question = format!(
                "Is {from} a foreign key to {to}? Cardinality is {cardinality}. Type compatibility is {compat}.",
                from = rel.from_label,
                to = rel.to_label,
                cardinality = rel.cardinality
            );
            experts.push(expert("relation", &rel.stable_id, question, rel.uncertainty, relation_evidence(rel)));
        }
    }
    for conflict in conflicts {
        let question = format!(
            "Column {dataset}.{column} has more than one relation candidate. No candidate was selected.",
            dataset = conflict.dataset,
            column = conflict.column
        );
        experts.push(expert(
            "conflict",
            &conflict.stable_id,
            question,
            "0.5",
            serde_json::json!({"rule_ids": ["relation.conflict"], "selected": null}),
        ));
    }
    experts.sort_by(|left, right| left["stable_id"].as_str().cmp(&right["stable_id"].as_str()));
    experts
}

fn expert(kind: &str, anchor: &str, question: String, uncertainty: &str, evidence: Value) -> Value {
    serde_json::json!({
        "stable_id": format!("ask/{kind}/{anchor}"),
        "question": question,
        "evidence": evidence,
        "uncertainty": uncertainty,
        "anchors": [anchor],
        "provenance": "rule"
    })
}

fn column_evidence(col: &OutCol) -> Value {
    serde_json::json!({
        "rule_ids": rule_values(col.decision.rules),
        "profile": col.profile
    })
}

fn relation_evidence(rel: &Rel) -> Value {
    let mut evidence = serde_json::json!({
        "rule_ids": rule_values(rel.rules),
        "name_match": rel.name_match,
        "type_compatible": rel.type_compatible,
        "profile": rel.profile,
        "constraint": rel.constraint.clone()
    });
    if let Some(bridge) = &rel.bridge {
        evidence["bridge_dataset"] = Value::String(bridge.clone());
    }
    evidence
}

fn column_value(col: &OutCol) -> Value {
    serde_json::json!({
        "stable_id": col.stable_id,
        "dataset": col.dataset,
        "column": col.column,
        "attnum": col.attnum,
        "semantic_type": col.decision.semantic,
        "pii": col.decision.pii,
        "state": col.state,
        "confidence": col.decision.confidence,
        "uncertainty": col.decision.uncertainty,
        "rule_ids": rule_values(col.decision.rules)
    })
}

fn relation_value(rel: &Rel) -> Value {
    serde_json::json!({
        "stable_id": rel.stable_id,
        "provenance": rel.provenance,
        "cardinality": rel.cardinality,
        "conflict": rel.conflict,
        "confidence": rel.confidence,
        "uncertainty": rel.uncertainty,
        "type_compatible": rel.type_compatible,
        "from": rel.from_label,
        "to": rel.to_label,
        "status": "candidate"
    })
}

fn conflict_value(conflict: &ConflictOut) -> Value {
    serde_json::json!({
        "stable_id": conflict.stable_id,
        "selected": null,
        "dataset": conflict.dataset,
        "column": conflict.column,
        "attnum": conflict.attnum
    })
}

fn rule_values(rules: &[&str]) -> Vec<Value> {
    rules.iter().map(|rule| Value::String((*rule).to_owned())).collect()
}

fn decide(tables: &[Table], table: &Table, col: &Col) -> Decision {
    if has_name(&col.name, SENSITIVE) {
        return Decision { semantic: "unknown", pii: "sensitive", confidence: "1", uncertainty: "1", rules: &["name.sensitive"] };
    }
    if let Some(decision) = personal(&col.name) {
        return decision;
    }
    if col.name == "dh" {
        return Decision { semantic: "unknown", pii: "unknown", confidence: "0", uncertainty: "1", rules: &["name.ambiguous"] };
    }
    if col.type_name == "bool" {
        return Decision { semantic: "boolean_flag", pii: "none", confidence: "1", uncertainty: "0", rules: &["type.bool"] };
    }
    if let Some(decision) = time_decision(&col.type_name, &col.name) {
        return decision;
    }
    if let Some(decision) = measure_decision(col) {
        return decision;
    }
    let low = proven_low(col.n_distinct.as_deref(), table.reltuples.as_deref());
    if low && is_text_or_int(&col.type_name) && is_status_name(&col.name) {
        return Decision { semantic: "status_code", pii: "none", confidence: "1", uncertainty: "0", rules: &["name.status", "profile.low_cardinality"] };
    }
    if low && is_text_or_int(&col.type_name) && is_code_name(&col.name) {
        return Decision { semantic: "code", pii: "none", confidence: "1", uncertainty: "0", rules: &["name.code", "profile.low_cardinality"] };
    }
    if let Some(compatible) = column_fk_compatible(tables, table, &col.name) {
        if compatible {
            return Decision { semantic: "foreign_key", pii: "none", confidence: "1", uncertainty: "0", rules: &["key.foreign_declared", "relation.type_compatible"] };
        }
        return Decision { semantic: "unknown", pii: "unknown", confidence: "0.5", uncertainty: "1", rules: &["key.foreign_declared", "relation.type_incompatible"] };
    }
    if is_sole_pk(table, &col.name) && (col.name == "id" || col.name.ends_with("_id") || col.name.ends_with("_key") || is_int(&col.type_name)) {
        let rules: &'static [&str] = if is_int(&col.type_name) { &["key.primary", "type.integer"] } else { &["key.primary"] };
        return Decision { semantic: "identifier", pii: "none", confidence: "1", uncertainty: "0", rules };
    }
    let targets = naming_targets(tables, table, col);
    if targets.len() == 1 {
        return Decision { semantic: "foreign_key", pii: "none", confidence: "0.5", uncertainty: "0.5", rules: &["relation.naming"] };
    }
    if targets.len() >= 2 {
        return Decision { semantic: "unknown", pii: "none", confidence: "0.5", uncertainty: "0.5", rules: &["relation.naming", "relation.conflict"] };
    }
    if table.profile_present {
        Decision { semantic: "unknown", pii: "unknown", confidence: "0", uncertainty: "1", rules: &["unmatched"] }
    } else {
        Decision { semantic: "unknown", pii: "unknown", confidence: "0", uncertainty: "1", rules: &["profile.absent"] }
    }
}

fn decide_state(auto: &str, pii: &str, confidence: &str, uncertainty: &str, semantic: &str) -> &'static str {
    const ALLOW: &[&str] = &[
        "boolean_flag",
        "money",
        "quantity",
        "ratio",
        "event_time",
        "date",
        "audit_time",
        "identifier",
        "foreign_key",
        "status_code",
        "code",
    ];
    if auto == "rule_high_confidence" && pii == "none" && confidence == "1" && uncertainty == "0" && ALLOW.contains(&semantic) {
        "auto_accepted"
    } else {
        "pending"
    }
}

fn personal(name: &str) -> Option<Decision> {
    if has_name(name, PHONE) {
        return Some(Decision { semantic: "phone", pii: "personal", confidence: "1", uncertainty: "0", rules: &["name.phone"] });
    }
    if has_name(name, &["email"]) {
        return Some(Decision { semantic: "email", pii: "personal", confidence: "1", uncertainty: "0", rules: &["name.email"] });
    }
    if matches!(name, "display_name" | "person_name" | "full_name" | "first_name" | "last_name") {
        return Some(Decision { semantic: "person_name", pii: "personal", confidence: "1", uncertainty: "0", rules: &["name.person"] });
    }
    if name == "address" || name == "addr" || name.strip_suffix("address").is_some_and(|prefix| prefix.ends_with('_')) {
        return Some(Decision { semantic: "address", pii: "personal", confidence: "1", uncertainty: "0", rules: &["name.address"] });
    }
    None
}

fn time_decision(type_name: &str, name: &str) -> Option<Decision> {
    let audit = matches!(name, "created_at" | "cjsj" | "create_time" | "updated_at" | "xgsj" | "update_time");
    let decision = |semantic, rules| Decision { semantic, pii: "none", confidence: "1", uncertainty: "0", rules };
    match (type_name, audit) {
        ("date", true) => Some(decision("audit_time", &["name.audit", "type.date"])),
        ("date", false) => Some(decision("date", &["type.date"])),
        ("timestamp" | "timestamptz", true) => Some(decision("audit_time", &["name.audit", "type.timestamp"])),
        ("timestamp" | "timestamptz", false) => Some(decision("event_time", &["type.timestamp"])),
        _ => None,
    }
}

fn measure_decision(col: &Col) -> Option<Decision> {
    let numeric = col.type_name == "numeric";
    let rules = |name: &'static str| -> &'static [&'static str] {
        if numeric {
            match name {
                "money" => &["name.money", "type.numeric"],
                "quantity" => &["name.quantity", "type.numeric"],
                _ => &["name.ratio", "type.numeric"],
            }
        } else {
            match name {
                "money" => &["name.money", "type.integer"],
                _ => &["name.quantity", "type.integer"],
            }
        }
    };
    if is_money_name(&col.name) && (numeric || is_int(&col.type_name)) {
        return Some(Decision { semantic: "money", pii: "none", confidence: "1", uncertainty: "0", rules: rules("money") });
    }
    if is_quantity_name(&col.name) && (numeric || is_int(&col.type_name)) {
        return Some(Decision { semantic: "quantity", pii: "none", confidence: "1", uncertainty: "0", rules: rules("quantity") });
    }
    if is_ratio_name(&col.name) && numeric {
        return Some(Decision { semantic: "ratio", pii: "none", confidence: "1", uncertainty: "0", rules: rules("ratio") });
    }
    None
}

fn relations(tables: &[Table]) -> (Vec<Rel>, Vec<ConflictOut>) {
    let mut rels = Vec::new();
    for table in tables {
        for fk in &table.fks {
            rels.push(declared_relation(tables, table, fk));
        }
        for col in &table.cols {
            for target in naming_targets(tables, table, col) {
                rels.push(naming_relation(tables, table, col, target));
            }
        }
        if let Some(rel) = bridge_relation(tables, table) {
            rels.push(rel);
        }
    }
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, rel) in rels.iter().enumerate() {
        if rel.provenance == "inferred" {
            continue;
        }
        groups.entry(rel.group.clone()).or_default().push(index);
    }
    let mut conflicts = Vec::new();
    for indexes in groups.values() {
        if indexes.len() < 2 {
            continue;
        }
        for index in indexes {
            rels[*index].conflict = true;
        }
        let first = &rels[indexes[0]];
        conflicts.push(ConflictOut {
            stable_id: format!("conflict/{}.{}.{}#{}", first.schema, first.table, first.column, first.attnum),
            dataset: format!("{}.{}", first.schema, first.table),
            column: first.column.clone(),
            attnum: first.attnum,
        });
    }
    rels.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    conflicts.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    (rels, conflicts)
}

fn declared_relation(tables: &[Table], table: &Table, fk: &Fk) -> Rel {
    let locals: Vec<String> = fk.pairs.iter().map(|pair| pair.name.clone()).collect();
    let refs: Vec<String> = fk.pairs.iter().map(|pair| pair.ref_name.clone()).collect();
    let head = &fk.pairs[0];
    let same = fk.pairs.iter().all(|pair| pair.ref_schema == head.ref_schema && pair.ref_table == head.ref_table);
    let compatible = same && fk_compatible(tables, table, fk);
    let (cardinality, card_unc) = if same {
        cardinality_of(end_unique(tables, &table.schema, &table.name, &locals), end_unique(tables, &head.ref_schema, &head.ref_table, &refs))
    } else {
        ("unknown", "1")
    };
    let uncertainty = if compatible { card_unc } else { max_unc(card_unc, "0.5") };
    let from_label = end_label(&table.schema, &table.name, &locals);
    let to_label = end_label(&head.ref_schema, &head.ref_table, &refs);
    Rel {
        stable_id: format!("rel/{from_label}/to/{to_label}/declared_fk"),
        provenance: "declared_fk",
        cardinality,
        conflict: false,
        confidence: "1",
        uncertainty,
        type_compatible: compatible,
        group: from_label.clone(),
        schema: table.schema.clone(),
        table: table.name.clone(),
        column: head.name.clone(),
        attnum: head.attnum,
        name_match: name_match(&fk.pairs),
        constraint: Some(fk.name.clone()),
        profile: if table.profile_present { "present" } else { "absent" },
        rules: if compatible { &["relation.declared_fk", "relation.type_compatible"] } else { &["relation.declared_fk", "relation.type_incompatible"] },
        bridge: None,
        from_label,
        to_label,
    }
}

fn naming_relation(tables: &[Table], table: &Table, col: &Col, target: (String, String, String)) -> Rel {
    let locals = vec![col.name.clone()];
    let refs = vec![target.2.clone()];
    let (cardinality, card_unc) = cardinality_of(
        end_unique(tables, &table.schema, &table.name, &locals),
        end_unique(tables, &target.0, &target.1, &refs),
    );
    let from_label = end_label(&table.schema, &table.name, &locals);
    let to_label = end_label(&target.0, &target.1, &refs);
    let pair = Pair {
        attnum: col.attnum,
        name: col.name.clone(),
        ref_schema: target.0,
        ref_table: target.1,
        ref_attnum: 0,
        ref_name: target.2,
    };
    Rel {
        stable_id: format!("rel/{from_label}/to/{to_label}/naming"),
        provenance: "naming",
        cardinality,
        conflict: false,
        confidence: "0.5",
        uncertainty: max_unc("0.5", card_unc),
        type_compatible: true,
        group: from_label.clone(),
        schema: table.schema.clone(),
        table: table.name.clone(),
        column: col.name.clone(),
        attnum: col.attnum,
        name_match: name_match(std::slice::from_ref(&pair)),
        constraint: None,
        profile: if table.profile_present { "present" } else { "absent" },
        rules: &["relation.naming"],
        bridge: None,
        from_label,
        to_label,
    }
}

fn bridge_relation(tables: &[Table], table: &Table) -> Option<Rel> {
    if table.fks.len() != 2 || table.fks.iter().any(|fk| fk.pairs.len() != 1) {
        return None;
    }
    let mut locals: Vec<String> = table.fks.iter().map(|fk| fk.pairs[0].name.clone()).collect();
    if !key_covers(&table.pk, &locals) {
        return None;
    }
    let mut ends = Vec::new();
    for fk in &table.fks {
        let pair = &fk.pairs[0];
        if !is_sole_pk_of(tables, &pair.ref_schema, &pair.ref_table, &pair.ref_name) {
            return None;
        }
        ends.push((pair.ref_schema.clone(), pair.ref_table.clone(), pair.ref_name.clone()));
    }
    ends.sort();
    locals.sort();
    let left = &ends[0];
    let right = &ends[1];
    let from_label = end_label(&left.0, &left.1, std::slice::from_ref(&left.2));
    let to_label = end_label(&right.0, &right.1, std::slice::from_ref(&right.2));
    let bridge = format!("{}.{}", table.schema, table.name);
    Some(Rel {
        stable_id: format!("rel/{bridge}/{from_label}/to/{to_label}/inferred"),
        provenance: "inferred",
        cardinality: "many_to_many",
        conflict: false,
        confidence: "0.5",
        uncertainty: "0.5",
        type_compatible: true,
        group: format!("bridge:{bridge}"),
        schema: table.schema.clone(),
        table: table.name.clone(),
        column: locals[0].clone(),
        attnum: 0,
        name_match: "none",
        constraint: None,
        profile: if table.profile_present { "present" } else { "absent" },
        rules: &["relation.bridge"],
        bridge: Some(bridge),
        from_label,
        to_label,
    })
}

fn enums(tables: &[Table], columns: &[OutCol]) -> Vec<Value> {
    let mut out = Vec::new();
    for table in tables {
        for col in &table.cols {
            let Some(found) = columns.iter().find(|item| item.dataset == format!("{}.{}", table.schema, table.name) && item.column == col.name) else {
                continue;
            };
            let low = proven_low(col.n_distinct.as_deref(), table.reltuples.as_deref());
            if !low || col.name == "dh" || found.decision.pii == "personal" || found.decision.pii == "sensitive" {
                continue;
            }
            if col.type_name != "text" && !is_int(&col.type_name) {
                continue;
            }
            out.push(serde_json::json!({
                "stable_id": format!("enum/{}.{}.{}#{}", table.schema, table.name, col.name, col.attnum),
                "dataset": format!("{}.{}", table.schema, table.name),
                "column": col.name,
                "attnum": col.attnum,
                "status": "undetermined",
                "approved": false,
                "class": "undetermined",
                "values": "withheld",
                "time_window": "unobserved",
                "rows": null,
                "jaccard": null
            }));
        }
    }
    out.sort_by(|left, right| left["stable_id"].as_str().cmp(&right["stable_id"].as_str()));
    out
}

fn dictionaries(tables: &[Table]) -> Vec<Value> {
    let mut out = Vec::new();
    for table in tables {
        if !is_dict_name(&table.name) || !dict_rows(table.reltuples.as_deref()) {
            continue;
        }
        let code = table.cols.iter().any(|col| col.name == "code" || col.name == "dm");
        let label = table.cols.iter().any(|col| col.name == "name" || col.name == "label" || col.name == "mc");
        if !(code && label) {
            continue;
        }
        out.push(serde_json::json!({
            "stable_id": format!("dict/{}.{}", table.schema, table.name),
            "status": "undetermined",
            "approved": false,
            "match": "structural_only",
            "dictionary_link": "uncompared"
        }));
    }
    out.sort_by(|left, right| left["stable_id"].as_str().cmp(&right["stable_id"].as_str()));
    out
}

fn bind(catalog: &Value, profile: &Value, cards: &Value) -> Result<Vec<Table>, Fail> {
    let catalog_map = object(catalog)?;
    require_protocol(catalog_map, "oo.source.pgsql/v1")?;
    let schema_hash = hex_field(catalog_map, "schema_hash")?;
    let mut tables = parse_catalog(catalog_map)?;
    let profile_map = object(profile)?;
    require_protocol(profile_map, "oo.pg-builder.profile/v1")?;
    sample_none(profile_map)?;
    let profiles = parse_profile(profile_map, &tables)?;
    let cards_map = object(cards)?;
    require_protocol(cards_map, "oo.pg-builder.cards/v1")?;
    sample_none(cards_map)?;
    if cards_map.get("schema_version").and_then(Value::as_str) != Some("oo.pg-builder.card/v1") {
        return Err(Fail::new("schema"));
    }
    if hex_field(cards_map, "schema_hash")? != schema_hash {
        return Err(Fail::new("identity.stale"));
    }
    let _evidence = hex_field(cards_map, "evidence_hash")?;
    apply_cards(&mut tables, cards_map, &profiles)?;
    Ok(tables)
}

fn parse_catalog(map: &Map<String, Value>) -> Result<Vec<Table>, Fail> {
    let items = array(map.get("tables").ok_or(Fail::new("schema"))?)?;
    if items.len() > 64 {
        return Err(Fail::new("bound.tables"));
    }
    let mut tables = Vec::new();
    for item in items {
        tables.push(parse_table(object(item)?)?);
    }
    let mut names = BTreeMap::new();
    for table in &tables {
        if names.insert(format!("{}.{}", table.schema, table.name), ()).is_some() {
            return Err(Fail::new("schema"));
        }
    }
    check_targets(&tables)?;
    Ok(tables)
}

fn parse_table(map: &Map<String, Value>) -> Result<Table, Fail> {
    let schema = ident_field(map, "schema")?;
    let name = ident_field(map, "name")?;
    if ident_field(map, "schema_ident")? != schema || ident_field(map, "name_ident")? != name {
        return Err(Fail::new("identity.stale"));
    }
    let col_items = array(map.get("columns").ok_or(Fail::new("schema"))?)?;
    if col_items.len() > 256 {
        return Err(Fail::new("bound.columns"));
    }
    let mut cols = Vec::new();
    for item in col_items {
        let col_map = object(item)?;
        let col_name = ident_field(col_map, "name")?;
        if cols.iter().any(|col: &Col| col.name == col_name) {
            return Err(Fail::new("schema"));
        }
        allow_text(col_map, "correlation")?;
        allow_text(col_map, "null_frac")?;
        cols.push(Col {
            attnum: json_i32(col_map.get("attnum").ok_or(Fail::new("schema"))?)?,
            name: col_name,
            type_name: ident_field(col_map, "type_name")?,
            not_null: json_bool(col_map.get("not_null").ok_or(Fail::new("schema"))?)?,
            n_distinct: None,
        });
    }
    let pk = key_names(map.get("primary_key").ok_or(Fail::new("schema"))?, &cols)?;
    let mut unique = Vec::new();
    for item in array(map.get("unique_keys").ok_or(Fail::new("schema"))?)? {
        let uk = object(item)?;
        unique.push(key_names(uk.get("columns").ok_or(Fail::new("schema"))?, &cols)?);
    }
    let foreign_keys = map.get("foreign_keys").cloned().ok_or(Fail::new("schema"))?;
    let mut fks = Vec::new();
    for item in array(&foreign_keys)? {
        fks.push(parse_fk(object(item)?, &cols)?);
    }
    Ok(Table { schema, name, pk, unique, fks, foreign_keys, cols, reltuples: None, profile_present: false })
}

fn parse_fk(map: &Map<String, Value>, cols: &[Col]) -> Result<Fk, Fail> {
    let name = ident_field(map, "name")?;
    let mut pairs = Vec::new();
    for item in array(map.get("columns").ok_or(Fail::new("schema"))?)? {
        let pair = object(item)?;
        let local = ident_field(pair, "name")?;
        let attnum = json_i32(pair.get("attnum").ok_or(Fail::new("schema"))?)?;
        if !cols.iter().any(|col| col.name == local && col.attnum == attnum) {
            return Err(Fail::new("schema"));
        }
        pairs.push(Pair {
            attnum,
            name: local,
            ref_schema: ident_field(pair, "ref_schema")?,
            ref_table: ident_field(pair, "ref_table")?,
            ref_attnum: json_i32(pair.get("ref_attnum").ok_or(Fail::new("schema"))?)?,
            ref_name: ident_field(pair, "ref_name")?,
        });
    }
    if pairs.is_empty() {
        return Err(Fail::new("schema"));
    }
    Ok(Fk { name, pairs })
}

fn check_targets(tables: &[Table]) -> Result<(), Fail> {
    for table in tables {
        for pair in table.fks.iter().flat_map(|fk| &fk.pairs) {
            if let Some(remote) = find_col(tables, &pair.ref_schema, &pair.ref_table, &pair.ref_name)
                && remote.attnum != pair.ref_attnum
            {
                return Err(Fail::new("schema"));
            }
        }
    }
    Ok(())
}

fn parse_profile(map: &Map<String, Value>, tables: &[Table]) -> Result<BTreeMap<String, ProfileTable>, Fail> {
    let mut out = BTreeMap::new();
    for item in array(map.get("tables").ok_or(Fail::new("schema"))?)? {
        let row = object(item)?;
        let schema = ident_field(row, "schema")?;
        let name = ident_field(row, "name")?;
        let dataset = format!("{schema}.{name}");
        if !tables.iter().any(|table| table.schema == schema && table.name == name) {
            return Err(Fail::new("identity.stale"));
        }
        if out.contains_key(&dataset) {
            return Err(Fail::new("schema"));
        }
        let reltuples = stat_str(row, "reltuples")?;
        let mut cols = BTreeMap::new();
        for col in array(row.get("columns").ok_or(Fail::new("schema"))?)? {
            let col_map = object(col)?;
            optional_withheld(col_map, "most_common_vals")?;
            optional_withheld(col_map, "histogram_bounds")?;
            allow_text(col_map, "correlation")?;
            allow_text(col_map, "null_frac")?;
            let col_name = ident_field(col_map, "name")?;
            let attnum = json_i32(col_map.get("attnum").ok_or(Fail::new("schema"))?)?;
            let n_distinct = stat_str(col_map, "n_distinct")?;
            if cols.insert(col_name, (attnum, n_distinct)).is_some() {
                return Err(Fail::new("schema"));
            }
        }
        out.insert(dataset, ProfileTable { reltuples, cols });
    }
    Ok(out)
}

struct ProfileTable {
    reltuples: Option<String>,
    cols: BTreeMap<String, (i32, Option<String>)>,
}

fn apply_cards(tables: &mut [Table], cards: &Map<String, Value>, profiles: &BTreeMap<String, ProfileTable>) -> Result<(), Fail> {
    let items = array(cards.get("tables").ok_or(Fail::new("schema"))?)?;
    if items.len() != tables.len() {
        return Err(Fail::new("identity.stale"));
    }
    let mut seen = BTreeMap::new();
    for item in items {
        let card = object(item)?;
        card_hash_ok(item)?;
        let schema = ident_field(card, "schema")?;
        let name = ident_field(card, "name")?;
        if ident_field(card, "schema_ident")? != schema || ident_field(card, "name_ident")? != name {
            return Err(Fail::new("identity.stale"));
        }
        if req_str(card, "dataset")? != format!("{schema}.{name}") {
            return Err(Fail::new("identity.stale"));
        }
        let dataset = format!("{schema}.{name}");
        let Some(table) = tables.iter_mut().find(|table| table.schema == schema && table.name == name) else {
            return Err(Fail::new("identity.stale"));
        };
        if seen.insert(dataset.clone(), ()).is_some() {
            return Err(Fail::new("schema"));
        }
        let card_cols = card_column_set(card)?;
        let catalog_cols = catalog_column_set(table)?;
        if card_cols != catalog_cols {
            return Err(Fail::new("identity.stale"));
        }
        let card_fk = card.get("foreign_keys").ok_or(Fail::new("schema"))?;
        if canonical_json_value(card_fk) != canonical_json_value(&table.foreign_keys) {
            return Err(Fail::new("identity.stale"));
        }
        let card_rows = stat_str(card, "reltuples")?;
        if let Some(profile) = profiles.get(&dataset) {
            if profile.cols.len() != table.cols.len() {
                return Err(Fail::new("identity.stale"));
            }
            for col in &table.cols {
                let Some((attnum, _)) = profile.cols.get(&col.name) else {
                    return Err(Fail::new("identity.stale"));
                };
                if *attnum != col.attnum {
                    return Err(Fail::new("identity.stale"));
                }
            }
            if let (Some(left), Some(right)) = (&profile.reltuples, &card_rows)
                && left != right
            {
                return Err(Fail::new("identity.stale"));
            }
            table.profile_present = true;
            table.reltuples.clone_from(&profile.reltuples);
            for col in &mut table.cols {
                col.n_distinct.clone_from(&profile.cols[&col.name].1);
            }
        }
    }
    if seen.len() != tables.len() {
        return Err(Fail::new("identity.stale"));
    }
    Ok(())
}

fn card_hash_ok(card: &Value) -> Result<(), Fail> {
    let map = object(card)?;
    let claimed = hex_field(map, "card_sha256")?;
    let mut copy = map.clone();
    copy.remove("card_sha256");
    let actual = canonical_sha256(&Value::Object(copy)).ok_or(Fail::new("json"))?;
    if actual != claimed {
        return Err(Fail::new("identity.stale"));
    }
    Ok(())
}

fn card_column_set(card: &Map<String, Value>) -> Result<BTreeMap<String, (i32, String, bool)>, Fail> {
    let mut out = BTreeMap::new();
    for item in array(card.get("columns").ok_or(Fail::new("schema"))?)? {
        let map = object(item)?;
        withheld(map, "most_common_vals")?;
        withheld(map, "histogram_bounds")?;
        allow_text(map, "correlation")?;
        allow_text(map, "null_frac")?;
        stat_str(map, "n_distinct")?;
        let name = ident_field(map, "name")?;
        let row = (
            json_i32(map.get("attnum").ok_or(Fail::new("schema"))?)?,
            ident_field(map, "type_name")?,
            json_bool(map.get("not_null").ok_or(Fail::new("schema"))?)?,
        );
        if out.insert(name, row).is_some() {
            return Err(Fail::new("schema"));
        }
    }
    Ok(out)
}

fn catalog_column_set(table: &Table) -> Result<BTreeMap<String, (i32, String, bool)>, Fail> {
    let mut out = BTreeMap::new();
    for col in &table.cols {
        out.insert(col.name.clone(), (col.attnum, col.type_name.clone(), col.not_null));
    }
    Ok(out)
}

fn canonical_json_value(value: &Value) -> String {
    crate::canonical::canonical_json(value)
}

fn naming_targets(tables: &[Table], table: &Table, col: &Col) -> Vec<(String, String, String)> {
    if table.fks.iter().any(|fk| fk.pairs.iter().any(|pair| pair.name == col.name)) || is_sole_pk(table, &col.name) {
        return Vec::new();
    }
    let mut found = Vec::new();
    for other in tables {
        if other.schema == table.schema && other.name == table.name || other.pk.len() != 1 {
            continue;
        }
        let pk_name = &other.pk[0];
        let Some(pk) = other.cols.iter().find(|candidate| candidate.name == *pk_name) else {
            continue;
        };
        if pk.type_name != col.type_name {
            continue;
        }
        let suffix = pk_name == "id" && col.name.strip_prefix(&other.name).is_some_and(|rest| rest == "_id");
        if col.name == pk.name || suffix {
            found.push((other.schema.clone(), other.name.clone(), pk.name.clone()));
        }
    }
    found.sort();
    found
}

fn column_fk_compatible(tables: &[Table], table: &Table, column: &str) -> Option<bool> {
    let mut saw = false;
    let mut compatible = true;
    for fk in &table.fks {
        if fk.pairs.iter().any(|pair| pair.name == column) {
            saw = true;
            if !fk_compatible(tables, table, fk) {
                compatible = false;
            }
        }
    }
    saw.then_some(compatible)
}

fn fk_compatible(tables: &[Table], table: &Table, fk: &Fk) -> bool {
    let Some(head) = fk.pairs.first() else {
        return false;
    };
    if fk.pairs.iter().any(|pair| pair.ref_schema != head.ref_schema || pair.ref_table != head.ref_table) {
        return false;
    }
    for pair in &fk.pairs {
        let Some(local) = table.cols.iter().find(|col| col.name == pair.name) else {
            return false;
        };
        let Some(remote) = find_col(tables, &pair.ref_schema, &pair.ref_table, &pair.ref_name) else {
            return false;
        };
        if remote.attnum != pair.ref_attnum || local.type_name != remote.type_name {
            return false;
        }
    }
    true
}

fn end_unique(tables: &[Table], schema: &str, name: &str, columns: &[String]) -> Option<bool> {
    let table = find_table(tables, schema, name)?;
    if key_covers(&table.pk, columns) || table.unique.iter().any(|key| key_covers(key, columns)) {
        return Some(true);
    }
    if columns.len() == 1 {
        let col = table.cols.iter().find(|col| col.name == columns[0])?;
        return stat_unique(col.n_distinct.as_deref());
    }
    None
}

fn cardinality_of(source: Option<bool>, target: Option<bool>) -> (&'static str, &'static str) {
    match (source, target) {
        (Some(true), Some(true)) => ("one_to_one", "0"),
        (Some(false), Some(true)) => ("many_to_one", "0"),
        (Some(true), Some(false)) => ("one_to_many", "0"),
        _ => ("unknown", "1"),
    }
}

fn key_covers(key: &[String], columns: &[String]) -> bool {
    if key.is_empty() || key.len() != columns.len() {
        return false;
    }
    let mut left = key.to_vec();
    let mut right = columns.to_vec();
    left.sort();
    right.sort();
    left == right
}

fn is_sole_pk(table: &Table, column: &str) -> bool {
    table.pk.len() == 1 && table.pk[0] == column
}

fn is_sole_pk_of(tables: &[Table], schema: &str, name: &str, column: &str) -> bool {
    find_table(tables, schema, name).is_some_and(|table| is_sole_pk(table, column))
}

fn find_table<'a>(tables: &'a [Table], schema: &str, name: &str) -> Option<&'a Table> {
    tables.iter().find(|table| table.schema == schema && table.name == name)
}

fn find_col<'a>(tables: &'a [Table], schema: &str, name: &str, column: &str) -> Option<&'a Col> {
    find_table(tables, schema, name)?.cols.iter().find(|col| col.name == column)
}

fn end_label(schema: &str, table: &str, columns: &[String]) -> String {
    format!("{schema}.{table}.{}", columns.join("+"))
}

fn name_match(pairs: &[Pair]) -> &'static str {
    if pairs.iter().all(|pair| pair.name == pair.ref_name) {
        "exact"
    } else if pairs.iter().all(|pair| pair.ref_name == "id" && pair.name.strip_prefix(&pair.ref_table).is_some_and(|rest| rest == "_id")) {
        "suffix"
    } else {
        "none"
    }
}

fn has_name(name: &str, stems: &[&str]) -> bool {
    stems.iter().any(|stem| name == *stem || name.strip_suffix(stem).is_some_and(|prefix| prefix.ends_with('_')))
}

fn is_money_name(name: &str) -> bool {
    name == "je" || name == "amount" || suffixed(name, "je") || suffixed(name, "amount")
}

fn is_quantity_name(name: &str) -> bool {
    name == "sl" || name == "qty" || suffixed(name, "sl") || suffixed(name, "qty")
}

fn is_ratio_name(name: &str) -> bool {
    name == "ratio" || suffixed(name, "ratio")
}

fn is_status_name(name: &str) -> bool {
    name == "zt" || name == "status" || suffixed(name, "zt") || suffixed(name, "status")
}

fn is_code_name(name: &str) -> bool {
    name == "code" || suffixed(name, "code")
}

fn suffixed(name: &str, stem: &str) -> bool {
    name.strip_suffix(stem).is_some_and(|prefix| prefix.ends_with('_'))
}

fn is_dict_name(name: &str) -> bool {
    name == "sys_dict" || name == "t_code" || name.ends_with("_dict")
}

fn is_int(type_name: &str) -> bool {
    matches!(type_name, "int2" | "int4" | "int8")
}

fn is_text_or_int(type_name: &str) -> bool {
    type_name == "text" || is_int(type_name)
}

fn proven_low(n_distinct: Option<&str>, reltuples: Option<&str>) -> bool {
    let Some(count) = n_distinct.and_then(parse_scaled) else {
        return false;
    };
    if count > 0 {
        return (SCALE..=32 * SCALE).contains(&count);
    }
    if count == 0 || count == -SCALE {
        return false;
    }
    let Some(rows) = reltuples.and_then(parse_scaled) else {
        return false;
    };
    if rows < 0 {
        return false;
    }
    let Some(estimate) = count.abs().checked_mul(rows).and_then(|product| product.checked_div(SCALE)) else {
        return false;
    };
    (SCALE..=32 * SCALE).contains(&estimate)
}

fn stat_unique(n_distinct: Option<&str>) -> Option<bool> {
    let count = parse_scaled(n_distinct?)?;
    Some(count == SCALE || count == -SCALE)
}

fn dict_rows(reltuples: Option<&str>) -> bool {
    let Some(rows) = reltuples.and_then(parse_scaled) else {
        return false;
    };
    (0..=5000 * SCALE).contains(&rows)
}

fn parse_scaled(text: &str) -> Option<i128> {
    if text.is_empty() || text.bytes().any(|byte| matches!(byte, b'e' | b'E' | b'+')) {
        return None;
    }
    let (neg, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if rest.is_empty() {
        return None;
    }
    let (whole, frac) = match rest.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (rest, ""),
    };
    if whole.is_empty() || frac.len() > 8 {
        return None;
    }
    if !whole.bytes().all(|byte| byte.is_ascii_digit()) || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if whole.len() > 1 && whole.starts_with('0') || whole.len() > 18 {
        return None;
    }
    let whole_n: i128 = whole.parse().ok()?;
    let mut value = whole_n.checked_mul(SCALE)?;
    if !frac.is_empty() {
        let frac_n: i128 = frac.parse().ok()?;
        let mut place = SCALE;
        for _ in 0..frac.len() {
            place /= 10;
        }
        value = value.checked_add(frac_n.checked_mul(place)?)?;
    }
    if neg {
        value = value.checked_neg()?;
    }
    Some(value)
}

fn max_unc(left: &'static str, right: &'static str) -> &'static str {
    if unc_rank(left) >= unc_rank(right) { left } else { right }
}

fn unc_rank(value: &str) -> u8 {
    match value {
        "0" => 0,
        "0.5" => 1,
        _ => 2,
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, Fail> {
    value.as_object().ok_or(Fail::new("schema"))
}

fn array(value: &Value) -> Result<&Vec<Value>, Fail> {
    value.as_array().ok_or(Fail::new("schema"))
}

fn req_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, Fail> {
    map.get(key).and_then(Value::as_str).ok_or(Fail::new("schema"))
}

fn ident_field(map: &Map<String, Value>, key: &str) -> Result<String, Fail> {
    let text = req_str(map, key)?;
    if !is_ident(text) {
        return Err(Fail::new("schema"));
    }
    Ok(text.to_owned())
}

fn hex_field(map: &Map<String, Value>, key: &str) -> Result<String, Fail> {
    let text = req_str(map, key)?;
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return Err(Fail::new("schema"));
    }
    Ok(text.to_owned())
}

fn is_ident(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    text.len() <= 63 && (first.is_ascii_alphabetic() || first == '_') && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn json_i32(value: &Value) -> Result<i32, Fail> {
    let number = value.as_i64().ok_or(Fail::new("schema"))?;
    i32::try_from(number).map_err(|_| Fail::new("schema"))
}

fn json_bool(value: &Value) -> Result<bool, Fail> {
    value.as_bool().ok_or(Fail::new("schema"))
}

fn stat_str(map: &Map<String, Value>, key: &str) -> Result<Option<String>, Fail> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(Fail::new("schema")),
    }
}

fn allow_text(map: &Map<String, Value>, key: &str) -> Result<(), Fail> {
    match map.get(key) {
        None | Some(Value::Null) | Some(Value::String(_)) => Ok(()),
        Some(_) => Err(Fail::new("schema")),
    }
}

fn withheld(map: &Map<String, Value>, key: &str) -> Result<(), Fail> {
    if map.get(key).and_then(Value::as_str) == Some("withheld") {
        Ok(())
    } else {
        Err(Fail::new("profile.values"))
    }
}

fn optional_withheld(map: &Map<String, Value>, key: &str) -> Result<(), Fail> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(text)) if text == "withheld" => Ok(()),
        Some(_) => Err(Fail::new("profile.values")),
    }
}

fn sample_none(map: &Map<String, Value>) -> Result<(), Fail> {
    match map.get("sample_policy").and_then(Value::as_str) {
        Some("none") => Ok(()),
        Some(_) => Err(Fail::new("profile.samples")),
        None => Err(Fail::new("schema")),
    }
}

fn require_protocol(map: &Map<String, Value>, protocol: &str) -> Result<(), Fail> {
    if map.get("protocol").and_then(Value::as_str) == Some(protocol) {
        Ok(())
    } else {
        Err(Fail::new("schema"))
    }
}

fn key_names(value: &Value, cols: &[Col]) -> Result<Vec<String>, Fail> {
    let mut names = Vec::new();
    for item in array(value)? {
        let map = object(item)?;
        let name = ident_field(map, "name")?;
        let attnum = json_i32(map.get("attnum").ok_or(Fail::new("schema"))?)?;
        if !cols.iter().any(|col| col.name == name && col.attnum == attnum) || names.iter().any(|seen| seen == &name) {
            return Err(Fail::new("schema"));
        }
        names.push(name);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::{decide_state, proven_low, stat_unique};

    #[test]
    fn low_cardinality_and_state_are_hand_values() {
        assert!(proven_low(Some("4"), Some("10")));
        assert!(proven_low(Some("32"), Some("1")));
        assert!(!proven_low(Some("33"), Some("1")));
        assert!(!proven_low(Some("50"), Some("10")));
        assert!(!proven_low(Some("-1"), Some("10")));
        assert!(proven_low(Some("-0.2"), Some("100")));
        assert!(!proven_low(Some("-0.2"), Some("1000")));
        assert!(!proven_low(Some("-0.941"), Some("2000")));
        assert!(!proven_low(Some("-0.25"), Some("8000")));
        assert!(!proven_low(Some("-0.111"), Some("90000")));
        assert!(!proven_low(Some("-0.9206333"), Some("90000")));
        assert!(!proven_low(None, Some("10")));
        assert!(!proven_low(Some("-0.5"), None));
        assert_eq!(stat_unique(Some("-1")), Some(true));
        assert_eq!(stat_unique(Some("1")), Some(true));
        assert_eq!(stat_unique(Some("1.0")), Some(true));
        assert_eq!(stat_unique(Some("2")), Some(false));
        assert_eq!(stat_unique(Some("-0.25")), Some(false));
        assert_eq!(stat_unique(None), None);
        assert_eq!(decide_state("rule_high_confidence", "personal", "1", "0", "phone"), "pending");
        assert_eq!(decide_state("rule_high_confidence", "sensitive", "1", "0", "unknown"), "pending");
        assert_eq!(decide_state("none", "none", "1", "0", "boolean_flag"), "pending");
        assert_eq!(decide_state("rule_high_confidence", "none", "1", "0", "boolean_flag"), "auto_accepted");
        assert_eq!(decide_state("rule_high_confidence", "none", "0.5", "0.5", "foreign_key"), "pending");
    }
}
