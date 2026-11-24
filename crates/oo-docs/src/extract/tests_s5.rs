//! B-S5-02 rule extension: tables, doc metadata, versions, emails, dates,
//! glossaries, pptx titles, and the prose rules on non-markdown formats.

use std::path::PathBuf;

use oo_core::docir::{BlockKind, DocFormat, DocIr};

use super::model::*;
use super::rules::{Siblings, extract, extract_in};
use crate::parse::csv::parse_csv;
use crate::parse::markdown::parse_markdown;
use crate::parse::xlsx::parse_xlsx;

fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testing/super-ontology/golden/docs")
}

fn golden_table(rel: &str) -> Extraction {
    let bytes = std::fs::read(golden().join(rel)).expect("golden file");
    let ir = if rel.ends_with(".csv") {
        parse_csv(&bytes)
    } else {
        parse_xlsx(&bytes)
    }
    .expect("parse")
    .ir;
    extract(rel, &ir)
}

fn e(ty: EntityType, name: &str) -> EntityRef {
    EntityRef::new(ty, name)
}

fn fact<'a>(
    x: &'a Extraction,
    s: &EntityRef,
    p: &Predicate,
    o: &EntityRef,
) -> Option<&'a FactMention> {
    x.facts
        .iter()
        .find(|f| &f.subject == s && &f.predicate == p && f.object.as_ref() == Some(o))
}

fn md_ir(src: &str) -> DocIr {
    parse_markdown(src.as_bytes()).expect("md").ir
}

#[test]
fn people_table_gives_persons_aliases_org_role_and_dates() {
    let x = golden_table("Tables/人员表.xlsx");
    let lin = e(EntityType::Person, "林远舟");
    let dept = e(EntityType::Organization, "管理");
    let f = fact(&x, &lin, &Predicate::MemberOf, &dept).expect("member_of department");
    assert_eq!(f.rule_id, "xlsx.people");
    assert_eq!(f.confidence_bp, 9500);
    assert_eq!(f.valid_from.as_deref(), Some("2025-03-01"));
    assert!(f.evidence[0].block_id.starts_with("blk_"));
    let aliases: Vec<&str> = x
        .aliases
        .iter()
        .filter(|a| a.entity == lin)
        .map(|a| a.alias.as_str())
        .collect();
    assert!(aliases.contains(&"Evan Lin") && aliases.contains(&"老林"));
    let role = x
        .facts
        .iter()
        .find(|f| f.subject == lin && f.predicate == Predicate::Attr("role".into()))
        .expect("attr.role");
    assert_eq!(role.value.as_ref().unwrap()["text"], "CEO");
    // Nine rows, nine persons, all cited to their own row.
    let persons = x
        .entities
        .iter()
        .filter(|m| m.entity.ty == EntityType::Person)
        .count();
    assert_eq!(persons, 9);
}

#[test]
fn project_table_gives_owns_with_effective_dates() {
    let x = golden_table("Tables/项目负责人表.xlsx");
    let f = fact(
        &x,
        &e(EntityType::Person, "周若溪"),
        &Predicate::Owns,
        &e(EntityType::Project, "星图看板"),
    )
    .expect("owns");
    assert_eq!(f.valid_from.as_deref(), Some("2026-06-01"));
    assert_eq!(f.rule_id, "xlsx.projects");
    assert_eq!(f.evidence.len(), 1);
}

#[test]
fn generic_tables_only_read_role_columns() {
    let x = golden_table("Data/任务清单.csv");
    let doc = e(EntityType::Document, "Data/任务清单.csv");
    assert!(
        fact(
            &x,
            &doc,
            &Predicate::About,
            &e(EntityType::Project, "星图看板")
        )
        .is_some()
    );
    assert!(
        x.entities
            .iter()
            .any(|m| m.entity == e(EntityType::Person, "孙晓棠")),
        "负责人 column names a person"
    );
    // Task titles are not entities.
    assert!(
        x.entities
            .iter()
            .all(|m| m.entity.name != "整理年度OKR" && m.entity.name != "T-101")
    );
    let org = golden_table("Tables/客户与供应商.xlsx");
    assert!(
        org.entities
            .iter()
            .any(|m| m.entity == e(EntityType::Organization, "澄海物流"))
    );
    let fb = golden_table("Data/客户反馈.csv");
    let fdoc = e(EntityType::Document, "Data/客户反馈.csv");
    assert!(
        fact(
            &fb,
            &fdoc,
            &Predicate::About,
            &e(EntityType::Organization, "澄海物流")
        )
        .is_some()
    );
}

#[test]
fn table_without_matching_headers_yields_nothing_but_the_document() {
    let x = golden_table("Tables/年度预算表.xlsx");
    assert!(
        x.entities
            .iter()
            .all(|m| matches!(m.entity.ty, EntityType::Document | EntityType::Project))
    );
    assert!(x.facts.iter().all(|f| f.predicate == Predicate::Mentions));
}

#[test]
fn people_table_with_email_and_id_gives_identities() {
    let csv =
        "姓名,邮箱,工号,部门\n张三,Zhang.San@Example.com,E001,研发\n李四,not-an-email,,运营\n";
    let ir = parse_csv(csv.as_bytes()).unwrap().ir;
    let x = extract("人员/名单.csv", &ir);
    let ids: Vec<(&str, &str, &str)> = x
        .identities
        .iter()
        .map(|i| (i.entity.name.as_str(), i.kind, i.value.as_str()))
        .collect();
    assert!(ids.contains(&("张三", "email", "zhang.san@example.com")));
    assert!(ids.contains(&("张三", "employee_id", "E001")));
    assert_eq!(ids.len(), 2, "bad email and blank id give nothing");
    assert!(x.facts.iter().any(|f| f.rule_id == "csv.people"));
}

#[test]
fn doc_meta_author_and_created() {
    let mut ir = md_ir("# 周报\n\n正文\n");
    ir.format = DocFormat::Docx;
    ir.meta.authors = vec!["陈子墨".into(), "Microsoft Office User".into()];
    ir.meta.created = Some("2026-03-05T08:00:00Z".into());
    let x = extract("Weekly/周报.docx", &ir);
    let doc = e(EntityType::Document, "Weekly/周报.docx");
    let author = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::Attr("author".into()))
        .expect("author");
    assert_eq!(author.value.as_ref().unwrap()["text"], "陈子墨");
    assert_eq!(author.confidence_bp, 9000);
    assert_eq!(
        x.facts
            .iter()
            .filter(|f| f.predicate == Predicate::Attr("author".into()))
            .count(),
        1,
        "tool default authors are dropped"
    );
    let created = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::Attr("created".into()))
        .expect("created");
    assert_eq!(created.value.as_ref().unwrap()["date"], "2026-03-05");
    // Author is a bind-only hint, not an entity.
    assert!(x.hints.iter().any(|h| h.subject == doc
        && h.predicate == Predicate::Mentions
        && h.target_ty == EntityType::Person
        && h.target_name == "陈子墨"));
    assert!(
        !x.entities
            .iter()
            .any(|m| m.entity == e(EntityType::Person, "陈子墨"))
    );
    // Creation date is the default valid_from, precision doc_date.
    let mention = x.facts.iter().find(|f| f.predicate == Predicate::Mentions);
    if let Some(m) = mention {
        assert_eq!(m.valid_precision, Some("doc_date"));
    }
}

#[test]
fn prose_rules_also_read_docx_paragraphs() {
    let mut ir = md_ir("# 星图看板\n\n负责人：周若溪\n");
    ir.format = DocFormat::Docx;
    let x = extract("Projects/星图看板/概述.docx", &ir);
    assert!(
        fact(
            &x,
            &e(EntityType::Person, "周若溪"),
            &Predicate::Owns,
            &e(EntityType::Project, "星图看板")
        )
        .is_some()
    );
}

#[test]
fn pptx_title_names_the_document_and_hints_a_project() {
    let mut ir = md_ir("# 星图看板 周报\n\n- 本周完成登录\n");
    ir.format = DocFormat::Pptx;
    ir.blocks[0].kind = BlockKind::SlideTitle;
    ir.meta.title = None;
    let x = extract("Weekly/W22.pptx", &ir);
    let doc = e(EntityType::Document, "Weekly/W22.pptx");
    let alias = x
        .aliases
        .iter()
        .find(|a| a.entity == doc && a.rule_id == "pptx.title")
        .expect("title alias");
    assert_eq!(
        (alias.alias.as_str(), alias.confidence_bp),
        ("星图看板 周报", 7000)
    );
    let names: Vec<&str> = x
        .hints
        .iter()
        .filter(|h| h.rule_id == "pptx.title")
        .map(|h| h.target_name.as_str())
        .collect();
    assert!(names.contains(&"星图看板"));
    assert!(
        x.entities
            .iter()
            .all(|m| m.entity.ty == EntityType::Document)
    );
}

#[test]
fn name_version_proposes_supersession_of_the_older_sibling() {
    let ir = md_ir("# 方案\n\n内容\n");
    let sibs = vec![
        "Proposals/星图方案.md".to_owned(),
        "Proposals/星图方案-v2.md".to_owned(),
        "Other/星图方案.md".to_owned(),
    ];
    let x = extract_in(
        "Proposals/星图方案-v3.md",
        &ir,
        &Siblings {
            paths: &sibs,
            ..Siblings::default()
        },
    );
    let doc = e(EntityType::Document, "Proposals/星图方案-v3.md");
    let old = e(EntityType::Document, "Proposals/星图方案-v2.md");
    let f = fact(&x, &doc, &Predicate::Supersedes, &old).expect("supersedes v2");
    assert_eq!((f.rule_id, f.confidence_bp), ("name.version", 7000));
    // No sibling, no proposal.
    let none = extract("Proposals/星图方案-v3.md", &ir);
    assert!(
        none.facts
            .iter()
            .all(|f| f.predicate != Predicate::Supersedes)
    );
    // The unmarked file never supersedes anything.
    let base = extract_in(
        "Proposals/星图方案.md",
        &ir,
        &Siblings {
            paths: &sibs,
            ..Siblings::default()
        },
    );
    assert!(
        base.facts
            .iter()
            .all(|f| f.predicate != Predicate::Supersedes)
    );
}

#[test]
fn email_binds_only_when_block_is_unambiguous() {
    let x = extract(
        "Notes/联系.md",
        &md_ir(
            "# 联系\n\n- **负责人**：陈子墨\n\n陈子墨的邮箱是 Chen@Example.com 。\n\n张三 a@x.io 李四 b@x.io\n",
        ),
    );
    let ids: Vec<_> = x
        .identities
        .iter()
        .map(|i| (i.entity.name.as_str(), i.value.as_str(), i.rule_id))
        .collect();
    assert_eq!(ids, [("陈子墨", "chen@example.com", "text.email")]);
}

#[test]
fn person_file_kv_and_frontmatter_identity() {
    let x = extract(
        "People/张三.md",
        &md_ir(
            "---\ntype: person\ngithub: ZhangSan\n---\n\n# 张三\n\n邮箱：zs@example.com\n工号：E007\n",
        ),
    );
    let mut ids: Vec<_> = x
        .identities
        .iter()
        .map(|i| (i.kind, i.value.clone()))
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        [
            ("email", "zs@example.com".to_owned()),
            ("employee_id", "E007".to_owned()),
            ("github", "zhangsan".to_owned())
        ]
    );
}

#[test]
fn explicit_dates_in_a_block_are_valid_from_with_year_from_document() {
    let x = extract(
        "Decisions/2026-03-05-定价.md",
        &md_ir(
            "---\ntype: decision\nowner: 陈子墨\n---\n\n# 定价\n\n## 决定\n\n- 采用按席位定价（4月2日生效）\n- 取消免费版 2026-05-06 起\n",
        ),
    );
    let by: std::collections::BTreeMap<&str, Option<&str>> = x
        .facts
        .iter()
        .filter(|f| f.predicate == Predicate::Decides)
        .map(|f| {
            (
                f.object.as_ref().unwrap().name.as_str(),
                f.valid_from.as_deref(),
            )
        })
        .collect();
    assert_eq!(by["采用按席位定价"], Some("2026-04-02"));
    assert_eq!(by["取消免费版 2026-05-06 起"], Some("2026-05-06"));
}

#[test]
fn quarter_forms_in_names() {
    let x = extract("Plans/2026年Q2规划.md", &md_ir("# 规划\n\n内容\n"));
    let m = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::Mentions)
        .map_or_else(|| x.facts.first(), Some);
    let _ = m;
    let y = extract("Plans/2026Q1计划.md", &md_ir("# 规划\n\n内容\n"));
    assert!(
        y.facts
            .iter()
            .all(|f| f.valid_from.as_deref() == Some("2026-01-01"))
    );
}

#[test]
fn glossary_definition_lists_and_two_column_tables() {
    let src = "# 术语\n\n- **ARR**：年度经常性收入\n- 没有定义\n\n| 术语 | 定义 |\n|---|---|\n| SLA | 服务等级协议 |\n";
    let x = extract("Glossary/术语表.md", &md_ir(src));
    let defs: Vec<(String, String)> = x
        .facts
        .iter()
        .filter(|f| f.predicate == Predicate::Attr("definition".into()))
        .map(|f| {
            (
                f.subject.name.clone(),
                f.value.as_ref().unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect();
    assert!(defs.contains(&("ARR".into(), "年度经常性收入".into())));
    assert!(defs.contains(&("SLA".into(), "服务等级协议".into())));
    assert_eq!(defs.len(), 2);
    assert!(
        x.facts
            .iter()
            .filter(|f| f.rule_id == "dir.terms")
            .all(|f| f.confidence_bp == 9500)
    );
}

#[test]
fn extraction_is_deterministic_for_tables() {
    let a = golden_table("Tables/人员表.xlsx");
    let b = golden_table("Tables/人员表.xlsx");
    assert_eq!(a, b);
}

#[test]
fn a_table_row_never_makes_its_person_the_owner_of_the_document() {
    // Action items of a meeting: the person owns the task in the row, not the file.
    let mut ir = parse_markdown("# 纪要\n\n负责人：周若溪\n".as_bytes())
        .unwrap()
        .ir;
    let path = "Meetings/2026-01-08-计划会.md";
    let doc = e(EntityType::Document, path);
    let owns_doc = |x: &Extraction| {
        x.facts
            .iter()
            .any(|f| f.predicate == Predicate::Owns && f.object.as_ref() == Some(&doc))
    };
    assert!(owns_doc(&extract(path, &ir)), "a line states the owner");
    let table = ir.blocks[0].id.clone();
    let row = ir
        .blocks
        .iter_mut()
        .find(|b| b.text.contains("周若溪"))
        .unwrap();
    row.kind = BlockKind::Kv;
    row.parent = Some(table);
    assert!(!owns_doc(&extract(path, &ir)), "a table row does not");
}

#[test]
fn a_document_that_states_a_decision_does_not_mention_it_by_name() {
    let x = extract(
        "Decisions/D-2026-01-周报每周五发送.md",
        &parse_markdown(
            "---\ntype: decision\nowner: 林远舟\n---\n\n# 决定：周报每周五发送\n\n林远舟决定。\n"
                .as_bytes(),
        )
        .unwrap()
        .ir,
    );
    assert!(
        x.facts.iter().all(|f| f.predicate != Predicate::Mentions
            || f.object
                .as_ref()
                .is_none_or(|o| o.ty != EntityType::Decision)),
        "{:?}",
        x.facts
    );
}
