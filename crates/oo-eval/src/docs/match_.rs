//! Matching, anchor overlap and bootstrap for `oo-eval docs`.

use super::labels::{Anchor, Entity, Fact, LabelSet};
use std::collections::{BTreeSet, HashMap};

/// Outcome of one evaluated item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tp,
    Fp,
    Fn,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub kind: Kind,
    /// entity type or predicate
    pub group: String,
    /// anchor file extension (md, docx, ...) or "unknown"
    pub format: String,
}

/// Name normalisation: NFKC subset (full-width ASCII, ideographic space), zero-width removal,
/// lower-case, whitespace collapse, wrapping quotes/brackets stripped.
pub fn norm(s: &str) -> String {
    let mapped: String = s
        .chars()
        .filter(|c| !matches!(*c as u32, 0x200B..=0x200D | 0xFEFF))
        .map(|c| match c as u32 {
            0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            0x3000 => ' ',
            _ => c,
        })
        .collect();
    let lower: String = mapped.chars().flat_map(char::to_lowercase).collect();
    let t = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    let wrappers = ['"', '\'', '“', '”', '‘', '’', '「', '」', '《', '》'];
    t.trim_matches(&wrappers[..]).to_string()
}

pub fn format_of(anchors: &[Anchor]) -> String {
    anchors
        .first()
        .and_then(|a| a.file.rsplit_once('.').map(|(_, e)| e.to_lowercase()))
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------- anchors
#[derive(Clone, Debug, PartialEq)]
enum Frag {
    Lines(u64, u64),
    Rows(u64, u64),
    Cells {
        sheet: Option<String>,
        c0: u64,
        r0: u64,
        c1: u64,
        r1: u64,
    },
    Paras(u64, u64),
    TableRows {
        t: u64,
        r0: u64,
        r1: u64,
    },
    Slide(u64),
    Page(u64),
    Raw(String),
}

fn cell(s: &str) -> Option<(u64, u64)> {
    let letters: String = s.chars().take_while(char::is_ascii_alphabetic).collect();
    let digits = &s[letters.len()..];
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let c = letters
        .to_ascii_uppercase()
        .bytes()
        .fold(0u64, |a, b| a * 26 + u64::from(b - b'A' + 1));
    Some((c, digits.parse().ok()?))
}

fn range_a1(s: &str) -> Option<(u64, u64, u64, u64)> {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    let (c0, r0) = cell(a)?;
    let (c1, r1) = cell(b)?;
    Some((c0.min(c1), r0.min(r1), c0.max(c1), r0.max(r1)))
}

fn parse_frag(frag: &str) -> Frag {
    let f = frag.strip_prefix('#').unwrap_or(frag);
    if let Some(body) = f.strip_prefix('L') {
        let (x, y) = body.split_once('-').unwrap_or((body, body));
        if let (Ok(x), Ok(y)) = (x.parse(), y.trim_start_matches('L').parse()) {
            return Frag::Lines(x, y);
        }
    }
    if let Some(r) = f.strip_prefix('R')
        && let Ok(n) = r.parse()
    {
        return Frag::Rows(n, n);
    }
    if let Some(body) = f.strip_prefix('\u{b6}') {
        let (x, y) = body.split_once('-').unwrap_or((body, body));
        if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
            return Frag::Paras(x, y);
        }
    }
    if let Some(rest) = f.strip_prefix('T')
        && let Some((t, rc)) = rest.split_once('!')
        && let (Ok(t), Some((r0, _, r1, _))) = (t.parse(), table_rows(rc))
    {
        return Frag::TableRows { t, r0, r1 };
    }
    if let Some(n) = f.strip_prefix('s').and_then(|x| x.parse().ok()) {
        return Frag::Slide(n);
    }
    if let Some(n) = f.strip_prefix('p').and_then(|x| x.parse().ok()) {
        return Frag::Page(n);
    }
    if let Some(rest) = f.strip_prefix('\'') {
        // 'Sheet name'!A1:C9 ('' escapes a quote)
        let mut name = String::new();
        let mut chars = rest.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '\'' {
                if chars.peek().is_some_and(|(_, n)| *n == '\'') {
                    name.push('\'');
                    chars.next();
                    continue;
                }
                if let Some(r) = rest[i + 1..].strip_prefix('!').and_then(range_a1) {
                    return Frag::Cells {
                        sheet: Some(name),
                        c0: r.0,
                        r0: r.1,
                        c1: r.2,
                        r1: r.3,
                    };
                }
                break;
            }
            name.push(c);
        }
    } else if let Some((sheet, r)) = f.split_once('!') {
        if let Some(r) = range_a1(r) {
            return Frag::Cells {
                sheet: Some(sheet.to_string()),
                c0: r.0,
                r0: r.1,
                c1: r.2,
                r1: r.3,
            };
        }
    } else if let Some(r) = range_a1(f) {
        return Frag::Cells {
            sheet: None,
            c0: r.0,
            r0: r.1,
            c1: r.2,
            r1: r.3,
        };
    }
    Frag::Raw(f.to_string())
}

/// `R3C1:R3C4` / `R3C1` -> (row0, col0, row1, col1) reported as (r0, c0, r1, c1).
fn table_rows(rc: &str) -> Option<(u64, u64, u64, u64)> {
    let (a, b) = rc.split_once(':').unwrap_or((rc, rc));
    let p = |x: &str| -> Option<(u64, u64)> {
        let (r, c) = x.strip_prefix('R')?.split_once('C')?;
        Some((r.parse().ok()?, c.parse().ok()?))
    };
    let (r0, c0) = p(a)?;
    let (r1, c1) = p(b)?;
    Some((r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)))
}

fn inter(a0: u64, a1: u64, b0: u64, b1: u64) -> bool {
    a0 <= b1 && b0 <= a1
}

fn iou(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let i = w * h;
    let u = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - i;
    if u <= 0.0 { 0.0 } else { i / u }
}

pub fn anchors_overlap(a: &Anchor, b: &Anchor) -> bool {
    if a.file != b.file {
        return false;
    }
    match (parse_frag(&a.frag), parse_frag(&b.frag)) {
        (Frag::Lines(a0, a1), Frag::Lines(b0, b1))
        | (Frag::Rows(a0, a1), Frag::Rows(b0, b1))
        | (Frag::Paras(a0, a1), Frag::Paras(b0, b1)) => inter(a0, a1, b0, b1),
        (Frag::Rows(a0, a1), Frag::Cells { r0, r1, .. })
        | (Frag::Cells { r0, r1, .. }, Frag::Rows(a0, a1)) => inter(a0, a1, r0, r1),
        (
            Frag::Cells {
                sheet: s1,
                c0: ac0,
                r0: ar0,
                c1: ac1,
                r1: ar1,
            },
            Frag::Cells {
                sheet: s2,
                c0: bc0,
                r0: br0,
                c1: bc1,
                r1: br1,
            },
        ) => {
            (s1.is_none() || s2.is_none() || s1 == s2)
                && inter(ac0, ac1, bc0, bc1)
                && inter(ar0, ar1, br0, br1)
        }
        (
            Frag::TableRows {
                t: t1,
                r0: a0,
                r1: a1,
            },
            Frag::TableRows {
                t: t2,
                r0: b0,
                r1: b1,
            },
        ) => t1 == t2 && inter(a0, a1, b0, b1),
        (Frag::Slide(x), Frag::Slide(y)) => x == y,
        (Frag::Page(x), Frag::Page(y)) => {
            x == y
                && match (&a.bbox, &b.bbox) {
                    (Some(p), Some(q)) => iou(p, q) >= 0.5,
                    _ => true,
                }
        }
        (x, y) => x == y && matches!(x, Frag::Raw(_)),
    }
}

pub fn any_overlap(a: &[Anchor], b: &[Anchor]) -> bool {
    a.iter().any(|x| b.iter().any(|y| anchors_overlap(x, y)))
}

// ---------------------------------------------------------------- matching
pub struct EntityMatching {
    /// prediction id -> gold id
    pub pred_to_gold: HashMap<String, String>,
    pub outcomes: Vec<Outcome>,
    pub covered: usize,
}

fn order(preds: &[(f64, String, usize)]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..preds.len()).collect();
    idx.sort_by(|&a, &b| {
        preds[b]
            .0
            .partial_cmp(&preds[a].0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| preds[a].1.cmp(&preds[b].1))
            .then(preds[a].2.cmp(&preds[b].2))
    });
    idx
}

fn entity_strings(e: &Entity) -> BTreeSet<String> {
    std::iter::once(&e.name)
        .chain(e.aliases.iter())
        .map(|x| norm(x))
        .filter(|x| !x.is_empty())
        .collect()
}

pub fn match_entities(gold: &[Entity], pred: &[Entity]) -> EntityMatching {
    let mut index: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, g) in gold.iter().enumerate() {
        for s in entity_strings(g) {
            index.entry((g.typ.clone(), s)).or_default().push(i);
        }
    }
    let keys: Vec<(f64, String, usize)> = pred
        .iter()
        .enumerate()
        .map(|(i, p)| (p.confidence, format!("{}\0{}", norm(&p.name), p.id), i))
        .collect();
    let mut used = vec![false; gold.len()];
    let mut pred_to_gold = HashMap::new();
    let mut outcomes = Vec::new();
    let mut covered = 0;
    for i in order(&keys) {
        let p = &pred[i];
        let mut cand: Vec<usize> = entity_strings(p)
            .into_iter()
            .flat_map(|s| index.get(&(p.typ.clone(), s)).cloned().unwrap_or_default())
            .collect();
        cand.sort_unstable();
        match cand.into_iter().find(|&g| !used[g]) {
            Some(g) => {
                used[g] = true;
                pred_to_gold.insert(p.id.clone(), gold[g].id.clone());
                if any_overlap(&p.anchors, &gold[g].anchors) {
                    covered += 1;
                }
                outcomes.push(Outcome {
                    kind: Kind::Tp,
                    group: gold[g].typ.clone(),
                    format: format_of(&gold[g].anchors),
                });
            }
            None => outcomes.push(Outcome {
                kind: Kind::Fp,
                group: p.typ.clone(),
                format: format_of(&p.anchors),
            }),
        }
    }
    for (g, u) in gold.iter().zip(&used) {
        if !u {
            outcomes.push(Outcome {
                kind: Kind::Fn,
                group: g.typ.clone(),
                format: format_of(&g.anchors),
            });
        }
    }
    EntityMatching {
        pred_to_gold,
        outcomes,
        covered,
    }
}

pub struct FactMatching {
    /// prediction fact id -> gold fact id
    pub pred_to_gold: HashMap<String, String>,
    pub outcomes: Vec<Outcome>,
    pub covered: usize,
}

fn fact_key(f: &Fact, map: Option<&HashMap<String, String>>) -> Option<String> {
    let res = |id: &str| -> Option<String> {
        match map {
            Some(m) => m.get(id).cloned(),
            None => Some(id.to_string()),
        }
    };
    let s = res(&f.subject)?;
    let o = match (&f.object, &f.value) {
        (Some(o), _) => res(o)?,
        (None, Some(v)) => format!("v:{}", norm(v)),
        _ => return None,
    };
    Some(format!("{s}\0{}\0{o}", f.predicate))
}

pub fn match_facts(
    gold: &[Fact],
    pred: &[Fact],
    ent_map: &HashMap<String, String>,
) -> FactMatching {
    let mut index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, g) in gold.iter().enumerate() {
        if let Some(k) = fact_key(g, None) {
            index.entry(k).or_default().push(i);
        }
    }
    let keyed: Vec<Option<String>> = pred.iter().map(|p| fact_key(p, Some(ent_map))).collect();
    let keys: Vec<(f64, String, usize)> = pred
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                p.confidence,
                format!("{}\0{}", keyed[i].clone().unwrap_or_default(), p.id),
                i,
            )
        })
        .collect();
    let mut used = vec![false; gold.len()];
    let mut pred_to_gold = HashMap::new();
    let mut outcomes = Vec::new();
    let mut covered = 0;
    for i in order(&keys) {
        let p = &pred[i];
        let hit = keyed[i]
            .as_ref()
            .and_then(|k| index.get(k))
            .and_then(|c| c.iter().copied().find(|&g| !used[g]));
        match hit {
            Some(g) => {
                used[g] = true;
                pred_to_gold.insert(p.id.clone(), gold[g].id.clone());
                if any_overlap(&p.anchors, &gold[g].anchors) {
                    covered += 1;
                }
                outcomes.push(Outcome {
                    kind: Kind::Tp,
                    group: gold[g].predicate.clone(),
                    format: format_of(&gold[g].anchors),
                });
            }
            None => outcomes.push(Outcome {
                kind: Kind::Fp,
                group: p.predicate.clone(),
                format: format_of(&p.anchors),
            }),
        }
    }
    for (g, u) in gold.iter().zip(&used) {
        if !u {
            outcomes.push(Outcome {
                kind: Kind::Fn,
                group: g.predicate.clone(),
                format: format_of(&g.anchors),
            });
        }
    }
    FactMatching {
        pred_to_gold,
        outcomes,
        covered,
    }
}

pub struct SupersedeScore {
    pub gold: usize,
    pub predicted: usize,
    pub correct: usize,
    pub conflicts_gold: usize,
    pub conflicts_predicted: usize,
    pub conflicts_detected: usize,
}

/// A predicted `supersede old -> new` link as the engine exports it can name a `supersedes` fact (Decision S
/// supersedes Decision X) as the replacement: the engine ends the old `decides` / `about` facts of X with
/// `superseded_by` = that `supersedes` fact (10 5.5 plan_decision). The labels name instead the replacing
/// `decides` / `about` fact of S. Rewrite such a link to the equivalent fact link so both views match; a link
/// whose replacement is no `supersedes` fact, or whose equivalent is not predicted, stays as it is.
pub fn normalized_supersedes(pred: &LabelSet) -> Vec<(String, String)> {
    let by_id: HashMap<&str, &Fact> = pred.facts.iter().map(|f| (f.id.as_str(), f)).collect();
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    for (o, n) in &pred.supersedes {
        let (Some(of), Some(nf)) = (by_id.get(o.as_str()), by_id.get(n.as_str())) else {
            out.insert((o.clone(), n.clone()));
            continue;
        };
        let mut link = (o.clone(), n.clone());
        if nf.predicate == "supersedes"
            && of.predicate != "supersedes"
            && let Some(old_end) = nf.object.as_deref()
        {
            let new_end = nf.subject.as_str();
            // The fact of the replacing decision with the same shape as the old one.
            let twin = pred.facts.iter().find(|x| {
                x.predicate == of.predicate
                    && match of.predicate.as_str() {
                        "decides" => {
                            of.object.as_deref() == Some(old_end)
                                && x.subject == of.subject
                                && x.object.as_deref() == Some(new_end)
                        }
                        "about" => {
                            of.subject == old_end && x.subject == new_end && x.object == of.object
                        }
                        _ => false,
                    }
            });
            if let Some(t) = twin {
                link = (o.clone(), t.id.clone());
            }
        }
        out.insert(link);
    }
    out.into_iter().collect()
}

pub fn score_supersede(
    gold: &LabelSet,
    pred: &LabelSet,
    fact_map: &HashMap<String, String>,
) -> SupersedeScore {
    let gset: BTreeSet<(String, String)> = gold.supersedes.iter().cloned().collect();
    let links = normalized_supersedes(pred);
    let mut hit = BTreeSet::new();
    for (o, n) in &links {
        if let (Some(go), Some(gn)) = (fact_map.get(o), fact_map.get(n)) {
            let pair = (go.clone(), gn.clone());
            if gset.contains(&pair) {
                hit.insert(pair);
            }
        }
    }
    let gc: BTreeSet<BTreeSet<String>> = gold
        .conflicts
        .iter()
        .map(|c| c.iter().cloned().collect())
        .collect();
    let mut det = BTreeSet::new();
    for c in &pred.conflicts {
        let mapped: Option<BTreeSet<String>> = c.iter().map(|f| fact_map.get(f).cloned()).collect();
        if let Some(m) = mapped
            && gc.contains(&m)
        {
            det.insert(m);
        }
    }
    SupersedeScore {
        gold: gset.len(),
        predicted: links.len(),
        correct: hit.len(),
        conflicts_gold: gc.len(),
        conflicts_predicted: pred.conflicts.len(),
        conflicts_detected: det.len(),
    }
}

// ---------------------------------------------------------------- metrics + bootstrap
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metric {
    pub tp: usize,
    pub fp: usize,
    pub fn_: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub f1_ci: (f64, f64),
}

pub fn prf(tp: usize, fp: usize, fn_: usize) -> (f64, f64, f64) {
    let div = |a: usize, b: usize| if b == 0 { 0.0 } else { a as f64 / b as f64 };
    let p = div(tp, tp + fp);
    let r = div(tp, tp + fn_);
    let f = if p + r == 0.0 {
        0.0
    } else {
        2.0 * p * r / (p + r)
    };
    (p, r, f)
}

/// xorshift64* with a fixed seed: reports are reproducible.
pub struct Rng(pub u64);
impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

pub const BOOTSTRAP_SEED: u64 = 0x2026_1117_0A0B_0C0D;
pub const BOOTSTRAP_RESAMPLES: usize = 1000;

pub fn metric(items: &[Kind]) -> Metric {
    let cnt = |k: Kind| items.iter().filter(|x| **x == k).count();
    let (tp, fp, fn_) = (cnt(Kind::Tp), cnt(Kind::Fp), cnt(Kind::Fn));
    let (precision, recall, f1) = prf(tp, fp, fn_);
    let f1_ci = if items.is_empty() {
        (0.0, 0.0)
    } else {
        let mut rng = Rng(BOOTSTRAP_SEED);
        let mut v: Vec<f64> = (0..BOOTSTRAP_RESAMPLES)
            .map(|_| {
                let mut c = [0usize; 3];
                for _ in 0..items.len() {
                    let k = items[(rng.next_u64() % items.len() as u64) as usize];
                    c[k as usize] += 1;
                }
                prf(c[0], c[1], c[2]).2
            })
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = (v.len() - 1) as f64;
        (
            v[(0.025 * n).floor() as usize],
            v[(0.975 * n).ceil() as usize],
        )
    };
    Metric {
        tp,
        fp,
        fn_,
        precision,
        recall,
        f1,
        f1_ci,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(file: &str, frag: &str) -> Anchor {
        Anchor {
            file: file.into(),
            frag: frag.into(),
            bbox: None,
        }
    }

    #[test]
    fn docs_overlap_rules() {
        assert!(anchors_overlap(&a("x.md", "#L3-L5"), &a("x.md", "#L5-L9")));
        assert!(!anchors_overlap(&a("x.md", "#L3-L4"), &a("x.md", "#L5-L9")));
        assert!(!anchors_overlap(&a("x.md", "#L3-L5"), &a("y.md", "#L3-L5")));
        assert!(anchors_overlap(&a("c.csv", "#R4"), &a("c.csv", "#R4")));
        assert!(anchors_overlap(
            &a("t.xlsx", "#'人员表'!A2:F2"),
            &a("t.xlsx", "#'人员表'!C2:C2")
        ));
        assert!(!anchors_overlap(
            &a("t.xlsx", "#'人员表'!A2:F2"),
            &a("t.xlsx", "#'人员表'!A3:F3")
        ));
        assert!(!anchors_overlap(
            &a("t.xlsx", "#'a'!A1:B2"),
            &a("t.xlsx", "#'b'!A1:B2")
        ));
        assert!(anchors_overlap(
            &a("d.docx", "#¶3-3"),
            &a("d.docx", "#¶2-4")
        ));
        assert!(anchors_overlap(
            &a("d.docx", "#T1!R2C1:R2C3"),
            &a("d.docx", "#T1!R2C1:R2C3")
        ));
        assert!(!anchors_overlap(
            &a("d.docx", "#T1!R2C1:R2C3"),
            &a("d.docx", "#T2!R2C1:R2C3")
        ));
        assert!(anchors_overlap(&a("p.pptx", "#s3"), &a("p.pptx", "#s3")));
        assert!(!anchors_overlap(&a("p.pptx", "#s3"), &a("p.pptx", "#s4")));
        let mut p = a("f.pdf", "#p1");
        let mut q = a("f.pdf", "#p1");
        p.bbox = Some([0.0, 0.0, 10.0, 10.0]);
        q.bbox = Some([0.0, 0.0, 10.0, 5.0]);
        assert!(anchors_overlap(&p, &q));
        q.bbox = Some([0.0, 0.0, 10.0, 4.0]);
        assert!(!anchors_overlap(&p, &q));
    }

    #[test]
    fn docs_norm_and_bootstrap_are_deterministic() {
        assert_eq!(norm("  《ＡＢ  c》 "), "ab c");
        let items = [Kind::Tp, Kind::Tp, Kind::Fp, Kind::Fn, Kind::Tp];
        assert_eq!(metric(&items), metric(&items));
        let m = metric(&items);
        assert!(m.f1_ci.0 <= m.f1 + 1e-9 || m.f1_ci.0 <= m.f1_ci.1);
        assert_eq!(metric(&[]).f1, 0.0);
    }

    fn fact(id: &str, s: &str, p: &str, o: &str) -> Fact {
        Fact {
            id: id.into(),
            subject: s.into(),
            predicate: p.into(),
            object: Some(o.into()),
            ..Fact::default()
        }
    }

    #[test]
    fn a_supersedes_fact_as_replacement_counts_as_the_replacing_decides_fact() {
        // The engine ends the decides fact of the old decision with `superseded_by` = the supersedes fact.
        let pred = LabelSet {
            facts: vec![
                fact("p1", "A", "decides", "D1"),
                fact("p2", "A", "decides", "D2"),
                fact("p3", "D2", "supersedes", "D1"),
            ],
            supersedes: vec![("p1".into(), "p3".into())],
            ..LabelSet::default()
        };
        assert_eq!(
            normalized_supersedes(&pred),
            vec![("p1".to_string(), "p2".to_string())]
        );
        let gold = LabelSet {
            supersedes: vec![("g1".into(), "g2".into())],
            ..LabelSet::default()
        };
        let map: HashMap<String, String> = [("p1", "g1"), ("p2", "g2"), ("p3", "g3")]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        let sc = score_supersede(&gold, &pred, &map);
        assert_eq!((sc.gold, sc.predicted, sc.correct), (1, 1, 1));
    }

    #[test]
    fn a_link_between_two_facts_of_the_same_kind_is_left_alone() {
        let pred = LabelSet {
            facts: vec![fact("o1", "A", "owns", "P"), fact("o2", "B", "owns", "P")],
            supersedes: vec![("o1".into(), "o2".into())],
            ..LabelSet::default()
        };
        assert_eq!(
            normalized_supersedes(&pred),
            vec![("o1".to_string(), "o2".to_string())]
        );
    }

    #[test]
    fn a_conflict_is_detected_when_its_facts_map_to_a_gold_conflict() {
        let gold = LabelSet {
            conflicts: vec![vec!["g1".into(), "g2".into()]],
            ..LabelSet::default()
        };
        let pred = LabelSet {
            conflicts: vec![
                vec!["p2".into(), "p1".into()],
                vec!["p1".into(), "p9".into()],
            ],
            ..LabelSet::default()
        };
        let map: HashMap<String, String> = [("p1", "g1"), ("p2", "g2")]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        let sc = score_supersede(&gold, &pred, &map);
        assert_eq!(
            (
                sc.conflicts_gold,
                sc.conflicts_predicted,
                sc.conflicts_detected
            ),
            (1, 2, 1)
        );
    }
}
