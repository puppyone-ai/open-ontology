//! Structure induction: files of one record class share fields, and a field that recurs is a property (D93). Pure.

use std::collections::{BTreeMap, BTreeSet};

use super::observe::{CONCEPTS, FieldObs, FileObs, dir_levels, slug_of};
use super::vtype::{Lexicon, infer, items_of};
use super::{
    ClassStructure, FieldSource, MIN_COVERAGE, MIN_FIELD_FILES, MIN_FILES, MIN_ROWS, Property,
    REQUIRED_COVERAGE, Relation, Shape, Status, ValueType,
};
use crate::resolve::normalize::norm;

/// Jaccard similarity of two key sets that puts a file in a cluster.
const JOIN: f64 = 0.5;
/// Share of the items of a property that name records of another class before it is a relation.
const RELATION_MIN: f64 = 0.6;
/// Share of the files from which a recurring field is a candidate property of a class.
const CANDIDATE_COVERAGE: f64 = 0.1;
const EXAMPLES: usize = 4;
/// Distinct values kept per property, for resolving it against the database side.
const SAMPLE: usize = 40;

/// One file: its path and what it carries.
pub type FileRow<'a> = (&'a str, &'a FileObs);

/// The result of one induction.
#[derive(Clone, Debug, Default)]
pub struct Induced {
    pub classes: Vec<ClassStructure>,
    /// Record files that fit no class (too few like them, or no fields).
    pub unclustered: Vec<String>,
    /// File path to class key.
    pub membership: BTreeMap<String, String>,
    /// Fields that recur in at least [`super::PROPOSE_FILES`] files of a class without reaching its coverage threshold: what a
    /// confirmed class may propose as a new property (type promotion).
    pub candidates: BTreeMap<String, Vec<Property>>,
}

fn keyset(o: &FileObs) -> BTreeSet<String> {
    o.fields
        .iter()
        .filter(|f| f.source != FieldSource::Directory)
        .map(|f| f.key.clone())
        .collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let i = a.intersection(b).count();
    let u = a.union(b).count();
    if u == 0 { 0.0 } else { i as f64 / u as f64 }
}

struct Cluster {
    members: Vec<usize>,
    counts: BTreeMap<String, usize>,
}

impl Cluster {
    fn centroid(&self) -> BTreeSet<String> {
        let half = (self.members.len() as f64 * 0.5).ceil() as usize;
        self.counts
            .iter()
            .filter(|(_, n)| **n >= half.max(1))
            .map(|(k, _)| k.clone())
            .collect()
    }
}

fn cluster(shape: Shape, rows: &[FileRow<'_>]) -> Vec<Vec<usize>> {
    let idx: Vec<usize> = (0..rows.len())
        .filter(|i| rows[*i].1.shape == shape)
        .collect();
    // Keys that recur are the signal; a field seen in one file only is noise.
    let mut global: BTreeMap<String, usize> = BTreeMap::new();
    let sets: BTreeMap<usize, BTreeSet<String>> =
        idx.iter().map(|i| (*i, keyset(rows[*i].1))).collect();
    for s in sets.values() {
        for k in s {
            *global.entry(k.clone()).or_default() += 1;
        }
    }
    let recurring: BTreeMap<usize, BTreeSet<String>> = sets
        .iter()
        .map(|(i, s)| (*i, s.iter().filter(|k| global[*k] >= 2).cloned().collect()))
        .collect();
    let mut order: Vec<usize> = idx
        .into_iter()
        .filter(|i| !recurring[i].is_empty())
        .collect();
    order.sort_by(|a, b| rows[*a].0.cmp(rows[*b].0));
    let add = |c: &mut Cluster, i: usize| {
        c.members.push(i);
        for k in &recurring[&i] {
            *c.counts.entry(k.clone()).or_default() += 1;
        }
    };
    let best_of = |ks: &BTreeSet<String>, centroids: &[BTreeSet<String>]| -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (c, cen) in centroids.iter().enumerate() {
            let j = jaccard(ks, cen);
            if j >= JOIN && best.is_none_or(|(_, b)| j > b) {
                best = Some((c, j));
            }
        }
        best.map(|(c, _)| c)
    };
    // Pass 1: greedy, the centroid follows the members.
    let mut clusters: Vec<Cluster> = Vec::new();
    for i in &order {
        let cents: Vec<BTreeSet<String>> = clusters.iter().map(Cluster::centroid).collect();
        match best_of(&recurring[i], &cents) {
            Some(c) => add(&mut clusters[c], *i),
            None => {
                let mut c = Cluster {
                    members: Vec::new(),
                    counts: BTreeMap::new(),
                };
                add(&mut c, *i);
                clusters.push(c);
            }
        }
    }
    // Pass 2: with the settled centroids, every file picks its best class again.
    let cents: Vec<BTreeSet<String>> = clusters.iter().map(Cluster::centroid).collect();
    let mut second: Vec<Cluster> = cents
        .iter()
        .map(|_| Cluster {
            members: Vec::new(),
            counts: BTreeMap::new(),
        })
        .collect();
    let mut extra: Vec<Cluster> = Vec::new();
    for i in &order {
        match best_of(&recurring[i], &cents) {
            Some(c) => add(&mut second[c], *i),
            None => {
                let ex: Vec<BTreeSet<String>> = extra.iter().map(Cluster::centroid).collect();
                match best_of(&recurring[i], &ex) {
                    Some(c) => add(&mut extra[c], *i),
                    None => {
                        let mut c = Cluster {
                            members: Vec::new(),
                            counts: BTreeMap::new(),
                        };
                        add(&mut c, *i);
                        extra.push(c);
                    }
                }
            }
        }
    }
    second
        .into_iter()
        .chain(extra)
        .filter(|c| !c.members.is_empty())
        .map(|c| c.members)
        .collect()
}

/// The kind of a class from the concepts its properties name.
pub fn detect_kind(shape: Shape, keys: &BTreeSet<String>) -> &'static str {
    let has = |k: &str| keys.contains(k);
    let any = |ks: &[&str]| ks.iter().any(|k| keys.contains(*k));
    match shape {
        Shape::Table => {
            if has("name")
                && any(&[
                    "role",
                    "email",
                    "team",
                    "department",
                    "employee_id",
                    "manager",
                ])
                && !any(&["budget", "deadline", "status"])
            {
                "people_table"
            } else if any(&["name", "project"])
                && has("owner")
                && any(&["status", "budget", "deadline", "priority"])
            {
                "project_table"
            } else if any(&["party_a", "party_b", "contract_no"])
                && any(&["amount", "term", "start_date"])
            {
                "contract"
            } else {
                "other"
            }
        }
        Shape::Document => {
            if any(&["party_a", "party_b"])
                && any(&["amount", "term", "contract_no", "start_date", "end_date"])
            {
                "contract"
            } else if has("attendees") && any(&["decisions", "actions", "agenda", "summary"]) {
                "meeting_minutes"
            } else if has("decisions")
                && any(&["decided_by", "rationale", "alternatives", "status"])
            {
                "decision_log"
            } else if any(&["this_week", "next_week"])
                && any(&["next_week", "risks", "this_week", "week"])
            {
                "weekly_report"
            } else {
                "other"
            }
        }
    }
}

fn kind_label(kind: &str) -> &str {
    kind
}

fn most_common<'a>(it: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut m: BTreeMap<&str, usize> = BTreeMap::new();
    for s in it {
        *m.entry(s).or_default() += 1;
    }
    m.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
        .map(|(s, _)| s.to_owned())
}

/// The column that names the records of a table class.
fn identity_key(kind: &str) -> &'static str {
    if kind == "project_table" {
        "project"
    } else {
        "name"
    }
}

fn relation_predicate(vt: ValueType) -> &'static str {
    match vt {
        ValueType::Person => "mentions",
        _ => "about",
    }
}

struct Agg<'a> {
    files_with: usize,
    labels: Vec<&'a str>,
    sources: Vec<FieldSource>,
    values: Vec<String>,
    max_per_file: usize,
    nonempty_files: usize,
}

fn aggregate<'a>(cl: &[usize], rows: &[FileRow<'a>]) -> BTreeMap<String, Agg<'a>> {
    let mut out: BTreeMap<String, Agg<'a>> = BTreeMap::new();
    for i in cl {
        for f in &rows[*i].1.fields {
            if f.source == FieldSource::Directory {
                continue;
            }
            let a = out.entry(f.key.clone()).or_insert(Agg {
                files_with: 0,
                labels: Vec::new(),
                sources: Vec::new(),
                values: Vec::new(),
                max_per_file: 0,
                nonempty_files: 0,
            });
            a.files_with += 1;
            a.labels.push(f.label.as_str());
            a.sources.push(f.source);
            a.max_per_file = a.max_per_file.max(f.values.len());
            if !f.values.is_empty() {
                a.nonempty_files += 1;
            }
            a.values.extend(f.values.iter().cloned());
        }
    }
    out
}

fn build_property(
    key: &str,
    a: &Agg<'_>,
    nfiles: usize,
    lexicon: &Lexicon,
    force: Option<ValueType>,
) -> Property {
    let source_of = {
        let mut m: BTreeMap<FieldSource, usize> = BTreeMap::new();
        for s in &a.sources {
            *m.entry(*s).or_default() += 1;
        }
        m.into_iter()
            .max_by_key(|(_, n)| *n)
            .map_or(FieldSource::Line, |(s, _)| s)
    };
    let inf = infer(key, &a.values, lexicon, source_of != FieldSource::Section);
    let vt = force.unwrap_or(inf.value_type);
    let source = source_of;
    let coverage = a.files_with as f64 / nfiles as f64;
    let mut ex: Vec<String> = Vec::new();
    for v in &a.values {
        let v = v.trim();
        if !v.is_empty() && !ex.iter().any(|e| e == v) {
            ex.push(v.chars().take(60).collect());
        }
        if ex.len() >= EXAMPLES {
            break;
        }
    }
    let label = most_common(a.labels.iter().copied()).unwrap_or_else(|| key.to_owned());
    let name = label.trim().to_owned();
    let multi = inf.multi
        || (a.max_per_file > 1 && matches!(source, FieldSource::Section | FieldSource::Line));
    let enum_values = if vt == ValueType::Enum {
        inf.enum_values
    } else {
        Vec::new()
    };
    let relation = vt.entity_type().map(|t| Relation {
        target: t.to_owned(),
        target_kind: "entity_type".into(),
        resolved: (inf.resolved * 100.0).round() / 100.0,
        predicate: relation_predicate(vt).into(),
    });
    let mut sample: Vec<String> = Vec::new();
    for v in &a.values {
        for i in if vt.entity_type().is_some() {
            items_of(v)
        } else {
            vec![v.trim().to_owned()]
        } {
            if !i.is_empty() && !sample.contains(&i) && sample.len() < SAMPLE {
                sample.push(i.chars().take(80).collect());
            }
        }
    }
    let rows_total = a.values.len().max(1);
    Property {
        slug: slug_of(key),
        name,
        keys: vec![key.to_owned()],
        source,
        value_type: vt,
        multi,
        files_with: a.files_with as u32,
        coverage: (coverage * 1000.0).round() / 1000.0,
        fill: ((a.values.iter().filter(|v| !v.trim().is_empty()).count() as f64
            / rows_total as f64)
            * 1000.0)
            .round()
            / 1000.0,
        required: coverage >= REQUIRED_COVERAGE,
        examples: ex,
        sample_values: sample,
        enum_values,
        relation,
        status: Status::Proposed,
        edited: false,
    }
}

fn dominant_dir(paths: &[&str]) -> Option<String> {
    most_common(paths.iter().map(|p| {
        let parts: Vec<&str> = p.split('/').collect();
        if parts.len() >= 2 {
            parts[parts.len() - 2]
        } else {
            ""
        }
    }))
    .filter(|d| !d.is_empty())
}

fn ascii_slug(s: &str) -> String {
    let mut out: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    out = out
        .split('_')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if out.is_empty() || !out.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        return String::new();
    }
    out.chars().take(40).collect()
}

/// Induce the classes of a set of record files.
pub fn induce(rows: &[FileRow<'_>], lexicon: &Lexicon) -> Induced {
    let mut result = Induced::default();
    let mut used_keys: BTreeSet<String> = BTreeSet::new();
    let mut clustered: BTreeSet<usize> = BTreeSet::new();
    // (class, identity items) pairs for relation resolution across classes.
    let mut identities: Vec<(String, ValueType, BTreeSet<String>)> = Vec::new();

    for shape in [Shape::Table, Shape::Document] {
        let min_files = if shape == Shape::Table { 1 } else { MIN_FILES };
        for cl in cluster(shape, rows) {
            let total_rows: usize = cl.iter().map(|i| rows[*i].1.rows as usize).sum();
            if cl.len() < min_files || (shape == Shape::Table && total_rows < MIN_ROWS) {
                continue;
            }
            let nfiles = cl.len();
            let agg = aggregate(&cl, rows);
            let keys: BTreeSet<String> = agg
                .iter()
                .filter(|(_, a)| a.files_with as f64 / nfiles as f64 >= MIN_COVERAGE)
                .map(|(k, _)| k.clone())
                .collect();
            let kind = detect_kind(shape, &keys);
            let force_person = |k: &str| kind == "people_table" && k == "name";
            let force_project = |k: &str| kind == "project_table" && k == identity_key(kind);
            let mut props: Vec<Property> = agg
                .iter()
                .filter(|(_, a)| {
                    a.files_with as f64 / nfiles as f64 >= MIN_COVERAGE
                        && a.files_with >= MIN_FIELD_FILES.min(nfiles)
                })
                .map(|(k, a)| {
                    build_property(
                        k,
                        a,
                        nfiles,
                        lexicon,
                        force_person(k)
                            .then_some(ValueType::Person)
                            .or_else(|| force_project(k).then_some(ValueType::Project)),
                    )
                })
                .collect();
            // A varying directory level is a property of its own (directory conventions).
            if shape == Shape::Document {
                let dirs: Vec<(String, String)> =
                    cl.iter().map(|i| dir_levels(rows[*i].0)).collect();
                let parents: BTreeSet<&str> = dirs.iter().map(|d| d.0.as_str()).collect();
                let grand = most_common(dirs.iter().map(|d| d.1.as_str()));
                let grand_share = grand
                    .as_deref()
                    .map_or(0, |g| dirs.iter().filter(|d| d.1 == g).count());
                if parents.len() >= 2
                    && parents.len() * 2 <= nfiles
                    && grand_share * 5 >= nfiles * 4
                    && !parents.contains("")
                {
                    let a = Agg {
                        files_with: nfiles,
                        labels: vec!["directory"],
                        sources: vec![FieldSource::Directory],
                        values: dirs.iter().map(|d| d.0.clone()).collect(),
                        max_per_file: 1,
                        nonempty_files: nfiles,
                    };
                    let mut p = build_property("directory", &a, nfiles, lexicon, None);
                    p.source = FieldSource::Directory;
                    p.slug = "directory".into();
                    props.push(p);
                }
            }
            if props.is_empty() {
                continue;
            }
            let own: BTreeSet<String> = props.iter().flat_map(|p| p.keys.clone()).collect();
            let extra: Vec<Property> = agg
                .iter()
                .filter(|(k, a)| {
                    !own.contains(*k)
                        && a.files_with >= super::PROPOSE_FILES
                        && a.files_with as f64 / nfiles as f64 >= CANDIDATE_COVERAGE
                })
                .map(|(k, a)| build_property(k, a, nfiles, lexicon, None))
                .collect();
            props.sort_by(|a, b| {
                b.coverage
                    .partial_cmp(&a.coverage)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.slug.cmp(&b.slug))
            });
            let paths: Vec<&str> = cl.iter().map(|i| rows[*i].0).collect();
            let base = if kind != "other" {
                kind.to_owned()
            } else {
                dominant_dir(&paths)
                    .map(|d| ascii_slug(&d))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "records".to_owned())
            };
            let mut class_key = base.clone();
            let mut n = 2;
            while used_keys.contains(&class_key) {
                class_key = format!("{base}_{n}");
                n += 1;
            }
            used_keys.insert(class_key.clone());
            let label = if kind == "other" {
                dominant_dir(&paths).unwrap_or_else(|| class_key.clone())
            } else {
                kind_label(kind).to_owned()
            };
            let signature: Vec<String> = props
                .iter()
                .filter(|p| p.source != FieldSource::Directory)
                .flat_map(|p| p.keys.clone())
                .collect();
            let mut sample: Vec<String> = paths.iter().map(|s| (*s).to_owned()).collect();
            sample.sort();
            sample.truncate(5);
            if shape == Shape::Table {
                let ident = match kind {
                    "people_table" => Some(ValueType::Person),
                    "project_table" => Some(ValueType::Project),
                    "contract" => None,
                    _ => None,
                };
                if let Some(vt) = ident {
                    let mut set = BTreeSet::new();
                    for i in &cl {
                        for f in rows[*i]
                            .1
                            .fields
                            .iter()
                            .filter(|f| f.key == identity_key(kind))
                        {
                            for v in &f.values {
                                set.insert(norm(v));
                            }
                        }
                    }
                    identities.push((class_key.clone(), vt, set));
                }
            }
            for i in &cl {
                result
                    .membership
                    .insert(rows[*i].0.to_owned(), class_key.clone());
                clustered.insert(*i);
            }
            if !extra.is_empty() {
                result.candidates.insert(class_key.clone(), extra);
            }
            result.classes.push(ClassStructure {
                class_key,
                label,
                kind: kind.to_owned(),
                shape,
                status: Status::Proposed,
                version: 1,
                files: nfiles as u32,
                rows: total_rows as u32,
                properties: props,
                sample_paths: sample,
                signature,
            });
        }
    }
    // Relations between classes: a value that names a record of another class (the way a foreign key names a row).
    let value_sets: Vec<(usize, usize, Vec<String>)> = result
        .classes
        .iter()
        .enumerate()
        .flat_map(|(ci, c)| {
            c.properties
                .iter()
                .enumerate()
                .map(move |(pi, _)| (ci, pi))
                .collect::<Vec<_>>()
        })
        .map(|(ci, pi)| {
            let key = result.classes[ci].properties[pi].keys[0].clone();
            if result.classes[ci].properties[pi].source == FieldSource::Directory {
                return (
                    ci,
                    pi,
                    result.classes[ci].properties[pi].sample_values.clone(),
                );
            }
            let members: BTreeSet<&str> = result
                .membership
                .iter()
                .filter(|(_, k)| **k == result.classes[ci].class_key)
                .map(|(p, _)| p.as_str())
                .collect();
            let mut vals = Vec::new();
            for (p, o) in rows {
                if members.contains(p) {
                    for f in o.fields.iter().filter(|f| f.key == key) {
                        for v in &f.values {
                            vals.extend(items_of(v));
                        }
                    }
                }
            }
            (ci, pi, vals)
        })
        .collect();
    for (ci, pi, vals) in value_sets {
        if vals.is_empty() {
            continue;
        }
        let own = result.classes[ci].class_key.clone();
        let is_identity_col = result.classes[ci].shape == Shape::Table
            && result.classes[ci].properties[pi].keys[0] == identity_key(&result.classes[ci].kind);
        if is_identity_col {
            continue;
        }
        let mut best: Option<(&String, ValueType, f64)> = None;
        for (k, vt, set) in &identities {
            if *k == own {
                continue;
            }
            let r =
                vals.iter().filter(|v| set.contains(&norm(v))).count() as f64 / vals.len() as f64;
            if r >= RELATION_MIN && best.is_none_or(|(_, _, b)| r > b) {
                best = Some((k, *vt, r));
            }
        }
        if let Some((k, vt, r)) = best {
            let p = &mut result.classes[ci].properties[pi];
            if matches!(
                p.value_type,
                ValueType::Text
                    | ValueType::Enum
                    | ValueType::Id
                    | ValueType::Person
                    | ValueType::Project
                    | ValueType::Organization
            ) {
                p.value_type = vt;
                p.enum_values.clear();
                p.relation = Some(Relation {
                    target: k.clone(),
                    target_kind: "class".into(),
                    resolved: (r * 100.0).round() / 100.0,
                    predicate: relation_predicate(vt).into(),
                });
            }
        }
    }
    for (i, (p, _)) in rows.iter().enumerate() {
        if !clustered.contains(&i) {
            result.unclustered.push((*p).to_owned());
        }
    }
    result.unclustered.sort();
    result
}

/// The class a file belongs to: the class whose signature it matches best (Jaccard of the keys of its fields).
pub fn assign<'a>(obs: &FileObs, classes: &'a [ClassStructure]) -> Option<&'a ClassStructure> {
    let ks = keyset(obs);
    let mut best: Option<(&ClassStructure, f64)> = None;
    for c in classes
        .iter()
        .filter(|c| c.shape == obs.shape && c.status != Status::Rejected)
    {
        let mut sig: BTreeSet<String> = BTreeSet::new();
        for p in c
            .live_properties()
            .filter(|p| p.source != FieldSource::Directory)
        {
            sig.extend(p.keys.iter().cloned());
        }
        if sig.is_empty() {
            continue;
        }
        let j = jaccard(&ks, &sig);
        if j >= JOIN && best.is_none_or(|(_, b)| j > b) {
            best = Some((c, j));
        }
    }
    best.map(|(c, _)| c)
}

/// All concept slugs, for the UI and the tests.
pub fn concept_slugs() -> Vec<&'static str> {
    CONCEPTS.iter().map(|(s, _)| *s).collect()
}

/// Fields of `obs` that carry a given property's keys.
pub fn fields_of<'a>(obs: &'a FileObs, prop: &Property) -> Vec<&'a FieldObs> {
    obs.fields
        .iter()
        .filter(|f| prop.keys.contains(&f.key))
        .collect()
}

/// A property that appeared in a confirmed class and was not there when it was confirmed.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub class_key: String,
    pub slug: String,
    pub name: String,
    pub files: u32,
    pub examples: Vec<String>,
}

fn class_sig(c: &ClassStructure) -> BTreeSet<String> {
    c.properties
        .iter()
        .filter(|p| p.source != FieldSource::Directory)
        .flat_map(|p| p.keys.iter().cloned())
        .collect()
}

/// Carry a person's decisions over to a fresh induction: the stored classes keep their key, label, status and the
/// properties their names, status, merged keys and (when confirmed) type. Properties that appear in a confirmed class
/// are proposals (type promotion).
pub fn reconcile(
    old: &[ClassStructure],
    fresh: Vec<ClassStructure>,
    candidates: &BTreeMap<String, Vec<Property>>,
) -> (Vec<ClassStructure>, Vec<Proposal>) {
    let mut out: Vec<ClassStructure> = Vec::new();
    let mut proposals: Vec<Proposal> = Vec::new();
    let mut taken: BTreeSet<usize> = BTreeSet::new();
    for mut n in fresh {
        let nsig = class_sig(&n);
        let mut best: Option<(usize, f64)> = None;
        for (i, o) in old.iter().enumerate() {
            if taken.contains(&i) || o.shape != n.shape {
                continue;
            }
            let j = if o.class_key == n.class_key {
                1.0
            } else {
                jaccard(&class_sig(o), &nsig)
            };
            if (o.class_key == n.class_key || j >= 0.6) && best.is_none_or(|(_, b)| j > b) {
                best = Some((i, j));
            }
        }
        let Some((i, _)) = best else {
            out.push(n);
            continue;
        };
        taken.insert(i);
        let o = &old[i];
        let renamed_key = o.class_key != n.class_key;
        n.class_key = o.class_key.clone();
        n.label = o.label.clone();
        n.status = o.status;
        n.version = o.version;
        let mut props: Vec<Property> = Vec::new();
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for mut np in n.properties {
            let hit =
                o.properties.iter().enumerate().find(|(_, op)| {
                    op.slug == np.slug || op.keys.iter().any(|k| np.keys.contains(k))
                });
            match hit {
                Some((pi, op)) => {
                    used.insert(pi);
                    let mut keys = op.keys.clone();
                    for k in &np.keys {
                        if !keys.contains(k) {
                            keys.push(k.clone());
                        }
                    }
                    np.slug = op.slug.clone();
                    np.keys = keys;
                    np.status = op.status;
                    np.edited = op.edited;
                    if op.edited {
                        np.name = op.name.clone();
                    }
                    if op.status == Status::Confirmed {
                        np.value_type = op.value_type;
                    }
                    props.push(np);
                }
                None => {
                    if o.status == Status::Confirmed && np.source != FieldSource::Directory {
                        if np.files_with as usize >= super::PROPOSE_FILES {
                            proposals.push(Proposal {
                                class_key: o.class_key.clone(),
                                slug: np.slug.clone(),
                                name: np.name.clone(),
                                files: np.files_with,
                                examples: np.examples.clone(),
                            });
                            props.push(np);
                        }
                        // Seen in fewer files than a proposal needs: not yet a candidate.
                    } else {
                        props.push(np);
                    }
                }
            }
        }
        // What a person decided stays even when the files no longer show it.
        for (pi, op) in o.properties.iter().enumerate() {
            if !used.contains(&pi) && (op.status != Status::Proposed || op.edited) {
                let mut kept = op.clone();
                kept.files_with = 0;
                kept.coverage = 0.0;
                props.push(kept);
            }
        }
        let _ = renamed_key;
        // A field that recurs without reaching the coverage of a property is a proposal in a confirmed class.
        if o.status == Status::Confirmed {
            for cand in candidates
                .get(&n.class_key)
                .into_iter()
                .flatten()
                .chain(candidates.get(&o.class_key).into_iter().flatten())
            {
                let known = props
                    .iter()
                    .any(|p| p.slug == cand.slug || p.keys.iter().any(|k| cand.keys.contains(k)));
                if !known {
                    proposals.push(Proposal {
                        class_key: o.class_key.clone(),
                        slug: cand.slug.clone(),
                        name: cand.name.clone(),
                        files: cand.files_with,
                        examples: cand.examples.clone(),
                    });
                    props.push(cand.clone());
                }
            }
        }
        n.properties = props;
        out.push(n);
    }
    for (i, o) in old.iter().enumerate() {
        if !taken.contains(&i) && o.status != Status::Proposed {
            let mut kept = o.clone();
            kept.files = 0;
            kept.rows = 0;
            out.push(kept);
        }
    }
    out.sort_by(|a, b| a.class_key.cmp(&b.class_key));
    (out, proposals)
}
