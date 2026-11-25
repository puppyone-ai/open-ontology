//! Slot filling of a confirmed table class (D93: columns are properties, values decide type and relations, rows are
//! instances). Deterministic, no model, streaming. Pure.
//!
//! The structure of a table class is confirmed by a person; here every data row of a table of that class is read against
//! it. Each row becomes one instance (a Person, a Project, an Organization, or `Thing{label = class}`), named by the
//! row's identity column; each column mapped to a confirmed property becomes an `attr.<slug>` fact whose value has the
//! type of the property (a date as ISO, an amount with its currency, a ratio as a fraction, an enum, a version, text),
//! read with the normaliser so a value written in words is still that value; each relation column resolves to entities
//! (by name, alias or identity attribute, at publish) of the other classes. The provenance of every fact is the row's
//! own block, whose anchor is the cell range (`Sheet1!A5:D5`).
//!
//! A row that qualifies does so through its structural position (D96): it sits in a table of a confirmed class. A file
//! is read up to [`MAX_ROWS_PER_JOB`] rows per extraction; the rest is counted (`truncated`) and reported, not read.

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::{Block, BlockKind, DocIr};
use serde_json::{Value, json};

use super::observe::canon_key;
use super::template::{add_entity, sort_extraction};
use super::vtype::{items_of, name_like};
use super::{ClassStructure, Property, Shape, Status, ValueType};
use crate::extract::anchors::doc_ctx;
use crate::extract::model::{
    EntityRef, EntityType, EvidenceRef, Extraction, FactMention, IdentityMention, Predicate,
};
use crate::extract::rules::dates::parse_exact;
use crate::values::{DateCtx, money, ratio};

/// Confidence of a fact read from a cell of a confirmed class.
pub const ROW_BP: u16 = 9300;
/// Rule id of the facts made from rows.
pub const RULE_ROW: &str = "structure.row";
/// Data rows read per extraction (per file).
pub const MAX_ROWS_PER_JOB: u32 = 5_000;
/// Items of one multi-valued cell that are read.
const MAX_ITEMS: usize = 12;
const MAX_TEXT: usize = 200;
const MAX_NAME_CHARS: usize = 120;

/// What filling did to one file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filled {
    /// Data rows of the tables of the file.
    pub rows_total: u32,
    /// Rows that became an instance.
    pub rows_filled: u32,
    /// Rows without a value in the identity column.
    pub rows_unnamed: u32,
    /// Cells written as a typed value.
    pub cells: u32,
    /// Relations written (a cell that names an entity of another class).
    pub relations: u32,
    /// Cells that did not read as the type of their property (kept out, not written as another type).
    pub rejected: u32,
    /// The file has more rows than [`MAX_ROWS_PER_JOB`].
    pub truncated: bool,
}

/// One column of a table mapped to a confirmed property.
struct Column<'a> {
    header: String,
    prop: &'a Property,
}

fn key_matches(p: &Property, key: &str, aligned: Option<&String>) -> bool {
    let hit = |k: &str| p.keys.iter().any(|x| x == k);
    hit(key)
        || aligned.is_some_and(|a| hit(a))
        || p.aliases.iter().any(|a| canon_key(a) == key)
        || canon_key(&p.name) == key
}

/// Header cells of a table block: `(column, text)`.
fn headers(table: &Block) -> Vec<(u32, String)> {
    let Some(g) = &table.table else {
        return Vec::new();
    };
    let row = g.header_rows.saturating_sub(1);
    let mut out: Vec<(u32, String)> = g
        .cells
        .iter()
        .filter(|c| c.r == row && !c.text.trim().is_empty())
        .map(|c| (c.c, c.text.trim().to_owned()))
        .collect();
    out.sort();
    out
}

/// Split the `header: value` lines of a row block by the known headers (the longest header that fits a line wins, a
/// line that fits none continues the cell before).
fn row_cells(text: &str, headers: &[String]) -> BTreeMap<usize, String> {
    let mut out: BTreeMap<usize, String> = BTreeMap::new();
    let mut last: Option<usize> = None;
    for line in text.split('\n') {
        let mut hit: Option<(usize, usize, &str)> = None;
        for (i, h) in headers.iter().enumerate() {
            if let Some(rest) = line.strip_prefix(h.as_str())
                && let Some(value) = rest.strip_prefix(": ")
                && hit.is_none_or(|(len, _, _)| h.len() > len)
            {
                hit = Some((h.len(), i, value));
            }
        }
        match hit {
            Some((_, i, v)) => {
                out.entry(i).or_insert_with(|| v.to_owned());
                last = Some(i);
            }
            None => {
                if let Some(i) = last
                    && let Some(v) = out.get_mut(&i)
                {
                    v.push('\n');
                    v.push_str(line);
                }
            }
        }
    }
    out.values_mut().for_each(|v| *v = v.trim().to_owned());
    out.retain(|_, v| !v.is_empty());
    out
}

/// Keys that name the row itself, in order of preference.
const IDENTITY_KEYS: &[&str] = &[
    "name",
    "project",
    "vendor",
    "supplier",
    "customer",
    "title",
    "item",
    "line",
    "category",
    "po",
    "id",
    "code",
    "receipt",
    "employee_id",
];

/// A column can name the instance when its values are names or codes of the rows themselves: not a list, and not a
/// relation to the records of another class (an `employee` column of an expense table points at the people table).
fn identity_candidate(p: &Property, class_key: &str) -> bool {
    p.relation
        .as_ref()
        .is_none_or(|r| r.target_kind != "class" || r.target == class_key)
        && matches!(
            p.value_type,
            ValueType::Person
                | ValueType::Project
                | ValueType::Organization
                | ValueType::Id
                | ValueType::Text
                | ValueType::Enum
        )
        && !p.multi
}

/// Share of the rows whose identity cell is distinct for a column to name the instance (a surrogate name is made otherwise).
const UNIQUE_SHARE: f64 = 0.9;

/// The column that names the instance: a key that says so, else the leftmost column that can name a thing, among the
/// columns whose values tell the rows apart (`distinct[i]` is the share of distinct non-empty cells of column `i`).
fn identity_column(
    cols: &[Option<Column<'_>>],
    distinct: &[f64],
    class_key: &str,
) -> Option<usize> {
    let unique = |i: usize| distinct.get(i).is_some_and(|d| *d >= UNIQUE_SHARE);
    for k in IDENTITY_KEYS {
        if let Some(i) = cols.iter().enumerate().position(|(i, c)| {
            unique(i)
                && c.as_ref().is_some_and(|c| {
                    identity_candidate(c.prop, class_key) && c.prop.keys.iter().any(|x| x == k)
                })
        }) {
            return Some(i);
        }
    }
    cols.iter().enumerate().position(|(i, c)| {
        unique(i)
            && c.as_ref()
                .is_some_and(|c| identity_candidate(c.prop, class_key))
    })
}

/// The label of the `Thing` an instance of this class is (`[A-Za-z0-9 CJK]`, at most 20 characters).
pub fn thing_label(class: &ClassStructure) -> String {
    let l: String = class
        .label
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect::<String>()
        .trim()
        .chars()
        .take(20)
        .collect();
    if l.is_empty() { "Record".into() } else { l }
}

/// The entity type of an instance of `class` whose identity column is `ident`.
fn subject_type(class: &ClassStructure, ident: &Property, header: &str) -> EntityType {
    use crate::extract::rules::tables::{Role, role_of};
    match (class.kind.as_str(), ident.value_type, role_of(header)) {
        ("people_table", ValueType::Person, _) | (_, ValueType::Person, _) => EntityType::Person,
        ("project_table", _, _) | (_, ValueType::Project, _) => EntityType::Project,
        (_, ValueType::Organization, _) | (_, _, Some(Role::Org)) => EntityType::Organization,
        _ => EntityType::Thing,
    }
}

/// The relation a cell writes: a person who belongs to an organization or a project is a member of it, every other
/// row is about what its cell names (the row is the subject, so the instance of a row is always the subject of its facts).
fn relation_between(subject: EntityType, object: EntityType) -> Predicate {
    match (subject, object) {
        (EntityType::Person, EntityType::Organization | EntityType::Project) => Predicate::MemberOf,
        _ => Predicate::About,
    }
}

fn clip(s: &str, n: usize) -> String {
    s.trim().chars().take(n).collect()
}

/// One cell read as the type of its property: the value key and text of the fact, and whether the text changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coerced {
    pub key: &'static str,
    pub value: String,
}

fn percent_header(header: &str) -> bool {
    let h = header.to_lowercase();
    h.contains('%')
        || h.contains('％')
        || h.contains("率")
        || h.contains("percent")
        || h.contains("占比")
}

/// Read one cell as `ty`. `None`: it is not a value of that type.
pub fn coerce(ty: ValueType, header: &str, raw: &str, ctx: &DateCtx) -> Option<Coerced> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match ty {
        ValueType::Date => {
            let first = raw.split_whitespace().next().unwrap_or("");
            if let Some(d) = parse_exact(first).filter(|d| d.precision == "day") {
                return Some(Coerced {
                    key: "date",
                    value: d.iso,
                });
            }
            crate::values::date::parse(raw, ctx).map(|d| Coerced {
                key: "date",
                value: d.iso(),
            })
        }
        ValueType::Amount => {
            let m = money::parse(raw, true)?;
            let (currency, scale) = money::header_hint(header).unwrap_or((None, 1));
            let amount = if m.scale.is_none() && m.currency.is_none() {
                m.amount.times_u128(scale)?
            } else {
                m.amount
            };
            let c = money::Money {
                amount,
                currency: m.currency.or(currency),
                scale: m.scale,
            };
            Some(Coerced {
                key: "amount",
                value: c.canonical(),
            })
        }
        ValueType::Number => {
            let (d, _) = money::number(raw, false)?;
            Some(Coerced {
                key: "number",
                value: d.to_plain(),
            })
        }
        ValueType::Percent => {
            if let Some(r) = ratio::parse(raw) {
                return Some(Coerced {
                    key: "ratio",
                    value: r.canonical(),
                });
            }
            let (d, _) = money::number(raw, false)?;
            percent_header(header)
                .then(|| ratio::Ratio::from_percent(d))
                .flatten()
                .map(|r| Coerced {
                    key: "ratio",
                    value: r.canonical(),
                })
        }
        ValueType::Enum => Some(Coerced {
            key: "enum",
            value: clip(raw, MAX_TEXT),
        }),
        ValueType::Email
        | ValueType::Url
        | ValueType::Id
        | ValueType::Version
        | ValueType::Text
        | ValueType::LongText => Some(Coerced {
            key: "text",
            value: clip(raw, MAX_TEXT),
        }),
        ValueType::Person | ValueType::Project | ValueType::Organization => None,
    }
}

fn attr_value(c: &Coerced, raw: &str) -> Value {
    let mut v = json!({ c.key: c.value });
    // The text as written, when it is not the value (`六百八十五万`, `¥120`, `2027年3月16日`).
    let original = clip(raw, MAX_TEXT);
    if original != c.value && !matches!(c.key, "text" | "enum") {
        v["original"] = Value::String(original);
    }
    v
}

fn has_fact(x: &Extraction, subject: &EntityRef, pred: &Predicate, value: Option<&Value>) -> bool {
    x.facts.iter().any(|f| {
        &f.subject == subject
            && &f.predicate == pred
            && match (value, &f.value) {
                (Some(v), Some(w)) => same_value(v, w),
                (None, None) => true,
                _ => false,
            }
    })
}

/// Two attribute values are the same when their typed part is (the original spelling does not make another fact).
fn same_value(a: &Value, b: &Value) -> bool {
    let typed = |v: &Value| -> Option<(String, String)> {
        v.as_object().and_then(|o| {
            o.iter()
                .find(|(k, _)| k.as_str() != "original")
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned()))
        })
    };
    typed(a) == typed(b)
}

fn row_number(b: &Block) -> u32 {
    match &b.anchor {
        oo_core::docir::Anchor::Cells { range, .. } => range
            .trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or(0),
        _ => 0,
    }
}

/// Fill the confirmed table `class` from every table of the file.
pub fn apply(
    path: &str,
    ir: &DocIr,
    x: &mut Extraction,
    class: &ClassStructure,
    align: &BTreeMap<String, String>,
) -> Filled {
    let mut done = Filled::default();
    if class.status != Status::Confirmed || class.shape != Shape::Table {
        return done;
    }
    let ctx = doc_ctx(path, ir);
    let mut idx = 0;
    while idx < ir.blocks.len() {
        let table = &ir.blocks[idx];
        idx += 1;
        if !matches!(table.kind, BlockKind::SheetTable | BlockKind::Table) || table.table.is_none()
        {
            continue;
        }
        let hs = headers(table);
        if hs.is_empty() {
            continue;
        }
        let header_texts: Vec<String> = hs.iter().map(|h| h.1.clone()).collect();
        let cols: Vec<Option<Column<'_>>> = header_texts
            .iter()
            .map(|h| {
                let key = canon_key(h);
                let aligned = align.get(&key);
                class
                    .slots()
                    .find(|p| key_matches(p, &key, aligned))
                    .map(|prop| Column {
                        header: h.clone(),
                        prop,
                    })
            })
            .collect();
        if cols.iter().all(Option::is_none) {
            continue;
        }
        // The rows of this table (up to the bound), each with its cells by column.
        let mut rows: Vec<(&Block, BTreeMap<usize, String>)> = Vec::new();
        let mut j = idx;
        while j < ir.blocks.len() {
            let b = &ir.blocks[j];
            j += 1;
            if b.kind != BlockKind::Kv || b.parent.as_deref() != Some(table.id.as_str()) {
                if matches!(b.kind, BlockKind::SheetTable | BlockKind::Table) {
                    j -= 1;
                    break;
                }
                continue;
            }
            done.rows_total += 1;
            if rows.len() as u32 >= MAX_ROWS_PER_JOB {
                done.truncated = true;
                continue;
            }
            rows.push((b, row_cells(&b.text, &header_texts)));
        }
        let distinct: Vec<f64> = (0..cols.len())
            .map(|i| {
                let vals: Vec<&String> = rows.iter().filter_map(|r| r.1.get(&i)).collect();
                if vals.is_empty() {
                    return 0.0;
                }
                let set: BTreeSet<&&String> = vals.iter().collect();
                set.len() as f64 / rows.len().max(1) as f64
            })
            .collect();
        let ident = identity_column(&cols, &distinct, &class.class_key);
        let sty = match ident.and_then(|i| cols[i].as_ref()) {
            Some(c) => subject_type(class, c.prop, &c.header),
            None => EntityType::Thing,
        };
        let label = thing_label(class);
        for (b, cells) in &rows {
            let b: &Block = b;
            let name = match ident {
                Some(i) => cells
                    .get(&i)
                    .map(|n| {
                        clip(
                            &items_of(n).into_iter().next().unwrap_or_else(|| n.clone()),
                            MAX_NAME_CHARS,
                        )
                    })
                    .filter(|n| !n.is_empty() && n.chars().any(char::is_alphanumeric)),
                // No column tells the rows apart: the row is named by its position, like a surrogate key.
                None => Some(format!("{label} #{}", row_number(b))),
            };
            let Some(name) = name else {
                done.rows_unnamed += 1;
                continue;
            };
            if sty == EntityType::Person && !name_like(&name) {
                done.rows_unnamed += 1;
                continue;
            }
            let ev = EvidenceRef::from_block(b);
            let subject = if sty == EntityType::Thing {
                EntityRef {
                    ty: EntityType::Thing,
                    thing_label: Some(label.clone()),
                    subkind: None,
                    name,
                }
            } else {
                EntityRef::new(sty, name)
            };
            add_entity(x, &subject, &ev);
            done.rows_filled += 1;
            for (ci, col) in cols.iter().enumerate() {
                let (Some(col), Some(raw)) = (col, cells.get(&ci)) else {
                    continue;
                };
                if Some(ci) == ident {
                    continue;
                }
                let p = col.prop;
                if let Some(oty) = p.value_type.entity_type().and_then(EntityType::parse) {
                    for item in items_of(raw).into_iter().take(MAX_ITEMS) {
                        if oty == EntityType::Person && !name_like(&item) {
                            done.rejected += 1;
                            continue;
                        }
                        let obj = EntityRef::new(oty, clip(&item, MAX_NAME_CHARS));
                        if obj == subject {
                            continue;
                        }
                        add_entity(x, &obj, &ev);
                        let pred = relation_between(sty, oty);
                        if !x.facts.iter().any(|f| {
                            f.subject == subject
                                && f.predicate == pred
                                && f.object.as_ref() == Some(&obj)
                        }) {
                            x.facts.push(FactMention {
                                subject: subject.clone(),
                                predicate: pred,
                                object: Some(obj),
                                value: None,
                                valid_from: None,
                                valid_to: None,
                                valid_precision: None,
                                confidence_bp: ROW_BP,
                                rule_id: RULE_ROW,
                                evidence: vec![ev.clone()],
                            });
                            done.relations += 1;
                        }
                    }
                    continue;
                }
                let Some(c) = coerce(p.value_type, &col.header, raw, &ctx) else {
                    done.rejected += 1;
                    continue;
                };
                let Some(pred) = Predicate::parse(&format!("attr.{}", p.slug)) else {
                    continue;
                };
                let value = attr_value(&c, raw);
                // An identity attribute of the person resolves the same person across files.
                if sty == EntityType::Person {
                    let kind = match p.value_type {
                        ValueType::Email if crate::extract::rules::text::is_email(raw.trim()) => {
                            Some("email")
                        }
                        ValueType::Id if p.keys.iter().any(|k| k == "employee_id") => {
                            Some("employee_id")
                        }
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        let v = if kind == "email" {
                            raw.trim().to_lowercase()
                        } else {
                            raw.trim().to_owned()
                        };
                        if !x
                            .identities
                            .iter()
                            .any(|i| i.entity == subject && i.kind == kind && i.value == v)
                        {
                            x.identities.push(IdentityMention {
                                entity: subject.clone(),
                                kind,
                                value: v,
                                confidence_bp: ROW_BP,
                                rule_id: RULE_ROW,
                                evidence: vec![ev.clone()],
                            });
                        }
                    }
                }
                if !has_fact(x, &subject, &pred, Some(&value)) {
                    x.facts.push(FactMention {
                        subject: subject.clone(),
                        predicate: pred,
                        object: None,
                        value: Some(value),
                        valid_from: None,
                        valid_to: None,
                        valid_precision: None,
                        confidence_bp: ROW_BP,
                        rule_id: RULE_ROW,
                        evidence: vec![ev.clone()],
                    });
                    done.cells += 1;
                }
            }
        }
        idx = j.max(idx);
    }
    sort_extraction(x);
    done
}

#[cfg(test)]
mod tests;
