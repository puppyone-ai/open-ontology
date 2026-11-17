//! `table.header_map`, `xlsx.people`, `xlsx.projects`, `csv.*` and the
//! generic table mentions (10 §5.1). Pure.
//!
//! The header row comes from the `sheet_table` block's grid; the rows are the
//! `kv` child blocks (one per non-empty data row, text `header: value` lines),
//! so every fact cites the row it came from. Header words are compared after
//! NFKC, lower-casing and removing blanks. Names only come from columns that
//! say what they hold (`姓名`, `负责人`, `预订人`, ...), never from free cells.

use oo_core::docir::{Block, BlockKind};
use serde_json::json;
use unicode_normalization::UnicodeNormalization;

use super::dates::ParsedDate;
use super::kv;
use super::{Ctx, MAX_NAME_CHARS, clip};
use crate::extract::model::{EntityRef, EntityType, EvidenceRef, Predicate};

/// What a column holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Name,
    EnglishName,
    Nickname,
    Dept,
    Title,
    Email,
    EmployeeId,
    Project,
    BelongsProject,
    Owner,
    Status,
    From,
    To,
    Contact,
    Org,
    Kind,
    /// `名称`: names the row's subject; only an organisation table reads it.
    Label,
}

/// Header synonyms, Chinese and English, compared after [`header_key`].
pub const HEADER_SYNONYMS: &[(Role, &[&str])] = &[
    (
        Role::Name,
        &["姓名", "名字", "name", "fullname", "全名", "成员"],
    ),
    (Role::EnglishName, &["英文名", "englishname"]),
    (
        Role::Nickname,
        &["昵称", "别名", "花名", "alias", "nickname"],
    ),
    (Role::Dept, &["部门", "团队", "team", "department", "dept"]),
    (
        Role::Title,
        &["角色", "职位", "职务", "岗位", "role", "title", "position"],
    ),
    (
        Role::Email,
        &["邮箱", "邮件", "电子邮箱", "email", "e-mail"],
    ),
    (
        Role::EmployeeId,
        &["工号", "员工号", "员工编号", "id", "employeeid", "empid"],
    ),
    (
        Role::Project,
        &["项目", "项目名称", "项目名", "project", "projectname"],
    ),
    (Role::BelongsProject, &["所属项目", "关联项目", "相关项目"]),
    (Role::Owner, &["负责人", "项目负责人", "责任人", "owner"]),
    (Role::Status, &["状态", "status"]),
    (
        Role::From,
        &[
            "开始",
            "开始日期",
            "起始日期",
            "生效日期",
            "入职日期",
            "start",
            "startdate",
            "since",
        ],
    ),
    (
        Role::To,
        &[
            "截止",
            "截止日期",
            "结束",
            "结束日期",
            "end",
            "enddate",
            "deadline",
            "due",
        ],
    ),
    (
        Role::Contact,
        &[
            "联系人",
            "预订人",
            "跟进人",
            "经办人",
            "创建人",
            "contact",
            "assignee",
            "reporter",
            "booker",
        ],
    ),
    (
        Role::Org,
        &[
            "客户",
            "供应商",
            "客户名称",
            "customer",
            "vendor",
            "supplier",
        ],
    ),
    (Role::Kind, &["类型", "type"]),
    (Role::Label, &["名称", "单位", "公司"]),
];

/// Comparison form of a header cell: NFKC, no blanks, lower case, without a
/// trailing parenthesised unit (`预算（万元）`).
pub fn header_key(text: &str) -> String {
    let s: String = text.nfkc().collect();
    let s = match s.find('(') {
        Some(i) => &s[..i],
        None => &s,
    };
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The role of one header cell.
pub fn role_of(header: &str) -> Option<Role> {
    let key = header_key(header);
    if key.is_empty() {
        return None;
    }
    HEADER_SYNONYMS
        .iter()
        .find(|(_, words)| words.contains(&key.as_str()))
        .map(|(r, _)| *r)
}

/// What a whole table is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableKind {
    /// `xlsx.people` / `csv.people`.
    People,
    /// `xlsx.projects` / `csv.projects`.
    Projects,
    /// Anything else: only mentions from role columns.
    Generic,
}

/// Classify a table by its mapped headers (10 §5.1).
pub fn table_kind(roles: &[Option<Role>]) -> TableKind {
    let has = |r: Role| roles.contains(&Some(r));
    let people_cols = [
        Role::Name,
        Role::Dept,
        Role::Title,
        Role::Email,
        Role::EmployeeId,
        Role::Project,
        Role::Owner,
    ]
    .iter()
    .filter(|r| has(**r))
    .count();
    if has(Role::Name) && people_cols >= 2 {
        return TableKind::People;
    }
    let project_cols = [
        Role::Project,
        Role::Owner,
        Role::Status,
        Role::From,
        Role::To,
    ]
    .iter()
    .filter(|r| has(**r))
    .count();
    if has(Role::Project) && project_cols >= 2 {
        return TableKind::Projects;
    }
    TableKind::Generic
}

/// Organisation kinds in a `类型` column.
fn org_kind(value: &str) -> bool {
    matches!(
        header_key(value).as_str(),
        "客户" | "供应商" | "合作伙伴" | "customer" | "vendor" | "supplier" | "partner"
    )
}

/// One data row: its `kv` block and the cells by column.
struct Row<'a> {
    block: &'a Block,
    cells: Vec<(Role, String)>,
}

impl Row<'_> {
    fn all(&self, r: Role) -> impl Iterator<Item = &str> {
        self.cells
            .iter()
            .filter(move |(x, _)| *x == r)
            .map(|(_, v)| v.as_str())
    }

    fn first(&self, r: Role) -> Option<&str> {
        self.all(r).next()
    }
}

/// Split the `header: value` lines of a `kv` block by the known headers.
fn row_cells(text: &str, headers: &[(String, Option<Role>)]) -> Vec<(Role, String)> {
    let mut out: Vec<(Option<Role>, String)> = Vec::new();
    for line in text.split('\n') {
        let mut hit = None;
        for (h, role) in headers {
            if let Some(rest) = line.strip_prefix(h.as_str())
                && let Some(value) = rest.strip_prefix(": ")
                && hit
                    .as_ref()
                    .is_none_or(|(len, _, _): &(usize, _, _)| h.len() > *len)
            {
                hit = Some((h.len(), *role, value.to_owned()));
            }
        }
        match hit {
            Some((_, role, value)) => out.push((role, value)),
            None => {
                if let Some(last) = out.last_mut() {
                    last.1.push('\n');
                    last.1.push_str(line);
                }
            }
        }
    }
    out.into_iter()
        .filter_map(|(r, v)| r.map(|r| (r, v.trim().to_owned())))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

fn date_of(row: &Row<'_>, r: Role) -> Option<ParsedDate> {
    row.first(r).and_then(kv::date_value)
}

fn names(value: &str) -> Vec<String> {
    kv::split_names(value)
        .into_iter()
        .map(|p| clip(&p.name, MAX_NAME_CHARS))
        .filter(|n| !n.is_empty() && n.chars().any(char::is_alphabetic))
        .collect()
}

/// Apply the table rules to every table block of the IR.
pub(super) fn apply(ctx: &mut Ctx<'_>, blocks: &[Block]) {
    for table in blocks {
        if table.kind != BlockKind::SheetTable {
            continue;
        }
        let Some(grid) = &table.table else { continue };
        if grid.header_rows != 1 {
            continue;
        }
        let mut headers: Vec<(String, Option<Role>)> = grid
            .cells
            .iter()
            .filter(|c| c.r == 0 && !c.text.trim().is_empty())
            .map(|c| (c.text.trim().to_owned(), role_of(&c.text)))
            .collect();
        // Longer header words first so `所属项目` wins over `项目`.
        headers.sort_by_key(|h| std::cmp::Reverse(h.0.len()));
        let roles: Vec<Option<Role>> = headers.iter().map(|(_, r)| *r).collect();
        let kind = table_kind(&roles);
        let org_table = roles.contains(&Some(Role::Label)) && roles.contains(&Some(Role::Kind));
        let rows: Vec<Row<'_>> = blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Kv && b.parent.as_deref() == Some(table.id.as_str()))
            .map(|b| Row {
                block: b,
                cells: row_cells(&b.text, &headers),
            })
            .collect();
        for row in &rows {
            let ev = EvidenceRef::from_block(row.block);
            match kind {
                TableKind::People => people_row(ctx, row, &ev),
                TableKind::Projects => project_row(ctx, row, &ev),
                TableKind::Generic => {}
            }
            generic_row(ctx, row, &ev, kind, org_table);
        }
    }
}

fn rule_ids(kind: TableKind, csv: bool) -> &'static str {
    match (kind, csv) {
        (TableKind::People, false) => "xlsx.people",
        (TableKind::People, true) => "csv.people",
        (TableKind::Projects, false) => "xlsx.projects",
        (TableKind::Projects, true) => "csv.projects",
        (TableKind::Generic, _) => "table.header_map",
    }
}

fn is_csv(ctx: &Ctx<'_>) -> bool {
    ctx.csv
}

fn people_row(ctx: &mut Ctx<'_>, row: &Row<'_>, ev: &EvidenceRef) {
    let rule = rule_ids(TableKind::People, is_csv(ctx));
    let Some(name) = row.first(Role::Name).map(|n| clip(n, MAX_NAME_CHARS)) else {
        return;
    };
    if name.is_empty() || !name.chars().any(char::is_alphabetic) {
        return;
    }
    let person = EntityRef::new(EntityType::Person, name);
    ctx.col.entity(&person, Some(ev));
    let from = date_of(row, Role::From);
    for r in [Role::EnglishName, Role::Nickname] {
        for v in row.all(r) {
            for part in v.split(['、', '，', ',', '/', ';', '；']) {
                ctx.col.alias(&person, part, 9500, rule, ev);
            }
        }
    }
    for dept in row.all(Role::Dept).flat_map(names) {
        let org = EntityRef::new(EntityType::Organization, dept);
        ctx.fact(
            &person,
            Predicate::MemberOf,
            Some(org),
            None,
            from.as_ref(),
            None,
            9500,
            rule,
            ev,
        );
    }
    for title in row.all(Role::Title) {
        let t = clip(title, 200);
        ctx.fact(
            &person,
            Predicate::Attr("role".into()),
            None,
            Some(json!({"text": t})),
            from.as_ref(),
            None,
            9500,
            rule,
            ev,
        );
    }
    for project in row.all(Role::Project).flat_map(names) {
        let p = EntityRef::new(EntityType::Project, project);
        ctx.fact(
            &person,
            Predicate::MemberOf,
            Some(p),
            None,
            from.as_ref(),
            None,
            9500,
            rule,
            ev,
        );
    }
    for e in row.all(Role::Email) {
        let e = e.trim().trim_matches(['<', '>']);
        if super::text::is_email(e) {
            ctx.col.identity(&person, "email", e, 9500, rule, ev);
        }
    }
    for id in row.all(Role::EmployeeId) {
        let id = id.trim();
        if !id.is_empty() && id.chars().count() <= 64 && !id.contains(char::is_whitespace) {
            ctx.col.identity(&person, "employee_id", id, 9500, rule, ev);
        }
    }
}

fn project_row(ctx: &mut Ctx<'_>, row: &Row<'_>, ev: &EvidenceRef) {
    let rule = rule_ids(TableKind::Projects, is_csv(ctx));
    let Some(name) = row.first(Role::Project).map(|n| clip(n, MAX_NAME_CHARS)) else {
        return;
    };
    if name.is_empty() {
        return;
    }
    let project = EntityRef::new(EntityType::Project, name);
    ctx.col.entity(&project, Some(ev));
    let from = date_of(row, Role::From);
    let to = date_of(row, Role::To);
    for owner in row.all(Role::Owner).flat_map(names) {
        let person = EntityRef::new(EntityType::Person, owner);
        ctx.fact(
            &person,
            Predicate::Owns,
            Some(project.clone()),
            None,
            from.as_ref(),
            None,
            9500,
            rule,
            ev,
        );
    }
    if let Some(status) = row.first(Role::Status) {
        let s = clip(status.trim_end_matches('。'), 200);
        if !s.is_empty() {
            ctx.fact(
                &project,
                Predicate::Attr("status".into()),
                None,
                Some(json!({"text": s})),
                from.as_ref(),
                to.as_ref(),
                9500,
                rule,
                ev,
            );
        }
    }
}

/// Mentions from role columns, for every kind of table.
fn generic_row(
    ctx: &mut Ctx<'_>,
    row: &Row<'_>,
    ev: &EvidenceRef,
    kind: TableKind,
    org_table: bool,
) {
    let rule = rule_ids(TableKind::Generic, is_csv(ctx));
    let doc = ctx.doc.clone();
    if kind != TableKind::People {
        for p in row
            .all(Role::Owner)
            .chain(row.all(Role::Contact))
            .flat_map(names)
        {
            ctx.col
                .entity(&EntityRef::new(EntityType::Person, p), Some(ev));
        }
    }
    if kind == TableKind::Generic {
        for p in row.all(Role::Project).flat_map(names) {
            ctx.col
                .entity(&EntityRef::new(EntityType::Project, p), Some(ev));
        }
    }
    for p in row.all(Role::BelongsProject).flat_map(names) {
        let project = EntityRef::new(EntityType::Project, p);
        ctx.fact(
            &doc,
            Predicate::About,
            Some(project),
            None,
            None,
            None,
            9000,
            rule,
            ev,
        );
    }
    for o in row.all(Role::Org).flat_map(names) {
        let org = EntityRef::new(EntityType::Organization, o);
        ctx.fact(
            &doc,
            Predicate::About,
            Some(org),
            None,
            None,
            None,
            9000,
            rule,
            ev,
        );
    }
    if org_table
        && row.first(Role::Kind).is_some_and(org_kind)
        && let Some(n) = row.first(Role::Label)
    {
        for o in names(n) {
            ctx.col
                .entity(&EntityRef::new(EntityType::Organization, o), Some(ev));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_map_by_nfkc_and_blank_free_key() {
        assert_eq!(role_of("姓 名"), Some(Role::Name));
        assert_eq!(role_of("ＥＭＡＩＬ"), Some(Role::Email));
        assert_eq!(role_of("预算（万元）"), None);
        assert_eq!(role_of("所属项目"), Some(Role::BelongsProject));
        assert_eq!(role_of("项目"), Some(Role::Project));
        assert_eq!(role_of("Owner"), Some(Role::Owner));
        assert_eq!(role_of(""), None);
    }

    #[test]
    fn kinds_follow_the_spec_thresholds() {
        let r = |v: &[&str]| v.iter().map(|h| role_of(h)).collect::<Vec<_>>();
        assert_eq!(table_kind(&r(&["姓名", "部门"])), TableKind::People);
        assert_eq!(table_kind(&r(&["姓名", "备注"])), TableKind::Generic);
        assert_eq!(table_kind(&r(&["项目", "负责人"])), TableKind::Projects);
        assert_eq!(
            table_kind(&r(&["项目", "预算（万元）"])),
            TableKind::Generic
        );
        assert_eq!(
            table_kind(&r(&["任务", "负责人", "状态", "所属项目"])),
            TableKind::Generic
        );
    }

    #[test]
    fn row_cells_use_the_longest_header_and_keep_continuations() {
        let headers = vec![
            ("所属项目".to_owned(), Some(Role::BelongsProject)),
            ("项目".to_owned(), Some(Role::Project)),
            ("备注".to_owned(), None),
        ];
        let got = row_cells("所属项目: 星图\n备注: 第一行\n第二行", &headers);
        assert_eq!(got, vec![(Role::BelongsProject, "星图".to_owned())]);
    }
}
