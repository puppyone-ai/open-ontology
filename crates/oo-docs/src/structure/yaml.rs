//! The ontology YAML of both sides (D93): the structure layer of the documents and the structure of the database in one
//! file, with the cross-links between them. Pure; the database side comes in as plain data ([`DbSide`]).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::{ClassStructure, Property, Status, ValueType};
use crate::resolve::normalize::norm;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbForeignKey {
    pub column: String,
    pub ref_table: String,
    pub ref_column: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbTable {
    /// `schema.table`.
    pub name: String,
    pub rows: Option<i64>,
    pub columns: Vec<DbColumn>,
    pub foreign_keys: Vec<DbForeignKey>,
}

/// A link rule (`oo source link add`): which column of a table names an entity of a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbLinkRule {
    pub table: String,
    pub key_col: String,
    pub name_col: String,
    pub entity_type: String,
}

/// The database side: its tables, its link rules and the names the rules match (normalized).
#[derive(Clone, Debug, Default)]
pub struct DbSide {
    pub tables: Vec<DbTable>,
    pub rules: Vec<DbLinkRule>,
    /// `schema.table` to the normalized names of its name and alias columns.
    pub names: BTreeMap<String, BTreeSet<String>>,
    /// Links the projection already made per table (`_oo_docproj.entity_link`, rejected ones left out).
    pub projected: BTreeMap<String, i64>,
}

/// One cross-link: a document property whose values name rows of a database table.
#[derive(Clone, Debug, PartialEq)]
pub struct CrossLink {
    pub class_key: String,
    pub property: String,
    pub table: String,
    pub column: String,
    pub matched: usize,
    pub of: usize,
    pub entity_type: String,
    /// Document entities the projection linked to rows of the table (the existing source link).
    pub projected: i64,
}

fn plain(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        && !matches!(
            s,
            "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
        )
        && s.parse::<f64>().is_err()
}

/// A YAML scalar: plain when it is safe, else a double-quoted JSON string (valid YAML).
pub fn scalar(s: &str) -> String {
    if plain(s) {
        s.to_owned()
    } else {
        serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
    }
}

fn flow_list(items: &[String]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|s| scalar(s))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Document properties that name rows of a database table through a link rule (the existing source link).
pub fn cross_links(classes: &[ClassStructure], db: &DbSide) -> Vec<CrossLink> {
    let mut out = Vec::new();
    for c in classes.iter().filter(|c| c.status != Status::Rejected) {
        for p in c.live_properties() {
            let Some(ty) = p.value_type.entity_type() else {
                continue;
            };
            for rule in db
                .rules
                .iter()
                .filter(|r| r.entity_type == ty || r.entity_type.is_empty())
            {
                let Some(names) = db.names.get(&rule.table) else {
                    continue;
                };
                let of = p.sample_values.len();
                if of == 0 {
                    continue;
                }
                let matched = p
                    .sample_values
                    .iter()
                    .filter(|v| names.contains(&norm(v)))
                    .count();
                if matched > 0 {
                    out.push(CrossLink {
                        class_key: c.class_key.clone(),
                        property: p.slug.clone(),
                        table: rule.table.clone(),
                        column: rule.name_col.clone(),
                        matched,
                        of,
                        entity_type: ty.to_owned(),
                        projected: db.projected.get(&rule.table).copied().unwrap_or(0),
                    });
                }
            }
        }
    }
    out
}

fn db_type_label(dt: &str) -> &'static str {
    let d = dt.to_lowercase();
    if d.contains("timestamp") || d == "date" {
        "date"
    } else if d.contains("numeric") || d.contains("money") || d.contains("decimal") {
        "amount"
    } else if d.contains("int") || d.contains("double") || d.contains("real") || d.contains("float")
    {
        "number"
    } else if d.contains("bool") {
        "boolean"
    } else if d.contains("uuid") {
        "id"
    } else {
        "text"
    }
}

fn property_yaml(out: &mut String, p: &Property, links: &[&CrossLink]) {
    let _ = writeln!(out, "      - name: {}", scalar(&p.slug));
    let _ = writeln!(out, "        label: {}", scalar(&p.name));
    let _ = writeln!(out, "        type: {}", p.value_type.as_str());
    let _ = writeln!(out, "        source: {}", p.source.as_str());
    let _ = writeln!(out, "        status: {}", p.status.as_str());
    let _ = writeln!(out, "        coverage: {:.2}", p.coverage);
    if p.multi {
        let _ = writeln!(out, "        multi: true");
    }
    if p.required {
        let _ = writeln!(out, "        required: true");
    }
    if p.value_type == ValueType::Enum && !p.enum_values.is_empty() {
        let _ = writeln!(out, "        values: {}", flow_list(&p.enum_values));
    }
    if !p.examples.is_empty() {
        let _ = writeln!(out, "        examples: {}", flow_list(&p.examples));
    }
    if let Some(r) = &p.relation {
        let _ = writeln!(
            out,
            "        relation: {{target: {}, kind: {}, resolved: {:.2}, predicate: {}}}",
            scalar(&r.target),
            r.target_kind,
            r.resolved,
            scalar(&r.predicate)
        );
    }
    for l in links {
        let _ = writeln!(
            out,
            "        links_to: {{database: {}, column: {}, matched: {}, of: {}, via: source_link}}",
            scalar(&l.table),
            scalar(&l.column),
            l.matched,
            l.of
        );
    }
}

/// Render the ontology YAML. `db` is the database side when there is one.
pub fn render(version: u32, classes: &[ClassStructure], db: Option<&DbSide>) -> String {
    let links = db.map(|d| cross_links(classes, d)).unwrap_or_default();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Ontology: structure layer of documents and database, one file ({})",
        super::PROTOCOL
    );
    let _ = writeln!(out, "protocol: {}", super::PROTOCOL);
    let _ = writeln!(out, "structure_version: {version}");
    let _ = writeln!(
        out,
        "sources: [{}]",
        if db.is_some() {
            "documents, database"
        } else {
            "documents"
        }
    );
    let live: Vec<&ClassStructure> = classes
        .iter()
        .filter(|c| c.status != Status::Rejected)
        .collect();
    let _ = writeln!(out, "types:");
    for c in &live {
        let _ = writeln!(out, "  - name: {}", scalar(&c.class_key));
        let _ = writeln!(out, "    label: {}", scalar(&c.label));
        let _ = writeln!(out, "    layer: structure");
        let _ = writeln!(out, "    origin: documents");
        let _ = writeln!(out, "    kind: {}", scalar(&c.kind));
        let _ = writeln!(out, "    shape: {}", c.shape.as_str());
        let _ = writeln!(out, "    status: {}", c.status.as_str());
        let _ = writeln!(out, "    files: {}", c.files);
        if c.rows > c.files {
            let _ = writeln!(out, "    rows: {}", c.rows);
        }
        let _ = writeln!(out, "    properties:");
        for p in c.live_properties() {
            let pl: Vec<&CrossLink> = links
                .iter()
                .filter(|l| l.class_key == c.class_key && l.property == p.slug)
                .collect();
            property_yaml(&mut out, p, &pl);
        }
    }
    if let Some(db) = db {
        for t in &db.tables {
            let _ = writeln!(out, "  - name: {}", scalar(&t.name));
            let _ = writeln!(out, "    layer: structure");
            let _ = writeln!(out, "    origin: database");
            if let Some(r) = t.rows {
                let _ = writeln!(out, "    rows: {r}");
            }
            let pk: Vec<String> = t
                .columns
                .iter()
                .filter(|c| c.primary_key)
                .map(|c| c.name.clone())
                .collect();
            if !pk.is_empty() {
                let _ = writeln!(out, "    primary_key: {}", flow_list(&pk));
            }
            let _ = writeln!(out, "    properties:");
            for c in &t.columns {
                let _ = writeln!(out, "      - name: {}", scalar(&c.name));
                let _ = writeln!(out, "        type: {}", db_type_label(&c.data_type));
                let _ = writeln!(out, "        source: column");
                if !c.nullable {
                    let _ = writeln!(out, "        required: true");
                }
                if let Some(fk) = t.foreign_keys.iter().find(|f| f.column == c.name) {
                    let _ = writeln!(
                        out,
                        "        relation: {{target: {}, kind: foreign_key, column: {}}}",
                        scalar(&fk.ref_table),
                        scalar(&fk.ref_column)
                    );
                }
            }
        }
    }
    let _ = writeln!(out, "relations:");
    let mut any = false;
    for c in &live {
        for p in c.live_properties() {
            if let Some(r) = &p.relation {
                any = true;
                let _ = writeln!(
                    out,
                    "  - from: {}",
                    scalar(&format!("{}.{}", c.class_key, p.slug))
                );
                let _ = writeln!(out, "    to: {}", scalar(&r.target));
                let _ = writeln!(out, "    kind: {}", r.target_kind);
            }
        }
    }
    if let Some(db) = db {
        for t in &db.tables {
            for f in &t.foreign_keys {
                any = true;
                let _ = writeln!(
                    out,
                    "  - from: {}",
                    scalar(&format!("{}.{}", t.name, f.column))
                );
                let _ = writeln!(
                    out,
                    "    to: {}",
                    scalar(&format!("{}.{}", f.ref_table, f.ref_column))
                );
                let _ = writeln!(out, "    kind: foreign_key");
            }
        }
    }
    for l in &links {
        any = true;
        let _ = writeln!(
            out,
            "  - from: {}",
            scalar(&format!("{}.{}", l.class_key, l.property))
        );
        let _ = writeln!(
            out,
            "    to: {}",
            scalar(&format!("{}.{}", l.table, l.column))
        );
        let _ = writeln!(out, "    kind: source_link");
        let _ = writeln!(out, "    matched: {}", l.matched);
        let _ = writeln!(out, "    of: {}", l.of);
        let _ = writeln!(out, "    projected_links: {}", l.projected);
    }
    if !any {
        out.truncate(out.len() - "relations:\n".len());
        let _ = writeln!(out, "relations: []");
    }
    out
}
