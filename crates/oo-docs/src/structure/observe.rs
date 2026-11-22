//! What one file carries: its fields and their values (D93). Pure.
//!
//! A field is a frontmatter key, a table header, a section heading, a `key: value` line or a varying directory level.
//! Its canonical key folds spelling and language (`参会人`, `与会人员` and `Attendees` are one concept), so the files of
//! one class agree on keys whatever the language they are written in.

use oo_core::docir::{BlockKind, DocFormat, DocIr};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use super::{FieldSource, Shape};
use crate::extract::rules::kv::split_kv;

/// Concept names: a list of spellings (normalized) per slug. The slug is the property's stable name.
pub const CONCEPTS: &[(&str, &[&str])] = &[
    (
        "attendees",
        &[
            "参会人",
            "参会人员",
            "与会人",
            "与会人员",
            "出席人员",
            "出席",
            "参加人员",
            "attendees",
            "attendee",
            "participants",
            "participant",
        ],
    ),
    (
        "date",
        &[
            "日期",
            "时间",
            "会议时间",
            "会议日期",
            "date",
            "time",
            "meeting date",
            "day",
        ],
    ),
    ("agenda", &["议程", "会议议程", "议题", "agenda", "topics"]),
    (
        "decisions",
        &[
            "决定",
            "决议",
            "结论",
            "会议结论",
            "decisions",
            "decision",
            "resolutions",
        ],
    ),
    (
        "actions",
        &[
            "待办",
            "待办事项",
            "行动项",
            "行动计划",
            "action items",
            "actions",
            "todo",
            "to do",
            "next steps",
            "followups",
            "follow ups",
        ],
    ),
    (
        "summary",
        &[
            "摘要",
            "概要",
            "总结",
            "会议纪要",
            "summary",
            "overview",
            "notes",
        ],
    ),
    (
        "owner",
        &[
            "负责人",
            "责任人",
            "owner",
            "lead",
            "responsible",
            "assignee",
            "经办人",
        ],
    ),
    (
        "decided_by",
        &[
            "决策人",
            "决定人",
            "批准人",
            "decided by",
            "decider",
            "approver",
            "approved by",
        ],
    ),
    (
        "project",
        &["项目", "所属项目", "项目名称", "project", "project name"],
    ),
    ("status", &["状态", "进度状态", "status", "state"]),
    (
        "rationale",
        &[
            "理由",
            "原因",
            "背景",
            "决策依据",
            "rationale",
            "reason",
            "context",
            "background",
        ],
    ),
    (
        "alternatives",
        &[
            "备选方案",
            "被否决方案",
            "否决方案",
            "alternatives",
            "options considered",
            "rejected options",
        ],
    ),
    (
        "this_week",
        &[
            "本周进展",
            "本周完成",
            "本周",
            "进展",
            "progress",
            "this week",
            "done this week",
            "accomplishments",
            "completed",
        ],
    ),
    (
        "next_week",
        &[
            "下周计划",
            "下周",
            "计划",
            "next week",
            "plan",
            "plans",
            "planned",
        ],
    ),
    (
        "risks",
        &[
            "风险",
            "风险与问题",
            "问题",
            "阻塞",
            "risks",
            "risk",
            "blockers",
            "issues",
        ],
    ),
    (
        "author",
        &[
            "作者",
            "汇报人",
            "撰写人",
            "author",
            "reporter",
            "written by",
            "prepared by",
        ],
    ),
    ("week", &["周次", "周", "week", "week of", "reporting week"]),
    (
        "name",
        &[
            "姓名",
            "名字",
            "名称",
            "员工",
            "name",
            "full name",
            "employee",
        ],
    ),
    (
        "role",
        &[
            "角色",
            "职位",
            "岗位",
            "职务",
            "role",
            "title",
            "position",
            "job title",
        ],
    ),
    ("team", &["团队", "小组", "team", "squad"]),
    ("department", &["部门", "department", "dept", "division"]),
    (
        "email",
        &["邮箱", "电子邮箱", "邮件", "email", "e-mail", "mail"],
    ),
    (
        "phone",
        &["电话", "手机", "手机号", "phone", "mobile", "tel"],
    ),
    (
        "employee_id",
        &[
            "工号",
            "员工号",
            "员工编号",
            "employee id",
            "employee_id",
            "staff id",
        ],
    ),
    (
        "manager",
        &["上级", "直属上级", "经理", "manager", "reports to"],
    ),
    ("budget", &["预算", "项目预算", "budget"]),
    (
        "amount",
        &[
            "金额",
            "合同金额",
            "总价",
            "价格",
            "amount",
            "price",
            "total",
            "contract value",
            "value",
        ],
    ),
    (
        "deadline",
        &[
            "截止日期",
            "截止",
            "交付日期",
            "到期日",
            "deadline",
            "due date",
            "due",
            "delivery date",
        ],
    ),
    (
        "start_date",
        &[
            "开始日期",
            "起始日期",
            "起始",
            "生效日期",
            "start date",
            "start",
            "effective date",
            "begins",
            "入职日期",
            "入职时间",
            "hire date",
            "joined",
            "join date",
        ],
    ),
    (
        "end_date",
        &[
            "结束日期",
            "终止日期",
            "到期日期",
            "end date",
            "end",
            "expires",
            "expiry",
        ],
    ),
    ("customer", &["客户", "客户名称", "customer", "client"]),
    ("party_a", &["甲方", "party a", "buyer", "first party"]),
    (
        "party_b",
        &[
            "乙方",
            "party b",
            "seller",
            "supplier",
            "vendor",
            "second party",
        ],
    ),
    (
        "contract_no",
        &[
            "合同编号",
            "合同号",
            "编号",
            "contract no",
            "contract number",
            "contract id",
            "no",
            "number",
            "id",
        ],
    ),
    (
        "term",
        &[
            "期限",
            "合同期限",
            "有效期",
            "term",
            "duration",
            "contract term",
        ],
    ),
    ("priority", &["优先级", "priority", "level"]),
    (
        "category",
        &["类别", "分类", "类型", "category", "type", "kind"],
    ),
    ("tags", &["标签", "tags", "tag", "labels"]),
    (
        "location",
        &["地点", "会议地点", "location", "venue", "place"],
    ),
    (
        "host",
        &[
            "主持人",
            "召集人",
            "host",
            "chair",
            "facilitator",
            "organizer",
        ],
    ),
    (
        "payment",
        &[
            "付款条款",
            "付款方式",
            "支付条款",
            "payment",
            "payment terms",
        ],
    ),
    ("impact", &["影响", "影响范围", "impact"]),
    ("root_cause", &["根因", "根本原因", "root cause", "cause"]),
    ("severity", &["严重程度", "严重等级", "severity"]),
    (
        "reported",
        &["报告日期", "上报日期", "reported", "reported on"],
    ),
    ("reporter", &["报告人", "上报人", "reporter"]),
    ("service", &["服务", "所属服务", "service"]),
    (
        "contact",
        &["对接人", "联系人", "contact", "contact person"],
    ),
    ("outcome", &["结果", "outcome", "result"]),
    ("duration", &["时长", "通话时长", "duration"]),
    ("approved", &["已审批", "审批", "approved"]),
    (
        "receipt",
        &[
            "票据号",
            "票据",
            "发票号",
            "receipt",
            "receipt no",
            "invoice no",
        ],
    ),
    ("version", &["版本", "版本号", "version"]),
    (
        "released",
        &["发布日期", "发布时间", "released", "release date"],
    ),
    ("features", &["新功能", "功能", "features", "new features"]),
    ("fixes", &["修复", "缺陷修复", "fixes", "bug fixes"]),
    ("known_issues", &["已知问题", "known issues"]),
    (
        "fix",
        &["处置", "处理措施", "fix", "remediation", "resolution"],
    ),
    (
        "notes",
        &["备注", "说明", "remark", "remarks", "comments", "comment"],
    ),
];

/// One field of a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldObs {
    /// Canonical key (the concept slug when the spelling is known, else the folded text).
    pub key: String,
    /// As written.
    pub label: String,
    pub source: FieldSource,
    /// The values: one per record (table rows), the items of a list, or the text of a section.
    pub values: Vec<String>,
}

/// What one file carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileObs {
    pub shape: Shape,
    pub fields: Vec<FieldObs>,
    /// Records (table rows; 1 for a document).
    pub rows: u32,
}

const MAX_VALUES: usize = 200;
const MAX_VALUE_CHARS: usize = 400;

fn fold(s: &str) -> String {
    let s: String = s.nfkc().collect();
    let s = s
        .trim()
        .trim_matches(|c: char| matches!(c, ':' | '：' | '*' | '_' | '#' | ' '))
        .to_lowercase();
    // A leading number (`1.`, `（一）`, `一、`) and a trailing unit in brackets are not part of the name.
    let s = s.trim_start_matches(|c: char| {
        c.is_ascii_digit()
            || matches!(
                c,
                '.' | '、'
                    | ')'
                    | '('
                    | '）'
                    | '（'
                    | ' '
                    | '一'
                    | '二'
                    | '三'
                    | '四'
                    | '五'
                    | '六'
                    | '七'
                    | '八'
                    | '九'
                    | '十'
                    | '-'
            )
    });
    let s = match s.find(['(', '（']) {
        Some(i) if i > 0 => &s[..i],
        _ => s,
    };
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Canonical key of a field label: the slug of its concept when known, else the folded text.
pub fn canon_key(label: &str) -> String {
    let f = fold(label);
    if f.is_empty() {
        return String::new();
    }
    for (slug, words) in CONCEPTS {
        if words.iter().any(|w| *w == f) {
            return (*slug).to_owned();
        }
    }
    // A bilingual label (`参会人 / Attendees`): its parts agree on one concept.
    let parts: Vec<String> = f
        .split(['/', '|', '／', '、'])
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() > 1 {
        let keys: Vec<String> = parts.iter().map(|p| canon_key(p)).collect();
        let known = |k: &String| CONCEPTS.iter().any(|(s, _)| s == k);
        if known(&keys[0]) && keys.iter().all(|k| k == &keys[0] || !known(k)) {
            return keys[0].clone();
        }
    }
    f
}

/// ASCII slug of a canonical key (`attr.<slug>`, 1 to 40 of `[a-z_]`).
pub fn slug_of(key: &str) -> String {
    if CONCEPTS.iter().any(|(s, _)| *s == key) {
        return key.to_owned();
    }
    let ascii: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let squeezed: String = ascii
        .split('_')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    let letters_only: String = squeezed
        .chars()
        .filter(|c| c.is_ascii_lowercase() || *c == '_')
        .collect();
    if key.is_ascii() && letters_only.len() >= 2 && letters_only.len() == squeezed.len() {
        return letters_only.chars().take(40).collect();
    }
    // Not expressible in `[a-z_]`: a stable letter code of the key.
    let h = blake3::hash(key.as_bytes());
    let code: String = h
        .as_bytes()
        .iter()
        .take(6)
        .map(|b| char::from(b'a' + b % 26))
        .collect();
    format!("f_{code}")
}

fn clip(s: &str) -> String {
    s.trim().chars().take(MAX_VALUE_CHARS).collect()
}

fn value_strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![clip(s)],
        Value::Number(n) => vec![n.to_string()],
        Value::Bool(b) => vec![b.to_string()],
        Value::Array(a) => a.iter().flat_map(value_strings).collect(),
        _ => Vec::new(),
    }
}

fn push(fields: &mut Vec<FieldObs>, label: &str, source: FieldSource, values: Vec<String>) {
    let key = canon_key(label);
    if key.is_empty() || key.chars().count() > 40 {
        return;
    }
    let values: Vec<String> = values
        .into_iter()
        .map(|v| clip(&v))
        .filter(|v| !v.is_empty())
        .take(MAX_VALUES)
        .collect();
    if let Some(f) = fields
        .iter_mut()
        .find(|f| f.key == key && f.source == source)
    {
        f.values.extend(values);
        f.values.truncate(MAX_VALUES);
    } else {
        fields.push(FieldObs {
            key,
            label: label.trim().to_owned(),
            source,
            values,
        });
    }
}

/// Fields that name the file itself, not a property of its class.
fn generic(key: &str) -> bool {
    matches!(
        key,
        "title" | "标题" | "" | "type" | "类型" | "tags" | "标签"
    )
}

/// The fields of one parsed file. `None` for formats without fields (audio, images, empty files).
pub fn observe(ir: &DocIr) -> Option<FileObs> {
    if matches!(
        ir.format,
        DocFormat::Audio | DocFormat::Video | DocFormat::Image
    ) {
        return None;
    }
    let tabular = matches!(ir.format, DocFormat::Csv | DocFormat::Xlsx);
    let mut fields: Vec<FieldObs> = Vec::new();
    let mut rows: u32 = 1;
    let mut table_blocks = 0usize;
    let mut headings = 0usize;
    let mut prose = 0usize;

    if let Some(fm) = &ir.meta.frontmatter {
        for (k, v) in fm {
            push(&mut fields, k, FieldSource::Frontmatter, value_strings(v));
        }
    }
    let blocks = &ir.blocks;
    let mut i = 0usize;
    while i < blocks.len() {
        let b = &blocks[i];
        match b.kind {
            BlockKind::Table | BlockKind::SheetTable => {
                table_blocks += 1;
                if let Some(g) = &b.table {
                    let header_row = if g.header_rows > 0 {
                        g.header_rows - 1
                    } else {
                        0
                    };
                    let mut cols: Vec<(u32, String, Vec<String>)> = Vec::new();
                    for c in g.cells.iter().filter(|c| c.r == header_row) {
                        cols.push((c.c, c.text.trim().to_owned(), Vec::new()));
                    }
                    for cell in g.cells.iter().filter(|c| c.r > header_row) {
                        if let Some(col) = cols.iter_mut().find(|c| c.0 == cell.c) {
                            col.2.push(cell.text.clone());
                        }
                    }
                    let data_rows = g.rows.saturating_sub(header_row + 1);
                    rows = rows.max(data_rows);
                    for (_, label, values) in cols {
                        if !label.is_empty() {
                            push(&mut fields, &label, FieldSource::Column, values);
                        }
                    }
                }
            }
            BlockKind::Heading => {
                let level = b.level.unwrap_or(1);
                if level >= 2 {
                    headings += 1;
                    let mut values: Vec<String> = Vec::new();
                    let mut j = i + 1;
                    while j < blocks.len() {
                        let n = &blocks[j];
                        match n.kind {
                            BlockKind::Heading if n.level.unwrap_or(1) <= level => break,
                            BlockKind::Heading => {}
                            BlockKind::Paragraph
                            | BlockKind::ListItem
                            | BlockKind::Quote
                            | BlockKind::Kv => {
                                values.push(
                                    n.text
                                        .trim()
                                        .trim_start_matches(['-', '*', '+', ' '])
                                        .to_owned(),
                                );
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    push(&mut fields, &b.text, FieldSource::Section, values);
                }
            }
            BlockKind::Paragraph | BlockKind::ListItem | BlockKind::Kv => {
                prose += 1;
                if let Some((k, v)) = split_kv(&b.text) {
                    let mut values = Vec::new();
                    if v.trim().is_empty() {
                        let mut j = i + 1;
                        while j < blocks.len() && blocks[j].kind == BlockKind::ListItem {
                            values.push(
                                blocks[j]
                                    .text
                                    .trim()
                                    .trim_start_matches(['-', '*', '+', ' '])
                                    .to_owned(),
                            );
                            j += 1;
                        }
                    } else {
                        values.push(v);
                    }
                    push(&mut fields, &k, FieldSource::Line, values);
                }
            }
            _ => {}
        }
        i += 1;
    }
    fields.retain(|f| !generic(&f.key));
    if fields.is_empty() {
        return None;
    }
    let shape = if tabular || (table_blocks >= 1 && headings == 0 && prose <= 2 && rows >= 3) {
        Shape::Table
    } else {
        Shape::Document
    };
    if shape == Shape::Table {
        fields.retain(|f| f.source == FieldSource::Column);
        if fields.is_empty() {
            return None;
        }
    } else {
        rows = 1;
    }
    fields.sort_by(|a, b| (a.source, &a.key).cmp(&(b.source, &b.key)));
    Some(FileObs {
        shape,
        fields,
        rows,
    })
}

/// The directory level a file sits in: its parent directory, and the parent of that.
pub fn dir_levels(path: &str) -> (String, String) {
    let parts: Vec<&str> = path.split('/').collect();
    let n = parts.len();
    let parent = if n >= 2 { parts[n - 2] } else { "" };
    let grand = if n >= 3 { parts[n - 3] } else { "" };
    (parent.nfc().collect(), grand.nfc().collect())
}
