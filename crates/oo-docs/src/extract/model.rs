//! Candidate types produced by the extraction rules (10 §3.6, §5.1). Pure data.

use std::collections::BTreeMap;

use oo_core::docir::{Anchor, Block};
use serde_json::Value;

use crate::resolve::normalize::norm;

/// Core entity types (D15) plus the open candidate `Thing` (D16).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityType {
    Person,
    Organization,
    Project,
    Decision,
    Document,
    Topic,
    Event,
    Term,
    Thing,
}

impl EntityType {
    /// All types in declaration order.
    pub const ALL: [EntityType; 9] = [
        Self::Person,
        Self::Organization,
        Self::Project,
        Self::Decision,
        Self::Document,
        Self::Topic,
        Self::Event,
        Self::Term,
        Self::Thing,
    ];

    /// `entity.type` column value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Person => "Person",
            Self::Organization => "Organization",
            Self::Project => "Project",
            Self::Decision => "Decision",
            Self::Document => "Document",
            Self::Topic => "Topic",
            Self::Event => "Event",
            Self::Term => "Term",
            Self::Thing => "Thing",
        }
    }

    /// Key prefix (10 §3.6).
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Organization => "organization",
            Self::Project => "project",
            Self::Decision => "decision",
            Self::Document => "document",
            Self::Topic => "topic",
            Self::Event => "event",
            Self::Term => "term",
            Self::Thing => "thing",
        }
    }

    /// Inverse of [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == text)
    }
}

/// Core relations (D15) and `attr.<name>` properties.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Predicate {
    Owns,
    MemberOf,
    Decides,
    Supersedes,
    Rejects,
    Mentions,
    About,
    TriedAndFailed,
    /// `attr.<name>`, name `[a-z_]{1,40}`.
    Attr(String),
}

impl Predicate {
    /// `fact.predicate` column value.
    pub fn name(&self) -> String {
        match self {
            Self::Owns => "owns".into(),
            Self::MemberOf => "member_of".into(),
            Self::Decides => "decides".into(),
            Self::Supersedes => "supersedes".into(),
            Self::Rejects => "rejects".into(),
            Self::Mentions => "mentions".into(),
            Self::About => "about".into(),
            Self::TriedAndFailed => "tried_and_failed".into(),
            Self::Attr(a) => format!("attr.{a}"),
        }
    }

    /// Inverse of [`Self::name`]; `None` for anything outside the D15 set and
    /// well-formed `attr.*`.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "owns" => Self::Owns,
            "member_of" => Self::MemberOf,
            "decides" => Self::Decides,
            "supersedes" => Self::Supersedes,
            "rejects" => Self::Rejects,
            "mentions" => Self::Mentions,
            "about" => Self::About,
            "tried_and_failed" => Self::TriedAndFailed,
            other => {
                let attr = other.strip_prefix("attr.")?;
                let ok = (1..=40).contains(&attr.len())
                    && attr.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
                if !ok {
                    return None;
                }
                Self::Attr(attr.to_owned())
            }
        })
    }

    /// Properties carry a literal value, relations an object entity.
    pub fn is_attr(&self) -> bool {
        matches!(self, Self::Attr(_))
    }
}

/// How an entity is named in one place of a document; also the identity the
/// entity key is computed from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityRef {
    pub ty: EntityType,
    /// Present exactly for `Thing`.
    pub thing_label: Option<String>,
    /// `Some("preference")` for `Decision{subkind=preference}` (D15).
    pub subkind: Option<&'static str>,
    /// Display name as written in the document.
    pub name: String,
}

impl EntityRef {
    pub fn new(ty: EntityType, name: impl Into<String>) -> Self {
        Self {
            ty,
            thing_label: None,
            subkind: None,
            name: name.into(),
        }
    }

    pub fn preference(name: impl Into<String>) -> Self {
        Self {
            subkind: Some("preference"),
            ..Self::new(EntityType::Decision, name)
        }
    }

    /// Canonical name of the mention.
    pub fn norm(&self) -> String {
        norm(&self.name)
    }

    /// Entity key of this mention (before resolution can bind it elsewhere).
    pub fn key(&self) -> String {
        crate::resolve::normalize::entity_key(
            self.ty,
            self.thing_label.as_deref(),
            &self.norm(),
            None,
        )
    }
}

/// A pointer into the IR: the block and its anchor, plus the hash of the
/// quoted block text (the text itself stays in the IR, 10 §3.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceRef {
    pub block_id: String,
    pub anchor: Anchor,
    /// BLAKE3 of the NFC block text, hex.
    pub quote_b3: String,
}

// `Anchor` has no total order; the block id is content-derived (anchor and
// text), so ordering by (block id, quote hash) is total and deterministic.
impl Ord for EvidenceRef {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.block_id, &self.quote_b3).cmp(&(&other.block_id, &other.quote_b3))
    }
}

impl PartialOrd for EvidenceRef {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl EvidenceRef {
    pub fn from_block(block: &Block) -> Self {
        use unicode_normalization::UnicodeNormalization;
        let text: String = block.text.nfc().collect();
        Self {
            block_id: block.id.clone(),
            anchor: block.anchor.clone(),
            quote_b3: blake3::hash(text.as_bytes()).to_hex().to_string(),
        }
    }
}

/// One entity mentioned in a file.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityMention {
    pub entity: EntityRef,
    pub evidence: Vec<EvidenceRef>,
}

/// An alias candidate (nickname, English name, title).
#[derive(Clone, Debug, PartialEq)]
pub struct AliasMention {
    pub entity: EntityRef,
    pub alias: String,
    pub confidence_bp: u16,
    pub rule_id: &'static str,
    pub evidence: Vec<EvidenceRef>,
}

/// A fact candidate. Exactly one of `object` and `value` is set.
#[derive(Clone, Debug, PartialEq)]
pub struct FactMention {
    pub subject: EntityRef,
    pub predicate: Predicate,
    pub object: Option<EntityRef>,
    /// `{"text":..}`, `{"date":..}` or `{"enum":..}`.
    pub value: Option<Value>,
    /// `YYYY-MM-DD`.
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    /// `day`, `month`, `year`, `doc_date`, `file_mtime`, `unknown`.
    pub valid_precision: Option<&'static str>,
    pub confidence_bp: u16,
    pub rule_id: &'static str,
    pub evidence: Vec<EvidenceRef>,
}

/// A deterministic identity attribute of an entity (10 §5.4 step 2): the
/// resolver binds on an exact equal `(kind, value)` of the same type.
#[derive(Clone, Debug, PartialEq)]
pub struct IdentityMention {
    pub entity: EntityRef,
    /// `email`, `employee_id` or `github`.
    pub kind: &'static str,
    /// Normalized value (lowercase for `email` and `github`).
    pub value: String,
    pub confidence_bp: u16,
    pub rule_id: &'static str,
    pub evidence: Vec<EvidenceRef>,
}

/// A relation whose far end must already exist (`pptx.title`, `doc.meta`
/// author): resolve turns it into a fact only when `target_name` binds to a
/// stored entity of `target_ty`, and never creates that entity (10 §5.1).
#[derive(Clone, Debug, PartialEq)]
pub struct BindHint {
    pub subject: EntityRef,
    pub predicate: Predicate,
    pub target_ty: EntityType,
    pub target_name: String,
    pub confidence_bp: u16,
    pub rule_id: &'static str,
    pub evidence: Vec<EvidenceRef>,
}

/// Provenance of an LLM extraction (10 §5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlmMeta {
    /// `extract/1`.
    pub prompt_version: String,
    /// Hash of the batch cache keys the facts came from.
    pub extraction_key: String,
}

/// Everything the rules found in one file, sorted and de-duplicated, so the
/// same input always gives the same value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extraction {
    /// Drive-relative path (NFC) the rules ran on.
    pub path: String,
    /// Set for the output of the LLM supplement: its facts, aliases and
    /// evidence are stored with `origin = 'llm'`.
    pub llm: Option<LlmMeta>,
    pub entities: Vec<EntityMention>,
    pub aliases: Vec<AliasMention>,
    pub facts: Vec<FactMention>,
    pub identities: Vec<IdentityMention>,
    pub hints: Vec<BindHint>,
}

/// Merge helper: ordered maps give the deterministic output order.
#[derive(Default)]
pub(crate) struct Collector {
    pub entities: BTreeMap<EntityRef, Vec<EvidenceRef>>,
    pub aliases: BTreeMap<(EntityRef, String), AliasMention>,
    pub facts: BTreeMap<FactSlot, FactMention>,
    pub identities: BTreeMap<(EntityRef, &'static str, String), IdentityMention>,
    pub hints: BTreeMap<(EntityRef, Predicate, EntityType, String), BindHint>,
}

/// Identity of a fact candidate inside one file.
pub(crate) type FactSlot = (
    EntityRef,
    Predicate,
    Option<EntityRef>,
    Option<String>,
    Option<String>,
);

impl Collector {
    pub fn entity(&mut self, entity: &EntityRef, evidence: Option<&EvidenceRef>) {
        let slot = self.entities.entry(entity.clone()).or_default();
        if let Some(ev) = evidence
            && !slot.contains(ev)
        {
            slot.push(ev.clone());
        }
    }

    pub fn alias(
        &mut self,
        entity: &EntityRef,
        alias: &str,
        conf: u16,
        rule: &'static str,
        ev: &EvidenceRef,
    ) {
        let alias = alias.trim();
        if alias.is_empty() || norm(alias) == entity.norm() {
            return;
        }
        self.entity(entity, Some(ev));
        let slot = self
            .aliases
            .entry((entity.clone(), norm(alias)))
            .or_insert_with(|| AliasMention {
                entity: entity.clone(),
                alias: alias.to_owned(),
                confidence_bp: conf,
                rule_id: rule,
                evidence: Vec::new(),
            });
        if conf > slot.confidence_bp {
            slot.confidence_bp = conf;
            slot.rule_id = rule;
        }
        if !slot.evidence.contains(ev) {
            slot.evidence.push(ev.clone());
        }
    }

    pub fn identity(
        &mut self,
        entity: &EntityRef,
        kind: &'static str,
        value: &str,
        conf: u16,
        rule: &'static str,
        ev: &EvidenceRef,
    ) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        let value = if kind == "employee_id" {
            value.to_owned()
        } else {
            value.to_lowercase()
        };
        self.entity(entity, Some(ev));
        let slot = self
            .identities
            .entry((entity.clone(), kind, value.clone()))
            .or_insert_with(|| IdentityMention {
                entity: entity.clone(),
                kind,
                value,
                confidence_bp: conf,
                rule_id: rule,
                evidence: Vec::new(),
            });
        if conf > slot.confidence_bp {
            slot.confidence_bp = conf;
            slot.rule_id = rule;
        }
        if !slot.evidence.contains(ev) {
            slot.evidence.push(ev.clone());
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hint(
        &mut self,
        subject: &EntityRef,
        predicate: Predicate,
        target_ty: EntityType,
        target_name: &str,
        conf: u16,
        rule: &'static str,
        ev: &EvidenceRef,
    ) {
        let target_name = target_name.trim();
        if target_name.is_empty() {
            return;
        }
        self.entity(subject, Some(ev));
        let slot = self
            .hints
            .entry((
                subject.clone(),
                predicate.clone(),
                target_ty,
                norm(target_name),
            ))
            .or_insert_with(|| BindHint {
                subject: subject.clone(),
                predicate,
                target_ty,
                target_name: target_name.to_owned(),
                confidence_bp: conf,
                rule_id: rule,
                evidence: Vec::new(),
            });
        if !slot.evidence.contains(ev) {
            slot.evidence.push(ev.clone());
        }
    }

    pub fn fact(&mut self, fact: FactMention) {
        self.entity(&fact.subject, fact.evidence.first());
        if let Some(o) = &fact.object {
            self.entity(o, fact.evidence.first());
        }
        let slot: FactSlot = (
            fact.subject.clone(),
            fact.predicate.clone(),
            fact.object.clone(),
            fact.value.as_ref().map(oo_builder::canonical_json),
            fact.valid_from.clone(),
        );
        match self.facts.get_mut(&slot) {
            Some(have) => {
                if fact.confidence_bp > have.confidence_bp {
                    have.confidence_bp = fact.confidence_bp;
                    have.rule_id = fact.rule_id;
                }
                for ev in fact.evidence {
                    if !have.evidence.contains(&ev) {
                        have.evidence.push(ev);
                    }
                }
                if have.valid_to.is_none() {
                    have.valid_to = fact.valid_to;
                }
            }
            None => {
                self.facts.insert(slot, fact);
            }
        }
    }

    pub fn finish(self, path: &str) -> Extraction {
        let sort = |v: &mut Vec<EvidenceRef>| v.sort();
        let entities = self
            .entities
            .into_iter()
            .map(|(entity, mut evidence)| {
                sort(&mut evidence);
                EntityMention { entity, evidence }
            })
            .collect();
        let aliases = self
            .aliases
            .into_values()
            .map(|mut a| {
                sort(&mut a.evidence);
                a
            })
            .collect();
        let facts = self
            .facts
            .into_values()
            .map(|mut f| {
                sort(&mut f.evidence);
                f
            })
            .collect();
        let identities = self
            .identities
            .into_values()
            .map(|mut i| {
                sort(&mut i.evidence);
                i
            })
            .collect();
        let hints = self
            .hints
            .into_values()
            .map(|mut h| {
                sort(&mut h.evidence);
                h
            })
            .collect();
        Extraction {
            path: path.to_owned(),
            llm: None,
            entities,
            aliases,
            facts,
            identities,
            hints,
        }
    }
}
