//! `VisibleGraph`: the only exit of D26 (10 §7.2). Everything the renderers,
//! the BRIEF builder and the pack writer know about the ontology comes
//! through this type. Its fields are private; the only constructor is
//! [`VisibleGraph::load`], which asks the reader for one scope and then checks
//! the answer again: a fact survives only with at least one evidence in a
//! snapshot file version, and only such evidence is attached; an entity
//! survives only if a surviving fact or alias points at it.
//!
//! Identities in the graph are entity keys and fact keys (content hashes),
//! never database ids, so output cannot depend on row ids or read order:
//! facts, entities, aliases and evidence are all stored sorted.

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::Anchor;
use serde_json::Value;

use super::PackError;
use super::reader::{OntologyReader, Scoped, SnapshotFile, VersionInfo};
use super::source::frag;
use crate::extract::model::{EntityType, Predicate};
use crate::resolve::normalize::norm;

/// Fact and entity counts held back by the scope. Counts only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockedCounts {
    pub facts: u64,
    pub entities: u64,
}

/// One in-scope evidence: where the fact comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleEvidence {
    /// Drive-relative path of the file version (NFC).
    pub path: String,
    pub version_id: String,
    pub block_id: String,
    pub anchor: Anchor,
}

impl VisibleEvidence {
    /// Sort key: `(path, fragment, block id)`.
    pub fn sort_key(&self) -> (&str, String, &str) {
        (&self.path, frag(&self.anchor), &self.block_id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisibleFact {
    pub key: String,
    pub subject: String,
    pub predicate: Predicate,
    pub object: Option<String>,
    pub value: Option<Value>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub origin: String,
    pub evidence: Vec<VisibleEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleEntity {
    pub key: String,
    pub ty: EntityType,
    pub subkind: Option<String>,
    pub thing_label: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleAlias {
    pub alias: String,
    pub norm: String,
    pub evidence: Vec<VisibleEvidence>,
}

/// An open constraint violation, reduced to what the scope may see.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibleConflict {
    pub entity_key: String,
    pub constraint_id: String,
    /// Held-back facts with in-scope evidence, ordered by fact key.
    pub sides: Vec<VisibleFact>,
    /// Sides the scope cannot see (a count, nothing else).
    pub hidden_sides: u32,
}

pub struct VisibleGraph {
    version: VersionInfo,
    scope: Vec<String>,
    snapshot: Vec<SnapshotFile>,
    facts: Vec<VisibleFact>,
    entities: BTreeMap<String, VisibleEntity>,
    aliases: BTreeMap<String, Vec<VisibleAlias>>,
    by_entity: BTreeMap<String, Vec<usize>>,
    blocks: BTreeMap<String, Vec<(u32, String, String)>>,
    blocked: BlockedCounts,
    conflicts: Vec<VisibleConflict>,
}

fn sort_evidence(v: &mut Vec<VisibleEvidence>) {
    v.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    v.dedup();
}

impl VisibleGraph {
    /// The only constructor. Returns the graph and, separately, the blocked
    /// word list for the leak guard (the graph itself never holds it).
    pub fn load(
        reader: &dyn OntologyReader,
        version: &VersionInfo,
        scope: &[String],
    ) -> Result<(Self, Vec<String>), PackError> {
        let scoped = reader.scoped(version, scope)?;
        Ok(Self::from_scoped(version, scope, scoped))
    }

    fn from_scoped(version: &VersionInfo, scope: &[String], s: Scoped) -> (Self, Vec<String>) {
        let Scoped {
            snapshot,
            entities,
            aliases,
            facts,
            blocks,
            blocked,
            conflicts,
        } = s;
        let path_of: BTreeMap<&str, &str> = snapshot
            .iter()
            .map(|f| (f.version_id.as_str(), f.path.as_str()))
            .collect();
        let visible_ev = |evs: &[super::reader::RawEvidence]| -> Vec<VisibleEvidence> {
            let mut out: Vec<VisibleEvidence> = evs
                .iter()
                .filter_map(|e| {
                    path_of.get(e.version_id.as_str()).map(|p| VisibleEvidence {
                        path: (*p).to_owned(),
                        version_id: e.version_id.clone(),
                        block_id: e.block_id.clone(),
                        anchor: e.anchor.clone(),
                    })
                })
                .collect();
            sort_evidence(&mut out);
            out
        };
        let mut ents: BTreeMap<String, VisibleEntity> = entities
            .into_iter()
            .map(|e| {
                (
                    e.key.clone(),
                    VisibleEntity {
                        key: e.key,
                        ty: e.ty,
                        subkind: e.subkind,
                        thing_label: e.thing_label,
                        name: e.name,
                    },
                )
            })
            .collect();
        // D26 on indices: `kernels::visible_decide` (proved as KP5) says which facts, aliases and
        // entities survive; this code only supplies the evidence in scope and builds what survives.
        let eidx: BTreeMap<String, usize> = ents
            .keys()
            .enumerate()
            .map(|(i, k)| (k.clone(), i))
            .collect();
        let slot = |k: &str| eidx.get(k).copied().unwrap_or(usize::MAX);
        let entity_known = vec![true; ents.len()];
        let fact_ev: Vec<Vec<VisibleEvidence>> =
            facts.iter().map(|f| visible_ev(&f.evidence)).collect();
        let alias_ev: Vec<Vec<VisibleEvidence>> =
            aliases.iter().map(|a| visible_ev(&a.evidence)).collect();
        let fact_has: Vec<bool> = fact_ev.iter().map(|e| !e.is_empty()).collect();
        let fact_subject: Vec<usize> = facts.iter().map(|f| slot(&f.subject_key)).collect();
        let fact_object: Vec<Option<usize>> = facts
            .iter()
            .map(|f| f.object_key.as_deref().map(slot))
            .collect();
        let alias_has: Vec<bool> = alias_ev.iter().map(|e| !e.is_empty()).collect();
        let alias_owner: Vec<usize> = aliases.iter().map(|a| slot(&a.entity_key)).collect();
        let mut fact_vis = vec![false; facts.len()];
        let mut alias_vis = vec![false; aliases.len()];
        let mut ent_vis = vec![false; ents.len()];
        super::kernels::visible_decide(
            &fact_has,
            &fact_subject,
            &fact_object,
            &alias_has,
            &alias_owner,
            &entity_known,
            &mut fact_vis,
            &mut alias_vis,
            &mut ent_vis,
        );
        let mut vis_facts: Vec<VisibleFact> = Vec::new();
        let mut seen_facts: BTreeSet<String> = BTreeSet::new();
        for ((f, evidence), keep) in facts.into_iter().zip(fact_ev).zip(fact_vis) {
            if !keep || !seen_facts.insert(f.fact_key.clone()) {
                continue;
            }
            vis_facts.push(VisibleFact {
                key: f.fact_key,
                subject: f.subject_key,
                predicate: f.predicate,
                object: f.object_key,
                value: f.value,
                valid_from: f.valid_from,
                valid_to: f.valid_to,
                origin: f.origin,
                evidence,
            });
        }
        vis_facts.sort_by(|a, b| a.key.cmp(&b.key));
        let mut vis_aliases: BTreeMap<String, Vec<VisibleAlias>> = BTreeMap::new();
        for ((a, evidence), keep) in aliases.into_iter().zip(alias_ev).zip(alias_vis) {
            if !keep {
                continue;
            }
            let slot = vis_aliases.entry(a.entity_key).or_default();
            let n = norm(&a.alias);
            match slot.iter_mut().find(|x| x.norm == n) {
                Some(x) => {
                    x.evidence.extend(evidence);
                    sort_evidence(&mut x.evidence);
                    if a.alias < x.alias {
                        x.alias = a.alias;
                    }
                }
                None => slot.push(VisibleAlias {
                    alias: a.alias,
                    norm: n,
                    evidence,
                }),
            }
        }
        for v in vis_aliases.values_mut() {
            v.sort_by(|a, b| (&a.norm, &a.alias).cmp(&(&b.norm, &b.alias)));
        }
        // Keep only entities something visible points at.
        let keep_entity: BTreeSet<String> = ents
            .keys()
            .zip(ent_vis)
            .filter(|(_, v)| *v)
            .map(|(k, _)| k.clone())
            .collect();
        ents.retain(|k, _| keep_entity.contains(k));
        let mut by_entity: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, f) in vis_facts.iter().enumerate() {
            by_entity.entry(f.subject.clone()).or_default().push(i);
            if let Some(o) = &f.object
                && *o != f.subject
            {
                by_entity.entry(o.clone()).or_default().push(i);
            }
        }
        let blocks = blocks
            .into_iter()
            .filter(|(vid, _)| path_of.contains_key(vid.as_str()))
            .map(|(vid, mut v)| {
                v.sort_by(|a, b| (a.ordinal, &a.block_id).cmp(&(b.ordinal, &b.block_id)));
                (
                    vid,
                    v.into_iter()
                        .map(|b| (b.ordinal, b.block_id, b.text))
                        .collect(),
                )
            })
            .collect();
        let mut vis_conflicts: Vec<VisibleConflict> = Vec::new();
        for c in conflicts {
            if !ents.contains_key(&c.entity_key) {
                continue;
            }
            let mut hidden = c.hidden_sides;
            let mut sides: Vec<VisibleFact> = Vec::new();
            for f in c.sides {
                let evidence = visible_ev(&f.evidence);
                let ends_known = ents.contains_key(&f.subject_key)
                    && f.object_key.as_ref().is_none_or(|o| ents.contains_key(o));
                if evidence.is_empty() || !ends_known {
                    hidden += 1;
                    continue;
                }
                sides.push(VisibleFact {
                    key: f.fact_key,
                    subject: f.subject_key,
                    predicate: f.predicate,
                    object: f.object_key,
                    value: f.value,
                    valid_from: f.valid_from,
                    valid_to: f.valid_to,
                    origin: f.origin,
                    evidence,
                });
            }
            sides.sort_by(|a, b| a.key.cmp(&b.key));
            sides.dedup_by(|a, b| a.key == b.key);
            if sides.is_empty() {
                continue;
            }
            vis_conflicts.push(VisibleConflict {
                entity_key: c.entity_key,
                constraint_id: c.constraint_id,
                sides,
                hidden_sides: hidden,
            });
        }
        vis_conflicts.sort_by(|a, b| {
            (&a.entity_key, &a.constraint_id).cmp(&(&b.entity_key, &b.constraint_id))
        });
        let mut terms = blocked.terms;
        terms.sort();
        terms.dedup();
        (
            Self {
                version: version.clone(),
                scope: scope.to_vec(),
                snapshot,
                facts: vis_facts,
                entities: ents,
                aliases: vis_aliases,
                by_entity,
                blocks,
                blocked: BlockedCounts {
                    facts: blocked.facts,
                    entities: blocked.entities,
                },
                conflicts: vis_conflicts,
            },
            terms,
        )
    }

    /// Open constraint violations on visible entities (10 §16.7).
    pub fn conflicts(&self) -> &[VisibleConflict] {
        &self.conflicts
    }

    pub fn version(&self) -> &VersionInfo {
        &self.version
    }

    pub fn scope(&self) -> &[String] {
        &self.scope
    }

    /// File versions of the snapshot, sorted by `(path, b3)`.
    pub fn snapshot(&self) -> &[SnapshotFile] {
        &self.snapshot
    }

    /// Facts with at least one in-scope evidence, ordered by fact key.
    pub fn facts(&self) -> impl Iterator<Item = &VisibleFact> {
        self.facts.iter()
    }

    pub fn evidence<'a>(&'a self, f: &'a VisibleFact) -> &'a [VisibleEvidence] {
        &f.evidence
    }

    /// Entities with a visible fact or alias, ordered by key.
    pub fn entities(&self) -> impl Iterator<Item = &VisibleEntity> {
        self.entities.values()
    }

    pub fn entity(&self, key: &str) -> Option<&VisibleEntity> {
        self.entities.get(key)
    }

    pub fn aliases(&self, key: &str) -> &[VisibleAlias] {
        self.aliases.get(key).map_or(&[], Vec::as_slice)
    }

    /// Visible facts with this entity as subject or object, ordered by fact key.
    pub fn facts_of(&self, key: &str) -> impl Iterator<Item = &VisibleFact> {
        self.by_entity
            .get(key)
            .into_iter()
            .flatten()
            .map(|i| &self.facts[*i])
    }

    /// Counts only.
    pub fn blocked(&self) -> BlockedCounts {
        self.blocked
    }

    /// The evidence block with up to one block on each side, joined by blank
    /// lines. All blocks come from the same in-scope file version.
    pub fn quote(&self, e: &VisibleEvidence) -> Result<String, PackError> {
        let blocks = self.blocks.get(&e.version_id).ok_or(PackError::Internal(
            "quote: file version has no blocks in scope",
        ))?;
        let i = blocks
            .iter()
            .position(|(_, id, _)| *id == e.block_id)
            .ok_or(PackError::Internal("quote: block not found in scope"))?;
        let from = i.saturating_sub(1);
        let to = (i + 1).min(blocks.len() - 1);
        let parts: Vec<&str> = blocks[from..=to]
            .iter()
            .map(|(_, _, t)| t.as_str())
            .collect();
        Ok(parts.join("\n\n"))
    }

    /// Text of one in-scope block (vector scoring).
    pub(crate) fn block_text(&self, version_id: &str, block_id: &str) -> Option<&str> {
        self.blocks
            .get(version_id)?
            .iter()
            .find(|(_, id, _)| id == block_id)
            .map(|(_, _, t)| t.as_str())
    }

    /// Normalised in-scope block text, for the leak guard's "the scope itself
    /// says this word" check. One string per file version.
    pub(crate) fn scope_texts(&self) -> Vec<String> {
        self.blocks
            .values()
            .map(|v| {
                v.iter()
                    .map(|(_, _, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect()
    }
}
