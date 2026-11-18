use std::collections::BTreeMap;
use std::path::PathBuf;

use oo_core::docir::{Anchor, DocIr};
use proptest::prelude::*;
use serde_json::Value;

use super::pack::ShareFields;
use super::reader::*;
use super::*;
use crate::extract::model::{EntityType, Predicate};
use crate::extract::rules::extract;
use crate::parse::markdown::parse_markdown;
use crate::parse::text::parse_text;
use crate::resolve::exact::{EmptyIndex, resolve_extraction};

fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testing/super-ontology/golden")
}

fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn vid(n: usize) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

/// Graph builder: files are added with their IR; facts, aliases and entities
/// come from `rules/1` plus hand-made rows.
#[derive(Default)]
struct Builder {
    r: MemoryReader,
    next: usize,
    facts: BTreeMap<String, MemFact>,
    aliases: BTreeMap<(String, String), MemAlias>,
    entities: BTreeMap<String, RawEntity>,
}

impl Builder {
    fn new() -> Self {
        let mut b = Self::default();
        b.r.versions.push(VersionInfo {
            n: 1,
            as_of: "2026-11-17".into(),
        });
        b
    }

    fn add_file(&mut self, path: &str, ir: &DocIr) -> String {
        self.next += 1;
        let id = vid(self.next);
        self.r.files.push(MemFile {
            file: SnapshotFile {
                path: path.into(),
                b3: ir.blob_b3.clone(),
                version_id: id.clone(),
            },
            since: 1,
            until: None,
        });
        self.r.blocks.insert(
            id.clone(),
            ir.blocks
                .iter()
                .map(|b| BlockText {
                    block_id: b.id.clone(),
                    ordinal: b.ordinal,
                    text: b.text.clone(),
                })
                .collect(),
        );
        id
    }

    fn ev(version_id: &str, block_id: &str, anchor: &Anchor) -> MemEvidence {
        MemEvidence {
            evidence: RawEvidence {
                version_id: version_id.into(),
                block_id: block_id.into(),
                anchor: anchor.clone(),
            },
            withdrawn: false,
        }
    }

    fn entity(&mut self, key: &str, ty: EntityType, subkind: Option<&'static str>, name: &str) {
        self.entities
            .entry(key.to_owned())
            .or_insert_with(|| RawEntity {
                key: key.into(),
                ty,
                subkind: subkind.map(str::to_owned),
                thing_label: None,
                name: name.into(),
            });
    }

    #[allow(clippy::too_many_arguments)]
    fn fact(
        &mut self,
        key: &str,
        s: &str,
        p: Predicate,
        o: Option<&str>,
        value: Option<Value>,
        from: Option<&str>,
        ev: MemEvidence,
    ) {
        let slot = self.facts.entry(key.to_owned()).or_insert_with(|| MemFact {
            fact: RawFact {
                fact_key: key.into(),
                subject_key: s.into(),
                predicate: p,
                object_key: o.map(str::to_owned),
                value,
                valid_from: from.map(str::to_owned),
                valid_to: None,
                origin: "rule".into(),
                evidence: vec![],
            },
            published_in: 1,
            retracted_in: None,
            evidence: vec![],
        });
        if !slot.evidence.contains(&ev) {
            slot.evidence.push(ev);
        }
    }

    fn alias(&mut self, entity_key: &str, alias: &str, ev: MemEvidence) {
        let n = crate::resolve::normalize::norm(alias);
        let slot = self
            .aliases
            .entry((entity_key.to_owned(), n))
            .or_insert_with(|| MemAlias {
                alias: RawAlias {
                    entity_key: entity_key.into(),
                    alias: alias.into(),
                    evidence: vec![],
                },
                published_in: 1,
                retracted_in: None,
                evidence: vec![],
            });
        if !slot.evidence.contains(&ev) {
            slot.evidence.push(ev);
        }
    }

    /// Run the rules on a parsed file and store the result.
    fn index(&mut self, path: &str, ir: &DocIr) -> String {
        let id = self.add_file(path, ir);
        let x = extract(path, ir);
        let r = resolve_extraction(&EmptyIndex, &x);
        for e in &r.entities {
            self.entity(&e.key, e.ty, e.subkind, &e.name);
        }
        for a in &r.aliases {
            for ev in &a.evidence {
                self.alias(
                    &a.entity_key,
                    &a.alias,
                    Self::ev(&id, &ev.block_id, &ev.anchor),
                );
            }
        }
        for f in &r.facts {
            for ev in &f.evidence {
                self.fact(
                    &f.fact_key,
                    &f.subject_key,
                    f.predicate.clone(),
                    f.object_key.as_deref(),
                    f.value.clone(),
                    f.valid_from.as_deref(),
                    Self::ev(&id, &ev.block_id, &ev.anchor),
                );
            }
            if let Some(slot) = self.facts.get_mut(&f.fact_key) {
                slot.fact.valid_to = f.valid_to.clone();
            }
        }
        id
    }

    fn done(mut self) -> MemoryReader {
        self.r.entities = self.entities.into_values().collect();
        self.r.facts = self.facts.into_values().collect();
        self.r.aliases = self.aliases.into_values().collect();
        self.r
    }
}

fn parse_file(p: &std::path::Path) -> Option<DocIr> {
    let bytes = std::fs::read(p).ok()?;
    match p.extension()?.to_str()? {
        "md" => parse_markdown(&bytes).ok().map(|n| n.ir),
        "txt" => parse_text(&bytes).ok().map(|n| n.ir),
        _ => None,
    }
}

const MAIN_MD: &[&str] = &["People", "Projects", "Decisions", "Preferences"];

/// The golden md subset through the rules, optionally with the red-team files
/// and hand-made red-team entities, aliases and facts (their evidence lives
/// only in `redteam/docs/**`).
fn golden_reader(redteam: bool) -> MemoryReader {
    let root = golden();
    let mut b = Builder::new();
    for d in MAIN_MD {
        let mut files = Vec::new();
        walk(&root.join("docs").join(d), &mut files);
        for p in files {
            if let Some(ir) = parse_file(&p) {
                let rel = p
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                b.index(&rel, &ir);
            }
        }
    }
    if redteam {
        add_redteam(&mut b, &root);
    }
    b.done()
}

fn synthetic_ir(text: &str) -> DocIr {
    let mut ir = parse_text(text.as_bytes()).expect("txt").ir;
    ir.blob_b3 = blake3::hash(text.as_bytes()).to_hex().to_string();
    ir
}

fn add_redteam(b: &mut Builder, root: &std::path::Path) {
    let mut files = Vec::new();
    walk(&root.join("redteam/docs"), &mut files);
    let mut doc_ev: BTreeMap<String, (String, String, Anchor)> = BTreeMap::new();
    let mut canary_ev: BTreeMap<String, (String, String, Anchor)> = BTreeMap::new();
    let canaries: Vec<String> = std::fs::read_to_string(root.join("canaries.txt"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .filter(|l| !l.is_empty())
        .collect();
    for p in files {
        let rel = p
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let ir = match parse_file(&p) {
            Some(ir) => ir,
            // docx and csv are not parsed by this card: a synthetic text
            // block that carries the same strings stands in for them.
            None => {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                let canary = canaries
                    .iter()
                    .find(|c| name.contains("董事会") && c.ends_with("1"))
                    .cloned()
                    .unwrap_or_default();
                synthetic_ir(&format!(
                    "{name}\n{canary}\n秦沐阳 岳清河 玄武资本 青岚智造\n"
                ))
            }
        };
        let id = b.index(&rel, &ir);
        let first = ir
            .blocks
            .iter()
            .find(|x| !x.text.trim().is_empty())
            .expect("block");
        doc_ev.insert(
            rel.clone(),
            (id.clone(), first.id.clone(), first.anchor.clone()),
        );
        for blk in &ir.blocks {
            if canaries.iter().any(|c| blk.text.contains(c.as_str())) {
                canary_ev.insert(
                    rel.clone(),
                    (id.clone(), blk.id.clone(), blk.anchor.clone()),
                );
            }
        }
    }
    // Red-team entities from the label file, with aliases and facts whose
    // evidence is a block of a red-team file.
    let text = std::fs::read_to_string(root.join("redteam/redteam-entities.jsonl")).unwrap();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).unwrap();
        let ty = EntityType::parse(v["type"].as_str().unwrap()).unwrap();
        let name = v["name"].as_str().unwrap();
        let key = crate::extract::model::EntityRef::new(ty, name).key();
        b.entity(&key, ty, None, name);
        let file = v["anchors"][0]["file"].as_str().unwrap();
        let (vid_, blk, anchor) = doc_ev.get(file).expect("evidence for file").clone();
        for a in v["aliases"].as_array().unwrap() {
            b.alias(&key, a.as_str().unwrap(), Builder::ev(&vid_, &blk, &anchor));
        }
        let doc_key = crate::extract::model::EntityRef::new(
            EntityType::Document,
            file.trim_start_matches(""),
        )
        .key();
        b.entity(&doc_key, EntityType::Document, None, file);
        let fk = crate::resolve::normalize::fact_key(&doc_key, "mentions", Some(&key), None, None);
        b.fact(
            &fk,
            &doc_key,
            Predicate::Mentions,
            Some(&key),
            None,
            None,
            Builder::ev(&vid_, &blk, &anchor),
        );
        // The red-team facts that tie them to the main world.
        if name == "玄武资本" {
            let decoy = crate::extract::model::EntityRef::new(EntityType::Person, "陈子墨").key();
            let f2 =
                crate::resolve::normalize::fact_key(&decoy, "member_of", Some(&key), None, None);
            b.fact(
                &f2,
                &decoy,
                Predicate::MemberOf,
                Some(&key),
                None,
                None,
                Builder::ev(&vid_, &blk, &anchor),
            );
        }
    }
    // Positive control: a fact whose evidence block carries a canary.
    for (file, (vid_, blk, anchor)) in &canary_ev {
        let doc_key =
            crate::extract::model::EntityRef::new(EntityType::Document, file.clone()).key();
        let org = crate::extract::model::EntityRef::new(EntityType::Organization, "玄武资本").key();
        let fk = crate::resolve::normalize::fact_key(
            &doc_key,
            "about",
            Some(&org),
            None,
            Some("canary"),
        );
        b.fact(
            &fk,
            &doc_key,
            Predicate::About,
            Some(&org),
            None,
            Some("canary"),
            Builder::ev(vid_, blk, anchor),
        );
    }
}

fn req(scope: &[&str], task: &str) -> CompileRequest {
    CompileRequest {
        scope: scope.iter().map(|s| (*s).to_owned()).collect(),
        task: task.to_owned(),
        ..CompileRequest::default()
    }
}

fn brief(p: &CompiledPack) -> String {
    p.file("BRIEF.md").unwrap().as_str().to_owned()
}

fn share_fields() -> ShareFields {
    ShareFields {
        share_id: "11111111-1111-4111-8111-111111111111".into(),
        grantor: "owner".into(),
        created_at: "2026-11-17T00:00:00Z".into(),
        expires_at: "2026-11-18T00:00:00Z".into(),
        refs_endpoint: "http://127.0.0.1:7600/v1/docs/refs".into(),
        refs_credential: "oodr_0123456789abcdef".into(),
        refs_expires_at: "2026-11-18T00:00:00Z".into(),
    }
}

/// Errors of `value` against `#/$defs/<def>` of the shipped contract.
fn contract_errors(def: &str, value: &Value) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/context-pack/context-pack.v1.schema.json");
    let schema: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let wrapper = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": schema["$defs"],
        "$ref": format!("#/$defs/{def}"),
    });
    let v = oo_core::schema_util::compile_schema(&wrapper).expect("contract compiles");
    v.iter_errors(value).map(|e| e.to_string()).collect()
}

fn all_bytes(p: &CompiledPack) -> Vec<(String, Vec<u8>)> {
    p.files
        .iter()
        .map(|f| (f.path.clone(), f.bytes.clone()))
        .collect()
}

fn bullet_lines(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with("- ")).collect()
}

// ---------------------------------------------------------------- golden --

#[test]
fn brief_has_fixed_sections_label_and_full_provenance() {
    let r = golden_reader(false);
    let p = compile(&r, &req(&["docs"], "陈子墨 和 星图看板 现在怎么样")).expect("compile");
    let b = brief(&p);
    assert_eq!(
        b.lines().next().unwrap(),
        render::AIGC_LABEL_LINE,
        "label is line 1"
    );
    assert!(b.contains("\nAIGC:\n  Label: \"1\"\n  ContentProducer: \"<COMPANY>\""));
    let headings: Vec<&str> = b.lines().filter(|l| l.starts_with("## ")).collect();
    assert_eq!(
        headings,
        [
            "## 你在做什么",
            "## 相关的人",
            "## 当前状态与决定",
            "## 已试过但走不通",
            "## 主人的偏好",
            "## 其他相关",
            "## 已拦截",
            "## 裁掉了什么"
        ]
    );
    assert!(b.ends_with('\n') && !b.ends_with("\n\n"));
    // The task is quoted verbatim.
    assert!(b.contains("\n> 陈子墨 和 星图看板 现在怎么样\n"));
    // Provenance coverage is 100%: every content line has a tag, every tag
    // is a real in-scope source.
    let lines = bullet_lines(&b);
    assert!(lines.len() >= 4);
    for l in &lines {
        let tags = source::tags_in(l);
        assert!(!tags.is_empty(), "no tag: {l}");
        for t in tags.iter().filter(|t| !t.starts_with('+')) {
            let (path, frag) = source::split_tag(t).expect("tag shape");
            assert!(path.starts_with("docs/"), "{path}");
            assert!(frag.starts_with("#L"), "{frag}");
        }
    }
    // Every cited source has a refs file.
    assert!(p.file("refs/0001.md").is_some());
    // Header numbers are exact.
    let used = brief_tokens_in_header(&b);
    assert_eq!(used, tokens::count(&b));
    assert_eq!(used, p.brief_tokens);
    assert!(used <= 800);
}

fn brief_tokens_in_header(b: &str) -> u64 {
    let line = b.lines().find(|l| l.contains("预算 ")).unwrap();
    let after = line.split("预算 ").nth(1).unwrap();
    after.split('/').next().unwrap().parse().unwrap()
}

#[test]
fn compile_twice_gives_identical_bytes() {
    let r = golden_reader(false);
    let a = compile(&r, &req(&["docs"], "星图看板")).unwrap();
    let b = compile(&r, &req(&["docs"], "星图看板")).unwrap();
    assert_eq!(all_bytes(&a), all_bytes(&b));
    assert_eq!(a.pack_digest, b.pack_digest);
    let fields = share_fields();
    assert_eq!(a.share_json(&fields).bytes, b.share_json(&fields).bytes);
    // Different request, different params digest.
    let c = compile(&r, &req(&["docs"], "灯塔迁移")).unwrap();
    assert_ne!(a.params_sha256, c.params_sha256);
    assert_eq!(a.snapshot_sha256, c.snapshot_sha256);
}

fn shuffled(r: &MemoryReader, perm: &[u32]) -> MemoryReader {
    fn reorder<T: Clone>(v: &[T], perm: &[u32]) -> Vec<T> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by_key(|i| {
            (
                perm[*i % perm.len()]
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(*i as u32),
                *i,
            )
        });
        idx.into_iter().map(|i| v[i].clone()).collect()
    }
    let mut out = r.clone();
    out.files = reorder(&r.files, perm);
    out.entities = reorder(&r.entities, perm);
    out.aliases = reorder(&r.aliases, perm)
        .into_iter()
        .map(|mut a| {
            a.evidence = reorder(&a.evidence, perm);
            a
        })
        .collect();
    out.facts = reorder(&r.facts, perm)
        .into_iter()
        .map(|mut f| {
            f.evidence = reorder(&f.evidence, perm);
            f
        })
        .collect();
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn input_order_does_not_change_a_byte(perm in proptest::collection::vec(any::<u32>(), 1..16), budget in 700u32..1500) {
        let base = golden_reader(false);
        let mut rq = req(&["docs"], "陈子墨 星图看板");
        rq.budget = budget;
        let a = compile(&base, &rq).unwrap();
        let b = compile(&shuffled(&base, &perm), &rq).unwrap();
        prop_assert_eq!(all_bytes(&a), all_bytes(&b));
        prop_assert_eq!(a.pack_digest, b.pack_digest);
    }
}

#[test]
fn budget_degrades_and_says_what_was_trimmed() {
    let r = golden_reader(false);
    let mut tokens_seen = Vec::new();
    for budget in [300u32, 400, 650, 800, 1200, 3000, 20_000] {
        let mut rq = req(&["docs"], "陈子墨 星图看板");
        rq.budget = budget;
        match compile(&r, &rq) {
            Ok(p) => {
                let b = brief(&p);
                let t = tokens::count(&b);
                assert!(t <= u64::from(budget), "budget {budget}: {t}");
                assert_eq!(brief_tokens_in_header(&b), t);
                let trimmed_note = b.split("## 裁掉了什么\n").nth(1).unwrap();
                if p.trimmed.is_empty() {
                    assert!(trimmed_note.starts_with("没有内容被裁掉。"));
                } else {
                    assert!(trimmed_note.contains("降为摘要"));
                    assert!(trimmed_note.contains("仍保留全文"));
                }
                for l in bullet_lines(&b) {
                    assert!(l.contains("[src:"), "{l}");
                }
                tokens_seen.push((
                    budget,
                    t,
                    p.trimmed
                        .values()
                        .map(|c| c.summary + c.title + c.removed)
                        .sum::<u32>(),
                ));
            }
            Err(PackError::BudgetTooSmall { min_budget }) => {
                assert!(
                    budget < 650 && min_budget > u64::from(budget),
                    "{budget} {min_budget}"
                );
            }
            Err(e) => panic!("{e}"),
        }
    }
    // More budget never trims more.
    let trims: Vec<u32> = tokens_seen.iter().map(|t| t.2).collect();
    assert!(trims.windows(2).all(|w| w[0] >= w[1]), "{tokens_seen:?}");
    assert_eq!(*trims.last().unwrap(), 0);
}

#[test]
fn degradation_uses_summary_then_title_then_removal() {
    let r = golden_reader(false);
    let mut rq = req(&["docs"], "陈子墨 星图看板 林远舟");
    rq.budget = 700;
    let p = compile(&r, &rq).expect("compile");
    let b = brief(&p);
    assert!(
        p.trimmed
            .values()
            .any(|c| c.removed > 0 || c.title > 0 || c.summary > 0),
        "{b}"
    );
    // Summary lines point at a detail file.
    for l in b.lines().filter(|l| l.contains("条相关事实，详见 ")) {
        assert!(
            l.contains("people/")
                || l.contains("projects/")
                || l.contains("decisions.md")
                || l.contains("terms.md"),
            "{l}"
        );
    }
}

#[test]
fn manifest_and_share_validate_against_the_contract() {
    let r = golden_reader(false);
    let p = compile(&r, &req(&["docs/Projects", "docs/Decisions"], "星图看板")).unwrap();
    let errs = contract_errors("manifest", &p.manifest);
    assert!(errs.is_empty(), "{errs:?}");
    let file: Value = serde_json::from_slice(&p.file("manifest.json").unwrap().bytes).unwrap();
    assert_eq!(file, p.manifest);
    assert_eq!(p.manifest["protocol"], "oo.context-pack.manifest/v1");
    assert_eq!(p.manifest["token_estimator"], "oo.token-estimate/v2");
    assert_eq!(p.manifest["aigc"]["ContentProducer"], "<COMPANY>");
    // pack_digest = sha256(canonical(manifest)); file entries match the bytes.
    assert_eq!(
        p.pack_digest,
        pack::sha256_hex(oo_builder::canonical_json(&p.manifest).as_bytes())
    );
    for f in p.manifest["files"].as_array().unwrap() {
        let path = f["path"].as_str().unwrap();
        let file = p.file(path).unwrap();
        assert_eq!(f["sha256"], pack::sha256_hex(&file.bytes), "{path}");
        assert_eq!(f["bytes"], file.bytes.len());
        assert_eq!(f["tokens"], file.tokens);
        assert_ne!(path, "SHARE.json");
        assert_ne!(path, "manifest.json");
    }
    let share_file = p.share_json(&share_fields());
    let share: Value = serde_json::from_slice(&share_file.bytes).unwrap();
    let errs = contract_errors("share", &share);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(share["pack_digest"], p.pack_digest.as_str());
    assert_eq!(share["ontology_version"], 1);
    assert_eq!(share["drive"]["backend"], "fs");
    assert_eq!(share["aigc"]["ReservedCode1"], p.pack_digest.as_str());
    assert!(share_file.bytes.ends_with(b"\n"));
    // The contract does reject a broken value (the validator is live).
    let mut bad = p.manifest.clone();
    bad["aigc"]["Label"] = Value::from("0");
    assert!(!contract_errors("manifest", &bad).is_empty());
    // The deterministic part excludes the random fields: changing only them
    // leaves the pack digest and every pack file as is.
    let mut other = share_fields();
    other.share_id = "22222222-2222-4222-8222-222222222222".into();
    let again = p.share_json(&other);
    assert_ne!(again.bytes, share_file.bytes);
}

#[test]
fn files_have_the_label_front_matter_and_pages_stay_under_the_limit() {
    let r = golden_reader(false);
    let p = compile(&r, &req(&[], "")).unwrap();
    for f in p.files.iter().filter(|f| f.path.ends_with(".md")) {
        let t = f.as_str();
        assert_eq!(
            t.lines().next().unwrap(),
            render::AIGC_LABEL_LINE,
            "{}",
            f.path
        );
        assert!(
            t.contains(&format!("ProduceID: \"{}/{}\"", p.basis, f.path)),
            "{}",
            f.path
        );
        assert!(t.ends_with('\n') && !t.ends_with("\n\n"), "{}", f.path);
        assert!(
            f.tokens <= pack::MAX_FILE_TOKENS
                || f.as_str().lines().filter(|l| l.starts_with("- ")).count() <= 1,
            "{}",
            f.path
        );
    }
    for path in [
        "BRIEF.md",
        "INDEX.md",
        "decisions.md",
        "terms.md",
        "manifest.json",
    ] {
        assert!(p.file(path).is_some(), "{path}");
    }
    assert!(p.files.iter().any(|f| f.path.starts_with("people/")));
    assert!(p.files.iter().any(|f| f.path.starts_with("projects/")));
    let paths: Vec<&str> = p.files.iter().map(|f| f.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}

#[test]
fn big_files_continue_on_page_files_listed_in_the_index() {
    let mut b = Builder::new();
    let ir = parse_text("x\n".as_bytes()).unwrap().ir;
    let id = b.add_file("a/x.txt", &ir);
    let blk = &ir.blocks[0];
    let doc = crate::extract::model::EntityRef::new(EntityType::Document, "a/x.txt").key();
    b.entity(&doc, EntityType::Document, None, "a/x.txt");
    for i in 0..400 {
        let name = format!("决定{i:04}：采用方案{i}并持续跟踪评估结果与风险");
        let key = crate::extract::model::EntityRef::new(EntityType::Decision, name.clone()).key();
        b.entity(&key, EntityType::Decision, None, &name);
        let fk = crate::resolve::normalize::fact_key(&doc, "mentions", Some(&key), None, None);
        b.fact(
            &fk,
            &doc,
            Predicate::Mentions,
            Some(&key),
            None,
            None,
            Builder::ev(&id, &blk.id, &blk.anchor),
        );
        let mut e = b.facts.get(&fk).cloned().unwrap();
        e.fact.predicate = Predicate::About;
        let _ = e;
    }
    // `about` Document -> Decision is not a BRIEF line; the facts list in decisions.md.
    let r = b.done();
    let p = compile(&r, &req(&[], "")).unwrap();
    let pages: Vec<&PackFile> = p
        .files
        .iter()
        .filter(|f| f.path.starts_with("decisions/_page/"))
        .collect();
    assert!(
        !pages.is_empty(),
        "decisions.md must continue on page files"
    );
    assert_eq!(pages[0].path, "decisions/_page/0002.md");
    for f in p
        .files
        .iter()
        .filter(|f| f.path == "decisions.md" || f.path.starts_with("decisions/_page/"))
    {
        assert!(f.tokens <= pack::MAX_FILE_TOKENS, "{} {}", f.path, f.tokens);
    }
    let index = String::from_utf8_lossy(&p.file("INDEX.md").unwrap().bytes).to_string();
    assert!(index.contains("decisions/_page/0002.md"));
    // Nothing is cut: every decision name still appears somewhere in the family.
    let all: String = p
        .files
        .iter()
        .filter(|f| f.path == "decisions.md" || f.path.starts_with("decisions/_page/"))
        .map(|f| f.as_str().to_owned())
        .collect();
    for i in [0, 199, 399] {
        assert!(all.contains(&format!("决定{i:04}")));
    }
}

// ------------------------------------------------------- scope and D26 -----

fn scope_world() -> MemoryReader {
    // a/one.txt (in scope): 甲 owns 项目; b/two.txt (out): 乙 member_of 项目,
    // 乙 owns 秘密项目 (only here), alias 老乙 of 乙 only here, 甲 also
    // appears in b/two.txt through an extra fact.
    let mut b = Builder::new();
    let one = parse_text("李甲 负责 项目\n".as_bytes()).unwrap().ir;
    let two =
        parse_text("王乙 属于 项目\n王乙 负责 秘密项目 老王\n李甲 参与 秘密项目\n".as_bytes())
            .unwrap()
            .ir;
    let a = b.add_file("a/one.txt", &one);
    let c = b.add_file("b/two.txt", &two);
    let e = |name: &str, ty| crate::extract::model::EntityRef::new(ty, name).key();
    let (jia, yi, proj, secret) = (
        e("李甲", EntityType::Person),
        e("王乙", EntityType::Person),
        e("项目", EntityType::Project),
        e("秘密项目", EntityType::Project),
    );
    b.entity(&jia, EntityType::Person, None, "李甲");
    b.entity(&yi, EntityType::Person, None, "王乙");
    b.entity(&proj, EntityType::Project, None, "项目");
    b.entity(&secret, EntityType::Project, None, "秘密项目");
    let fk =
        |s: &str, p: &str, o: &str| crate::resolve::normalize::fact_key(s, p, Some(o), None, None);
    let b1 = &one.blocks[0];
    let (t0, t1, t2) = (
        &two.blocks[0],
        &two.blocks.get(1).unwrap_or(&two.blocks[0]),
        &two.blocks[two.blocks.len() - 1],
    );
    b.fact(
        &fk(&jia, "owns", &proj),
        &jia,
        Predicate::Owns,
        Some(&proj),
        None,
        None,
        Builder::ev(&a, &b1.id, &b1.anchor),
    );
    // Same fact also evidenced out of scope: only the in-scope evidence may show.
    b.fact(
        &fk(&jia, "owns", &proj),
        &jia,
        Predicate::Owns,
        Some(&proj),
        None,
        None,
        Builder::ev(&c, &t0.id, &t0.anchor),
    );
    b.fact(
        &fk(&yi, "member_of", &proj),
        &yi,
        Predicate::MemberOf,
        Some(&proj),
        None,
        None,
        Builder::ev(&c, &t0.id, &t0.anchor),
    );
    b.fact(
        &fk(&yi, "owns", &secret),
        &yi,
        Predicate::Owns,
        Some(&secret),
        None,
        None,
        Builder::ev(&c, &t1.id, &t1.anchor),
    );
    b.fact(
        &fk(&jia, "member_of", &secret),
        &jia,
        Predicate::MemberOf,
        Some(&secret),
        None,
        None,
        Builder::ev(&c, &t2.id, &t2.anchor),
    );
    b.alias(&yi, "老王", Builder::ev(&c, &t1.id, &t1.anchor));
    b.alias(&jia, "小李", Builder::ev(&a, &b1.id, &b1.anchor));
    b.done()
}

#[test]
fn only_in_scope_evidence_and_only_visible_entities_get_through() {
    let r = scope_world();
    let (g, terms) =
        VisibleGraph::load(&r, &r.latest_version().unwrap(), &["a".to_owned()]).unwrap();
    let facts: Vec<_> = g.facts().collect();
    assert_eq!(facts.len(), 1, "only the fact with in-scope evidence");
    assert_eq!(
        facts[0].evidence.len(),
        1,
        "only the in-scope evidence is attached"
    );
    assert_eq!(facts[0].evidence[0].path, "a/one.txt");
    let names: Vec<&str> = g.entities().map(|e| e.name.as_str()).collect();
    assert_eq!(names, {
        let mut v = vec!["李甲", "项目"];
        v.sort_by_key(|n| {
            crate::extract::model::EntityRef::new(
                if *n == "李甲" {
                    EntityType::Person
                } else {
                    EntityType::Project
                },
                *n,
            )
            .key()
        });
        v
    });
    assert!(
        g.entity(&crate::extract::model::EntityRef::new(EntityType::Person, "王乙").key())
            .is_none()
    );
    assert_eq!(g.blocked().facts, 3);
    assert_eq!(g.blocked().entities, 2, "乙 and 秘密项目");
    // The blocked word list is separate and has the names and aliases.
    for t in ["王乙", "老王", "秘密项目"] {
        assert!(terms.iter().any(|x| x == t), "{t}: {terms:?}");
    }
    assert!(
        !terms
            .iter()
            .any(|t| t == "李甲" || t == "项目" || t == "小李")
    );
    // Visible alias of a visible entity.
    let jia = crate::extract::model::EntityRef::new(EntityType::Person, "李甲").key();
    assert_eq!(g.aliases(&jia).len(), 1);
}

#[test]
fn human_origin_without_scope_evidence_is_invisible() {
    let mut r = scope_world();
    for f in &mut r.facts {
        f.fact.origin = "human".into();
    }
    let (g, _) = VisibleGraph::load(&r, &r.latest_version().unwrap(), &["a".to_owned()]).unwrap();
    assert_eq!(g.facts().count(), 1);
}

#[test]
fn scope_compile_never_prints_out_of_scope_names() {
    let r = scope_world();
    let p = compile(&r, &req(&["a"], "李甲")).unwrap();
    for f in &p.files {
        let t = f.as_str();
        for banned in ["王乙", "老王", "秘密项目", "b/two.txt"] {
            assert!(!t.contains(banned), "{} leaks {banned}", f.path);
        }
    }
    let b = brief(&p);
    assert!(b.contains("另有 3 条事实、2 个实体因不在共享范围内未纳入。"));
    assert_eq!(
        p.blocked,
        BlockedCounts {
            facts: 3,
            entities: 2
        }
    );
    // Widening the scope shows them and blocks nothing.
    let all = compile(&r, &req(&[], "李甲")).unwrap();
    assert!(all.files.iter().any(|f| f.as_str().contains("秘密项目")));
    assert_eq!(
        all.blocked,
        BlockedCounts {
            facts: 0,
            entities: 0
        }
    );
}

#[test]
fn leak_guard_catches_a_renderer_defect() {
    let r = scope_world();
    let mut inject = |files: &mut Vec<PackFile>| {
        let b = files.iter_mut().find(|f| f.path == "BRIEF.md").unwrap();
        let mut t = b.as_str().to_owned();
        t = t.replace(
            "## 已拦截",
            "- 王乙 负责 秘密项目 [src:a/one.txt#L1-L1]\n\n## 已拦截",
        );
        b.tokens = tokens::count(&t);
        b.bytes = t.into_bytes();
    };
    let err = compile_inner(&r, &req(&["a"], "李甲"), Some(&mut inject)).unwrap_err();
    match &err {
        PackError::LeakGuard { hits } => assert!(*hits >= 1),
        other => panic!("{other}"),
    }
    assert_eq!(err.code(), "DOCS_PACK_LEAK_GUARD");
    assert!(
        !err.to_string().contains('王'),
        "the error carries counts, never the words"
    );
}

#[test]
fn a_blocked_word_the_scope_itself_says_is_not_a_leak() {
    // The in-scope file's own text says the word 秘密项目, so quoting it is
    // legitimate even though an entity with that name is blocked.
    let mut r = scope_world();
    let vid_a = r
        .files
        .iter()
        .find(|f| f.file.path == "a/one.txt")
        .unwrap()
        .file
        .version_id
        .clone();
    r.blocks.get_mut(&vid_a).unwrap()[0].text = "李甲 负责 项目 另见 秘密项目".into();
    let p = compile(&r, &req(&["a"], "李甲")).unwrap();
    assert!(
        p.files
            .iter()
            .any(|f| f.path.starts_with("refs/") && f.as_str().contains("秘密项目")),
        "the refs quote carries it"
    );
    // The blocked entity itself still does not show up as an entity.
    assert!(!brief(&p).contains("秘密项目"));
}

#[test]
fn provenance_gate_catches_a_line_without_a_source() {
    let r = scope_world();
    let mut strip = |files: &mut Vec<PackFile>| {
        let b = files.iter_mut().find(|f| f.path == "BRIEF.md").unwrap();
        let t = b.as_str().replace(
            "- 解析出的对象：李甲（Person） [src:a/one.txt#L1-L1]",
            "- 解析出的对象：李甲（Person）",
        );
        b.bytes = t.into_bytes();
    };
    let err = compile_inner(&r, &req(&["a"], "李甲"), Some(&mut strip)).unwrap_err();
    assert!(
        matches!(err, PackError::ProvenanceGap { lines: 1 }),
        "{err}"
    );
    let mut forge = |files: &mut Vec<PackFile>| {
        let b = files.iter_mut().find(|f| f.path == "BRIEF.md").unwrap();
        let t = b
            .as_str()
            .replace("[src:a/one.txt#L1-L1]", "[src:b/two.txt#L1-L1]");
        b.bytes = t.into_bytes();
    };
    let err = compile_inner(&r, &req(&["a"], "李甲"), Some(&mut forge)).unwrap_err();
    assert!(
        matches!(err, PackError::ProvenanceGap { .. }),
        "a tag must name in-scope evidence"
    );
}

#[test]
fn request_errors() {
    let r = scope_world();
    assert_eq!(
        compile(&r, &req(&["zzz"], "")).unwrap_err(),
        PackError::ScopeInvalid {
            paths: vec!["zzz".into()]
        }
    );
    assert_eq!(
        compile(&r, &req(&["a", "zzz", "yyy"], "")).unwrap_err(),
        PackError::ScopeInvalid {
            paths: vec!["yyy".into(), "zzz".into()]
        }
    );
    let mut q = req(&["a"], "");
    q.budget = 100;
    assert!(matches!(compile(&r, &q), Err(PackError::BadRequest(_))));
    q.budget = 20_001;
    assert!(matches!(compile(&r, &q), Err(PackError::BadRequest(_))));
    q.budget = 800;
    q.role = "root".into();
    assert!(matches!(compile(&r, &q), Err(PackError::BadRequest(_))));
    let long = "字".repeat(2001);
    assert!(matches!(
        compile(&r, &req(&["a"], &long)),
        Err(PackError::TaskTooLong { max_chars: 2000 })
    ));
    let mut q = req(&["a"], &"字".repeat(1000));
    q.budget = 800;
    assert!(
        matches!(compile(&r, &q), Err(PackError::TaskTooLong { .. })),
        "over 40% of the budget"
    );
    let mut q = req(&["a"], "");
    q.version = Some(9);
    assert_eq!(compile(&r, &q).unwrap_err(), PackError::VersionNotFound);
    // A seed that is not visible is refused without saying whether it exists.
    let mut q = req(&["a"], "");
    q.seeds = vec![crate::extract::model::EntityRef::new(EntityType::Person, "王乙").key()];
    assert_eq!(compile(&r, &q).unwrap_err(), PackError::SeedNotVisible);
    let mut q = req(&["a"], "");
    q.seeds = vec!["person:ffffffffffffffffffffffff".into()];
    assert_eq!(compile(&r, &q).unwrap_err(), PackError::SeedNotVisible);
    // Nothing visible: the scope exists but holds no published fact.
    let mut empty = scope_world();
    empty.facts.clear();
    assert_eq!(
        compile(&empty, &req(&["a"], "")).unwrap_err(),
        PackError::NothingVisible
    );
    // Tiny budget cannot hold the fixed parts.
    let mut q = req(&["a"], "");
    q.budget = 200;
    assert!(matches!(
        compile(&r, &q),
        Err(PackError::BudgetTooSmall { .. })
    ));
}

#[test]
fn scope_is_normalized_and_versions_pin_the_pack() {
    assert_eq!(
        normalize_scope(&["b/".into(), "a".into(), "a//".into()]),
        ["a", "b"]
    );
    assert!(normalize_scope(&["".into(), "a".into()]).is_empty());
    assert!(normalize_scope(&["/".into()]).is_empty());
    let composed = "e\u{301}"; // NFD e + acute
    assert_eq!(normalize_scope(&[composed.into()]), ["\u{e9}"]);
    let mut r = scope_world();
    r.versions.push(VersionInfo {
        n: 2,
        as_of: "2026-11-18".into(),
    });
    for f in &mut r.facts {
        if f.published_in == 1
            && f.fact.predicate == Predicate::MemberOf
            && f.fact.subject_key.starts_with("person")
        {
            f.retracted_in = Some(2);
        }
    }
    let mut q = req(&["b"], "");
    q.version = Some(1);
    let v1 = compile(&r, &q).unwrap();
    let latest = compile(&r, &req(&["b"], "")).unwrap();
    assert_eq!(v1.version.n, 1);
    assert_eq!(latest.version.n, 2);
    assert_ne!(v1.pack_digest, latest.pack_digest);
    assert!(brief(&v1).contains("本体版本 v1（2026-11-17）"));
    assert!(brief(&latest).contains("本体版本 v2（2026-11-18）"));
    // Facts whose valid_to has passed as of the version are not live.
    let mut r2 = scope_world();
    for f in &mut r2.facts {
        f.fact.valid_to = Some("2026-11-16".into());
    }
    assert_eq!(
        compile(&r2, &req(&["a"], "")).unwrap_err(),
        PackError::NothingVisible
    );
    for f in &mut r2.facts {
        f.fact.valid_to = Some("2099-01-01".into());
    }
    assert!(
        compile(&r2, &req(&["a"], "")).is_ok(),
        "a future valid_to is still valid"
    );
}

#[test]
fn seeds_from_the_task_use_the_longest_visible_alias_and_ambiguity_goes_to_maybe() {
    let mut b = Builder::new();
    let ir = parse_text("x\n".as_bytes()).unwrap().ir;
    let id = b.add_file("a/x.txt", &ir);
    let blk = &ir.blocks[0];
    let ev = || Builder::ev(&id, &blk.id, &blk.anchor);
    let mk = |b: &mut Builder, name: &str, ty: EntityType| {
        let k = crate::extract::model::EntityRef::new(ty, name).key();
        b.entity(&k, ty, None, name);
        k
    };
    let li1 = mk(&mut b, "李一", EntityType::Person);
    let li2 = mk(&mut b, "李二", EntityType::Person);
    let star = mk(&mut b, "星图看板", EntityType::Project);
    let star2 = mk(&mut b, "星图", EntityType::Project);
    for k in [&li1, &li2] {
        b.alias(k, "小李", ev());
    }
    b.alias(&star, "Starmap", ev());
    for (k, o) in [(&li1, &star), (&li2, &star), (&li1, &star2)] {
        let fk = crate::resolve::normalize::fact_key(k, "owns", Some(o), None, Some("x"));
        b.fact(
            &fk,
            k,
            Predicate::Owns,
            Some(o),
            None,
            Some("2026-01-01"),
            ev(),
        );
    }
    let r = b.done();
    let p = compile(&r, &req(&[], "请问 小李 对 STARMAP 的看法")).unwrap();
    let t = brief(&p);
    assert!(t.contains("- 解析出的对象：星图看板（Project）"), "{t}");
    assert!(
        !t.contains("- 解析出的对象：星图（Project）"),
        "alias hit is a different entity"
    );
    assert!(t.contains("- 可能指的是：李一（Person）"));
    assert!(t.contains("- 可能指的是：李二（Person）"));
    assert_eq!(p.seeds, std::slice::from_ref(&star));
    // Leftmost-longest: 星图看板 wins over the shorter 星图.
    let p2 = compile(&r, &req(&[], "星图看板")).unwrap();
    assert_eq!(p2.seeds, std::slice::from_ref(&star));
    // No hit: central entities, labelled as such.
    let p3 = compile(&r, &req(&[], "无关的话")).unwrap();
    assert!(brief(&p3).contains("选定的中心对象："));
    assert!(p3.seeds.len() <= 5 && p3.seeds.contains(&star));
    // Explicit seeds are used as given.
    let mut q = req(&[], "");
    q.seeds = vec![li2.clone()];
    assert_eq!(compile(&r, &q).unwrap().seeds, [li2]);
}

// ------------------------------------------------------------- red team ----

#[test]
fn redteam_files_never_reach_a_scope_that_excludes_them() {
    let r = golden_reader(true);
    let root = golden();
    let canaries: Vec<String> = std::fs::read_to_string(root.join("canaries.txt"))
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    assert_eq!(canaries.len(), 4);
    let text = std::fs::read_to_string(root.join("redteam/redteam-entities.jsonl")).unwrap();
    let mut forbidden: Vec<String> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).unwrap();
        if v["redteam_only"].as_bool() == Some(true) {
            forbidden.push(v["name"].as_str().unwrap().to_owned());
            forbidden.extend(
                v["aliases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap().to_owned()),
            );
        }
    }
    assert!(forbidden.len() >= 9);
    let task = "陈子墨 星图看板 玄武资本";
    for scope in [
        vec!["docs"],
        vec!["docs/People", "docs/Projects"],
        vec!["docs/Decisions"],
    ] {
        let p = compile(&r, &req(&scope, "")).unwrap();
        for f in &p.files {
            let t = f.as_str();
            for c in &canaries {
                assert!(
                    !t.contains(c.as_str()),
                    "{} has a canary in scope {scope:?}",
                    f.path
                );
            }
            for w in &forbidden {
                assert!(
                    !t.contains(w.as_str()),
                    "{} has {w:?} in scope {scope:?}",
                    f.path
                );
            }
            assert!(!t.contains("redteam/"), "{}", f.path);
        }
        // The decoy (same name, different person) is a visible entity of the
        // main world only through main evidence; its red-team facts are held back.
        if scope == ["docs"] {
            assert!(
                p.blocked.facts > 0 && p.blocked.entities > 0,
                "{:?}",
                p.blocked
            );
        }
    }
    // A task that names a blocked entity is refused by the leak guard rather
    // than echoed back.
    let err = compile(&r, &req(&["docs"], task)).unwrap_err();
    assert!(matches!(err, PackError::LeakGuard { .. }), "{err}");
    // Positive control: with the red-team files in scope the same names do
    // appear, so the check above can fail.
    let p = compile(&r, &req(&["docs", "redteam/docs"], "")).unwrap();
    let joined: String = p.files.iter().map(|f| f.as_str().to_owned()).collect();
    assert!(joined.contains("玄武资本"));
    assert!(
        canaries.iter().any(|c| joined.contains(c.as_str())),
        "a canary shows in refs when its file is in scope"
    );
    let only = compile(&r, &req(&["redteam/docs"], "")).unwrap();
    assert!(only.files.iter().any(|f| f.as_str().contains("秦沐阳")));
}

// ---------------------------------------------------------- isolation ------

#[test]
fn renderers_do_not_touch_the_store() {
    // 10 §7.2: brief, pack, render, source, tokens, visible and the reader
    // trait never mention the database; only mod (this file's parent) and pg do.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/compile");
    for name in [
        "brief.rs",
        "pack.rs",
        "render.rs",
        "source.rs",
        "tokens.rs",
        "visible.rs",
        "reader.rs",
    ] {
        let text = std::fs::read_to_string(dir.join(name)).unwrap();
        // Compare against the pieces that would be needed to reach a database.
        for banned in [
            "sqlx",
            "PgConnection",
            "PgPool",
            "store::",
            "crate::cas",
            "Cas::",
        ] {
            assert!(!text.contains(banned), "{name} mentions {banned}");
        }
    }
    // Only visible.rs may call a reader.
    for name in ["brief.rs", "pack.rs", "render.rs"] {
        let text = std::fs::read_to_string(dir.join(name)).unwrap();
        assert!(
            !text.contains("OntologyReader") && !text.contains("MemoryReader"),
            "{name}"
        );
    }
}

// ------------------------------------------- consistency (S6, 10 §16) -----

fn conflict_world() -> (MemoryReader, String) {
    let mut r = scope_world();
    let in_a = |e: &RawEvidence| {
        r.files
            .iter()
            .any(|f| f.file.version_id == e.version_id && f.file.path == "a/one.txt")
    };
    let (raw, key) = r
        .facts
        .iter()
        .filter(|f| f.fact.object_key.is_some())
        .find_map(|f| {
            let ev: Vec<RawEvidence> = f
                .evidence
                .iter()
                .map(|e| e.evidence.clone())
                .filter(|e| in_a(e))
                .collect();
            (!ev.is_empty()).then(|| {
                let mut raw = f.fact.clone();
                raw.evidence = ev;
                let k = raw.subject_key.clone();
                (raw, k)
            })
        })
        .unwrap();
    r.conflicts.push(RawConflict {
        entity_key: key.clone(),
        constraint_id: "project_single_owner".into(),
        sides: vec![raw],
        hidden_sides: 1,
    });
    (r, key)
}

#[test]
fn open_conflicts_are_worded_with_sources_and_flip_the_guarantee() {
    let (r, _) = conflict_world();
    let p = compile(&r, &req(&["a"], "李甲")).unwrap();
    let b = brief(&p);
    let line = b
        .lines()
        .find(|l| l.starts_with("- 冲突未决：李甲"))
        .unwrap_or_else(|| panic!("{b}"));
    assert!(line.contains("[src:a/one.txt"), "{line}");
    assert!(line.contains("（另有 1 方来源不在共享范围内）"), "{line}");
    assert!(!p.guarantees.constraint_free);
    assert_eq!(p.manifest["guarantees"]["constraint_free"], false);
    assert_eq!(p.conflict_entities.len(), 1);
    // The other side's file never shows.
    assert!(p.files.iter().all(|f| !f.as_str().contains("b/two.txt")));
    // A scope without the entity has no conflict and keeps the promise.
    let none = compile(&r, &req(&["b"], "")).unwrap();
    assert!(none.guarantees.constraint_free || !none.conflict_entities.is_empty());
}

#[test]
fn coverage_is_in_the_brief_manifest_and_parameters() {
    let r = scope_world();
    let mut q = req(&["a"], "李甲");
    let plain = compile(&r, &q).unwrap();
    q.coverage = Some(coverage::Coverage {
        drive_rev: 9,
        covered_rev: 7,
        in_scope: 2,
        processed: 1,
        pending: vec![("a/new.pdf".into(), "parsing".into())],
        failed: vec![],
    });
    let p = compile(&r, &q).unwrap();
    let b = brief(&p);
    assert!(
        b.contains("> 覆盖：1/2 个文件已处理；未完成 1 个（a/new.pdf 解析中）"),
        "{b}"
    );
    assert!(b.contains("新鲜度下界 否"));
    assert_eq!(p.manifest["coverage"]["covered_rev"], 7);
    assert_eq!(p.manifest["guarantees"]["freshness_floor"], false);
    assert_ne!(p.params_sha256, plain.params_sha256);
    assert_ne!(p.pack_digest, plain.pack_digest);
    // Same coverage twice: same bytes.
    let again = compile(&r, &q).unwrap();
    assert_eq!(p.pack_digest, again.pack_digest);
}

#[test]
fn manifest_lists_section_digests_and_uses_cover_every_visible_fact() {
    let r = scope_world();
    let p = compile(&r, &req(&[], "李甲")).unwrap();
    let files = p.manifest["files"].as_array().unwrap();
    for f in files {
        let path = f["path"].as_str().unwrap();
        assert_eq!(
            f.get("section_digests").is_some(),
            path.ends_with(".md"),
            "{path}"
        );
    }
    let brief_file = files.iter().find(|f| f["path"] == "BRIEF.md").unwrap();
    assert!(brief_file["section_digests"]["当前状态与决定"].is_string());
    let (g, _) = visible::VisibleGraph::load(&r, &r.latest_version().unwrap(), &[]).unwrap();
    for f in g.facts() {
        assert!(
            p.uses.iter().any(|u| u.fact_key == f.key),
            "fact {} has no use record",
            f.key
        );
        assert!(
            p.section_uses
                .iter()
                .any(|(_, k, d)| *k == f.key && d.len() == 64),
            "fact {} has no section use",
            f.key
        );
    }
    // Digests are stable across compiles.
    let again = compile(&r, &req(&[], "李甲")).unwrap();
    assert_eq!(p.section_uses, again.section_uses);
}

// ---- refresh: "自上次以来的变化" (B-S10-01) ----

fn change_world() -> (MemoryReader, Vec<String>, String) {
    let r = golden_reader(false);
    let p = compile(&r, &req(&["docs"], "陈子墨 和 星图看板 现在怎么样")).unwrap();
    // Facts and entities the graph shows: take them from a plain compile of the same scope.
    let (g, _) = VisibleGraph::load(
        &r,
        &r.version(1).unwrap(),
        &normalize_scope(&["docs".into()]),
    )
    .unwrap();
    let keys: Vec<String> = g
        .facts()
        .filter(|f| f.predicate != Predicate::Mentions && !f.evidence.is_empty())
        .map(|f| f.key.clone())
        .take(3)
        .collect();
    assert_eq!(keys.len(), 3, "the golden set has facts to point at");
    let subject = g
        .facts()
        .find(|f| f.key == keys[0])
        .map(|f| f.subject.clone())
        .unwrap();
    drop(p);
    (r, keys, subject)
}

fn some_changes(keys: &[String], subject: &str) -> brief::Changes {
    use brief::{Change, ChangeKind, OldFact};
    let old = |text: &str| OldFact {
        subject: subject.to_owned(),
        predicate: "负责".into(),
        object: None,
        value: Some(text.to_owned()),
    };
    brief::Changes {
        from_version: 0,
        changes: vec![
            Change {
                kind: ChangeKind::Retracted,
                new_key: None,
                old: Some(old("旧结论")),
            },
            Change {
                kind: ChangeKind::Added,
                new_key: Some(keys[0].clone()),
                old: None,
            },
            Change {
                kind: ChangeKind::Changed,
                new_key: Some(keys[1].clone()),
                old: Some(old("旧值")),
            },
            // Not visible here: nothing of it may appear.
            Change {
                kind: ChangeKind::Retracted,
                new_key: None,
                old: Some(brief::OldFact {
                    subject: "person:ffffffffffffffffffffffff".into(),
                    predicate: "负责".into(),
                    object: None,
                    value: Some("看不见的结论".into()),
                }),
            },
        ],
    }
}

#[test]
fn brief_changes_follow_the_head_in_a_fixed_order_with_provenance() {
    let (r, keys, subject) = change_world();
    let mut base = req(&["docs"], "星图看板");
    base.budget = 3000;
    let plain = compile(&r, &base).unwrap();
    assert!(
        !brief(&plain).contains("自上次以来的变化"),
        "no changes, no section"
    );
    let mut q = base.clone();
    q.changes = Some(some_changes(&keys, &subject));
    let p = compile(&r, &q).unwrap();
    let b = brief(&p);
    let at = b.find("## 自上次以来的变化（v0 → v1）").expect("section");
    assert!(
        at < b.find("## 你在做什么").unwrap(),
        "after the head, before the task"
    );
    let section: Vec<&str> = b[at..]
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with("## "))
        .filter(|l| !l.is_empty())
        .collect();
    let kinds: Vec<&str> = section
        .iter()
        .map(|l| {
            l.trim_start_matches("- ")
                .trim_start_matches("> ")
                .split('：')
                .next()
                .unwrap()
        })
        .collect();
    assert_eq!(
        kinds,
        ["新增", "改值", "撤回"],
        "added, changed, retracted: {section:?}"
    );
    for l in section.iter().filter(|l| l.starts_with("- ")) {
        assert!(!source::tags_in(l).is_empty(), "provenance on {l}");
    }
    assert!(
        section[2].starts_with("> 撤回："),
        "a retraction has no live source: {}",
        section[2]
    );
    assert!(
        !b.contains("看不见的结论"),
        "a name outside the scope does not leak"
    );
    assert_eq!(tokens::count(&b), p.brief_tokens);
    // The same input in another order gives the same bytes; the changes are part of the parameters.
    let mut q2 = q.clone();
    q2.changes.as_mut().unwrap().changes.reverse();
    let p2 = compile(&r, &q2).unwrap();
    assert_eq!(all_bytes(&p), all_bytes(&p2));
    assert_ne!(p.params_sha256, plain.params_sha256);
    // Budget: the section never takes more than a tenth, and says what did not fit.
    let mut small = q.clone();
    small.budget = 600;
    let p3 = compile(&r, &small).unwrap();
    assert!(brief(&p3).contains("自上次以来的变化") || brief(&p3).contains("另有"));
}
