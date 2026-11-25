use super::*;
use crate::parse::csv::parse_csv;
use crate::structure::{FieldSource, Property};

fn prop(slug: &str, key: &str, ty: ValueType) -> Property {
    Property {
        slug: slug.into(),
        name: slug.into(),
        keys: vec![key.into()],
        source: FieldSource::Column,
        value_type: ty,
        multi: false,
        files_with: 3,
        coverage: 1.0,
        fill: 1.0,
        required: true,
        aliases: Vec::new(),
        examples: Vec::new(),
        sample_values: Vec::new(),
        enum_values: Vec::new(),
        relation: None,
        status: Status::Confirmed,
        edited: false,
    }
}

fn class(kind: &str, props: Vec<Property>) -> ClassStructure {
    ClassStructure {
        class_key: "tbl".into(),
        label: "budgets".into(),
        kind: kind.into(),
        shape: Shape::Table,
        status: Status::Confirmed,
        version: 1,
        files: 1,
        rows: 3,
        properties: props,
        sample_paths: Vec::new(),
        signature: Vec::new(),
    }
}

fn fill(csv: &str, c: &ClassStructure) -> (Extraction, Filled) {
    fill_at("finance/budget-2027-01-12.csv", csv, c)
}

fn fill_at(path: &str, csv: &str, c: &ClassStructure) -> (Extraction, Filled) {
    let ir = parse_csv(csv.as_bytes()).unwrap().ir;
    let mut x = Extraction::default();
    let f = apply(path, &ir, &mut x, c, &BTreeMap::new());
    (x, f)
}

fn attr<'a>(x: &'a Extraction, subject: &str, slug: &str) -> Vec<&'a Value> {
    x.facts
        .iter()
        .filter(|f| f.subject.name == subject && f.predicate == Predicate::Attr(slug.into()))
        .filter_map(|f| f.value.as_ref())
        .collect()
}

fn budget_class() -> ClassStructure {
    class(
        "other",
        vec![
            prop("category", "category", ValueType::Text),
            prop("budget", "budget", ValueType::Amount),
            prop("owner", "owner", ValueType::Person),
            prop("due", "due", ValueType::Date),
            prop("status", "status", ValueType::Enum),
            prop("yield_rate", "yield rate", ValueType::Percent),
            prop("version", "version", ValueType::Version),
            prop("qty", "qty", ValueType::Number),
        ],
    )
}

const BUDGET: &str = "Category,Budget,Owner,Due,Status,Yield rate,Version,Qty\n\
Hardware,\"¥3,100,000\",Daniel Ortiz,2027-03-05,open,百分之八十八,v1.2.0,三十\n\
Marketing,六百八十五万元,Priya Raman,March 16 2027,closed,85 percent,v1.3.0,12\n\
Logistics,1.2 million,Tianyou Jiang,TBD,open,12%,v2.0,7\n";

#[test]
fn every_row_is_an_instance_and_every_column_a_typed_property() {
    let (x, f) = fill(BUDGET, &budget_class());
    assert_eq!((f.rows_total, f.rows_filled, f.rows_unnamed), (3, 3, 0));
    let thing = |n: &str| EntityRef {
        ty: EntityType::Thing,
        thing_label: Some("budgets".into()),
        subkind: None,
        name: n.into(),
    };
    for n in ["Hardware", "Marketing", "Logistics"] {
        assert!(x.entities.iter().any(|m| m.entity == thing(n)), "{n}");
    }
    assert_eq!(attr(&x, "Hardware", "budget")[0]["amount"], "3100000 CNY");
    assert_eq!(attr(&x, "Marketing", "budget")[0]["amount"], "6850000 CNY");
    assert_eq!(
        attr(&x, "Marketing", "budget")[0]["original"],
        "六百八十五万元"
    );
    assert_eq!(attr(&x, "Logistics", "budget")[0]["amount"], "1200000");
    assert_eq!(attr(&x, "Hardware", "due")[0]["date"], "2027-03-05");
    assert_eq!(attr(&x, "Marketing", "due")[0]["date"], "2027-03-16");
    assert!(attr(&x, "Logistics", "due").is_empty(), "TBD is not a date");
    assert_eq!(f.rejected, 1);
    assert_eq!(attr(&x, "Hardware", "status")[0]["enum"], "open");
    assert_eq!(attr(&x, "Hardware", "yield_rate")[0]["ratio"], "0.88");
    assert_eq!(attr(&x, "Marketing", "yield_rate")[0]["ratio"], "0.85");
    assert_eq!(attr(&x, "Logistics", "yield_rate")[0]["ratio"], "0.12");
    assert_eq!(attr(&x, "Hardware", "version")[0]["text"], "v1.2.0");
    assert_eq!(attr(&x, "Hardware", "qty")[0]["number"], "30");
    assert_eq!(attr(&x, "Marketing", "qty")[0]["number"], "12");
}

#[test]
fn a_relation_column_resolves_to_entities_of_another_class() {
    let (x, f) = fill(BUDGET, &budget_class());
    assert_eq!(f.relations, 3);
    let hit = x.facts.iter().find(|f| {
        f.subject.name == "Hardware"
            && f.predicate == Predicate::About
            && f.object
                .as_ref()
                .is_some_and(|o| o.ty == EntityType::Person && o.name == "Daniel Ortiz")
    });
    assert!(
        hit.is_some(),
        "{:?}",
        x.facts
            .iter()
            .map(|f| f.predicate.name())
            .collect::<Vec<_>>()
    );
    assert!(
        x.entities
            .iter()
            .any(|m| m.entity.ty == EntityType::Person && m.entity.name == "Priya Raman")
    );
}

#[test]
fn provenance_is_the_cell_range_of_the_row() {
    let (x, _) = fill(BUDGET, &budget_class());
    let f = x
        .facts
        .iter()
        .find(|f| f.subject.name == "Marketing" && f.predicate == Predicate::Attr("budget".into()))
        .unwrap();
    assert_eq!(f.rule_id, RULE_ROW);
    match &f.evidence[0].anchor {
        oo_core::docir::Anchor::Cells { range, .. } => assert_eq!(range, "A3:H3"),
        a => panic!("{a:?}"),
    }
}

#[test]
fn a_people_table_gives_persons_with_identities_and_memberships() {
    let c = class(
        "people_table",
        vec![
            prop("name", "name", ValueType::Person),
            prop("email", "email", ValueType::Email),
            prop("department", "department", ValueType::Organization),
            prop("employee_id", "employee_id", ValueType::Id),
            prop("hired", "hired", ValueType::Date),
        ],
    );
    let csv = "Name,Email,Department,Employee ID,Hired\nDaniel Kim,Daniel.Kim@example.com,Platform,E2001,2023年2月13日\nKaren Wu,karen@example.com,Product,E2002,2022-07-18\n";
    let mut c2 = c.clone();
    c2.properties[3].keys = vec!["employee id".into()];
    let (x, f) = fill(csv, &c2);
    assert_eq!(f.rows_filled, 2);
    let p = EntityRef::new(EntityType::Person, "Daniel Kim");
    assert!(x.entities.iter().any(|m| m.entity == p));
    assert!(
        x.identities
            .iter()
            .any(|i| i.entity == p && i.kind == "email" && i.value == "daniel.kim@example.com")
    );
    assert!(x.facts.iter().any(|f| {
        f.subject == p
            && f.predicate == Predicate::MemberOf
            && f.object
                .as_ref()
                .is_some_and(|o| o.ty == EntityType::Organization && o.name == "Platform")
    }));
    assert_eq!(attr(&x, "Daniel Kim", "hired")[0]["date"], "2023-02-13");
}

#[test]
fn a_project_table_gives_projects_about_their_owners() {
    let c = class(
        "project_table",
        vec![
            prop("project", "project", ValueType::Project),
            prop("owner", "owner", ValueType::Person),
            prop("budget", "budget", ValueType::Amount),
        ],
    );
    let (x, _) = fill(
        "Project,Owner,Budget (万元)\n北辰,周若溪,80\n星图,吴启明,120\n",
        &c,
    );
    let p = EntityRef::new(EntityType::Project, "北辰");
    assert!(x.entities.iter().any(|m| m.entity == p));
    assert!(x.facts.iter().any(|f| {
        f.predicate == Predicate::About
            && f.subject == p
            && f.object
                .as_ref()
                .is_some_and(|o| o.ty == EntityType::Person && o.name == "周若溪")
    }));
    assert_eq!(attr(&x, "北辰", "budget")[0]["amount"], "800000 CNY");
    assert_eq!(attr(&x, "星图", "budget")[0]["amount"], "1200000 CNY");
}

#[test]
fn a_header_of_another_language_maps_through_the_alignment_and_the_aliases() {
    let mut c = budget_class();
    c.properties[1].aliases = vec!["预算".into()];
    let mut align = BTreeMap::new();
    align.insert("状态".to_owned(), "status".to_owned());
    let ir = parse_csv("类别,预算,状态\n硬件,3100000,进行中\n".as_bytes())
        .unwrap()
        .ir;
    let mut x = Extraction::default();
    c.properties[0].aliases = vec!["类别".into()];
    let f = apply("a.csv", &ir, &mut x, &c, &align);
    assert_eq!(f.rows_filled, 1);
    assert_eq!(attr(&x, "硬件", "budget")[0]["amount"], "3100000");
    assert_eq!(attr(&x, "硬件", "status")[0]["enum"], "进行中");
}

#[test]
fn only_a_confirmed_table_class_is_filled() {
    let mut c = budget_class();
    c.status = Status::Proposed;
    let (x, f) = fill(BUDGET, &c);
    assert_eq!(f, Filled::default());
    assert!(x.facts.is_empty() && x.entities.is_empty());
    let mut c = budget_class();
    c.shape = Shape::Document;
    assert_eq!(fill(BUDGET, &c).1, Filled::default());
    // A property that is only proposed fills nothing.
    let mut c = budget_class();
    c.properties[1].status = Status::Proposed;
    let (x, _) = fill(BUDGET, &c);
    assert!(attr(&x, "Hardware", "budget").is_empty());
}

#[test]
fn rows_without_a_name_are_counted_not_made() {
    let mut csv = String::from("Category,Budget\n");
    for i in 0..9 {
        csv.push_str(&format!("Line {i},{}\n", 100_000 + i));
    }
    csv.push_str(",200000\n");
    let (x, f) = fill(&csv, &budget_class());
    assert_eq!(f.rows_total, 10);
    assert_eq!(f.rows_filled, 9);
    assert_eq!(f.rows_unnamed, 1);
    assert_eq!(x.entities.len(), 9);
}

#[test]
fn a_column_that_tells_the_rows_apart_names_them_not_the_first_one() {
    // `Category` repeats, `Receipt` is unique: the receipt names the instance, so no two rows collapse into one.
    let mut c = budget_class();
    c.properties = vec![
        prop("category", "category", ValueType::Enum),
        prop("receipt", "receipt", ValueType::Id),
        prop("amount", "amount", ValueType::Amount),
    ];
    let csv = "Category,Receipt,Amount\ntravel,RC1000,$120\ntravel,RC1001,$157\nmeals,RC1002,$194\ntravel,RC1003,$231\n";
    let (x, f) = fill(csv, &c);
    assert_eq!(f.rows_filled, 4);
    assert!(x.entities.iter().any(|m| m.entity.name == "RC1002"));
    assert_eq!(attr(&x, "RC1000", "category")[0]["enum"], "travel");
    assert_eq!(attr(&x, "RC1001", "amount")[0]["amount"], "157 USD");
}

#[test]
fn rows_of_a_table_with_no_unique_column_are_named_by_their_position() {
    let c = class(
        "other",
        vec![
            prop("category", "category", ValueType::Enum),
            prop("qty", "qty", ValueType::Number),
        ],
    );
    let (x, f) = fill("Category,Qty\na,1\na,2\nb,3\n", &c);
    assert_eq!(f.rows_filled, 3);
    assert!(x.entities.iter().any(|m| m.entity.name == "budgets #2"));
    assert_eq!(attr(&x, "budgets #4", "qty")[0]["number"], "3");
}

#[test]
fn a_large_sheet_is_read_up_to_the_bound_and_the_rest_is_reported() {
    let mut csv = String::from("Category,Budget\n");
    for i in 0..(MAX_ROWS_PER_JOB + 120) {
        csv.push_str(&format!("Line {i},{}\n", 10_000 + i));
    }
    let (x, f) = fill(&csv, &budget_class());
    assert_eq!(f.rows_total, MAX_ROWS_PER_JOB + 120);
    assert_eq!(f.rows_filled, MAX_ROWS_PER_JOB);
    assert!(f.truncated);
    assert_eq!(x.entities.len() as u32, MAX_ROWS_PER_JOB);
}

#[test]
fn the_result_is_deterministic_and_idempotent() {
    let (a, _) = fill(BUDGET, &budget_class());
    let (b, _) = fill(BUDGET, &budget_class());
    assert_eq!(a, b);
    let ir = parse_csv(BUDGET.as_bytes()).unwrap().ir;
    let mut x = a.clone();
    apply(
        "finance/budget-2027-01-12.csv",
        &ir,
        &mut x,
        &budget_class(),
        &BTreeMap::new(),
    );
    assert_eq!(x, a, "filling twice adds nothing");
}

#[test]
fn coercion_per_type() {
    let ctx = DateCtx::on("2027-01-12");
    let c = |ty, h: &str, raw: &str| coerce(ty, h, raw, &ctx).map(|c| (c.key, c.value));
    assert_eq!(
        c(ValueType::Date, "d", "三月十六号"),
        Some(("date", "2027-03-16".into()))
    );
    assert_eq!(
        c(ValueType::Date, "d", "下周二"),
        Some(("date", "2027-01-19".into()))
    );
    assert_eq!(
        c(ValueType::Date, "d", "2026-03-04 10:00"),
        Some(("date", "2026-03-04".into()))
    );
    assert_eq!(c(ValueType::Date, "d", "soon"), None);
    assert_eq!(
        c(ValueType::Amount, "Budget CNY", "1420000"),
        Some(("amount", "1420000 CNY".into()))
    );
    assert_eq!(
        c(ValueType::Amount, "预算（万元）", "142"),
        Some(("amount", "1420000 CNY".into()))
    );
    assert_eq!(
        c(ValueType::Amount, "x", "$3.5m"),
        Some(("amount", "3500000 USD".into()))
    );
    assert_eq!(c(ValueType::Amount, "x", "n/a"), None);
    assert_eq!(
        c(ValueType::Percent, "Yield", "88%"),
        Some(("ratio", "0.88".into()))
    );
    assert_eq!(
        c(ValueType::Percent, "完成率", "88"),
        Some(("ratio", "0.88".into()))
    );
    assert_eq!(c(ValueType::Percent, "Yield", "88"), None);
    assert_eq!(
        c(ValueType::Number, "n", "二十五"),
        Some(("number", "25".into()))
    );
    assert_eq!(
        c(ValueType::Number, "n", "1,250"),
        Some(("number", "1250".into()))
    );
    assert_eq!(
        c(ValueType::Enum, "s", "open"),
        Some(("enum", "open".into()))
    );
    assert_eq!(
        c(ValueType::Version, "v", "v1.2.0"),
        Some(("text", "v1.2.0".into()))
    );
    assert_eq!(
        c(ValueType::Email, "e", "a@b.co"),
        Some(("text", "a@b.co".into()))
    );
    assert_eq!(
        c(ValueType::Id, "i", "PO-1001"),
        Some(("text", "PO-1001".into()))
    );
    assert_eq!(c(ValueType::Person, "p", "Daniel Ortiz"), None);
}
