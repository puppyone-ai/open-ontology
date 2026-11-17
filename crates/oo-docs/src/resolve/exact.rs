//! Exact-name resolution (10 §5.4 steps 1, 3 to 5; no context scoring, no
//! identity attributes, no merge proposals). Pure: the database layer fetches
//! the candidates for every canonical name of one file first, this module
//! decides.
//!
//! Scores: exact canonical name 10000, published alias 9000, draft alias 7500.
//! Bind when the best score is at least 8500 and leads the runner-up by at
//! least 1000. A best score at or above 8500 without that lead is ambiguous
//! (a temporary entity and an `alias_conflict` review item). Below 8500 a new
//! entity is created. Different types never match; a `Thing` only matches a
//! `Thing` with the same label (that is already part of the key).

use std::collections::BTreeMap;

use crate::extract::model::{
    AliasMention, EntityMention, EntityRef, EntityType, EvidenceRef, Extraction, FactMention,
    Predicate,
};
use crate::resolve::normalize::{fact_key, norm};

pub const BIND_MIN_BP: u16 = 8500;
pub const LEAD_MIN_BP: u16 = 1000;

/// Why a stored entity is a candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    CanonicalName,
    AliasPublished,
    AliasDraft,
}

impl Via {
    pub const fn score(self) -> u16 {
        match self {
            Self::CanonicalName => 10000,
            Self::AliasPublished => 9000,
            Self::AliasDraft => 7500,
        }
    }
}

/// A stored active entity that matched a canonical name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub key: String,
    pub via: Via,
}

/// Candidate lookup by `(type, thing label, canonical name)`.
pub trait CandidateIndex {
    fn candidates(&self, ty: EntityType, thing_label: Option<&str>, norm: &str) -> Vec<Candidate>;
}

/// Index with no stored entities.
pub struct EmptyIndex;

impl CandidateIndex for EmptyIndex {
    fn candidates(&self, _: EntityType, _: Option<&str>, _: &str) -> Vec<Candidate> {
        Vec::new()
    }
}

/// In-memory index, keyed by `(type, label norm, name norm)`.
#[derive(Default)]
pub struct MapIndex(pub BTreeMap<(EntityType, String, String), Vec<Candidate>>);

impl MapIndex {
    pub fn insert(
        &mut self,
        ty: EntityType,
        thing_label: Option<&str>,
        name_norm: &str,
        c: Candidate,
    ) {
        self.0
            .entry((
                ty,
                thing_label.map(norm).unwrap_or_default(),
                name_norm.to_owned(),
            ))
            .or_default()
            .push(c);
    }
}

impl CandidateIndex for MapIndex {
    fn candidates(
        &self,
        ty: EntityType,
        thing_label: Option<&str>,
        name_norm: &str,
    ) -> Vec<Candidate> {
        self.0
            .get(&(
                ty,
                thing_label.map(norm).unwrap_or_default(),
                name_norm.to_owned(),
            ))
            .cloned()
            .unwrap_or_default()
    }
}

/// Outcome for one mention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Use this stored entity.
    Bind(String),
    /// Create an entity under the mention's own key.
    Create,
    /// Two or more stored entities tie: create a temporary entity under the
    /// mention's own key and queue an `alias_conflict` (keys are sorted).
    Ambiguous(Vec<String>),
}

/// Decide one mention. Ties are broken by key (byte order), so the result
/// does not depend on the order the index returns candidates in.
pub fn resolve_one(index: &dyn CandidateIndex, m: &EntityRef) -> Resolution {
    let own_key = m.key();
    let mut cands = index.candidates(m.ty, m.thing_label.as_deref(), &m.norm());
    // A canonical-name hit under the mention's own key is the common case.
    cands.sort_by(|a, b| {
        b.via
            .score()
            .cmp(&a.via.score())
            .then_with(|| a.key.cmp(&b.key))
    });
    cands.dedup_by(|a, b| a.key == b.key);
    let Some(best) = cands.first() else {
        return Resolution::Create;
    };
    let best_score = best.via.score();
    if best_score < BIND_MIN_BP {
        return Resolution::Create;
    }
    match cands.get(1) {
        Some(second) if best_score - second.via.score() < LEAD_MIN_BP => {
            let mut keys: Vec<String> = cands
                .iter()
                .filter(|c| best_score - c.via.score() < LEAD_MIN_BP)
                .map(|c| c.key.clone())
                .collect();
            keys.sort();
            // Same key as the mention's own is not a conflict with itself.
            if keys.len() == 1 && keys[0] == own_key {
                return Resolution::Bind(own_key);
            }
            Resolution::Ambiguous(keys)
        }
        _ => Resolution::Bind(best.key.clone()),
    }
}

/// Resolution of every distinct mention of one extraction.
pub type Resolutions = BTreeMap<EntityRef, Resolution>;

/// Resolve all mentions of `x` (entities, alias owners, fact ends).
pub fn resolve_all(index: &dyn CandidateIndex, x: &Extraction) -> Resolutions {
    let mut out = Resolutions::new();
    let mut add = |e: &EntityRef| {
        out.entry(e.clone())
            .or_insert_with(|| resolve_one(index, e));
    };
    for e in &x.entities {
        add(&e.entity);
    }
    for a in &x.aliases {
        add(&a.entity);
    }
    for f in &x.facts {
        add(&f.subject);
        if let Some(o) = &f.object {
            add(o);
        }
    }
    out
}

/// An entity to store (or find) after resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedEntity {
    pub key: String,
    pub ty: EntityType,
    pub thing_label: Option<String>,
    pub subkind: Option<&'static str>,
    pub name: String,
    pub norm: String,
    /// `true` when the mention bound to an already stored entity (nothing is
    /// created, only evidence is added).
    pub bound: bool,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedAlias {
    pub entity_key: String,
    pub alias: String,
    pub norm: String,
    pub confidence_bp: u16,
    pub rule_id: &'static str,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFact {
    pub fact_key: String,
    pub subject_key: String,
    pub predicate: Predicate,
    pub object_key: Option<String>,
    pub value: Option<serde_json::Value>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub valid_precision: Option<&'static str>,
    pub confidence_bp: u16,
    pub rule_id: &'static str,
    pub evidence: Vec<EvidenceRef>,
}

/// An `alias_conflict` to queue: the name and the stored entities that tie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AliasConflict {
    pub norm: String,
    pub ty: EntityType,
    pub entity_keys: Vec<String>,
}

/// The extraction with every mention bound to an entity key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub path: String,
    pub entities: Vec<ResolvedEntity>,
    pub aliases: Vec<ResolvedAlias>,
    pub facts: Vec<ResolvedFact>,
    pub conflicts: Vec<AliasConflict>,
}

/// Apply resolutions: bind keys, recompute fact keys from the bound keys,
/// merge entities that resolved to one key, drop duplicate facts and aliases.
pub fn apply(x: &Extraction, resolutions: &Resolutions) -> Resolved {
    let key_of = |e: &EntityRef| -> String {
        match resolutions.get(e) {
            Some(Resolution::Bind(k)) => k.clone(),
            _ => e.key(),
        }
    };
    let mut entities: BTreeMap<String, ResolvedEntity> = BTreeMap::new();
    let mut conflicts: BTreeMap<(EntityType, String), AliasConflict> = BTreeMap::new();
    let mut put = |m: &EntityMention| {
        let key = key_of(&m.entity);
        let bound = matches!(resolutions.get(&m.entity), Some(Resolution::Bind(_)));
        if let Some(Resolution::Ambiguous(keys)) = resolutions.get(&m.entity) {
            conflicts
                .entry((m.entity.ty, m.entity.norm()))
                .or_insert_with(|| AliasConflict {
                    norm: m.entity.norm(),
                    ty: m.entity.ty,
                    entity_keys: keys.clone(),
                });
        }
        let slot = entities
            .entry(key.clone())
            .or_insert_with(|| ResolvedEntity {
                key,
                ty: m.entity.ty,
                thing_label: m.entity.thing_label.clone(),
                subkind: m.entity.subkind,
                name: m.entity.name.clone(),
                norm: m.entity.norm(),
                bound,
                evidence: Vec::new(),
            });
        slot.subkind = slot.subkind.or(m.entity.subkind);
        for ev in &m.evidence {
            if !slot.evidence.contains(ev) {
                slot.evidence.push(ev.clone());
            }
        }
    };
    for m in &x.entities {
        put(m);
    }
    let mut aliases: BTreeMap<(String, String), ResolvedAlias> = BTreeMap::new();
    for a in &x.aliases {
        let AliasMention {
            entity,
            alias,
            confidence_bp,
            rule_id,
            evidence,
        } = a;
        let key = key_of(entity);
        let n = norm(alias);
        let slot = aliases
            .entry((key.clone(), n.clone()))
            .or_insert_with(|| ResolvedAlias {
                entity_key: key,
                alias: alias.clone(),
                norm: n,
                confidence_bp: *confidence_bp,
                rule_id,
                evidence: Vec::new(),
            });
        if *confidence_bp > slot.confidence_bp {
            slot.confidence_bp = *confidence_bp;
            slot.rule_id = rule_id;
        }
        for ev in evidence {
            if !slot.evidence.contains(ev) {
                slot.evidence.push(ev.clone());
            }
        }
    }
    let mut facts: BTreeMap<String, ResolvedFact> = BTreeMap::new();
    for f in &x.facts {
        let FactMention {
            subject,
            predicate,
            object,
            value,
            valid_from,
            valid_to,
            valid_precision,
            confidence_bp,
            rule_id,
            evidence,
        } = f;
        let subject_key = key_of(subject);
        let object_key = object.as_ref().map(&key_of);
        let k = fact_key(
            &subject_key,
            &predicate.name(),
            object_key.as_deref(),
            value.as_ref(),
            valid_from.as_deref(),
        );
        let slot = facts.entry(k.clone()).or_insert_with(|| ResolvedFact {
            fact_key: k,
            subject_key,
            predicate: predicate.clone(),
            object_key,
            value: value.clone(),
            valid_from: valid_from.clone(),
            valid_to: valid_to.clone(),
            valid_precision: *valid_precision,
            confidence_bp: *confidence_bp,
            rule_id,
            evidence: Vec::new(),
        });
        if *confidence_bp > slot.confidence_bp {
            slot.confidence_bp = *confidence_bp;
            slot.rule_id = rule_id;
        }
        for ev in evidence {
            if !slot.evidence.contains(ev) {
                slot.evidence.push(ev.clone());
            }
        }
    }
    // Entities appearing only as fact ends were added to `entities` by the
    // extraction's collector, so `put` already saw them.
    Resolved {
        path: x.path.clone(),
        entities: entities.into_values().collect(),
        aliases: aliases.into_values().collect(),
        facts: facts.into_values().collect(),
        conflicts: conflicts.into_values().collect(),
    }
}

/// Convenience: resolve and apply in one go.
pub fn resolve_extraction(index: &dyn CandidateIndex, x: &Extraction) -> Resolved {
    apply(x, &resolve_all(index, x))
}

#[cfg(test)]
mod tests;
