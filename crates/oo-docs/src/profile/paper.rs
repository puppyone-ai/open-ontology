//! Research papers (D89): the paper, its authors, venue and year, the methods, datasets and metrics it names, and the
//! papers it cites.
//!
//! The paper itself is a `Paper` named by its title (with `attr.doi`, `attr.arxiv` and `attr.year`), so a paper that
//! another one cites lands on the same entity. Authors are `Person`. Venue is `Venue`. A cited work is a `Paper` named by
//! its title, and the document mentions it with rule `paper.cites`; the reference block is the evidence. Methods come
//! from "we propose X" sentences, datasets from "X dataset" phrases and `Datasets:` lines, metrics from a closed list of
//! well known names. Deterministic; nothing is asked of a model.

use oo_core::docir::{Block, BlockKind, DocIr};

use super::{DATASET, METHOD, METRIC, PAPER, VENUE, attr, clip, thing};
use crate::extract::model::{
    Collector, EntityRef, EntityType, EvidenceRef, FactMention, Predicate,
};

const MAX_AUTHORS: usize = 30;
const MAX_REFS: usize = 200;
const MAX_ITEMS: usize = 12;
const BP: u16 = 9000;

const REF_HEADINGS: &[&str] = &[
    "references",
    "bibliography",
    "参考文献",
    "literature cited",
    "works cited",
];

fn lower(s: &str) -> String {
    s.to_lowercase()
}

fn is_heading_like(b: &Block, words: &[&str]) -> bool {
    let t =
        lower(b.text.trim().trim_matches(|c: char| {
            !c.is_alphanumeric() && !('\u{4e00}'..='\u{9fff}').contains(&c)
        }));
    let short = t.chars().count() <= 40;
    short
        && words
            .iter()
            .any(|w| t == *w || t.ends_with(&format!(" {w}")) && t.chars().count() <= 20)
}

const NOT_NAME: &[&str] = &[
    "university",
    "institute",
    "laboratory",
    "lab",
    "labs",
    "department",
    "school",
    "college",
    "research",
    "inc",
    "corp",
    "ltd",
    "google",
    "microsoft",
    "meta",
    "abstract",
    "keywords",
    "arxiv",
    "email",
    "preprint",
    "center",
    "centre",
];

/// A person's name from one fragment of an author line, or `None`.
fn person_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '*' | '†' | '‡' | '§' | '¶' | '∗' | '⋆') && !c.is_ascii_digit())
        .collect();
    let cleaned = cleaned
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c: char| matches!(c, ',' | ';' | '.' | ':'))
        .trim()
        .to_owned();
    if cleaned.is_empty() || cleaned.contains('@') || cleaned.chars().count() > 40 {
        return None;
    }
    let cjk = cleaned
        .chars()
        .all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    if cjk {
        return ((2..=4).contains(&cleaned.chars().count())).then_some(cleaned);
    }
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if !(2..=4).contains(&words.len()) {
        return None;
    }
    let ok = words.iter().all(|w| {
        let first = w.chars().next().unwrap_or(' ');
        first.is_uppercase()
            && w.chars()
                .all(|c| c.is_alphabetic() || matches!(c, '.' | '-' | '\''))
            && !NOT_NAME.contains(&lower(w).as_str())
    });
    ok.then_some(cleaned)
}

fn split_authors(line: &str) -> Vec<String> {
    let s = line
        .replace(" and ", ",")
        .replace(" & ", ",")
        .replace(['、', '，', ';', '\n'], ",");
    s.split(',').filter_map(person_name).collect()
}

const VENUES: &[&str] = &[
    "NeurIPS", "ICML", "ICLR", "ACL", "EMNLP", "NAACL", "COLING", "CVPR", "ICCV", "ECCV", "AAAI",
    "IJCAI", "KDD", "WWW", "SIGIR", "SIGMOD", "VLDB", "OSDI", "SOSP", "NSDI", "USENIX", "ICSE",
    "FSE", "PLDI", "POPL", "CHI", "UIST", "JMLR", "TPAMI", "TACL", "Nature", "Science",
];

fn year_in(s: &str) -> Option<String> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(3)).find_map(|i| {
        let w = &s[i..i + 4];
        let before = i == 0 || !b[i - 1].is_ascii_digit();
        let after = b.get(i + 4).is_none_or(|c| !c.is_ascii_digit());
        (before
            && after
            && (w.starts_with("19") || w.starts_with("20"))
            && w.chars().all(|c| c.is_ascii_digit()))
        .then(|| w.to_owned())
    })
}

fn find_venue(blocks: &[Block], limit: usize) -> Option<(String, Option<String>, usize)> {
    for (i, b) in blocks.iter().take(limit).enumerate() {
        for line in b.text.lines() {
            let words: Vec<&str> = line
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .collect();
            for v in VENUES {
                let hit = words.iter().any(|w| w == v);
                if hit
                    && (lower(line).contains("proceedings")
                        || lower(line).contains("published")
                        || lower(line).contains("conference")
                        || line.trim().chars().count() <= 80
                        || lower(line).contains("journal"))
                {
                    return Some(((*v).to_owned(), year_in(line), i));
                }
            }
            if let Some(rest) = lower(line).strip_prefix("arxiv preprint") {
                let _ = rest;
                return Some(("arXiv".to_owned(), year_in(line), i));
            }
        }
    }
    None
}

fn find_after<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    let l = s.to_ascii_lowercase();
    l.find(marker).map(|i| &s[i + marker.len()..])
}

fn doi_in(s: &str) -> Option<String> {
    let l = s.to_ascii_lowercase();
    let i = l.find("10.")?;
    let tail = &s[i..];
    let end = tail
        .find(|c: char| c.is_whitespace() || matches!(c, '"' | '<' | '>' | ')' | ']'))
        .unwrap_or(tail.len());
    let cand = tail[..end].trim_end_matches(['.', ',', ';']);
    let (reg, suffix) = cand.split_once('/')?;
    let digits = reg.strip_prefix("10.")?;
    (digits.len() >= 4
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
        && !suffix.is_empty()
        && (l.contains("doi") || l.contains("doi.org")))
    .then(|| cand.to_owned())
}

fn arxiv_in(s: &str) -> Option<String> {
    let rest = find_after(s, "arxiv:")?;
    let id: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    (id.len() >= 9 && id.contains('.')).then_some(id)
}

const STOP_FIRST: &[&str] = &[
    "a",
    "an",
    "the",
    "novel",
    "new",
    "simple",
    "general",
    "unified",
    "efficient",
    "our",
    "this",
    "that",
    "approach",
    "method",
];

/// The method name after a "we propose" style cue, from `text`.
fn methods_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let l = text.to_ascii_lowercase();
    for cue in [
        "we propose ",
        "we introduce ",
        "we present ",
        "we develop ",
        "we design ",
        "this paper proposes ",
        "this paper presents ",
        "this paper introduces ",
        "called ",
        "named ",
        "dubbed ",
        "we describe ",
    ] {
        let mut from = 0;
        while let Some(i) = l[from..].find(cue) {
            let start = from + i + cue.len();
            from = start;
            let tail = &text[start.min(text.len())..];
            if let Some(name) = method_name(tail)
                && !out.contains(&name)
            {
                out.push(name);
            }
        }
    }
    // 提出了XXX方法/模型/框架/算法
    for cue in ["提出了", "提出"] {
        let mut from = 0;
        while let Some(i) = text[from..].find(cue) {
            let start = from + i + cue.len();
            from = start;
            let tail: String = text[start..].chars().take(40).collect();
            for suffix in ["方法", "模型", "框架", "算法", "网络"] {
                if let Some(j) = tail.find(suffix) {
                    let head = tail[..j]
                        .trim_start_matches(['了', '一', '种', '个', '新', '的'])
                        .trim();
                    let name = head
                        .rsplit(['，', '、', '。', '的'])
                        .next()
                        .unwrap_or(head)
                        .trim();
                    // A Latin name (MedCL) is the method itself; a Chinese descriptor keeps its suffix.
                    let n = if !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+'))
                    {
                        name.to_owned()
                    } else {
                        format!("{name}{suffix}")
                    };
                    if (2..=24).contains(&name.chars().count()) && !out.contains(&n) {
                        out.push(n);
                    }
                    break;
                }
            }
        }
    }
    out
}

fn method_name(tail: &str) -> Option<String> {
    let mut words = tail.split_whitespace().peekable();
    // skip determiners and adjectives
    let mut guard = 0;
    while let Some(w) = words.peek() {
        let wl = lower(w.trim_matches(|c: char| !c.is_alphanumeric()));
        if STOP_FIRST.contains(&wl.as_str()) && guard < 4 {
            words.next();
            guard += 1;
        } else {
            break;
        }
    }
    let mut name: Vec<String> = Vec::new();
    for w in words.by_ref().take(4) {
        let token = w.trim_end_matches([',', '.', ';', ':', ')']);
        let ends = w.len() != token.len();
        let core = token.trim_start_matches('(');
        let ok = core
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase() || c.is_ascii_digit())
            && core
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '+' | '.'));
        let camel = core.chars().skip(1).any(char::is_uppercase)
            || core.contains('-')
            || core.chars().any(|c| c.is_ascii_digit());
        if !ok {
            break;
        }
        // The first token must look like a name (CamelCase, hyphen, digits or upper case acronym); later tokens extend it.
        if name.is_empty()
            && !(camel
                || core.chars().all(|c| c.is_uppercase() || !c.is_alphabetic())
                || core.chars().count() >= 4)
        {
            break;
        }
        name.push(core.to_owned());
        if ends {
            break;
        }
    }
    let n = name.join(" ");
    let nl = lower(&n);
    let banned = [
        "we", "in", "our", "the", "this", "it", "a", "an", "however", "these", "such", "first",
        "to", "and", "section", "table", "figure",
    ];
    (n.chars().count() >= 2 && !banned.contains(&nl.as_str())).then_some(n)
}

const DATA_WORDS: &[&str] = &[
    "dataset",
    "datasets",
    "corpus",
    "corpora",
    "benchmark",
    "benchmarks",
    "treebank",
    "testbed",
];
const DATA_STOP: &[&str] = &[
    "on",
    "in",
    "at",
    "for",
    "to",
    "of",
    "with",
    "using",
    "from",
    "by",
    "as",
    "we",
    "our",
    "its",
    "their",
    "the",
    "a",
    "an",
    "this",
    "our",
    "new",
    "large",
    "public",
    "standard",
    "same",
    "each",
    "both",
    "several",
    "these",
    "those",
    "training",
    "test",
    "evaluation",
    "such",
    "common",
];

fn datasets_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // "Datasets: A, B and C" lines
    for line in text.lines() {
        let l = line.trim();
        let ll = lower(l);
        for key in [
            "datasets:",
            "dataset:",
            "benchmarks:",
            "数据集：",
            "数据集:",
        ] {
            if ll.starts_with(key) {
                let rest = &l[key.len()..];
                for part in rest
                    .replace(" and ", ",")
                    .replace(['、', '，'], ",")
                    .split(',')
                {
                    let p = part.trim().trim_matches(|c: char| matches!(c, '.' | ';'));
                    if (2..=40).contains(&p.chars().count()) && !out.iter().any(|o| o == p) {
                        out.push(p.to_owned());
                    }
                }
            }
        }
    }
    // "<Name> dataset" / "<A>, <B> and <C> benchmarks": names are the runs of name-like tokens before the word.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let namey = |tok: &str| {
        !tok.is_empty()
            && tok
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
            && tok.chars().any(|c| c.is_uppercase() || c.is_ascii_digit())
            && !DATA_STOP.contains(&lower(tok).as_str())
    };
    for (i, t) in tokens.iter().enumerate() {
        let word = lower(t.trim_matches(|c: char| !c.is_alphanumeric()));
        if !DATA_WORDS.contains(&word.as_str()) || i == 0 {
            continue;
        }
        let mut names: Vec<Vec<String>> = Vec::new();
        let mut cur: Vec<String> = Vec::new();
        let mut j = i;
        while j > 0 && names.len() < 4 {
            j -= 1;
            let raw = tokens[j];
            let tok =
                raw.trim_matches(|c: char| matches!(c, ',' | '.' | ';' | ':' | '(' | ')' | '"'));
            let tl = lower(tok);
            if matches!(tl.as_str(), "and" | "or" | "the" | "a" | "an") {
                if !cur.is_empty() {
                    names.push(std::mem::take(&mut cur));
                }
                continue;
            }
            if raw.ends_with(',') && !cur.is_empty() {
                names.push(std::mem::take(&mut cur));
            }
            if namey(tok) {
                cur.insert(0, tok.to_owned());
            } else {
                break;
            }
        }
        if !cur.is_empty() {
            names.push(cur);
        }
        for n in names.into_iter().rev() {
            let n = n.join(" ");
            if n.chars().any(char::is_uppercase) && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    // "XXX数据集"
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i + 2 < chars.len() {
        if chars[i] == '数' && chars[i + 1] == '据' && chars[i + 2] == '集' {
            let mut j = i;
            while j > 0
                && (chars[j - 1].is_ascii_alphanumeric() || matches!(chars[j - 1], '-' | '_' | '.'))
            {
                j -= 1;
            }
            let n: String = chars[j..i].iter().collect();
            if n.chars().count() >= 2 && !out.contains(&n) {
                out.push(n);
            }
        }
        i += 1;
    }
    out
}

const METRICS: &[(&str, &str)] = &[
    ("F1", "F1"),
    ("BLEU", "BLEU"),
    ("ROUGE-L", "ROUGE-L"),
    ("ROUGE-1", "ROUGE-1"),
    ("ROUGE-2", "ROUGE-2"),
    ("METEOR", "METEOR"),
    ("CIDEr", "CIDEr"),
    ("AUROC", "AUROC"),
    ("AUC", "AUC"),
    ("mAP", "mAP"),
    ("mIoU", "mIoU"),
    ("MRR", "MRR"),
    ("NDCG", "NDCG"),
    ("WER", "WER"),
    ("CER", "CER"),
    ("PSNR", "PSNR"),
    ("SSIM", "SSIM"),
    ("FID", "FID"),
    ("accuracy", "Accuracy"),
    ("precision", "Precision"),
    ("recall", "Recall"),
    ("perplexity", "Perplexity"),
    ("exact match", "Exact Match"),
    ("top-1 accuracy", "Top-1 Accuracy"),
];

fn metrics_in(text: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    // ASCII words only: `F1和MRR` is two metrics in a Chinese sentence.
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '@'))
        .filter(|w| !w.is_empty())
        .map(|w| w.split('@').next().unwrap_or(w))
        .collect();
    let lt = lower(text);
    for &(k, canon) in METRICS {
        let hit = if matches!(k, "accuracy" | "precision" | "recall" | "perplexity") {
            words.iter().any(|w| lower(w) == k)
        } else if k.chars().any(char::is_uppercase) {
            words.contains(&k)
        } else if k.contains(' ') || k.contains('-') {
            lt.contains(k)
        } else {
            words.contains(&k)
        };
        if hit && !out.contains(&canon) {
            out.push(canon);
        }
    }
    // Accuracy subsumes "top-1 accuracy".
    if out.contains(&"Top-1 Accuracy") {
        out.retain(|m| *m != "Accuracy");
    }
    out
}

/// Title of a reference entry, when it can be read.
pub fn reference_title(entry: &str) -> Option<String> {
    let mut e = entry.trim();
    if let Some(rest) = e.strip_prefix('[')
        && let Some((n, r)) = rest.split_once(']')
        && n.len() <= 4
    {
        e = r.trim();
    } else {
        let digits = e.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && digits <= 3 && e[digits..].starts_with(['.', ')']) {
            e = e[digits + 1..].trim();
        }
    }
    for (a, b) in [('“', '”'), ('"', '"')] {
        if let Some(i) = e.find(a)
            && let Some(j) = e[i + a.len_utf8()..].find(b)
        {
            let t = e[i + a.len_utf8()..i + a.len_utf8() + j]
                .trim()
                .trim_end_matches([',', '.']);
            if t.split_whitespace().count() >= 2 {
                return Some(clip(t, 200));
            }
        }
    }
    let segs: Vec<&str> = e
        .split(". ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    // The title is the segment before the one that names the venue or the year (`NAACL, 2019`, `arXiv ...`, `In Proc ...`).
    let venue_seg = segs.iter().enumerate().skip(2).find(|(_, s)| {
        year_in(s).is_some()
            || lower(s).starts_with("in ")
            || lower(s).starts_with("arxiv")
            || lower(s).starts_with("doi")
            || s.split(|c: char| !c.is_alphanumeric())
                .any(|w| VENUES.contains(&w))
    });
    if let Some((v, _)) = venue_seg {
        let t = segs[v - 1];
        if t.split_whitespace().count() >= 2 || t.chars().count() >= 8 {
            return Some(clip(t.trim_end_matches('.'), 200));
        }
    }
    for (i, s) in segs.iter().enumerate() {
        if i == 0 {
            continue; // authors
        }
        let words = s.split_whitespace().count();
        if words >= 3
            && !s.chars().next().is_some_and(|c| c.is_ascii_digit())
            && !lower(s).starts_with("in ")
            && !lower(s).starts_with("arxiv")
            && !lower(s).starts_with("doi")
        {
            return Some(clip(s.trim_end_matches('.'), 200));
        }
    }
    (segs.len() == 1 && segs[0].split_whitespace().count() >= 3).then(|| clip(segs[0], 200))
}

fn split_refs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let starts_new = t.starts_with('[')
            && t[1..]
                .split(']')
                .next()
                .is_some_and(|n| n.len() <= 4 && n.chars().all(|c| c.is_ascii_digit()))
            || t.chars().take_while(char::is_ascii_digit).count().min(3) > 0
                && t.chars()
                    .find(|c| !c.is_ascii_digit())
                    .is_some_and(|c| c == '.' || c == ')');
        if starts_new || out.is_empty() {
            out.push(t.to_owned());
        } else if let Some(last) = out.last_mut() {
            last.push(' ');
            last.push_str(t);
        }
    }
    out
}

fn mention(
    col: &mut Collector,
    doc: &EntityRef,
    object: EntityRef,
    rule: &'static str,
    ev: &EvidenceRef,
) {
    col.fact(FactMention {
        subject: doc.clone(),
        predicate: Predicate::Mentions,
        object: Some(object),
        value: None,
        valid_from: None,
        valid_to: None,
        valid_precision: None,
        confidence_bp: 10000,
        rule_id: rule,
        evidence: vec![ev.clone()],
    });
}

/// A venue or identifier line of the front matter: not part of the author list.
fn front_noise(text: &str) -> bool {
    let l = lower(text);
    l.contains("proceedings")
        || l.contains("arxiv")
        || l.contains("doi")
        || l.contains("preprint")
        || l.contains("journal")
        || l.contains("conference")
        || l.contains("university")
        || l.contains('@')
        || text
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| VENUES.contains(&w))
}

fn terminal(text: &str) -> bool {
    text.trim_end()
        .ends_with(['.', '。', '!', '?', '！', '？', ':', '：', ';', '；'])
}

/// `1 Introduction`, `2.1 Method`, `1 引言`: a numbered heading that the parser delivered as a paragraph.
fn numbered_heading(text: &str) -> bool {
    let t = text.trim();
    let digits = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .count();
    digits > 0 && t[digits..].starts_with(' ') && t.chars().count() <= 60 && !terminal(t)
}

/// The parts of a paper the way the parser delivered them: title and authors, the body after the abstract with wrapped
/// paragraphs joined, and the references with wrapped entries joined. Each carries the first block it came from.
struct Parts {
    title: String,
    title_block: usize,
    authors: Option<usize>,
    front: Vec<usize>,
    body: Vec<(String, usize)>,
    refs: Vec<(String, usize)>,
    has_refs: bool,
}

fn parts(ir: &DocIr) -> Option<Parts> {
    let vis: Vec<usize> = ir
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            !matches!(
                b.kind,
                BlockKind::PageHeader | BlockKind::PageFooter | BlockKind::Frontmatter
            ) && !b.text.trim().is_empty()
        })
        .map(|(i, _)| i)
        .collect();
    let at = |k: usize| &ir.blocks[vis[k]];
    let abstract_k = vis.iter().take(80).position(|&i| {
        let t = lower(ir.blocks[i].text.trim());
        (t.starts_with("abstract") || t.starts_with("摘要"))
            && ir.blocks[i].text.trim().chars().count() <= 60
            || t.starts_with("abstract") && ir.blocks[i].text.trim().chars().count() > 60
    });
    let refs_k = vis
        .iter()
        .position(|&i| is_heading_like(&ir.blocks[i], REF_HEADINGS));
    let region_end = abstract_k.unwrap_or(vis.len().min(8));
    if region_end == 0 {
        return None;
    }
    let region: Vec<usize> = (0..region_end).collect();
    // A first level heading that is not the abstract is the title; else the lines before the authors.
    let first = at(0);
    let (title, title_block, authors, front);
    if first.kind == BlockKind::Heading && first.level == Some(1) && abstract_k != Some(0) {
        title = first.text.lines().next().unwrap_or("").trim().to_owned();
        title_block = vis[0];
        authors = region
            .iter()
            .skip(1)
            .find(|&&k| !front_noise(&at(k).text) && !split_authors(&at(k).text).is_empty())
            .map(|&k| vis[k]);
        front = region.iter().map(|&k| vis[k]).collect::<Vec<_>>();
    } else {
        let a = region
            .iter()
            .skip(1)
            .rev()
            .find(|&&k| {
                !front_noise(&at(k).text)
                    && !numbered_heading(&at(k).text)
                    && !split_authors(&at(k).text).is_empty()
                    && at(k).text.chars().count() <= 400
            })
            .copied();
        let title_end = a.unwrap_or(1.min(region_end));
        title = region[..title_end.max(1)]
            .iter()
            .take(6)
            .map(|&k| at(k).text.lines().next().unwrap_or("").trim().to_owned())
            .collect::<Vec<_>>()
            .join(" ");
        title_block = vis[0];
        authors = a.map(|k| vis[k]);
        front = region.iter().map(|&k| vis[k]).collect();
    }
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() < 4 {
        return None;
    }
    // Body: abstract to the references, paragraphs that the page width broke joined again.
    let body_range = abstract_k.unwrap_or(region_end)..refs_k.unwrap_or(vis.len());
    let mut body: Vec<(String, usize)> = Vec::new();
    let mut open = false;
    for k in body_range {
        let b = at(k);
        let heading = b.kind == BlockKind::Heading || numbered_heading(&b.text);
        let joinable = matches!(b.kind, BlockKind::Paragraph | BlockKind::ListItem) && !heading;
        if open
            && joinable
            && let Some(last) = body.last_mut()
        {
            last.0.push(' ');
            last.0.push_str(b.text.trim());
        } else {
            body.push((b.text.trim().to_owned(), vis[k]));
        }
        open = joinable && !body.last().is_some_and(|l| terminal(&l.0));
    }
    // References: entries start at `[n]` or `n.`; a line that does not start one continues the entry before it.
    let mut refs: Vec<(String, usize)> = Vec::new();
    if let Some(r) = refs_k {
        for (k, _) in vis.iter().enumerate().skip(r + 1) {
            let b = at(k);
            if b.kind == BlockKind::Heading && !is_heading_like(b, REF_HEADINGS) && !refs.is_empty()
            {
                break;
            }
            for line in split_refs(&b.text) {
                let starts = line.starts_with('[')
                    || line.chars().next().is_some_and(|c| c.is_ascii_digit());
                if !starts && let Some(last) = refs.last_mut() {
                    last.0.push(' ');
                    last.0.push_str(&line);
                } else {
                    refs.push((line, vis[k]));
                }
            }
        }
    }
    Some(Parts {
        title,
        title_block,
        authors,
        front,
        body,
        refs,
        has_refs: refs_k.is_some(),
    })
}

pub(crate) fn extract(path: &str, ir: &DocIr, col: &mut Collector) {
    let doc = EntityRef::new(EntityType::Document, path);
    let Some(p) = parts(ir) else { return };
    let title_ev = EvidenceRef::from_block(&ir.blocks[p.title_block]);
    let paper = thing(PAPER, clip(&p.title, 200));
    col.entity(&paper, Some(&title_ev));

    // Authors
    if let Some(i) = p.authors {
        let ev = EvidenceRef::from_block(&ir.blocks[i]);
        for name in split_authors(&ir.blocks[i].text)
            .into_iter()
            .take(MAX_AUTHORS)
        {
            let person = EntityRef::new(EntityType::Person, name);
            col.entity(&person, Some(&ev));
            mention(col, &doc, person, "paper.author", &ev);
        }
    }
    // Venue and year, DOI and arXiv id of the paper itself (the front matter)
    let front: Vec<Block> = p.front.iter().map(|&i| ir.blocks[i].clone()).collect();
    let venue = find_venue(&front, front.len());
    let mut year = venue.as_ref().and_then(|v| v.1.clone());
    if let Some((v, _, k)) = &venue {
        let ev = EvidenceRef::from_block(&front[*k]);
        let ve = thing(VENUE, v.clone());
        col.entity(&ve, Some(&ev));
        mention(col, &doc, ve, "paper.venue", &ev);
    }
    for b in &front {
        let ev = EvidenceRef::from_block(b);
        if let Some(doi) = doi_in(&b.text) {
            attr(col, &paper, "doi", &doi, "paper.id", BP, &ev);
        }
        if let Some(id) = arxiv_in(&b.text) {
            attr(col, &paper, "arxiv", &id, "paper.id", BP, &ev);
            if year.is_none() && id.len() >= 4 && id[..2].chars().all(|c| c.is_ascii_digit()) {
                year = Some(format!("20{}", &id[..2]));
            }
        }
    }
    if year.is_none() {
        year = front.iter().find_map(|b| {
            (b.text.chars().count() < 160)
                .then(|| year_in(&b.text))
                .flatten()
        });
    }
    if let Some(y) = year {
        attr(col, &paper, "year", &y, "paper.id", BP, &title_ev);
    }

    // Methods, datasets, metrics from the body (abstract to before the references)
    let mut methods: Vec<(String, usize)> = Vec::new();
    let mut datasets: Vec<(String, usize)> = Vec::new();
    let mut metrics: Vec<(&str, usize)> = Vec::new();
    for (text, block) in &p.body {
        for m in methods_in(text) {
            if !methods.iter().any(|(x, _)| *x == m) {
                methods.push((m, *block));
            }
        }
        for d in datasets_in(text) {
            if !datasets.iter().any(|(x, _)| *x == d) {
                datasets.push((d, *block));
            }
        }
        for m in metrics_in(text) {
            if !metrics.iter().any(|(x, _)| *x == m) {
                metrics.push((m, *block));
            }
        }
    }
    for (name, i) in methods.into_iter().take(MAX_ITEMS) {
        let ev = EvidenceRef::from_block(&ir.blocks[i]);
        let e = thing(METHOD, clip(&name, 80));
        col.entity(&e, Some(&ev));
        mention(col, &doc, e, "paper.method", &ev);
    }
    for (name, i) in datasets.into_iter().take(MAX_ITEMS) {
        let ev = EvidenceRef::from_block(&ir.blocks[i]);
        let e = thing(DATASET, clip(&name, 80));
        col.entity(&e, Some(&ev));
        mention(col, &doc, e, "paper.dataset", &ev);
    }
    for (name, i) in metrics.into_iter().take(MAX_ITEMS) {
        let ev = EvidenceRef::from_block(&ir.blocks[i]);
        let e = thing(METRIC, name);
        col.entity(&e, Some(&ev));
        mention(col, &doc, e, "paper.metric", &ev);
    }

    // The paper itself is mentioned by its document, so a paper cited elsewhere lands on the same entity.
    mention(col, &doc, paper.clone(), "paper.self", &title_ev);

    // Cites: every reference after the heading.
    let _ = p.has_refs;
    for (n, (entry, block)) in p.refs.iter().enumerate() {
        if n >= MAX_REFS {
            break;
        }
        let ev = EvidenceRef::from_block(&ir.blocks[*block]);
        let doi = doi_in(entry);
        let Some(name) =
            reference_title(entry).or_else(|| doi.as_ref().map(|d| format!("doi:{d}")))
        else {
            continue;
        };
        let cited = thing(PAPER, clip(&name, 200));
        if cited == paper {
            continue;
        }
        col.entity(&cited, Some(&ev));
        if let Some(d) = doi {
            attr(col, &cited, "doi", &d, "paper.cites", BP, &ev);
        }
        mention(col, &doc, cited, "paper.cites", &ev);
    }
}
