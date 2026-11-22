use std::path::PathBuf;

use oo_core::docir::DocIr;

use super::model::*;
use super::rules::extract;
use crate::parse::markdown::parse_markdown;
use crate::parse::text::parse_text;

fn ir_md(src: &str) -> DocIr {
    parse_markdown(src.as_bytes()).expect("parse md").ir
}

fn fx(path: &str, src: &str) -> Extraction {
    extract(path, &ir_md(src))
}

fn e(ty: EntityType, name: &str) -> EntityRef {
    EntityRef::new(ty, name)
}

fn has_fact(x: &Extraction, s: &EntityRef, p: &Predicate, o: &EntityRef) -> Option<FactMention> {
    x.facts
        .iter()
        .find(|f| &f.subject == s && &f.predicate == p && f.object.as_ref() == Some(o))
        .cloned()
}

fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testing/super-ontology/golden")
}

fn golden_md(rel: &str) -> Extraction {
    let bytes = std::fs::read(golden().join("docs").join(rel)).expect("golden file");
    extract(rel, &parse_markdown(&bytes).expect("parse").ir)
}

#[test]
fn person_file_in_people_dir() {
    let x = fx(
        "People/陈子墨.md",
        "---\ntype: person\nrole: x\n---\n\n# 陈子墨\n\n正文\n",
    );
    let p = e(EntityType::Person, "陈子墨");
    assert!(x.entities.iter().any(|m| m.entity == p));
    let doc = e(EntityType::Document, "People/陈子墨.md");
    assert!(has_fact(&x, &doc, &Predicate::Mentions, &p).is_some());
    assert!(
        x.entities.iter().all(|m| !m.evidence.is_empty()),
        "every entity has evidence"
    );
    assert!(
        x.facts.iter().all(|f| !f.evidence.is_empty()),
        "every fact has evidence"
    );
    // Title equals the stem: no alias for the person.
    assert!(x.aliases.iter().all(|a| a.entity != p));
}

#[test]
fn sub_directory_of_people_is_the_organization() {
    let x = fx("人员/研发部/张三.md", "# 张三\n\n内容\n");
    let f = has_fact(
        &x,
        &e(EntityType::Person, "张三"),
        &Predicate::MemberOf,
        &e(EntityType::Organization, "研发部"),
    )
    .expect("member_of");
    assert_eq!((f.rule_id, f.confidence_bp), ("dir.people", 10000));
}

#[test]
fn project_dir_names_and_document_about() {
    let x = fx("Projects/星图/规格.md", "# 规格说明\n\n内容\n");
    let proj = e(EntityType::Project, "星图");
    let doc = e(EntityType::Document, "Projects/星图/规格.md");
    assert!(has_fact(&x, &doc, &Predicate::About, &proj).is_some());
    let a = x
        .aliases
        .iter()
        .find(|a| a.entity == doc && a.alias == "规格说明")
        .expect("title alias");
    assert_eq!(a.rule_id, "doc.entity");
    let x = fx("Projects/灯塔迁移.md", "# 灯塔迁移\n");
    assert!(
        x.entities
            .iter()
            .any(|m| m.entity == e(EntityType::Project, "灯塔迁移"))
    );
}

#[test]
fn frontmatter_type_overrides_directory_and_unknown_is_ignored() {
    let x = fx("People/a.md", "---\ntype: project\n---\n# A\n");
    assert!(
        x.entities
            .iter()
            .any(|m| m.entity.ty == EntityType::Project && m.entity.name == "A")
    );
    let x = fx("People/a.md", "---\ntype: memo\n---\n# A\n");
    assert!(x.entities.iter().any(|m| m.entity.ty == EntityType::Person));
}

#[test]
fn decision_file_with_owner_and_date() {
    let src = "---\ntype: decision\nstatus: active\ndate: 2026-04-02\nowner: 林远舟\n---\n\n# 决定：周报改为每周四发送\n\n本决定取代2026年1月10日“周报每周五发送”的决定。\n";
    let x = fx("Decisions/D-2026-04.md", src);
    let d = e(EntityType::Decision, "周报改为每周四发送");
    let p = e(EntityType::Person, "林远舟");
    let f = has_fact(&x, &p, &Predicate::Decides, &d).expect("decides");
    assert_eq!(f.valid_from.as_deref(), Some("2026-04-02"));
    assert_eq!(f.valid_precision, Some("day"));
    assert_eq!(f.rule_id, "fm.fields");
    let old = e(EntityType::Decision, "周报每周五发送");
    let s = has_fact(&x, &d, &Predicate::Supersedes, &old).expect("supersedes");
    assert_eq!((s.rule_id, s.confidence_bp), ("md.headings", 8500));
    let status = x
        .facts
        .iter()
        .find(|f| f.subject == d && f.predicate == Predicate::Attr("status".into()))
        .expect("status");
    assert_eq!(status.value, Some(serde_json::json!({"text": "active"})));
    // The Document is known by its title and by the stem.
    let doc = e(EntityType::Document, "Decisions/D-2026-04.md");
    assert!(
        x.aliases
            .iter()
            .any(|a| a.entity == doc && a.alias == "决定：周报改为每周四发送")
    );
    assert!(
        x.aliases
            .iter()
            .any(|a| a.entity == doc && a.alias == "D-2026-04")
    );
}

#[test]
fn passive_replacement_is_not_a_fact() {
    let x = fx(
        "Decisions/a.md",
        "# 决定：甲\n\n后续：本决定已被“乙”取代。\n",
    );
    assert!(x.facts.iter().all(|f| f.predicate != Predicate::Supersedes));
}

#[test]
fn kv_lines_and_owner_sentences_on_projects() {
    let src = "---\ntype: project\n---\n\n# 官网改版\n\n负责人：孙晓棠（2026-07-01 起）。\n\n2026-01-01 至 2026-05-31 由陈子墨负责。\n\n成员：贺文博负责前端；韩立负责客户案例素材。\n";
    let x = fx("Projects/官网改版.md", src);
    let proj = e(EntityType::Project, "官网改版");
    let owns = |n: &str| has_fact(&x, &e(EntityType::Person, n), &Predicate::Owns, &proj);
    let a = owns("孙晓棠").expect("kv owner");
    assert_eq!(
        (a.valid_from.as_deref(), a.rule_id),
        (Some("2026-07-01"), "md.kv")
    );
    let b = owns("陈子墨").expect("range owner");
    assert_eq!(
        (b.valid_from.as_deref(), b.valid_to.as_deref()),
        (Some("2026-01-01"), Some("2026-05-31"))
    );
    for n in ["贺文博", "韩立"] {
        assert!(
            has_fact(&x, &e(EntityType::Person, n), &Predicate::MemberOf, &proj).is_some(),
            "{n}"
        );
    }
}

#[test]
fn rejected_and_tried_alternatives_are_terms() {
    let src = "---\ntype: decision\n---\n# 决定：采购外部模型服务\n\n被否决方案：自建小模型，准确率只有六成。\n\n## 已尝试\n\n- 微调开源模型：效果差\n\n## 不做\n\n- 自研引擎\n";
    let x = fx("Decisions/x.md", src);
    let d = e(EntityType::Decision, "采购外部模型服务");
    let term = |n: &str| e(EntityType::Term, n);
    assert!(has_fact(&x, &d, &Predicate::Rejects, &term("自建小模型")).is_some());
    assert!(has_fact(&x, &d, &Predicate::TriedAndFailed, &term("微调开源模型")).is_some());
    assert!(has_fact(&x, &d, &Predicate::Rejects, &term("自研引擎")).is_some());
}

#[test]
fn decision_sections_in_a_plain_document_decide_for_the_owner() {
    let src = "---\nowner: 张三\ndate: 2026-02-03\n---\n# 会议纪要\n\n## 决定\n\n- 采用 PostgreSQL（原因见附录）\n- 周会改到周四\n\n## 背景\n\n- 无关项\n";
    let x = fx("Notes/会议.md", src);
    let p = e(EntityType::Person, "张三");
    for name in ["采用 PostgreSQL", "周会改到周四"] {
        let f = has_fact(&x, &p, &Predicate::Decides, &e(EntityType::Decision, name))
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!((f.rule_id, f.confidence_bp), ("md.headings", 8500));
    }
    assert!(x.entities.iter().all(|m| m.entity.name != "无关项"));
    // The document itself is the owned thing for an unclassified file.
    let doc = e(EntityType::Document, "Notes/会议.md");
    assert!(has_fact(&x, &p, &Predicate::Owns, &doc).is_some());
}

#[test]
fn preferences_use_list_items_or_paragraphs() {
    let x = fx(
        "Preferences/a.md",
        "---\nowner: 张三\n---\n# 偏好\n\n- 会议不超过半小时\n- 先写结论\n",
    );
    let prefs: Vec<_> = x
        .entities
        .iter()
        .filter(|m| m.entity.subkind == Some("preference"))
        .collect();
    assert_eq!(prefs.len(), 2);
    assert!(
        has_fact(
            &x,
            &e(EntityType::Person, "张三"),
            &Predicate::Decides,
            &EntityRef::preference("先写结论")
        )
        .is_some()
    );
    let x = fx("偏好/b.md", "# 偏好\n\n沟通优先使用文字。\n");
    let pref = x
        .entities
        .iter()
        .find(|m| m.entity.subkind == Some("preference"))
        .expect("paragraph preference");
    assert_eq!(pref.entity.name, "沟通优先使用文字");
}

#[test]
fn file_name_date_is_the_default_valid_from() {
    let x = fx("People/李四.md", "# 李四\n");
    assert!(x.facts.iter().all(|f| f.valid_from.is_none()));
    let x = fx("Projects/p-2026-03-05.md", "# P\n\n成员：王五\n");
    let f = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::MemberOf)
        .expect("member_of");
    assert_eq!(
        (f.valid_from.as_deref(), f.valid_precision),
        (Some("2026-03-05"), Some("day"))
    );
}

#[test]
fn frontmatter_aliases_and_project_links() {
    let x = fx(
        "Projects/星图.md",
        "---\naliases: [Starmap, 星图看板]\n---\n# 星图\n",
    );
    let p = e(EntityType::Project, "星图");
    let names: Vec<&str> = x
        .aliases
        .iter()
        .filter(|a| a.entity == p)
        .map(|a| a.alias.as_str())
        .collect();
    assert!(names.contains(&"Starmap") && names.contains(&"星图看板"));
    let x = fx(
        "Decisions/d.md",
        "---\nproject: 星图\n---\n# 决定：采用星图方案\n",
    );
    assert!(
        has_fact(
            &x,
            &e(EntityType::Decision, "采用星图方案"),
            &Predicate::About,
            &e(EntityType::Project, "星图")
        )
        .is_some()
    );
}

#[test]
fn other_formats_only_get_the_document() {
    let ir = parse_text(b"hello\nworld\n").expect("txt").ir;
    let x = extract("Notes/a.txt", &ir);
    assert_eq!(x.entities.len(), 1);
    assert_eq!(x.entities[0].entity.ty, EntityType::Document);
    assert!(x.facts.is_empty());
}

#[test]
fn empty_ir_gives_nothing() {
    let ir = ir_md("");
    let x = extract("a.md", &ir);
    assert!(x.entities.is_empty() && x.facts.is_empty() && x.aliases.is_empty());
}

#[test]
fn rules_never_create_things_and_ids_are_core() {
    for rel in [
        "People/陈子墨.md",
        "Projects/星图看板.md",
        "Decisions/D-2026-05-星图改用开源图表库.md",
    ] {
        let x = golden_md(rel);
        assert!(
            x.entities.iter().all(|m| m.entity.ty != EntityType::Thing),
            "{rel}"
        );
        assert!(
            x.facts
                .iter()
                .all(|f| (1..=10000).contains(&f.confidence_bp)),
            "{rel}"
        );
    }
}

#[test]
fn extraction_is_deterministic_and_sorted() {
    let a = golden_md("Projects/星图看板.md");
    let b = golden_md("Projects/星图看板.md");
    assert_eq!(a, b);
    let keys: Vec<_> = a.entities.iter().map(|m| m.entity.clone()).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn golden_star_board() {
    let x = golden_md("Projects/星图看板.md");
    let proj = e(EntityType::Project, "星图看板");
    let own = |n: &str| has_fact(&x, &e(EntityType::Person, n), &Predicate::Owns, &proj);
    let chen = own("陈子墨").expect("陈子墨 owns");
    assert_eq!(
        (chen.valid_from.as_deref(), chen.valid_to.as_deref()),
        (Some("2026-01-01"), Some("2026-05-31"))
    );
    let zhou = own("周若溪").expect("周若溪 owns");
    assert_eq!(zhou.valid_from.as_deref(), Some("2026-06-01"));
    for n in ["吴启明", "郑雨桐"] {
        assert!(
            has_fact(&x, &e(EntityType::Person, n), &Predicate::MemberOf, &proj).is_some(),
            "{n}"
        );
    }
}

#[test]
fn golden_decisions_chain() {
    let x = golden_md("Decisions/D-2026-08-取消周报.md");
    let d = e(EntityType::Decision, "取消周报改用看板自动汇总");
    assert!(
        has_fact(
            &x,
            &e(EntityType::Person, "林远舟"),
            &Predicate::Decides,
            &d
        )
        .is_some()
    );
    assert!(
        has_fact(
            &x,
            &d,
            &Predicate::Supersedes,
            &e(EntityType::Decision, "周报改为每周四发送")
        )
        .is_some()
    );
    let x = golden_md("Decisions/D-2026-05-星图改用开源图表库.md");
    let d = e(EntityType::Decision, "星图看板改用开源图表库");
    assert!(
        has_fact(
            &x,
            &d,
            &Predicate::Rejects,
            &e(EntityType::Term, "自研图表引擎")
        )
        .is_some()
    );
}

/// With `OO_DOCS_PREDICTIONS_DIR=<dir>` set, write the rule output for every
/// md and txt file of the golden set in the labels JSONL format, for
/// `oo-eval docs` (the B-S2-07 recall report). Without the variable it only
/// checks that the output is well formed.
#[test]
fn golden_predictions_are_well_formed_and_can_be_written() {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    use oo_core::docir::Anchor;
    use serde_json::json;

    let root = golden().join("docs");
    let mut files = Vec::new();
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
    walk(&root, &mut files);

    struct Ent {
        ty: EntityType,
        subkind: Option<&'static str>,
        name: String,
        aliases: BTreeMap<String, ()>,
        anchors: BTreeMap<(String, String), ()>,
    }
    let frag = |a: &Anchor| match a {
        Anchor::Text {
            line_start,
            line_end,
            ..
        } => format!("#L{line_start}-L{line_end}").replace("-L", "-"),
        Anchor::Cells {
            sheet: Some(sheet),
            range,
        } => format!("#'{}'!{range}", sheet.replace('\'', "''")),
        Anchor::Cells { sheet: None, range } => {
            let digits: String = range
                .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            format!("#R{digits}")
        }
        _ => "#L1-1".to_owned(),
    };
    let mut ents: BTreeMap<String, Ent> = BTreeMap::new();
    let mut facts: Vec<(String, FactMention, String)> = Vec::new();
    let mut parsed = 0;
    for p in files {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "md" | "txt" | "csv" | "xlsx") {
            continue;
        }
        let rel = p
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&p).unwrap();
        let ir = match ext {
            "md" => parse_markdown(&bytes),
            "csv" => crate::parse::csv::parse_csv(&bytes),
            "xlsx" => crate::parse::xlsx::parse_xlsx(&bytes),
            _ => parse_text(&bytes),
        }
        .expect("parse")
        .ir;
        let x = extract(&rel, &ir);
        parsed += 1;
        let file = format!("docs/{rel}");
        let add = |ents: &mut BTreeMap<String, Ent>, er: &EntityRef, evs: &[EvidenceRef]| {
            let ent = ents.entry(er.key()).or_insert_with(|| Ent {
                ty: er.ty,
                subkind: er.subkind,
                name: er.name.clone(),
                aliases: BTreeMap::new(),
                anchors: BTreeMap::new(),
            });
            ent.subkind = ent.subkind.or(er.subkind);
            for ev in evs {
                ent.anchors.insert((file.clone(), frag(&ev.anchor)), ());
            }
        };
        for m in &x.entities {
            add(&mut ents, &m.entity, &m.evidence);
        }
        for a in &x.aliases {
            add(&mut ents, &a.entity, &a.evidence);
            ents.get_mut(&a.entity.key())
                .unwrap()
                .aliases
                .insert(a.alias.clone(), ());
        }
        for f in &x.facts {
            facts.push((file.clone(), f.clone(), rel.clone()));
        }
    }
    assert_eq!(
        parsed, 28,
        "18 md + 3 txt + 3 csv + 4 xlsx files in the golden set"
    );
    let ids: BTreeMap<&String, String> = ents
        .keys()
        .enumerate()
        .map(|(i, k)| (k, format!("P{:04}", i + 1)))
        .collect();
    let mut entities_out = String::new();
    for (k, e) in &ents {
        let mut rec = json!({
            "kind": "entity", "id": ids[k], "type": e.ty.as_str(), "name": e.name,
            "aliases": e.aliases.keys().collect::<Vec<_>>(),
            "anchors": e.anchors.keys().map(|(f, g)| json!({"file": f, "frag": g})).collect::<Vec<_>>(),
        });
        if let Some(sk) = e.subkind {
            rec["subkind"] = json!(sk);
        }
        writeln!(entities_out, "{rec}").unwrap();
    }
    type Merged = (serde_json::Value, BTreeMap<(String, String), ()>);
    let mut merged: BTreeMap<String, Merged> = BTreeMap::new();
    for (file, f, _) in &facts {
        let (Some(obj), s) = (f.object.as_ref(), &f.subject) else {
            continue;
        };
        let k = format!(
            "{}|{}|{}|{:?}",
            s.key(),
            f.predicate.name(),
            obj.key(),
            f.valid_from
        );
        let slot = merged.entry(k).or_insert_with(|| {
            let mut rec = json!({"kind": "fact", "subject": ids[&s.key()], "predicate": f.predicate.name(), "object": ids[&obj.key()]});
            if let Some(v) = &f.valid_from { rec["valid_from"] = json!(v); }
            if let Some(v) = &f.valid_to { rec["valid_to"] = json!(v); }
            (rec, BTreeMap::new())
        });
        for ev in &f.evidence {
            slot.1.insert((file.clone(), frag(&ev.anchor)), ());
        }
    }
    let mut facts_out = String::new();
    for (i, (mut rec, anchors)) in merged.into_values().enumerate() {
        rec["id"] = json!(format!("PF{:04}", i + 1));
        rec["anchors"] = json!(
            anchors
                .keys()
                .map(|(f, g)| json!({"file": f, "frag": g}))
                .collect::<Vec<_>>()
        );
        writeln!(facts_out, "{rec}").unwrap();
    }
    assert!(!entities_out.is_empty() && !facts_out.is_empty());
    if let Ok(dir) = std::env::var("OO_DOCS_PREDICTIONS_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(PathBuf::from(&dir).join("entities.jsonl"), entities_out).unwrap();
        std::fs::write(PathBuf::from(&dir).join("facts.jsonl"), facts_out).unwrap();
    }
}

#[test]
fn a_replacement_starts_on_the_decision_date_not_the_date_it_names() {
    let x = fx(
        "Decisions/D-2027-04-周报改为每周四发送.md",
        "---\ntype: decision\ndate: 2027-04-02\nowner: 林远舟\n---\n\n# 决定：周报改为每周四发送\n\n本决定取代2027年1月10日“周报每周五发送”的决定。\n",
    );
    let f = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::Supersedes)
        .expect("a supersedes fact");
    assert_eq!(f.valid_from.as_deref(), Some("2027-04-02"));
}
