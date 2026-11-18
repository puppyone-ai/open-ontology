//! Mention resolution (10 §5.4 steps 1 to 5). Pure: the database layer fetches
//! the candidates for every name and identity of one file first, this module
//! decides.
//!
//! 1. A mention's identity attribute (`email`, `employee_id`, `github`) that
//!    equals the identity of a stored entity of the same type binds to it
//!    (score 10000, [`Via::Identity`]).
//! 2. Otherwise stored entities are candidates by canonical name (10000),
//!    published alias (9000), draft alias (7500) and, for people, the title-free
//!    forms of the name (5000, [`Via::Honorific`]). Different types never
//!    match; a `Thing` only matches a `Thing` with the same label.
//! 3. A candidate whose identity contradicts the mention's (same kind, other
//!    value) is a different person: it is dropped, and a mention that carries
//!    an identity then gets its own key with the identity in it
//!    ([`Resolution::Distinct`], plus an `alias_conflict`).
//! 4. Context adds 500 per shared one-hop neighbour (at most 1500) and 500
//!    for the same directory.
//! 5. Bind when the best score is at least 8500 and leads the runner-up by at
//!    least 1000. At or above 8500 without that lead is ambiguous (a temporary
//!    entity and an `alias_conflict`). Below 8500 a new entity is created.
//!
//! Every comparison is on bytes and ties go to the smaller key, so the answer
//! does not depend on the order of the input.

use std::collections::{BTreeMap, BTreeSet};

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
    /// A title-free form of the mention's name matched.
    Honorific,
    /// The identity attribute matched.
    Identity,
}

impl Via {
    pub const fn score(self) -> u16 {
        match self {
            Self::CanonicalName | Self::Identity => 10000,
            Self::AliasPublished => 9000,
            Self::AliasDraft => 7500,
            Self::Honorific => 5000,
        }
    }
}

/// Context bonus per shared one-hop neighbour and its cap (5.4 step 4).
pub const NEIGHBOR_BP: u16 = 500;
pub const NEIGHBOR_CAP_BP: u16 = 1500;
/// Bonus for a candidate that already has evidence in the same directory.
pub const SAME_DIR_BP: u16 = 500;

/// What a file around the mention looks like: the keys of the other entities
/// it names and its directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub keys: BTreeSet<String>,
    /// Directory of the file (empty at the drive root).
    pub dir: String,
}

/// A stored active entity that matched a name or an identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub key: String,
    pub via: Via,
    /// Keys of the entities one fact away.
    pub neighbors: BTreeSet<String>,
    /// Directories of the files that hold evidence for it.
    pub dirs: BTreeSet<String>,
    /// Its identity attributes (`kind` to value).
    pub identity: BTreeMap<String, String>,
}

impl Candidate {
    /// A candidate without context or identity.
    pub fn new(key: impl Into<String>, via: Via) -> Self {
        Self {
            key: key.into(),
            via,
            neighbors: BTreeSet::new(),
            dirs: BTreeSet::new(),
            identity: BTreeMap::new(),
        }
    }

    /// Score of this candidate in `ctx` (5.4 step 4).
    pub fn score(&self, ctx: &Context) -> u16 {
        let shared = self.neighbors.intersection(&ctx.keys).count();
        let shared = u16::try_from(shared).unwrap_or(u16::MAX);
        let near = shared.saturating_mul(NEIGHBOR_BP).min(NEIGHBOR_CAP_BP);
        let dir = if !ctx.dir.is_empty() && self.dirs.contains(&ctx.dir) {
            SAME_DIR_BP
        } else {
            0
        };
        self.via.score() + near + dir
    }
}

/// Candidate lookup by `(type, thing label, canonical name)`, by identity and
/// by surname.
pub trait CandidateIndex {
    fn candidates(&self, ty: EntityType, thing_label: Option<&str>, norm: &str) -> Vec<Candidate>;

    /// Stored entities of `ty` holding the identity `kind = value`.
    fn by_identity(&self, _ty: EntityType, _kind: &str, _value: &str) -> Vec<Candidate> {
        Vec::new()
    }

    /// Stored people whose canonical name starts with `surname` and is longer
    /// than it.
    fn by_surname(&self, _surname: &str) -> Vec<Candidate> {
        Vec::new()
    }
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
pub struct MapIndex {
    pub names: BTreeMap<(EntityType, String, String), Vec<Candidate>>,
    /// `(type, kind, value)` to the entities holding that identity.
    pub ids: BTreeMap<(EntityType, String, String), Vec<Candidate>>,
    /// Surname to the people whose canonical name starts with it.
    pub surnames: BTreeMap<String, Vec<Candidate>>,
}

impl MapIndex {
    pub fn insert(
        &mut self,
        ty: EntityType,
        thing_label: Option<&str>,
        name_norm: &str,
        c: Candidate,
    ) {
        self.names
            .entry((
                ty,
                thing_label.map(norm).unwrap_or_default(),
                name_norm.to_owned(),
            ))
            .or_default()
            .push(c);
    }

    pub fn insert_identity(&mut self, ty: EntityType, kind: &str, value: &str, c: Candidate) {
        self.ids
            .entry((ty, kind.to_owned(), value.to_owned()))
            .or_default()
            .push(c);
    }

    pub fn insert_surname(&mut self, surname: &str, c: Candidate) {
        self.surnames.entry(surname.to_owned()).or_default().push(c);
    }
}

impl CandidateIndex for MapIndex {
    fn candidates(
        &self,
        ty: EntityType,
        thing_label: Option<&str>,
        name_norm: &str,
    ) -> Vec<Candidate> {
        self.names
            .get(&(
                ty,
                thing_label.map(norm).unwrap_or_default(),
                name_norm.to_owned(),
            ))
            .cloned()
            .unwrap_or_default()
    }

    fn by_identity(&self, ty: EntityType, kind: &str, value: &str) -> Vec<Candidate> {
        self.ids
            .get(&(ty, kind.to_owned(), value.to_owned()))
            .map(|v| {
                v.iter()
                    .map(|c| Candidate {
                        via: Via::Identity,
                        ..c.clone()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn by_surname(&self, surname: &str) -> Vec<Candidate> {
        self.surnames.get(surname).cloned().unwrap_or_default()
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
    /// Same name as a stored entity but a contradicting identity: keep both,
    /// the new one under a key that includes the identity (3.6), and queue an
    /// `alias_conflict`. `against` is sorted.
    Distinct { key: String, against: Vec<String> },
}

/// Identity attributes of a mention (`kind` to value).
pub type Identity = BTreeMap<String, String>;

/// Entity key of a mention that carries `identity`.
pub fn identity_key(m: &EntityRef, identity: &Identity) -> String {
    let canonical = oo_builder::canonical_json(&serde_json::json!(identity));
    crate::resolve::normalize::entity_key(
        m.ty,
        m.thing_label.as_deref(),
        &m.norm(),
        Some(&canonical),
    )
}

/// The scored candidates of one mention, best first, and what was dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ranking {
    /// Stored entities holding the mention's identity.
    pub identity_hits: Vec<Candidate>,
    /// `(score, candidate)`, best first, one row per key.
    pub ranked: Vec<(u16, Candidate)>,
    /// Name matches dropped because their identity contradicts the mention.
    pub contradicted: Vec<Candidate>,
}

fn contradicts(c: &Candidate, identity: &Identity) -> bool {
    identity
        .iter()
        .any(|(k, v)| c.identity.get(k).is_some_and(|have| have != v))
}

/// Rank the stored entities for `m` (5.4 steps 2 to 4).
pub fn rank(
    index: &dyn CandidateIndex,
    m: &EntityRef,
    identity: &Identity,
    ctx: &Context,
) -> Ranking {
    let mut hits: Vec<Candidate> = identity
        .iter()
        .flat_map(|(k, v)| index.by_identity(m.ty, k, v))
        .collect();
    hits.sort_by(|a, b| a.key.cmp(&b.key));
    hits.dedup_by(|a, b| a.key == b.key);

    let mut cands = index.candidates(m.ty, m.thing_label.as_deref(), &m.norm());
    if m.ty == EntityType::Person {
        for form in crate::resolve::honorific::stripped_forms(&m.name) {
            let mut found = index.candidates(m.ty, None, &form);
            if let Some(surname) = crate::resolve::honorific::surname_form(&form) {
                found.extend(index.by_surname(&surname));
            }
            cands.extend(found.into_iter().map(|c| Candidate {
                via: Via::Honorific,
                ..c
            }));
        }
    }
    let (kept, contradicted): (Vec<_>, Vec<_>) =
        cands.into_iter().partition(|c| !contradicts(c, identity));
    let mut ranked: Vec<(u16, Candidate)> = kept.into_iter().map(|c| (c.score(ctx), c)).collect();
    // Best score first; equal scores go to the smaller key.
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.key.cmp(&b.1.key)));
    ranked.dedup_by(|b, a| a.1.key == b.1.key);
    let mut contradicted = contradicted;
    contradicted.sort_by(|a, b| {
        b.via
            .score()
            .cmp(&a.via.score())
            .then_with(|| a.key.cmp(&b.key))
    });
    contradicted.dedup_by(|b, a| a.key == b.key);
    Ranking {
        identity_hits: hits,
        ranked,
        contradicted,
    }
}

/// Decide one mention (5.4 steps 2 to 5).
pub fn resolve_scored(
    index: &dyn CandidateIndex,
    m: &EntityRef,
    identity: &Identity,
    ctx: &Context,
) -> Resolution {
    let own_key = m.key();
    let r = rank(index, m, identity, ctx);
    if let Some(hit) = r.identity_hits.first() {
        return Resolution::Bind(hit.key.clone());
    }
    let decision = match r.ranked.first() {
        None => Resolution::Create,
        Some((best, best_c)) => {
            if *best < BIND_MIN_BP {
                Resolution::Create
            } else {
                let close: Vec<&Candidate> = r
                    .ranked
                    .iter()
                    .skip(1)
                    .filter(|(s, _)| best - s < LEAD_MIN_BP)
                    .map(|(_, c)| c)
                    .collect();
                if close.is_empty() {
                    Resolution::Bind(best_c.key.clone())
                } else {
                    let mut keys: Vec<String> = std::iter::once(best_c)
                        .chain(close)
                        .map(|c| c.key.clone())
                        .collect();
                    keys.sort();
                    keys.dedup();
                    // Same key as the mention's own is not a conflict with itself.
                    if keys.len() == 1 && keys[0] == own_key {
                        Resolution::Bind(own_key.clone())
                    } else {
                        Resolution::Ambiguous(keys)
                    }
                }
            }
        }
    };
    if matches!(decision, Resolution::Bind(_)) || identity.is_empty() {
        return decision;
    }
    let mut against: Vec<String> = r
        .contradicted
        .iter()
        .filter(|c| c.score(ctx) >= BIND_MIN_BP)
        .map(|c| c.key.clone())
        .collect();
    if against.is_empty() {
        return decision;
    }
    against.sort();
    Resolution::Distinct {
        key: identity_key(m, identity),
        against,
    }
}

/// Decide one mention without identity and context. Ties are broken by key
/// (byte order), so the result does not depend on the order the index returns
/// candidates in.
pub fn resolve_one(index: &dyn CandidateIndex, m: &EntityRef) -> Resolution {
    resolve_scored(index, m, &Identity::new(), &Context::default())
}

/// Resolution of every distinct mention of one extraction, and of the bind
/// hints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolutions {
    pub entities: BTreeMap<EntityRef, Resolution>,
    /// Hint index (position in `Extraction::hints`) to the bound key; hints
    /// whose target does not bind are absent.
    pub hints: BTreeMap<usize, String>,
}

/// Identity attributes per mention of `x`: per kind the smallest value, so
/// two contradicting values in one file never depend on their order.
pub fn identities_of(x: &Extraction) -> BTreeMap<EntityRef, Identity> {
    let mut out: BTreeMap<EntityRef, Identity> = BTreeMap::new();
    for i in &x.identities {
        let slot = out.entry(i.entity.clone()).or_default();
        let value = slot.get(i.kind).map_or(i.value.clone(), |have| {
            std::cmp::min(have.clone(), i.value.clone())
        });
        slot.insert(i.kind.to_owned(), value);
    }
    out
}

/// Directory of a drive-relative path (empty at the root).
pub fn dir_of(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(d, _)| d.to_owned())
        .unwrap_or_default()
}

/// Resolve all mentions of `x` (entities, alias owners, fact ends, hints).
pub fn resolve_all(index: &dyn CandidateIndex, x: &Extraction) -> Resolutions {
    let identities = identities_of(x);
    let mut mentions: Vec<&EntityRef> = Vec::new();
    for e in &x.entities {
        mentions.push(&e.entity);
    }
    for a in &x.aliases {
        mentions.push(&a.entity);
    }
    for f in &x.facts {
        mentions.push(&f.subject);
        if let Some(o) = &f.object {
            mentions.push(o);
        }
    }
    for i in &x.identities {
        mentions.push(&i.entity);
    }
    let ctx = Context {
        keys: mentions.iter().map(|m| m.key()).collect(),
        dir: dir_of(&x.path),
    };
    let mut out = Resolutions::default();
    let empty = Identity::new();
    for m in mentions {
        out.entities
            .entry(m.clone())
            .or_insert_with(|| resolve_scored(index, m, identities.get(m).unwrap_or(&empty), &ctx));
    }
    for (i, h) in x.hints.iter().enumerate() {
        let target = EntityRef::new(h.target_ty, h.target_name.clone());
        if let Resolution::Bind(k) = resolve_scored(index, &target, &empty, &ctx) {
            out.hints.insert(i, k);
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
    /// Identity attributes found for it in this file.
    pub identity: Identity,
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
    /// `alias_tie` or `identity`.
    pub reason: &'static str,
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

/// Confidence of a generated title-free alias (5.4 step 1).
pub const HONORIFIC_ALIAS_BP: u16 = 5000;

/// Apply resolutions: bind keys, recompute fact keys from the bound keys,
/// merge entities that resolved to one key, drop duplicate facts and aliases,
/// turn bound hints into facts and add the title-free aliases of new people.
pub fn apply(x: &Extraction, resolutions: &Resolutions) -> Resolved {
    let key_of = |e: &EntityRef| -> String {
        match resolutions.entities.get(e) {
            Some(Resolution::Bind(k)) => k.clone(),
            Some(Resolution::Distinct { key, .. }) => key.clone(),
            _ => e.key(),
        }
    };
    let identities = identities_of(x);
    let mut entities: BTreeMap<String, ResolvedEntity> = BTreeMap::new();
    let mut conflicts: BTreeMap<(EntityType, String), AliasConflict> = BTreeMap::new();
    let mut put = |m: &EntityMention| {
        let key = key_of(&m.entity);
        let bound = matches!(
            resolutions.entities.get(&m.entity),
            Some(Resolution::Bind(_))
        );
        let conflict = match resolutions.entities.get(&m.entity) {
            Some(Resolution::Ambiguous(keys)) => Some((keys.clone(), "alias_tie")),
            Some(Resolution::Distinct { key, against }) => {
                let mut keys = against.clone();
                keys.push(key.clone());
                keys.sort();
                Some((keys, "identity"))
            }
            _ => None,
        };
        if let Some((keys, reason)) = conflict {
            conflicts
                .entry((m.entity.ty, m.entity.norm()))
                .or_insert_with(|| AliasConflict {
                    norm: m.entity.norm(),
                    ty: m.entity.ty,
                    entity_keys: keys,
                    reason,
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
                identity: Identity::new(),
                evidence: Vec::new(),
            });
        slot.subkind = slot.subkind.or(m.entity.subkind);
        // Several spellings can bind to one stored entity: the smallest wins,
        // so the input order never shows.
        if m.entity.name < slot.name {
            slot.name = m.entity.name.clone();
            slot.norm = m.entity.norm();
        }
        if let Some(id) = identities.get(&m.entity) {
            for (k, v) in id {
                let have = slot.identity.entry(k.clone()).or_insert_with(|| v.clone());
                if v < have {
                    *have = v.clone();
                }
            }
        }
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
    let mut add_alias = |key: String,
                         alias: &str,
                         confidence_bp: u16,
                         rule_id: &'static str,
                         evidence: &[EvidenceRef]| {
        let n = norm(alias);
        let slot = aliases
            .entry((key.clone(), n.clone()))
            .or_insert_with(|| ResolvedAlias {
                entity_key: key,
                alias: alias.to_owned(),
                norm: n,
                confidence_bp,
                rule_id,
                evidence: Vec::new(),
            });
        if confidence_bp > slot.confidence_bp {
            slot.confidence_bp = confidence_bp;
            slot.rule_id = rule_id;
        }
        for ev in evidence {
            if !slot.evidence.contains(ev) {
                slot.evidence.push(ev.clone());
            }
        }
    };
    for a in &x.aliases {
        let AliasMention {
            entity,
            alias,
            confidence_bp,
            rule_id,
            evidence,
        } = a;
        add_alias(key_of(entity), alias, *confidence_bp, rule_id, evidence);
    }
    // Title-free forms of people created here (5.4 step 1): candidates only.
    for e in entities.values() {
        if e.ty != EntityType::Person || e.bound {
            continue;
        }
        for form in crate::resolve::honorific::stripped_forms(&e.name) {
            add_alias(
                e.key.clone(),
                &form,
                HONORIFIC_ALIAS_BP,
                "resolve.honorific",
                &e.evidence,
            );
        }
    }
    let mut facts: BTreeMap<String, ResolvedFact> = BTreeMap::new();
    let mut add_fact = |subject_key: String,
                        predicate: &Predicate,
                        object_key: Option<String>,
                        value: &Option<serde_json::Value>,
                        valid_from: &Option<String>,
                        valid_to: &Option<String>,
                        valid_precision: Option<&'static str>,
                        confidence_bp: u16,
                        rule_id: &'static str,
                        evidence: &[EvidenceRef]| {
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
            valid_precision,
            confidence_bp,
            rule_id,
            evidence: Vec::new(),
        });
        if confidence_bp > slot.confidence_bp {
            slot.confidence_bp = confidence_bp;
            slot.rule_id = rule_id;
        }
        for ev in evidence {
            if !slot.evidence.contains(ev) {
                slot.evidence.push(ev.clone());
            }
        }
    };
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
        add_fact(
            key_of(subject),
            predicate,
            object.as_ref().map(&key_of),
            value,
            valid_from,
            valid_to,
            *valid_precision,
            *confidence_bp,
            rule_id,
            evidence,
        );
    }
    // Hints whose target bound: a fact to a stored entity. The target gets
    // the evidence too, so the pack can show where it was named.
    let mut hint_targets: Vec<(String, Vec<EvidenceRef>, EntityType, String)> = Vec::new();
    for (i, h) in x.hints.iter().enumerate() {
        let Some(target) = resolutions.hints.get(&i) else {
            continue;
        };
        add_fact(
            key_of(&h.subject),
            &h.predicate,
            Some(target.clone()),
            &None,
            &h.valid_from.as_ref().map(|(d, _)| d.clone()),
            &None,
            h.valid_from.as_ref().map(|(_, p)| *p),
            h.confidence_bp,
            h.rule_id,
            &h.evidence,
        );
        hint_targets.push((
            target.clone(),
            h.evidence.clone(),
            h.target_ty,
            h.target_name.clone(),
        ));
    }
    for (key, evidence, ty, name) in hint_targets {
        let slot = entities
            .entry(key.clone())
            .or_insert_with(|| ResolvedEntity {
                key,
                ty,
                thing_label: None,
                subkind: None,
                norm: norm(&name),
                name,
                bound: true,
                identity: Identity::new(),
                evidence: Vec::new(),
            });
        for ev in evidence {
            if !slot.evidence.contains(&ev) {
                slot.evidence.push(ev);
            }
        }
    }
    // Evidence lists are sorted so the order of the input never shows.
    for e in entities.values_mut() {
        e.evidence.sort();
    }
    for a in aliases.values_mut() {
        a.evidence.sort();
    }
    for f in facts.values_mut() {
        f.evidence.sort();
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
