//! Template-driven extraction (D93): a record file of a class with a confirmed structure is read against that
//! structure. Its properties fill their slots (a literal fact, or a relation to an entity); facts the structure has no
//! slot for do not enter the ontology. The model, when it is used, only fills the same slots. Pure.

use std::collections::BTreeSet;

use oo_core::docir::{BlockKind, DocIr};
use serde_json::{Value, json};

use super::observe::{FileObs, observe};
use super::vtype::{items_of, name_like};
use super::{ClassStructure, FieldSource, Property, Status, ValueType};
use crate::extract::model::{
    EntityMention, EntityRef, EntityType, EvidenceRef, Extraction, FactMention, Predicate,
};
use crate::extract::rules::file_default_valid_from;

/// Confidence of a fact read from a slot (a structured location is the most reliable place a value can be, D96).
pub const SLOT_BP: u16 = 9300;
/// Rule id of the facts made from slots.
pub const RULE_SLOT: &str = "structure.slot";
/// Values of one multi-valued slot written at most.
const MAX_ITEMS: usize = 12;
const MAX_TEXT: usize = 200;

/// What a template did to one extraction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub slots_filled: u32,
    pub relations: u32,
    pub dropped: u32,
}

fn evidence_for(ir: &DocIr, needle: &str, source: FieldSource) -> Option<EvidenceRef> {
    let want = if source == FieldSource::Frontmatter {
        BlockKind::Frontmatter
    } else {
        BlockKind::Heading
    };
    let hit = ir.blocks.iter().find(|b| {
        b.kind != BlockKind::Frontmatter && !needle.is_empty() && b.text.contains(needle)
    });
    hit.or_else(|| ir.blocks.iter().find(|b| b.kind == want))
        .or_else(|| ir.blocks.iter().find(|b| b.kind == BlockKind::Frontmatter))
        .or_else(|| ir.blocks.first())
        .map(EvidenceRef::from_block)
}

fn entity_type_of(p: &Property, classes: &[ClassStructure]) -> Option<EntityType> {
    let named = match &p.relation {
        Some(r) if r.target_kind == "class" => classes
            .iter()
            .find(|c| c.class_key == r.target)
            .and(match p.value_type {
                ValueType::Person => Some("Person"),
                ValueType::Project => Some("Project"),
                ValueType::Organization => Some("Organization"),
                _ => None,
            }),
        Some(r) => Some(r.target.as_str()),
        None => p.value_type.entity_type(),
    };
    named.and_then(EntityType::parse)
}

fn sort_extraction(x: &mut Extraction) {
    x.entities.sort_by(|a, b| a.entity.cmp(&b.entity));
    let key = |f: &FactMention| {
        (
            f.subject.clone(),
            f.predicate.name(),
            f.object.clone(),
            f.value.as_ref().map(Value::to_string),
            f.valid_from.clone(),
        )
    };
    x.facts.sort_by_key(key);
}

fn add_entity(x: &mut Extraction, e: &EntityRef, ev: &EvidenceRef) {
    match x.entities.iter_mut().find(|m| &m.entity == e) {
        Some(m) => {
            if !m.evidence.contains(ev) {
                m.evidence.push(ev.clone());
                m.evidence.sort();
            }
        }
        None => x.entities.push(EntityMention {
            entity: e.clone(),
            evidence: vec![ev.clone()],
        }),
    }
}

/// Fill the slots of `class` from the file and drop what the structure has no slot for.
pub fn apply(
    path: &str,
    ir: &DocIr,
    x: &mut Extraction,
    class: &ClassStructure,
    classes: &[ClassStructure],
) -> Applied {
    let mut done = Applied::default();
    let Some(obs) = observe(ir) else { return done };
    if class.status != Status::Confirmed
        || obs.shape != class.shape
        || class.shape != super::Shape::Document
    {
        return done;
    }
    let slugs: BTreeSet<&str> = class.slots().map(|p| p.slug.as_str()).collect();
    // 1. Outside the structure: literal properties the rules read from a field that is not a slot.
    let before = x.facts.len();
    x.facts.retain(|f| match &f.predicate {
        Predicate::Attr(a) => slugs.contains(a.as_str()),
        _ => true,
    });
    done.dropped += (before - x.facts.len()) as u32;
    // 2. Slots.
    let doc = EntityRef::new(EntityType::Document, path.to_owned());
    let from = file_default_valid_from(path, ir);
    for p in class.slots().filter(|p| p.source != FieldSource::Directory) {
        let fields: Vec<_> = obs
            .fields
            .iter()
            .filter(|f| p.keys.contains(&f.key))
            .collect();
        if fields.is_empty() {
            continue;
        }
        let mut items: Vec<(String, FieldSource)> = Vec::new();
        for f in &fields {
            for v in &f.values {
                if p.multi || p.value_type.entity_type().is_some() {
                    for i in items_of(v) {
                        items.push((i, f.source));
                    }
                } else {
                    items.push((v.clone(), f.source));
                }
            }
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        items.retain(|(i, _)| seen.insert(i.clone()));
        items.truncate(MAX_ITEMS);
        for (item, src) in items {
            let Some(ev) = evidence_for(ir, &item, src) else {
                continue;
            };
            if let Some(ty) = entity_type_of(p, classes) {
                if ty == EntityType::Person && !name_like(&item) {
                    continue;
                }
                let obj = EntityRef::new(ty, item.clone());
                add_entity(x, &obj, &ev);
                let pred = match (&p.relation, ty) {
                    (Some(r), _) => Predicate::parse(&r.predicate).unwrap_or(Predicate::Mentions),
                    (None, EntityType::Person) => Predicate::Mentions,
                    _ => Predicate::About,
                };
                x.facts.push(FactMention {
                    subject: doc.clone(),
                    predicate: pred,
                    object: Some(obj),
                    value: None,
                    valid_from: from.as_ref().map(|(d, _)| d.clone()),
                    valid_to: None,
                    valid_precision: from.as_ref().map(|(_, p)| *p),
                    confidence_bp: SLOT_BP,
                    rule_id: RULE_SLOT,
                    evidence: vec![ev],
                });
                done.relations += 1;
                continue;
            }
            let value = match p.value_type {
                ValueType::Date => match crate::extract::rules::dates::parse_exact(
                    item.split_whitespace().next().unwrap_or(""),
                ) {
                    Some(d) => json!({"date": d.iso}),
                    None => continue,
                },
                ValueType::Enum => json!({"enum": item.chars().take(MAX_TEXT).collect::<String>()}),
                _ => json!({"text": item.chars().take(MAX_TEXT).collect::<String>()}),
            };
            let Ok(pred) = Predicate::parse(&format!("attr.{}", p.slug)).ok_or(()) else {
                continue;
            };
            if x.facts.iter().any(|f| {
                f.subject == doc && f.predicate == pred && f.value.as_ref() == Some(&value)
            }) {
                continue;
            }
            add_entity(x, &doc, &ev);
            x.facts.push(FactMention {
                subject: doc.clone(),
                predicate: pred,
                object: None,
                value: Some(value),
                valid_from: from.as_ref().map(|(d, _)| d.clone()),
                valid_to: None,
                valid_precision: from.as_ref().map(|(_, p)| *p),
                confidence_bp: SLOT_BP,
                rule_id: RULE_SLOT,
                evidence: vec![ev],
            });
            done.slots_filled += 1;
        }
    }
    sort_extraction(x);
    done
}

/// What the model may write for a file of `class`: its output keeps only what the structure has a slot for. Literal
/// facts of other names and open `Thing` labels are dropped; the entities and relations of the slots stay.
pub fn constrain_llm(x: &mut Extraction, class: &ClassStructure) -> u32 {
    let slugs: BTreeSet<&str> = class.slots().map(|p| p.slug.as_str()).collect();
    let before = x.facts.len();
    x.facts.retain(|f| match &f.predicate {
        Predicate::Attr(a) => slugs.contains(a.as_str()),
        _ => true,
    });
    let mut dropped = before - x.facts.len();
    let things: BTreeSet<EntityRef> = x
        .entities
        .iter()
        .filter(|m| m.entity.ty == EntityType::Thing)
        .map(|m| m.entity.clone())
        .collect();
    if !things.is_empty() {
        let b = x.facts.len();
        x.facts.retain(|f| {
            !things.contains(&f.subject) && f.object.as_ref().is_none_or(|o| !things.contains(o))
        });
        dropped += b - x.facts.len();
        dropped += things.len();
        x.entities.retain(|m| !things.contains(&m.entity));
        x.aliases.retain(|a| !things.contains(&a.entity));
    }
    dropped as u32
}

/// The slots of a class as text for the model's prompt: only these are asked for.
pub fn prompt_section(class: &ClassStructure) -> String {
    let mut out = format!(
        "模板（{}）：只填写下列槽位，模板之外的内容不要抽取。\n",
        class.label
    );
    for p in class.slots() {
        let ty = match &p.relation {
            Some(r) => format!("{}（关系：{}）", p.value_type.as_str(), r.target),
            None => p.value_type.as_str().to_owned(),
        };
        let extra = if p.value_type == ValueType::Enum && !p.enum_values.is_empty() {
            format!("，取值：{}", p.enum_values.join("、"))
        } else {
            String::new()
        };
        out.push_str(&format!("- {}（{}）{}\n", p.name, ty, extra));
    }
    out
}

/// The class of a file from its observation, among the classes with a confirmed structure.
pub fn class_of<'a>(obs: &FileObs, classes: &'a [ClassStructure]) -> Option<&'a ClassStructure> {
    let confirmed: Vec<&ClassStructure> = classes
        .iter()
        .filter(|c| c.status == Status::Confirmed)
        .collect();
    let owned: Vec<ClassStructure> = confirmed.iter().map(|c| (*c).clone()).collect();
    let k = super::induce::assign(obs, &owned)?.class_key.clone();
    classes.iter().find(|c| c.class_key == k)
}
