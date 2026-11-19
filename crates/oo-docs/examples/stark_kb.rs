//! STaRK semi-structured KB retrieval on the oo-docs engine (benchmark
//! driver, not shipped). Started with `--nodes nodes.jsonl --edges edges.tsv`
//! (written by `tools/oo-bench/stark/prepare_*.py`), then spoken to over
//! stdio, one JSON object per line.
//!
//! Load: every KB node becomes one markdown document `kb/<id>.md` (the node
//! name as the title, its fields as a list), parsed and normalized into
//! `oo.doc-ir/v1` by the engine and run through the `rules/1` extraction; the
//! IR block text is what the `bm25/1` channel indexes. Node names and aliases
//! go into an alias table keyed by the resolver's canonical `norm`, and the
//! typed KB edges into an adjacency list.
//!
//! * `{"op":"search","text":..,"plan":{..}|null,"k":N,"candidate_types":[..]}`: resolves the anchor
//!   entities of the question (from `plan.anchors`, or by scanning the text
//!   for known names), walks one hop from them over the allowed relations,
//!   intersects the walks, and ranks the candidates of the answer types by
//!   (anchors satisfied, BM25 over the node text). Returns node ids, never an
//!   answer.
//! * `{"op":"docs","ids":[..],"max_chars":N,"rels":R}`: the node texts for a
//!   reranker, with up to `R` neighbor names per relation.
//! * `{"op":"stats"}`.
//!
//! Integer scores only and ties on node id, so a replay is byte-identical.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, Write};

use oo_docs::extract::rules;
use oo_docs::parse::markdown::parse_markdown;
use oo_docs::resolve::normalize::norm;
use oo_docs::vector::lexical::{Index, terms};
use oo_docs::vector::walk::{self, Anchor, Graph};
use serde_json::{Value, json};

/// Weight of one satisfied anchor against the BM25 text score (Q16 units,
/// so one anchor outweighs any text score below `ANCHOR_W`).
const ANCHOR_W: i64 = 1 << 26;
/// Text hits considered per query.
const TEXT_POOL: usize = 3000;
/// A name shared by more nodes than this is too ambiguous to anchor on when
/// scanning free text.
const MAX_AMBIGUITY: usize = 4;

struct Node {
    ty: u8,
    name: String,
    doc: String,
}

struct Kb {
    types: Vec<String>,
    rels: Vec<String>,
    nodes: Vec<Node>,
    text: Index,
    names: Index,
    name_ids: Vec<u32>,
    alias: HashMap<String, Vec<u32>>,
    graph: Graph,
    /// `(relation, type a, type b)` -> edge count, for the plan prompt.
    schema: BTreeMap<(u16, u8, u8), u64>,
    blocks: usize,
    facts: usize,
    unparsed: usize,
    max_words: usize,
    w_anchor: i64,
}

fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn intern(list: &mut Vec<String>, name: &str) -> u8 {
    if let Some(i) = list.iter().position(|x| x == name) {
        return u8::try_from(i).unwrap_or(u8::MAX);
    }
    list.push(name.to_string());
    u8::try_from(list.len() - 1).unwrap_or(u8::MAX)
}

/// Answer-type words, most specific first: `(phrase, KB type)`.
const TYPE_WORDS: &[(&str, &str)] = &[
    ("biological process", "biological_process"),
    ("molecular function", "molecular_function"),
    ("cellular component", "cellular_component"),
    ("cellular structure", "cellular_component"),
    ("anatomical", "anatomy"),
    ("anatomy", "anatomy"),
    ("tissue", "anatomy"),
    ("organ", "anatomy"),
    ("body part", "anatomy"),
    ("pathway", "pathway"),
    ("phenotype", "effect/phenotype"),
    ("symptom", "effect/phenotype"),
    ("side effect", "effect/phenotype"),
    ("effect", "effect/phenotype"),
    ("exposure", "exposure"),
    ("environmental", "exposure"),
    ("gene", "gene/protein"),
    ("protein", "gene/protein"),
    ("enzyme", "gene/protein"),
    ("drug", "drug"),
    ("medication", "drug"),
    ("medicine", "drug"),
    ("compound", "drug"),
    ("treatment", "drug"),
    ("therapy", "drug"),
    ("disease", "disease"),
    ("disorder", "disease"),
    ("condition", "disease"),
    ("illness", "disease"),
    ("syndrome", "disease"),
    ("process", "biological_process"),
    ("function", "molecular_function"),
    ("component", "cellular_component"),
    ("structure", "cellular_component"),
];

impl Kb {
    fn load(nodes_path: &str, edges_path: &str, extract: bool) -> Result<Self, String> {
        let mut kb = Kb {
            types: Vec::new(),
            rels: Vec::new(),
            nodes: Vec::new(),
            text: Index::new(),
            names: Index::new(),
            name_ids: Vec::new(),
            alias: HashMap::new(),
            graph: Graph::new(0),
            schema: BTreeMap::new(),
            blocks: 0,
            facts: 0,
            unparsed: 0,
            max_words: 10,
            w_anchor: ANCHOR_W,
        };
        let f = std::fs::File::open(nodes_path).map_err(|e| format!("{nodes_path}: {e}"))?;
        for (i, line) in std::io::BufReader::new(f).lines().enumerate() {
            let line = line.map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&line).map_err(|e| format!("line {i}: {e}"))?;
            let id = v
                .get("id")
                .and_then(Value::as_u64)
                .ok_or("node without id")?;
            if id as usize != i {
                return Err(format!("node ids must be dense, line {i} has {id}"));
            }
            let ty = intern(&mut kb.types, &s(&v, "type"));
            let name = s(&v, "name");
            let doc = s(&v, "doc");
            // The engine path: markdown -> normalized IR -> rules extraction.
            let md = format!("# {}\n\n{}\n", name.replace('\n', " "), doc);
            // A node the parser rejects (control bytes in a scraped
            // abstract) is indexed from its raw text and counted.
            match parse_markdown(md.as_bytes()) {
                Ok(parsed) => {
                    let ir = &parsed.ir;
                    kb.blocks += ir.blocks.len();
                    if extract {
                        kb.facts += rules::extract(&format!("kb/{id}.md"), ir).facts.len();
                    }
                    let text: Vec<&str> = ir.blocks.iter().map(|b| b.text.as_str()).collect();
                    kb.text.add(&text.join("\n"));
                }
                Err(_) => {
                    kb.unparsed += 1;
                    kb.text.add(&md);
                }
            }
            let mut all = vec![name.clone()];
            if let Some(a) = v.get("aliases").and_then(Value::as_array) {
                all.extend(a.iter().filter_map(Value::as_str).map(str::to_string));
            }
            let idu = u32::try_from(id).map_err(|e| e.to_string())?;
            let mut seen = BTreeSet::new();
            for a in &all {
                let k = norm(a);
                if k.is_empty() || !seen.insert(k.clone()) {
                    continue;
                }
                kb.alias.entry(k).or_default().push(idu);
            }
            kb.names.add(&all.join(" ; "));
            kb.name_ids.push(idu);
            kb.nodes.push(Node { ty, name, doc });
        }
        kb.graph = Graph::new(kb.nodes.len());
        let f = std::fs::File::open(edges_path).map_err(|e| format!("{edges_path}: {e}"))?;
        for line in std::io::BufReader::new(f).lines() {
            let line = line.map_err(|e| e.to_string())?;
            let mut it = line.split('\t');
            let (Some(h), Some(t), Some(r)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            let (Ok(h), Ok(t)) = (h.parse::<u32>(), t.parse::<u32>()) else {
                continue;
            };
            if h as usize >= kb.nodes.len() || t as usize >= kb.nodes.len() {
                continue;
            }
            let r = u16::from(intern(&mut kb.rels, r));
            kb.graph.add_edge(h, t, r);
            let (ta, tb) = (kb.nodes[h as usize].ty, kb.nodes[t as usize].ty);
            *kb.schema.entry((r, ta.min(tb), ta.max(tb))).or_insert(0) += 1;
        }
        kb.graph.seal();
        Ok(kb)
    }

    fn type_id(&self, name: &str) -> Option<u8> {
        self.types
            .iter()
            .position(|t| t == name)
            .and_then(|i| u8::try_from(i).ok())
    }

    fn rel_id(&self, name: &str) -> Option<u16> {
        self.rels
            .iter()
            .position(|t| t == name)
            .and_then(|i| u16::try_from(i).ok())
    }

    /// Nodes one hop from `nodes` whose type is in `via` (2-hop anchors:
    /// "a drug that shares a transporter with X" walks X -> gene -> drug).
    fn expand(&self, nodes: &[u32], via: &[u8], cap: usize) -> Vec<u32> {
        let mut out = BTreeSet::new();
        for &n in nodes {
            for &(_, nb) in self.graph.neighbors(n) {
                if via.contains(&self.nodes[nb as usize].ty) {
                    out.insert(nb);
                }
            }
        }
        out.into_iter().take(cap).collect()
    }

    /// Answer types named in a question: the earliest type word wins.
    fn infer_types(&self, q: &str) -> Vec<u8> {
        let lower = format!(" {} ", q.to_lowercase());
        let mut best: Option<(usize, u8)> = None;
        for (w, t) in TYPE_WORDS {
            let Some(tid) = self.type_id(t) else { continue };
            if let Some(pos) = lower.find(&format!(" {w}"))
                && best.is_none_or(|(p, _)| pos < p)
            {
                best = Some((pos, tid));
            }
        }
        best.map(|(_, t)| vec![t]).unwrap_or_default()
    }

    /// Resolve one mention: exact canonical-name hits first, otherwise the
    /// best names-channel hits; restricted to `types` when given.
    fn resolve(&self, mention: &str, types: &[u8], fuzzy: usize) -> Vec<u32> {
        let ok = |id: &u32| types.is_empty() || types.contains(&self.nodes[*id as usize].ty);
        if let Some(ids) = self.alias.get(&norm(mention)) {
            let hit: Vec<u32> = ids.iter().copied().filter(ok).collect();
            if !hit.is_empty() {
                return hit;
            }
        }
        if fuzzy == 0 {
            return Vec::new();
        }
        self.names
            .search(mention, 200)
            .into_iter()
            .map(|(d, _)| self.name_ids[d as usize])
            .filter(ok)
            .take(fuzzy)
            .collect()
    }

    /// Scan free text for known names, longest match first, never
    /// overlapping. Each match is one anchor group.
    fn scan(&self, q: &str) -> Vec<(String, Vec<u32>)> {
        let words: Vec<&str> = q
            .split(|c: char| c.is_whitespace())
            .filter(|w| !w.is_empty())
            .collect();
        let clean = |w: &str| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != ')' && c != '(')
                .to_string()
        };
        let mut used = vec![false; words.len()];
        let mut out = Vec::new();
        for n in (1..=self.max_words.min(words.len())).rev() {
            for i in 0..=words.len() - n {
                if used[i..i + n].iter().any(|u| *u) {
                    continue;
                }
                let span: Vec<String> = words[i..i + n].iter().map(|w| clean(w)).collect();
                let span = span.join(" ");
                let key = norm(&span);
                // Too short, all stopwords, an answer-type word, or a short
                // lower-case word (gene symbols are upper case).
                let short_word = n == 1 && key.chars().count() <= 4 && span == key;
                if key.chars().count() < 3
                    || terms(&key).is_empty()
                    || short_word
                    || TYPE_WORDS
                        .iter()
                        .any(|(w, _)| key == *w || key == format!("{w}s"))
                {
                    continue;
                }
                if let Some(ids) = self.alias.get(&key)
                    && ids.len() <= MAX_AMBIGUITY
                {
                    used[i..i + n].iter_mut().for_each(|u| *u = true);
                    out.push((span, ids.clone()));
                }
            }
        }
        out
    }

    fn search(&self, q: &Value) -> Result<Value, String> {
        let text = s(q, "text");
        let k = q.get("k").and_then(Value::as_u64).unwrap_or(100) as usize;
        let plan = q.get("plan").filter(|p| p.is_object());
        let mut types: Vec<u8> = Vec::new();
        let mut groups: Vec<(String, Vec<u32>, Vec<u16>)> = Vec::new();
        if let Some(p) = plan {
            for t in p
                .get("answer_types")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(id) = t.as_str().and_then(|t| self.type_id(t)) {
                    types.push(id);
                }
            }
            for a in p
                .get("anchors")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let mention = s(a, "mention");
                let atypes: Vec<u8> = a
                    .get("types")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.as_str().and_then(|t| self.type_id(t)))
                    .collect();
                let via: Vec<u8> = a
                    .get("via")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.as_str().and_then(|t| self.type_id(t)))
                    .collect();
                let rels: Vec<u16> = a
                    .get("relations")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.as_str().and_then(|t| self.rel_id(t)))
                    .collect();
                let fuzzy = q.get("fuzzy").and_then(Value::as_u64).unwrap_or(2) as usize;
                let mut ids = self.resolve(&mention, &atypes, fuzzy);
                if ids.is_empty() && !atypes.is_empty() {
                    ids = self.resolve(&mention, &[], fuzzy);
                }
                if !via.is_empty() {
                    ids = self.expand(&ids, &via, 5000);
                }
                if !ids.is_empty() {
                    groups.push((mention, ids, rels));
                }
            }
        } else {
            types = self.infer_types(&text);
            groups = self
                .scan(&text)
                .into_iter()
                .map(|(m, ids)| (m, ids, Vec::new()))
                .collect();
        }
        // The benchmark's candidate set (MAG: papers, Amazon: products)
        // overrides whatever the plan or the text implies.
        if let Some(forced) = q.get("candidate_types").and_then(Value::as_array) {
            types = forced
                .iter()
                .filter_map(|t| t.as_str().and_then(|t| self.type_id(t)))
                .collect();
        }
        let type_ok = |id: u32| types.is_empty() || types.contains(&self.nodes[id as usize].ty);
        let query_text = plan
            .map(|p| s(p, "text"))
            .filter(|t| !t.trim().is_empty())
            .map(|t| format!("{text} {t}"))
            .unwrap_or_else(|| text.clone());
        let mut bm: BTreeMap<u32, i64> = BTreeMap::new();
        for (d, sc) in self.text.search(&query_text, TEXT_POOL) {
            if type_ok(d) {
                bm.insert(d, sc);
            }
        }
        let anchors: Vec<Anchor> = groups
            .iter()
            .map(|(_, ids, rels)| Anchor {
                nodes: ids.clone(),
                relations: rels.clone(),
            })
            .collect();
        // Graph candidates the text pool missed still need a text score.
        let reach: BTreeSet<u32> = anchors
            .iter()
            .flat_map(|a| a.nodes.iter())
            .flat_map(|n| self.graph.neighbors(*n).iter().map(|x| x.1))
            .filter(|n| type_ok(*n) && !bm.contains_key(n))
            .collect();
        if !reach.is_empty() {
            for (d, sc) in self.text.search(&query_text, self.text.len()) {
                if reach.contains(&d) {
                    bm.insert(d, sc);
                }
            }
        }
        let scored = walk::rank(&self.graph, &anchors, type_ok, &bm, self.w_anchor, k);
        let anchor_out: Vec<Value> = groups
            .iter()
            .map(|(m, ids, rels)| {
                json!({
                    "mention": m,
                    "ids": ids.iter().take(8).collect::<Vec<_>>(),
                    "names": ids.iter().take(3).map(|i| self.nodes[*i as usize].name.clone()).collect::<Vec<_>>(),
                    "relations": rels.iter().map(|r| self.rels[*r as usize].clone()).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(json!({
            "ok": true,
            "ids": scored.iter().map(|x| x.0).collect::<Vec<_>>(),
            "scores": scored.iter().map(|x| x.1).collect::<Vec<_>>(),
            "anchors": anchor_out,
            "types": types.iter().map(|t| self.types[*t as usize].clone()).collect::<Vec<_>>(),
        }))
    }

    fn docs(&self, q: &Value) -> Result<Value, String> {
        let max = q.get("max_chars").and_then(Value::as_u64).unwrap_or(2000) as usize;
        let nrel = q.get("rels").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut out = Vec::new();
        for id in q.get("ids").and_then(Value::as_array).into_iter().flatten() {
            let Some(id) = id.as_u64().filter(|i| (*i as usize) < self.nodes.len()) else {
                continue;
            };
            let n = &self.nodes[id as usize];
            let mut doc: String = n.doc.chars().take(max).collect();
            if nrel > 0 {
                let mut by: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
                for &(r, nb) in self.graph.neighbors(id as u32) {
                    let list = by.entry(self.rels[r as usize].as_str()).or_default();
                    if list.len() < nrel {
                        list.push(self.nodes[nb as usize].name.as_str());
                    }
                }
                if !by.is_empty() {
                    doc.push_str("\n- relations:");
                    for (r, names) in by {
                        doc.push_str(&format!("\n  - {r}: {}", names.join(", ")));
                    }
                }
            }
            out.push(
                json!({"id": id, "name": n.name, "type": self.types[n.ty as usize], "doc": doc}),
            );
        }
        Ok(json!({"ok": true, "docs": out}))
    }
}

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(nodes), Some(edges)) = (arg(&args, "--nodes"), arg(&args, "--edges")) else {
        eprintln!("usage: stark_kb --nodes nodes.jsonl --edges edges.tsv [--no-extract]");
        std::process::exit(2);
    };
    let t0 = std::time::Instant::now();
    let kb = match Kb::load(&nodes, &edges, !args.iter().any(|a| a == "--no-extract")) {
        Ok(kb) => kb,
        Err(e) => {
            eprintln!("load failed: {e}");
            std::process::exit(1);
        }
    };
    let mut kb = kb;
    if let Some(w) = arg(&args, "--anchor-weight").and_then(|w| w.parse::<i64>().ok()) {
        kb.w_anchor = w;
    }
    if let Some(w) = arg(&args, "--max-words").and_then(|w| w.parse::<usize>().ok()) {
        kb.max_words = w;
    }
    let edges_n = kb.graph.entries();
    eprintln!(
        "loaded {} nodes ({} unparsed), {} IR blocks, {} rule facts, {} adjacency entries in {:.1}s",
        kb.nodes.len(),
        kb.unparsed,
        kb.blocks,
        kb.facts,
        edges_n,
        t0.elapsed().as_secs_f64()
    );
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(e) => json!({"ok": false, "error": format!("bad json: {e}")}),
            Ok(q) => {
                let r = match s(&q, "op").as_str() {
                    "search" => kb.search(&q),
                    "docs" => kb.docs(&q),
                    "stats" => Ok(json!({
                        "ok": true,
                        "nodes": kb.nodes.len(),
                        "blocks": kb.blocks,
                        "facts": kb.facts,
                        "unparsed": kb.unparsed,
                        "types": kb.types,
                        "relations": kb.rels,
                        "adjacency": edges_n,
                        "schema": kb.schema.iter().map(|((r, a, b), n)| json!([
                            kb.rels[*r as usize], kb.types[*a as usize], kb.types[*b as usize], n
                        ])).collect::<Vec<_>>(),
                    })),
                    other => Err(format!("unknown op {other:?}")),
                };
                r.unwrap_or_else(|e| json!({"ok": false, "error": e}))
            }
        };
        if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}
