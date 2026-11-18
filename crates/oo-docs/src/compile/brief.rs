//! BRIEF selection and rendering (D27, 10 §7.3). Deterministic: integers
//! only, explicit sorting, no relative dates, no model. Depends on
//! [`VisibleGraph`] only.
//!
//! Reading choices where the spec is silent (also in the B-S2-08 report):
//! * a section entry is one fact (or, for entities without a qualifying fact,
//!   the entity); degrading the lowest-ranked entry one level per round gives
//!   the order 6, 5, 4, 2, 3 of the spec, and the minimum quotas are realised
//!   by that order rather than by a separate allocation; section 6 is capped
//!   at 10% of the budget before the rounds start;
//! * summary lines of one subject in one section are merged into one line;
//! * `decides` goes to section 3 (to section 5 for a preference), `owns`,
//!   `member_of` and the attributes of a person to section 2; `mentions` and
//!   facts of a Document are never BRIEF lines;
//! * preferences are not limited to the seeds' neighbourhood: every visible
//!   preference is a candidate for section 5;
//! * Document entities are never seeds; with no seed found the central
//!   entities are used (also when `task` resolved to nothing unique);
//! * "content lines" are the lines that start with `- `; they all carry
//!   `[src:..]`. Headings, the task quote and the two counting notes (blocked,
//!   trimmed) are plain lines;
//! * the budget counts the whole file, label and front matter included.

use std::collections::{BTreeMap, BTreeSet};

use super::PackError;
use super::kernels;
use super::render::{
    Cites, entity_evidence, entity_file_name, entity_with_aliases, fact_text, first_src, md_file,
    src_tags,
};
use super::tokens;
use super::visible::{VisibleEntity, VisibleEvidence, VisibleFact, VisibleGraph};
use crate::extract::model::{EntityType, Predicate};
use crate::resolve::normalize::norm;

/// Maximum "可能指的是" lines.
pub const MAX_MAYBE: usize = 5;
/// Most "冲突未决" lines in BRIEF; the rest are in the entity files.
pub const MAX_CONFLICTS: usize = 5;
/// Longest task, in characters.
pub const MAX_TASK_CHARS: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Sec {
    People,
    State,
    Tried,
    Prefs,
    Other,
}

impl Sec {
    const DOC_ORDER: [Sec; 5] = [Sec::People, Sec::State, Sec::Tried, Sec::Prefs, Sec::Other];
    /// Degradation order (10 §7.3 step 6): 6, 5, 4, 2, 3.
    const DEGRADE_ORDER: [Sec; 5] = [Sec::Other, Sec::Prefs, Sec::Tried, Sec::People, Sec::State];

    fn key(self) -> &'static str {
        match self {
            Sec::People => "people",
            Sec::State => "state",
            Sec::Tried => "tried",
            Sec::Prefs => "preferences",
            Sec::Other => "other",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Sec::People => "相关的人",
            Sec::State => "当前状态与决定",
            Sec::Tried => "已试过但走不通",
            Sec::Prefs => "主人的偏好",
            Sec::Other => "其他相关",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Full,
    Summary,
    Title,
    Removed,
}

impl Level {
    fn next(self) -> Level {
        // the step is `kernels::next_level` (proved in KP6); numbers and variants line up
        match kernels::next_level(self as u8) {
            0 => Level::Full,
            1 => Level::Summary,
            2 => Level::Title,
            _ => Level::Removed,
        }
    }
}

/// Counts of entries that were degraded, by section.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrimCounts {
    pub summary: u32,
    pub title: u32,
    pub removed: u32,
}

/// How a fact differs between the version a Share was compiled at and the one a refresh compiles (10 16, B-S10-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    /// A fact the new version has and the old pack did not use.
    Added,
    /// A fact whose value replaced another fact of the same subject and predicate.
    Changed,
    /// A fact the old pack used and the new version retracted.
    Retracted,
}

/// The wording parts of the fact a change replaced or retracted, from the store.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OldFact {
    pub subject: String,
    /// Chinese predicate label, as `predicate_cn` writes it.
    pub predicate: String,
    pub object: Option<String>,
    pub value: Option<String>,
}

/// One change; `new_key` names the visible fact of `Added` and `Changed`, `old` the replaced or retracted one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Change {
    pub kind: ChangeKind,
    pub new_key: Option<String>,
    pub old: Option<OldFact>,
}

/// What a refresh passes to the compiler: the changes since the version the old Share was compiled at.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Changes {
    pub from_version: i64,
    pub changes: Vec<Change>,
}

impl Changes {
    /// Canonical form for the parameter digest.
    pub fn digest_value(&self) -> serde_json::Value {
        // Order of the input does not matter.
        let mut sorted: Vec<&Change> = self.changes.iter().collect();
        sorted.sort();
        sorted.dedup();
        let v: Vec<serde_json::Value> = sorted
            .into_iter()
            .map(|c| {
                serde_json::json!([
                    c.kind as u8,
                    c.new_key,
                    c.old.as_ref().map(|o| serde_json::json!([
                        o.subject,
                        o.predicate,
                        o.object,
                        o.value
                    ]))
                ])
            })
            .collect();
        serde_json::json!({"from": self.from_version, "changes": v})
    }
}

/// Most change lines BRIEF carries; the rest are summarised in one line and are in `UPDATES.md`.
pub const MAX_CHANGE_LINES: usize = 20;

/// One fact line's precision, for `share_fact_use`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactUse {
    pub fact_key: String,
    pub section: &'static str,
    pub precision: &'static str,
}

pub struct BriefParams<'a> {
    pub task: &'a str,
    pub seeds: &'a [String],
    pub budget: u32,
    pub role: &'a str,
    pub coverage: Option<&'a super::coverage::Coverage>,
    /// Refresh only: what changed since the old Share's version (a section after the head).
    pub changes: Option<&'a Changes>,
    /// Rank "other related" with the local vectors of `vector::MODEL_VERSION` (off by default).
    pub vector: bool,
}

pub struct Brief {
    pub text: String,
    pub tokens: u64,
    /// Seed entity keys, sorted.
    pub seeds: Vec<String>,
    pub trimmed: BTreeMap<&'static str, TrimCounts>,
    pub uses: Vec<FactUse>,
    pub cites: Cites,
}

struct Entry {
    sec: Sec,
    fact_key: Option<String>,
    subject: String,
    object: Option<String>,
    subject_name: String,
    file: String,
    seed_hit: bool,
    vec_bp: i32,
    valid_from: Option<String>,
    full: String,
    evs: Vec<VisibleEvidence>,
    entity_facts: usize,
    level: Level,
}

fn type_weight(e: &VisibleEntity) -> u64 {
    match e.ty {
        EntityType::Project => 3,
        EntityType::Decision => 2,
        _ => 1,
    }
}

fn detail_file(e: &VisibleEntity) -> String {
    match e.ty {
        EntityType::Person => format!("people/{}", entity_file_name(e)),
        EntityType::Project => format!("projects/{}", entity_file_name(e)),
        EntityType::Decision => "decisions.md".to_owned(),
        _ => "terms.md".to_owned(),
    }
}

fn is_preference(e: &VisibleEntity) -> bool {
    e.ty == EntityType::Decision && e.subkind.as_deref() == Some("preference")
}

struct Seeds {
    keys: Vec<String>,
    maybe: Vec<String>,
    from_task: bool,
}

fn central(g: &VisibleGraph) -> Vec<String> {
    let mut scored: Vec<(u64, Option<String>, String)> = g
        .entities()
        .filter(|e| e.ty != EntityType::Document)
        .map(|e| {
            let degree = g.facts_of(&e.key).count() as u64;
            let newest = g
                .facts_of(&e.key)
                .filter_map(|f| f.valid_from.clone())
                .max();
            (degree * type_weight(e), newest, e.key.clone())
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    scored.into_iter().take(5).map(|(_, _, k)| k).collect()
}

fn resolve_seeds(g: &VisibleGraph, task: &str, explicit: &[String]) -> Result<Seeds, PackError> {
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for s in explicit {
        if g.entity(s).is_none() {
            return Err(PackError::SeedNotVisible);
        }
        keys.insert(s.clone());
    }
    let mut maybe: BTreeSet<String> = BTreeSet::new();
    let mut from_task = false;
    let task_norm: Vec<char> = norm(task).chars().collect();
    if !task_norm.is_empty() {
        let mut table: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for e in g.entities().filter(|e| e.ty != EntityType::Document) {
            let names = std::iter::once(norm(&e.name))
                .chain(g.aliases(&e.key).iter().map(|a| a.norm.clone()));
            for n in names {
                if n.chars().count() >= 2 {
                    table.entry(n).or_default().insert(e.key.clone());
                }
            }
        }
        let max_len = table.keys().map(|k| k.chars().count()).max().unwrap_or(0);
        let mut i = 0;
        while i < task_norm.len() {
            let longest = (2..=max_len.min(task_norm.len() - i)).rev().find_map(|l| {
                let frag: String = task_norm[i..i + l].iter().collect();
                table.get(&frag).map(|set| (l, set))
            });
            match longest {
                Some((l, set)) => {
                    if set.len() == 1 {
                        keys.extend(set.iter().cloned());
                        from_task = true;
                    } else {
                        maybe.extend(set.iter().cloned());
                    }
                    i += l;
                }
                None => i += 1,
            }
        }
    }
    // A candidate that is also a unique seed is not "maybe".
    maybe.retain(|k| !keys.contains(k));
    let mut keys: Vec<String> = keys.into_iter().collect();
    if keys.is_empty() {
        keys = central(g);
        from_task = false;
    }
    Ok(Seeds {
        keys,
        maybe: maybe.into_iter().take(MAX_MAYBE).collect(),
        from_task,
    })
}

fn rank_key(e: &Entry) -> kernels::RankKey<'_> {
    kernels::RankKey {
        seed_hit: e.seed_hit,
        vec_bp: e.vec_bp,
        valid_from: e.valid_from.as_deref(),
        evs: e.evs.len(),
        subject: &e.subject,
        fact_key: e.fact_key.as_deref(),
    }
}

fn rank_cmp(a: &Entry, b: &Entry) -> std::cmp::Ordering {
    kernels::rank_cmp_keys(&rank_key(a), &rank_key(b))
}

/// Best in-scope block score per entity for the task (empty without a task).
fn vector_scores(g: &VisibleGraph, task: &str) -> crate::vector::Scores {
    let mut out = crate::vector::Scores::new();
    if task.trim().is_empty() {
        return out;
    }
    let q = crate::vector::embed(task);
    for e in g.entities() {
        let best = entity_evidence(g, &e.key)
            .iter()
            .filter_map(|ev| g.block_text(&ev.version_id, &ev.block_id))
            .map(|t| crate::vector::cosine_bp(&q, &crate::vector::embed(t)))
            .max();
        if let Some(bp) = best {
            out.insert(e.key.clone(), bp);
        }
    }
    out
}

fn select(
    g: &VisibleGraph,
    seeds: &BTreeSet<String>,
    scores: Option<&crate::vector::Scores>,
) -> Vec<Entry> {
    let bp_of = |k: &str| scores.and_then(|s| s.get(k)).copied().unwrap_or(0);
    let ty = |k: &str| g.entity(k).map(|e| e.ty);
    let mut chosen: BTreeMap<&str, (Sec, &VisibleFact)> = BTreeMap::new();
    // 1-hop facts around the seeds, then the supersedes chain.
    let hop: Vec<&VisibleFact> = g
        .facts()
        .filter(|f| {
            seeds.contains(&f.subject) || f.object.as_ref().is_some_and(|o| seeds.contains(o))
        })
        .collect();
    let mut reach: BTreeSet<&str> = seeds.iter().map(String::as_str).collect();
    for f in &hop {
        reach.insert(&f.subject);
        if let Some(o) = &f.object {
            reach.insert(o);
        }
    }
    let chain: Vec<&VisibleFact> = g
        .facts()
        .filter(|f| {
            f.predicate == Predicate::Supersedes
                && (reach.contains(f.subject.as_str())
                    || f.object.as_deref().is_some_and(|o| reach.contains(o)))
        })
        .collect();
    let prefs: Vec<&VisibleFact> = g
        .facts()
        .filter(|f| {
            f.predicate == Predicate::Decides
                && f.object
                    .as_deref()
                    .and_then(|o| g.entity(o))
                    .is_some_and(is_preference)
        })
        .collect();
    for f in hop.iter().chain(chain.iter()).chain(prefs.iter()) {
        let obj_pref = f
            .object
            .as_deref()
            .and_then(|o| g.entity(o))
            .is_some_and(is_preference);
        let sec = match (&f.predicate, ty(&f.subject)) {
            (Predicate::Rejects | Predicate::TriedAndFailed, _) => Some(Sec::Tried),
            (Predicate::Supersedes, _) => Some(Sec::State),
            (Predicate::Decides, _) if obj_pref => Some(Sec::Prefs),
            (Predicate::Decides, _) => Some(Sec::State),
            (Predicate::Owns | Predicate::MemberOf, Some(EntityType::Person)) => Some(Sec::People),
            (Predicate::Attr(_), Some(EntityType::Person)) => Some(Sec::People),
            (Predicate::Attr(_), Some(EntityType::Project)) => Some(Sec::State),
            (Predicate::About, Some(EntityType::Decision)) => Some(Sec::State),
            _ => None,
        };
        if let Some(sec) = sec {
            chosen.entry(&f.key).or_insert((sec, f));
        }
    }
    let mut entries: Vec<Entry> = Vec::new();
    for (sec, f) in chosen.values() {
        let Some(subject) = g.entity(&f.subject) else {
            continue;
        };
        entries.push(Entry {
            sec: *sec,
            fact_key: Some(f.key.clone()),
            subject: subject.key.clone(),
            object: f.object.clone(),
            subject_name: subject.name.clone(),
            file: detail_file(subject),
            seed_hit: seeds.contains(&f.subject)
                || f.object.as_ref().is_some_and(|o| seeds.contains(o)),
            vec_bp: 0,
            valid_from: f.valid_from.clone(),
            full: fact_text(g, f, true),
            evs: f.evidence.clone(),
            entity_facts: 1,
            level: Level::Full,
        });
    }
    // Entity-only entries.
    let has_fact_entry = |entries: &[Entry], sec: Sec, key: &str| {
        entries
            .iter()
            .any(|e| e.sec == sec && (e.subject == key || e.object.as_deref() == Some(key)))
    };
    let mut extra: Vec<Entry> = Vec::new();
    let mut add = |sec: Sec, e: &VisibleEntity, seed: bool| {
        let evs = entity_evidence(g, &e.key);
        if evs.is_empty() {
            return false;
        }
        extra.push(Entry {
            sec,
            fact_key: None,
            subject: e.key.clone(),
            object: None,
            subject_name: e.name.clone(),
            file: detail_file(e),
            seed_hit: seed,
            vec_bp: bp_of(&e.key),
            valid_from: g
                .facts_of(&e.key)
                .filter_map(|f| f.valid_from.clone())
                .max(),
            full: entity_with_aliases(g, e),
            evs,
            entity_facts: g.facts_of(&e.key).count(),
            level: Level::Full,
        });
        true
    };
    for k in seeds {
        if let Some(e) = g.entity(k)
            && e.ty == EntityType::Person
            && !has_fact_entry(&entries, Sec::People, k)
        {
            add(Sec::People, e, true);
        }
    }
    for e in g.entities() {
        if is_preference(e) && !has_fact_entry(&entries, Sec::Prefs, &e.key) {
            add(Sec::Prefs, e, false);
        }
    }
    for k in &reach {
        if let Some(e) = g.entity(k)
            && matches!(
                e.ty,
                EntityType::Topic
                    | EntityType::Term
                    | EntityType::Event
                    | EntityType::Organization
                    | EntityType::Thing
            )
        {
            add(Sec::Other, e, seeds.contains(*k));
        }
    }
    if let Some(s) = scores {
        // Vector recall: in-scope entities the walk did not reach, best score first, key breaks ties.
        let mut cand: Vec<(&str, i32)> = s
            .iter()
            .filter(|(k, bp)| **bp >= crate::vector::MIN_BP && !reach.contains(k.as_str()))
            .map(|(k, bp)| (k.as_str(), *bp))
            .collect();
        cand.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let mut taken = 0;
        for (k, _) in cand {
            if taken >= crate::vector::MAX_EXTRA {
                break;
            }
            if let Some(e) = g.entity(k)
                && matches!(
                    e.ty,
                    EntityType::Topic
                        | EntityType::Term
                        | EntityType::Event
                        | EntityType::Organization
                        | EntityType::Thing
                )
                && add(Sec::Other, e, false)
            {
                taken += 1;
            }
        }
    }
    entries.extend(extra);
    entries.sort_by(|a, b| a.sec.cmp(&b.sec).then_with(|| rank_cmp(a, b)));
    entries
}

fn render_section(entries: &[&Entry], cites: &mut Cites) -> Vec<String> {
    let mut lines = Vec::new();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for e in entries {
        match e.level {
            Level::Full => lines.push(format!("- {} {}", e.full, src_tags(&e.evs, cites))),
            Level::Summary | Level::Title => {
                if !done.insert(&e.subject) {
                    continue;
                }
                let group: Vec<&&Entry> = entries
                    .iter()
                    .filter(|x| {
                        x.subject == e.subject && matches!(x.level, Level::Summary | Level::Title)
                    })
                    .collect();
                let first: Option<&VisibleEvidence> = group
                    .iter()
                    .filter_map(|x| x.evs.first())
                    .min_by(|a, b| a.sort_key().cmp(&b.sort_key()));
                let src =
                    first.map_or_else(String::new, |ev| first_src(std::slice::from_ref(ev), cites));
                if group.iter().any(|x| x.level == Level::Summary) {
                    let facts = group.iter().filter(|x| x.fact_key.is_some()).count();
                    let k = if facts > 0 { facts } else { e.entity_facts };
                    lines.push(format!(
                        "- {}：{k} 条相关事实，详见 {} {src}",
                        e.subject_name, e.file
                    ));
                } else {
                    lines.push(format!("- {} {src}", e.subject_name));
                }
            }
            Level::Removed => {}
        }
    }
    lines
}

fn trim_counts(entries: &[Entry]) -> BTreeMap<&'static str, TrimCounts> {
    let mut out: BTreeMap<&'static str, TrimCounts> = BTreeMap::new();
    for e in entries {
        let slot = out.entry(e.sec.key()).or_default();
        match e.level {
            Level::Full => {}
            Level::Summary => slot.summary += 1,
            Level::Title => slot.title += 1,
            Level::Removed => slot.removed += 1,
        }
    }
    out.retain(|_, c| c.summary + c.title + c.removed > 0);
    out
}

struct Frame<'a> {
    g: &'a VisibleGraph,
    basis: &'a str,
    p: &'a BriefParams<'a>,
    seeds: &'a Seeds,
}

impl Frame<'_> {
    /// The lines of the change section in a fixed order (added, changed, retracted; then text, then key), at most
    /// [`MAX_CHANGE_LINES`] and at most a tenth of the budget; what does not fit is counted in one last line. A
    /// change whose entities are not visible in this scope is not shown (it is in `UPDATES.md` for those who may
    /// see it), so a retraction cannot leak a name.
    fn change_lines(&self, cites: &mut Cites) -> Vec<String> {
        let Some(ch) = self.p.changes.filter(|c| !c.changes.is_empty()) else {
            return Vec::new();
        };
        let g = self.g;
        let old_text = |o: &OldFact| -> Option<String> {
            let name = g.entity(&o.subject)?.name.clone();
            let object = match (&o.object, &o.value) {
                (Some(k), _) => g.entity(k)?.name.clone(),
                (None, Some(v)) => v.clone(),
                (None, None) => String::new(),
            };
            Some(format!("{name}：{} {object}", o.predicate))
        };
        let mut rows: Vec<(u8, String, String, String)> = Vec::new();
        for c in &ch.changes {
            let new_fact = c
                .new_key
                .as_ref()
                .and_then(|k| g.facts().find(|f| &f.key == k));
            let line = match (c.kind, new_fact, c.old.as_ref().and_then(old_text)) {
                (ChangeKind::Added, Some(f), _) => {
                    let src = first_src(&f.evidence, cites);
                    Some((
                        format!("新增：{}", fact_text(g, f, false)),
                        format!("- 新增：{} {src}", fact_text(g, f, false)),
                    ))
                }
                (ChangeKind::Changed, Some(f), Some(old)) => {
                    let src = first_src(&f.evidence, cites);
                    Some((
                        format!("改值：{old} → {}", fact_text(g, f, false)),
                        format!("- 改值：{old} → {} {src}", fact_text(g, f, false)),
                    ))
                }
                (ChangeKind::Retracted, _, Some(old)) => {
                    Some((format!("撤回：{old}"), format!("> 撤回：{old}")))
                }
                _ => None,
            };
            if let Some((text, line)) = line {
                let key = c.new_key.clone().unwrap_or_default();
                rows.push((c.kind as u8, text, key, line));
            }
        }
        rows.sort();
        rows.dedup();
        let cap = u64::from(self.p.budget) / 10;
        let mut out: Vec<String> = Vec::new();
        let mut spent = 0u64;
        let total = rows.len();
        for (i, (_, _, _, line)) in rows.into_iter().enumerate() {
            let t = tokens::count(&line);
            if out.len() >= MAX_CHANGE_LINES || spent + t > cap {
                out.push(format!("> 另有 {} 条变化见 UPDATES.md", total - i));
                break;
            }
            spent += t;
            out.push(line);
        }
        out
    }

    fn compose(&self, entries: &[Entry], used: u64, cites: &mut Cites) -> String {
        let v = self.g.version();
        let scope = if self.g.scope().is_empty() {
            "整个 Drive".to_owned()
        } else {
            self.g.scope().join("、")
        };
        let mut body: Vec<String> = vec![
            "# BRIEF".into(),
            format!(
                "> 本体版本 v{}（{}）· 范围 {} · 预算 {}/{} tokens · 角色 {}",
                v.n, v.as_of, scope, used, self.p.budget, self.p.role
            ),
            "> 每行末尾的 [src:路径#位置] 是出处；原文片段见 refs/，清单见 INDEX.md。".into(),
        ];
        if let Some(c) = self.p.coverage {
            body.push(format!("> {}", c.brief_line()));
        }
        body.push(format!(
            "> {}",
            super::coverage::Guarantees::compute(self.p.coverage, !self.g.conflicts().is_empty())
                .brief_line()
        ));
        let change_lines = self.change_lines(cites);
        if !change_lines.is_empty() {
            let from = self.p.changes.map_or(0, |c| c.from_version);
            body.extend([
                String::new(),
                format!("## 自上次以来的变化（v{from} → v{}）", v.n),
            ]);
            body.extend(change_lines);
        }
        body.extend([String::new(), "## 你在做什么".into()]);
        if self.p.task.trim().is_empty() {
            body.push("> （未指定任务）".into());
        } else {
            for l in self.p.task.lines() {
                body.push(format!("> {l}"));
            }
        }
        let label = if self.seeds.from_task {
            "解析出的对象"
        } else {
            "选定的中心对象"
        };
        for k in &self.seeds.keys {
            if let Some(e) = self.g.entity(k) {
                let evs = entity_evidence(self.g, k);
                body.push(format!(
                    "- {label}：{}（{}） {}",
                    e.name,
                    e.ty.as_str(),
                    first_src(&evs, cites)
                ));
            }
        }
        for k in &self.seeds.maybe {
            if let Some(e) = self.g.entity(k) {
                let evs = entity_evidence(self.g, k);
                body.push(format!(
                    "- 可能指的是：{}（{}） {}",
                    e.name,
                    e.ty.as_str(),
                    first_src(&evs, cites)
                ));
            }
        }
        for sec in Sec::DOC_ORDER {
            body.push(String::new());
            body.push(format!("## {}", sec.title()));
            let items: Vec<&Entry> = entries.iter().filter(|e| e.sec == sec).collect();
            let mut lines = Vec::new();
            if sec == Sec::State {
                let all: Vec<_> = self.g.conflicts().iter().take(MAX_CONFLICTS).collect();
                lines.extend(super::render::conflict_lines(self.g, &all, cites));
            }
            lines.extend(render_section(&items, cites));
            if lines.is_empty() {
                body.push("（无）".into());
            }
            body.extend(lines);
        }
        let b = self.g.blocked();
        body.push(String::new());
        body.push("## 已拦截".into());
        body.push(format!(
            "另有 {} 条事实、{} 个实体因不在共享范围内未纳入。",
            b.facts, b.entities
        ));
        body.push(String::new());
        body.push("## 裁掉了什么".into());
        let trimmed = trim_counts(entries);
        if trimmed.is_empty() {
            body.push("没有内容被裁掉。".into());
        } else {
            for sec in Sec::DOC_ORDER {
                if let Some(c) = trimmed.get(sec.key()) {
                    body.push(format!(
                        "章节「{}」：降为摘要 {} 条，降为标题 {} 条，移除 {} 条。",
                        sec.title(),
                        c.summary,
                        c.title,
                        c.removed
                    ));
                }
            }
            body.push(
                "被降级或移除的条目在 people/、projects/、decisions.md、terms.md 中仍保留全文。"
                    .into(),
            );
        }
        md_file(self.basis, "BRIEF.md", &body)
    }

    fn tokens_at(&self, entries: &[Entry], used: u64) -> u64 {
        tokens::count(&self.compose(entries, used, &mut Cites::default()))
    }
}

fn lowest_degradable(entries: &[Entry], sec: Sec) -> Option<usize> {
    kernels::lowest_degradable_by(
        entries.len(),
        |i| (entries[i].sec as u8, entries[i].level as u8),
        sec as u8,
    )
}

/// Build BRIEF.md for `g`.
pub fn build(g: &VisibleGraph, basis: &str, p: &BriefParams<'_>) -> Result<Brief, PackError> {
    let budget = u64::from(p.budget);
    if p.task.chars().count() > MAX_TASK_CHARS || tokens::count(p.task) > budget * 40 / 100 {
        return Err(PackError::TaskTooLong {
            max_chars: MAX_TASK_CHARS,
        });
    }
    let seeds = resolve_seeds(g, p.task, p.seeds)?;
    let seed_set: BTreeSet<String> = seeds.keys.iter().cloned().collect();
    let scores = p.vector.then(|| vector_scores(g, p.task));
    let mut entries = select(g, &seed_set, scores.as_ref());
    let frame = Frame {
        g,
        basis,
        p,
        seeds: &seeds,
    };

    // Section 6 is capped at 10% of the budget (D16).
    let cap = budget / 10;
    while let Some(i) = lowest_degradable(&entries, Sec::Other) {
        let other_tokens: u64 = {
            let items: Vec<&Entry> = entries.iter().filter(|e| e.sec == Sec::Other).collect();
            let lines = render_section(&items, &mut Cites::default());
            lines.iter().map(|l| tokens::count(l)).sum()
        };
        if other_tokens <= cap {
            break;
        }
        entries[i].level = entries[i].level.next();
    }

    let mut total = frame.tokens_at(&entries, budget);
    'rounds: while total > budget {
        let mut progressed = false;
        for sec in Sec::DEGRADE_ORDER {
            if let Some(i) = lowest_degradable(&entries, sec) {
                entries[i].level = entries[i].level.next();
                progressed = true;
                total = frame.tokens_at(&entries, budget);
                if total <= budget {
                    break 'rounds;
                }
            }
        }
        if !progressed {
            break;
        }
    }
    if total > budget {
        return Err(PackError::BudgetTooSmall { min_budget: total });
    }

    // Final render with the exact figure in the header (fixed point; the
    // header number is at most four digits, so one token either way, and the
    // loop settles at once).
    let mut used = frame.tokens_at(&entries, budget);
    let mut cites = Cites::default();
    let mut text = frame.compose(&entries, used, &mut cites);
    for _ in 0..8 {
        let t = tokens::count(&text);
        if t == used {
            break;
        }
        used = t;
        cites = Cites::default();
        text = frame.compose(&entries, used, &mut cites);
    }
    let final_tokens = tokens::count(&text);
    if final_tokens > budget {
        return Err(PackError::BudgetTooSmall {
            min_budget: final_tokens,
        });
    }

    let mut uses = Vec::new();
    for e in &entries {
        if let Some(k) = &e.fact_key {
            let precision = match e.level {
                Level::Full => "full",
                Level::Summary => "summary",
                Level::Title => "title",
                Level::Removed => continue,
            };
            uses.push(FactUse {
                fact_key: k.clone(),
                section: e.sec.key(),
                precision,
            });
        }
    }
    uses.sort_by(|a, b| a.fact_key.cmp(&b.fact_key));
    Ok(Brief {
        text,
        tokens: final_tokens,
        seeds: seeds.keys.clone(),
        trimmed: trim_counts(&entries),
        uses,
        cites,
    })
}

#[cfg(test)]
mod kernel_tests {
    use super::*;

    #[test]
    fn degradation_order_and_level_steps_are_the_kernels() {
        assert_eq!(
            Sec::DEGRADE_ORDER.map(|s| s as u8),
            kernels::DEGRADE_ORDER,
            "the order Kani proves is the order BRIEF uses"
        );
        for l in [Level::Full, Level::Summary, Level::Title, Level::Removed] {
            assert_eq!(l.next() as u8, kernels::next_level(l as u8));
        }
        assert_eq!(Level::Removed as u8, kernels::REMOVED);
    }
}
