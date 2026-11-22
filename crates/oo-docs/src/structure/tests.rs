use super::induce::{induce, reconcile};
use super::observe::{canon_key, observe, slug_of};
use super::store::{Decision, apply};
use super::template;
use super::vtype::{Lexicon, infer};
use super::yaml::{DbColumn, DbLinkRule, DbSide, DbTable, render};
use super::{FieldSource, Status, ValueType};
use crate::extract::rules::extract;
use crate::parse::markdown::parse_markdown;

fn ir(src: &str) -> oo_core::docir::DocIr {
    parse_markdown(src.as_bytes()).expect("parse").ir
}

fn vs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn keys_fold_language_and_spelling() {
    assert_eq!(canon_key("参会人"), "attendees");
    assert_eq!(canon_key("与会人员："), "attendees");
    assert_eq!(canon_key("Attendees"), "attendees");
    assert_eq!(canon_key("2. Action items"), "actions");
    assert_eq!(canon_key("预算（万元）"), "budget");
    assert_eq!(canon_key("参会人 / Attendees"), "attendees");
    assert_eq!(slug_of("attendees"), "attendees");
    assert!(slug_of("入职日期x").starts_with("f_"));
    assert_eq!(slug_of("hire"), "hire");
}

#[test]
fn values_decide_the_type() {
    let lex = Lexicon::new();
    let t = |k: &str, v: &[&str]| infer(k, &vs(v), &lex, true).value_type;
    assert_eq!(
        t("x", &["2026-03-01", "2026-04-02", "2026年5月3日"]),
        ValueType::Date
    );
    assert_eq!(
        t("x", &["2026-03-01 10:00", "2026-04-02 09:30"]),
        ValueType::Date
    );
    assert_eq!(
        t("x", &["¥120,000", "￥86,500", "30万元"]),
        ValueType::Amount
    );
    assert_eq!(t("budget", &["120", "80"]), ValueType::Amount);
    assert_eq!(t("x", &["12", "30", "4.5"]), ValueType::Number);
    assert_eq!(t("x", &["12%", "30%"]), ValueType::Percent);
    assert_eq!(t("x", &["a@b.com", "c@d.org"]), ValueType::Email);
    assert_eq!(t("x", &["https://a.com/x", "http://b.org"]), ValueType::Url);
    assert_eq!(t("x", &["E1001", "E1002", "E1003"]), ValueType::Id);
    assert_eq!(
        t("owner", &["陈子墨", "周若溪", "吴启明"]),
        ValueType::Person
    );
    assert_eq!(t("owner", &["Daniel Kim", "Karen Wu"]), ValueType::Person);
    assert_eq!(
        t(
            "x",
            &["已通过", "待定", "已通过", "已通过", "待定", "已取代"]
        ),
        ValueType::Enum
    );
    assert_eq!(
        t(
            "x",
            &["12个月", "24个月", "12个月", "12个月", "6个月", "12个月"]
        ),
        ValueType::Text
    );
    assert_eq!(
        t(
            "x",
            &[
                "一句很长的描述内容用来说明这个字段到底是什么意思而且比较啰嗦一句很长的描述内容用来说明这个字段到底是什么意思而且比较啰嗦一句很长的描述内容用来说明这个字段到底是什么意思",
                "另一句同样很长的描述内容用来说明那个字段到底是什么意思而且更加啰嗦一些另一句同样很长的描述内容用来说明那个字段到底是什么意思而且更加啰嗦一些另一句同样很长的描述内容"
            ]
        ),
        ValueType::LongText
    );
    // The section body of a file is prose, never an enum.
    assert_eq!(
        infer("x", &vs(&["是", "否", "是", "是", "否", "是"]), &lex, false).value_type,
        ValueType::Text
    );
}

#[test]
fn a_value_that_names_an_entity_makes_a_relation_type() {
    let mut lex = Lexicon::new();
    lex.add("Project", "星河");
    lex.add("Project", "北辰");
    let r = infer("x", &vs(&["星河", "北辰", "星河"]), &lex, true);
    assert_eq!(r.value_type, ValueType::Project);
    assert!(r.resolved > 0.99);
}

fn meeting(i: usize, who: &str) -> String {
    let d = i + 1;
    format!(
        "---\n日期: 2026-03-{d:02}\n项目: 星河\n---\n\n# 纪要{i}\n\n## 参会人\n\n- {who}\n- 周若溪\n\n## 决定\n\n- 采用灰度发布{i}\n"
    )
}

#[test]
fn recurring_fields_become_properties_with_coverage() {
    let docs: Vec<(String, _)> = (0..4)
        .map(|i| {
            (
                format!("meetings/m{i}.md"),
                observe(&ir(&meeting(i, "陈子墨"))).unwrap(),
            )
        })
        .collect();
    let rows: Vec<_> = docs.iter().map(|(p, o)| (p.as_str(), o)).collect();
    let r = induce(&rows, &Lexicon::new());
    assert_eq!(r.classes.len(), 1);
    let c = &r.classes[0];
    assert_eq!(c.kind, "meeting_minutes");
    assert_eq!(c.files, 4);
    let att = c.property("attendees").expect("attendees");
    assert_eq!(att.value_type, ValueType::Person);
    assert!(att.multi);
    assert_eq!(att.source, FieldSource::Section);
    assert!((att.coverage - 1.0).abs() < 1e-9);
    assert_eq!(c.property("date").unwrap().value_type, ValueType::Date);
    assert!(c.property("decisions").is_some());
}

#[test]
fn too_few_files_are_no_class() {
    let docs: Vec<(String, _)> = (0..2)
        .map(|i| {
            (
                format!("m{i}.md"),
                observe(&ir(&meeting(i, "陈子墨"))).unwrap(),
            )
        })
        .collect();
    let rows: Vec<_> = docs.iter().map(|(p, o)| (p.as_str(), o)).collect();
    let r = induce(&rows, &Lexicon::new());
    assert!(r.classes.is_empty());
    assert_eq!(r.unclustered.len(), 2);
}

fn classes() -> Vec<super::ClassStructure> {
    let docs: Vec<(String, _)> = (0..4)
        .map(|i| {
            (
                format!("meetings/m{i}.md"),
                observe(&ir(&meeting(i, "陈子墨"))).unwrap(),
            )
        })
        .collect();
    let rows: Vec<_> = docs.iter().map(|(p, o)| (p.as_str(), o)).collect();
    induce(&rows, &Lexicon::new()).classes
}

#[test]
fn decisions_rename_merge_and_reject() {
    let mut cs = classes();
    apply(
        &mut cs,
        &Decision::Rename {
            class_key: "meeting_minutes".into(),
            property: Some("attendees".into()),
            name: "Who came".into(),
        },
    )
    .unwrap();
    assert_eq!(cs[0].property("attendees").unwrap().name, "Who came");
    assert!(cs[0].property("attendees").unwrap().edited);
    apply(
        &mut cs,
        &Decision::Merge {
            class_key: "meeting_minutes".into(),
            property: "project".into(),
            into: "decisions".into(),
        },
    )
    .unwrap();
    assert!(cs[0].property("project").is_none());
    assert!(
        cs[0]
            .property("decisions")
            .unwrap()
            .keys
            .contains(&"project".to_owned())
    );
    apply(
        &mut cs,
        &Decision::Reject {
            class_key: "meeting_minutes".into(),
            property: Some("date".into()),
        },
    )
    .unwrap();
    assert_eq!(cs[0].property("date").unwrap().status, Status::Rejected);
    apply(
        &mut cs,
        &Decision::Accept {
            class_key: "meeting_minutes".into(),
            property: None,
        },
    )
    .unwrap();
    assert_eq!(cs[0].status, Status::Confirmed);
    assert_eq!(
        cs[0].property("attendees").unwrap().status,
        Status::Confirmed
    );
    assert_eq!(
        cs[0].property("date").unwrap().status,
        Status::Rejected,
        "a rejected property stays rejected"
    );
    assert!(
        apply(
            &mut cs,
            &Decision::Rename {
                class_key: "meeting_minutes".into(),
                property: None,
                name: " ".into()
            }
        )
        .is_err()
    );
    assert!(
        apply(
            &mut cs,
            &Decision::Accept {
                class_key: "nope".into(),
                property: None
            }
        )
        .is_err()
    );
}

#[test]
fn reconcile_keeps_decisions_and_proposes_new_properties_of_a_confirmed_class() {
    let mut old = classes();
    apply(
        &mut old,
        &Decision::Accept {
            class_key: "meeting_minutes".into(),
            property: None,
        },
    )
    .unwrap();
    apply(
        &mut old,
        &Decision::Rename {
            class_key: "meeting_minutes".into(),
            property: Some("attendees".into()),
            name: "Who came".into(),
        },
    )
    .unwrap();
    // Six more files, each with a new section that recurs.
    let more = |i: usize| format!("{}\n## 预算\n\n- 约{}万\n", meeting(i, "吴启明"), i + 10);
    let docs: Vec<(String, _)> = (0..8)
        .map(|i| {
            (
                format!("meetings/m{i}.md"),
                observe(&ir(&if i < 4 {
                    meeting(i, "陈子墨")
                } else {
                    more(i)
                }))
                .unwrap(),
            )
        })
        .collect();
    let rows: Vec<_> = docs.iter().map(|(p, o)| (p.as_str(), o)).collect();
    let induced = induce(&rows, &Lexicon::new());
    let (merged, proposals) = reconcile(&old, induced.classes, &induced.candidates);
    let c = merged
        .iter()
        .find(|c| c.class_key == "meeting_minutes")
        .unwrap();
    assert_eq!(c.status, Status::Confirmed);
    assert_eq!(c.property("attendees").unwrap().name, "Who came");
    assert_eq!(c.property("attendees").unwrap().status, Status::Confirmed);
    // The new section is seen in four files: a proposal, not a slot.
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].slug, "budget");
    assert_eq!(c.property("budget").unwrap().status, Status::Proposed);
    assert!(c.slots().all(|p| p.slug != "budget"));
}

#[test]
fn a_template_fills_slots_and_drops_what_has_no_slot() {
    let mut cs = classes();
    apply(
        &mut cs,
        &Decision::Accept {
            class_key: "meeting_minutes".into(),
            property: None,
        },
    )
    .unwrap();
    apply(
        &mut cs,
        &Decision::Reject {
            class_key: "meeting_minutes".into(),
            property: Some("decisions".into()),
        },
    )
    .unwrap();
    let src = format!("{}\n状态: 草稿\n", meeting(9, "林嘉禾"));
    let doc = ir(&src);
    let mut x = extract("meetings/m9.md", &doc);
    let had_status = x.facts.iter().any(|f| f.predicate.name() == "attr.status");
    let a = template::apply("meetings/m9.md", &doc, &mut x, &cs[0], &cs);
    assert!(
        a.relations >= 2,
        "the attendees fill the relation slot: {a:?}"
    );
    assert!(x.facts.iter().any(|f| f.predicate.name() == "mentions"
        && f.object.as_ref().is_some_and(|o| o.name == "林嘉禾")
        && f.rule_id == template::RULE_SLOT));
    assert!(x.facts.iter().any(|f| f.predicate.name() == "attr.date"
        && f.value.as_ref().is_some_and(|v| v["date"] == "2026-03-10")));
    if had_status {
        assert!(a.dropped >= 1);
        assert!(
            x.facts.iter().all(|f| f.predicate.name() != "attr.status"),
            "a fact outside the structure does not enter"
        );
    }
    // Without a confirmed structure nothing is applied.
    let mut y = extract("meetings/m9.md", &doc);
    let before = y.clone();
    cs[0].status = Status::Proposed;
    template::apply("meetings/m9.md", &doc, &mut y, &cs[0], &cs);
    assert_eq!(y, before);
}

#[test]
fn the_yaml_holds_both_sides_and_the_cross_links() {
    let mut lex = Lexicon::new();
    lex.add("Person", "陈子墨");
    let docs: Vec<(String, _)> = (0..4)
        .map(|i| {
            (
                format!("meetings/m{i}.md"),
                observe(&ir(&meeting(i, "陈子墨"))).unwrap(),
            )
        })
        .collect();
    let rows: Vec<_> = docs.iter().map(|(p, o)| (p.as_str(), o)).collect();
    let cs = induce(&rows, &lex).classes;
    let mut db = DbSide::default();
    db.tables.push(DbTable {
        name: "public.customers".into(),
        rows: Some(3),
        columns: vec![
            DbColumn {
                name: "id".into(),
                data_type: "integer".into(),
                nullable: false,
                primary_key: true,
            },
            DbColumn {
                name: "name".into(),
                data_type: "text".into(),
                nullable: false,
                primary_key: false,
            },
        ],
        foreign_keys: vec![],
    });
    db.rules.push(DbLinkRule {
        table: "public.customers".into(),
        key_col: "id".into(),
        name_col: "name".into(),
        entity_type: "Person".into(),
    });
    db.names
        .entry("public.customers".into())
        .or_default()
        .insert(crate::resolve::normalize::norm("陈子墨"));
    let y = render(2, &cs, Some(&db));
    assert!(y.contains("structure_version: 2"));
    assert!(y.contains("origin: documents") && y.contains("origin: database"));
    assert!(y.contains("kind: source_link"), "{y}");
    assert!(y.contains("links_to: {database: public.customers"));
    let docs_only = render(2, &cs, None);
    assert!(docs_only.contains("sources: [documents]") && !docs_only.contains("origin: database"));
}
