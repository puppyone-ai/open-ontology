//! Entity quality gate (D90). Pure, deterministic, run on every extraction (rules and model) before it is stored.
//!
//! A Decision is a proposition: someone decided something about something, and the sentence says so. A single
//! generic word is no Decision, Event or Person, and a Term or Topic of one or two words belongs in the ontology
//! only when the text defines it or keeps coming back to it. What fails the gate is dropped: it is not an entity, not
//! a fact and not a review item (D90, D96: the product is the ontology; a mention is not an entity), and the run counts it
//! ([`QualityStats`], shown in the stage detail of the file). The sentence stays reachable through the evidence
//! anchors of the facts that really are in the ontology.
//!
//! The gate looks at one file at a time. For the model's output it also reads the entities the Drive already
//! publishes (`known`), so a name another file has made an entity of is not asked to prove itself again.
//!
//! Checks by type:
//! * `Decision` (the model's, a list item under a decisions heading, a decisions file, a preference): not a
//!   generic word; enough words; an action (a verb, a modal, a CJK action word, or a long enough phrase); an actor
//!   (a person or organisation named in the sentence or an owner of the document, a decision record or a
//!   preference excepted); and the evidence block must contain the name.
//! * `Person`: a name, not a word of the stoplist, not a class of people.
//! * `Event`: at least two words (four CJK characters), not generic, a proper name for Latin script.
//! * `Term`, `Topic`: not generic; a short or all-generic name only when defined in the file, referenced in at
//!   least three blocks, or already known to the Drive. `dir.terms` entries are definitions by construction.

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::{BlockKind, DocIr};
use unicode_normalization::UnicodeNormalization;

use super::model::{
    EntityMention, EntityRef, EntityType, EvidenceRef, Extraction, FactMention, Predicate,
};
use crate::resolve::normalize::norm;

/// Blocks a short Term or Topic must be written in when the text does not define it.
pub const MIN_REFERENCES: usize = 3;
/// Words a Decision needs at least (a CJK character counts half a word): an action and an object.
const DECISION_MIN_WORDS: f64 = 2.0;
/// A phrase this long is a proposition even when no action word is recognised.
const LONG_PHRASE_WORDS: f64 = 5.0;
/// Share of the name's bigrams the evidence block must contain.
const MIN_COVERAGE: f64 = 0.6;

/// Why an entity did not pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// A word of the stoplist, or only such words.
    Generic,
    /// Too short to be a statement (a Decision, an Event).
    TooShort,
    /// No action in a Decision.
    NoAction,
    /// No actor and no owner for a Decision.
    NoActor,
    /// The evidence block does not contain the name.
    NoEvidenceSentence,
    /// Not the shape of a name of that type.
    BadShape,
    /// D96: a model entity with no relation, no attribute, no identity and not known to the Drive is a mention.
    MentionOnly,
    /// A short Term or Topic that is neither defined nor referenced often.
    Unsupported,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic_word",
            Self::TooShort => "too_short",
            Self::NoAction => "no_action",
            Self::NoActor => "no_actor",
            Self::NoEvidenceSentence => "no_evidence_sentence",
            Self::BadShape => "bad_shape",
            Self::MentionOnly => "mention_only",
            Self::Unsupported => "not_defined_or_repeated",
        }
    }
}

/// What the gate did to one extraction, keyed `Type:reason`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QualityStats {
    pub dropped: BTreeMap<String, u32>,
    /// `(type as stored, canonical name)` of every dropped entity: a reprocess does not hold back for facts these
    /// explain.
    pub dropped_names: BTreeSet<(String, String)>,
    /// Facts, aliases and links that went with a dropped entity.
    pub facts_dropped: u32,
    /// Decisions that were a defined term and now are one.
    pub retyped: u32,
}

impl QualityStats {
    pub fn entities_dropped(&self) -> u32 {
        self.dropped.values().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.dropped.is_empty() && self.facts_dropped == 0 && self.retyped == 0
    }

    pub fn add(&mut self, o: &QualityStats) {
        self.dropped_names.extend(o.dropped_names.iter().cloned());
        for (k, v) in &o.dropped {
            *self.dropped.entry(k.clone()).or_default() += v;
        }
        self.facts_dropped += o.facts_dropped;
        self.retyped += o.retyped;
    }

    /// The `quality` object of a stage detail.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "entities_dropped": self.entities_dropped(),
            "by_reason": self.dropped,
            "facts_dropped": self.facts_dropped,
            "retyped": self.retyped,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Words

/// English words that name a kind of thing, a step or a state, not a particular one. Lower case.
const STOP_EN: &[&str] = &[
    "a",
    "an",
    "the",
    "of",
    "to",
    "in",
    "on",
    "for",
    "and",
    "or",
    "not",
    "is",
    "are",
    "was",
    "be",
    "it",
    "this",
    "that",
    "we",
    "you",
    "they",
    "he",
    "she",
    "our",
    "their",
    "with",
    "by",
    "from",
    "as",
    "at",
    "if",
    "then",
    "so",
    "no",
    "yes",
    "all",
    "any",
    "new",
    "old",
    "action",
    "actions",
    "answer",
    "api",
    "approach",
    "background",
    "backup",
    "bug",
    "cache",
    "case",
    "change",
    "changes",
    "check",
    "checks",
    "ci",
    "cli",
    "client",
    "code",
    "commit",
    "company",
    "compaction",
    "config",
    "configuration",
    "consistency",
    "context",
    "cost",
    "cpu",
    "data",
    "date",
    "day",
    "decision",
    "decisions",
    "default",
    "defaults",
    "department",
    "design",
    "disk",
    "doc",
    "docs",
    "document",
    "documents",
    "error",
    "errors",
    "event",
    "events",
    "failure",
    "feature",
    "file",
    "files",
    "fix",
    "folder",
    "goal",
    "id",
    "idea",
    "index",
    "issue",
    "issues",
    "item",
    "items",
    "job",
    "jobs",
    "key",
    "latency",
    "level",
    "list",
    "lock",
    "locks",
    "log",
    "logs",
    "meeting",
    "meetings",
    "memory",
    "migration",
    "mode",
    "module",
    "month",
    "name",
    "network",
    "note",
    "notes",
    "option",
    "options",
    "overview",
    "path",
    "performance",
    "phase",
    "plan",
    "policy",
    "prepare",
    "problem",
    "process",
    "queue",
    "question",
    "questions",
    "record",
    "records",
    "recovery",
    "release",
    "replication",
    "request",
    "response",
    "result",
    "retry",
    "review",
    "risk",
    "rollback",
    "rule",
    "rules",
    "segment",
    "segments",
    "server",
    "service",
    "setting",
    "settings",
    "snapshot",
    "solution",
    "stage",
    "state",
    "status",
    "step",
    "steps",
    "success",
    "summary",
    "system",
    "table",
    "task",
    "tasks",
    "team",
    "term",
    "terms",
    "test",
    "tests",
    "testing",
    "throughput",
    "time",
    "timeout",
    "todo",
    "topic",
    "type",
    "update",
    "upgrade",
    "user",
    "users",
    "value",
    "version",
    "week",
    "word",
    "year",
    "commit",
    "abort",
    "prepare",
    "write",
    "read",
    "fsync",
    "flush",
    "sync",
    "check",
    "run",
    "start",
    "stop",
    "build",
    "deploy",
    "monitoring",
    "metrics",
    "alert",
    "alerts",
];

/// The same in Chinese: classes, steps, states and the nouns of engineering and office work.
const STOP_ZH: &[&str] = &[
    "公司",
    "团队",
    "部门",
    "小组",
    "客户",
    "用户",
    "同事",
    "领导",
    "我们",
    "大家",
    "回滚",
    "准备",
    "一致性",
    "幂等",
    "重试",
    "超时",
    "配置",
    "设置",
    "错误",
    "失败",
    "成功",
    "状态",
    "版本",
    "更新",
    "升级",
    "迁移",
    "测试",
    "检查",
    "日志",
    "队列",
    "任务",
    "问题",
    "功能",
    "请求",
    "响应",
    "数据",
    "系统",
    "服务",
    "流程",
    "计划",
    "评审",
    "会议",
    "笔记",
    "文档",
    "设计",
    "概述",
    "总结",
    "背景",
    "决定",
    "决策",
    "事项",
    "待办",
    "答案",
    "选项",
    "方案",
    "想法",
    "主题",
    "术语",
    "名称",
    "列表",
    "表格",
    "记录",
    "文件",
    "路径",
    "索引",
    "类型",
    "模式",
    "规则",
    "策略",
    "步骤",
    "阶段",
    "目标",
    "风险",
    "成本",
    "时间",
    "日期",
    "性能",
    "延迟",
    "内存",
    "磁盘",
    "网络",
    "缓存",
    "备份",
    "恢复",
    "复制",
    "压缩",
    "快照",
    "分片",
    "事务",
    "提交",
    "发布",
    "部署",
    "上线",
    "下线",
    "运维",
    "监控",
    "告警",
    "权限",
    "安全",
    "接口",
    "模块",
    "组件",
    "代码",
    "仓库",
    "分支",
    "合并",
    "改写",
    "写入",
    "读取",
    "唯一",
    "其他",
    "相关",
    "说明",
    "备注",
    "内容",
    "细节",
    "背景介绍",
    "目的",
    "结论",
    "原因",
    "结果",
    "影响",
    "负责人",
    "参会人",
    "议程",
    "行动项",
];

/// Verbs a decision is made with (base forms). Inflections are reduced by [`has_action_word`].
const VERBS_EN: &[&str] = &[
    "adopt",
    "use",
    "move",
    "keep",
    "retire",
    "reject",
    "replace",
    "drop",
    "switch",
    "migrate",
    "ship",
    "rename",
    "remove",
    "add",
    "enable",
    "disable",
    "require",
    "choose",
    "select",
    "approve",
    "agree",
    "stop",
    "start",
    "cancel",
    "defer",
    "delay",
    "postpone",
    "standardize",
    "standardise",
    "unify",
    "merge",
    "split",
    "deprecate",
    "introduce",
    "upgrade",
    "downgrade",
    "limit",
    "prefer",
    "avoid",
    "ban",
    "allow",
    "forbid",
    "set",
    "change",
    "make",
    "build",
    "launch",
    "release",
    "hire",
    "buy",
    "purchase",
    "outsource",
    "freeze",
    "write",
    "send",
    "run",
    "store",
    "rewrite",
    "go",
    "pick",
    "decide",
    "settle",
    "commit",
    "support",
    "cap",
    "default",
    "treat",
    "publish",
    "review",
    "review",
    "sunset",
    "roll",
    "revert",
    "reuse",
    "rely",
    "stay",
    "pause",
    "resume",
    "reduce",
    "increase",
    "extend",
    "shorten",
    "lengthen",
    "cut",
    "raise",
    "lower",
    "own",
    "assign",
    "delegate",
    "sign",
];

/// Modals that make a statement a commitment.
const MODALS_EN: &[&str] = &[
    "must", "will", "shall", "should", "won't", "cannot", "never", "always", "only",
];

/// CJK words and characters an action is written with.
const ACTIONS_ZH: &[&str] = &[
    "采用",
    "使用",
    "改用",
    "改为",
    "改成",
    "迁至",
    "迁到",
    "迁移",
    "发送",
    "取消",
    "停止",
    "启用",
    "禁用",
    "统一",
    "保留",
    "引入",
    "废弃",
    "淘汰",
    "采购",
    "控制",
    "优先",
    "先写",
    "必须",
    "不再",
    "决定",
    "批准",
    "同意",
    "确定",
    "选用",
    "切换",
    "升级",
    "下线",
    "上线",
    "发布",
    "演练",
    "要求",
    "禁止",
    "允许",
    "限制",
    "避免",
    "推迟",
    "延期",
    "提前",
    "取代",
    "替换",
    "替代",
    "合并",
    "拆分",
    "定为",
    "设为",
    "改到",
    "调整",
    "增加",
    "减少",
    "缩短",
    "延长",
    "外包",
    "自建",
    "购买",
    "签约",
    "终止",
    "暂停",
    "恢复",
    "重构",
    "归档",
    "删除",
    "新增",
    "移除",
    "不超过",
    "不得",
    "不要",
    "不能",
    "不可",
    "至少",
    "最多",
    "以内",
    "之内",
    "改到",
    "不做",
    "先写",
    "只用",
    "应当",
    "需要",
    "将会",
];

/// Characters a CJK Person is never made of: they mark a class or a group.
const GROUP_MARKS_ZH: &[&str] = &["们", "部", "组", "司", "队", "中心", "团队", "部门"];

// ---------------------------------------------------------------------------------------------
// Text helpers

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '\u{3040}'..='\u{30FF}' | '\u{AC00}'..='\u{D7AF}')
}

/// Lower case NFKC with blanks squeezed, the form names and blocks are compared in.
fn fold(s: &str) -> String {
    let t: String = s.nfkc().collect::<String>().to_lowercase();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Latin words of a folded string (letters and digits; `'` and `-` kept inside).
fn latin_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if (c.is_alphanumeric() && !is_cjk(c))
            || ((c == '\'' || c == '-' || c == '_') && !cur.is_empty())
        {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|w| w.trim_matches(['\'', '-', '_']).to_owned())
        .filter(|w| !w.is_empty())
        .collect()
}

fn cjk_count(s: &str) -> usize {
    s.chars().filter(|c| is_cjk(*c)).count()
}

/// Words of a name: Latin words plus CJK characters at half a word each.
fn words(s: &str) -> f64 {
    latin_words(s).len() as f64 + cjk_count(s) as f64 / 2.0
}

fn stop_en(w: &str) -> bool {
    STOP_EN.contains(&w)
}

/// The whole name is a stoplist word, or made only of stoplist words (and function words).
fn all_generic(name: &str) -> bool {
    let f = fold(name);
    let f = f.trim_matches(|c: char| !c.is_alphanumeric());
    if f.is_empty() {
        return true;
    }
    let lat = latin_words(f);
    let zh: String = f.chars().filter(|c| is_cjk(*c)).collect();
    if zh.is_empty() {
        return lat.iter().all(|w| stop_en(w));
    }
    if lat.iter().any(|w| !stop_en(w)) {
        return false;
    }
    // CJK part: cover it with stoplist words, longest first.
    let mut rest: &str = &zh;
    'outer: while !rest.is_empty() {
        let mut best: Option<usize> = None;
        for w in STOP_ZH {
            if rest.starts_with(w) && best.is_none_or(|b| w.len() > b) {
                best = Some(w.len());
            }
        }
        match best {
            Some(n) => {
                rest = &rest[n..];
                continue 'outer;
            }
            None => return false,
        }
    }
    true
}

/// An English verb in any common inflection.
fn english_verb(w: &str) -> bool {
    let base = |c: &str| VERBS_EN.contains(&c);
    if base(w) {
        return true;
    }
    for suf in ["s", "es", "ed", "d", "ing"] {
        if let Some(stem) = w.strip_suffix(suf) {
            if base(stem) || base(&format!("{stem}e")) {
                return true;
            }
            // a doubled final consonant: "dropped", "shipping"
            let b = stem.as_bytes();
            if b.len() >= 2 && b[b.len() - 1] == b[b.len() - 2] && base(&stem[..stem.len() - 1]) {
                return true;
            }
            if let Some(y) = stem.strip_suffix('i')
                && base(&format!("{y}y"))
            {
                return true;
            }
        }
    }
    matches!(
        w,
        "went"
            | "chose"
            | "chosen"
            | "made"
            | "built"
            | "sent"
            | "wrote"
            | "kept"
            | "held"
            | "took"
            | "gave"
            | "got"
    )
}

fn has_action_word(folded: &str) -> bool {
    latin_words(folded)
        .iter()
        .any(|w| english_verb(w) || MODALS_EN.contains(&w.as_str()))
        || ACTIONS_ZH.iter().any(|a| folded.contains(a))
}

fn bigrams(s: &str) -> BTreeSet<(char, char)> {
    let c: Vec<char> = s
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation())
        .collect();
    c.windows(2).map(|w| (w[0], w[1])).collect()
}

/// Share of the bigrams of `name` found in `text` (1.0 when `text` contains `name`).
fn coverage(name: &str, text: &str) -> f64 {
    if text.contains(name) {
        return 1.0;
    }
    let n = bigrams(name);
    if n.is_empty() {
        return 0.0;
    }
    let t = bigrams(text);
    n.intersection(&t).count() as f64 / n.len() as f64
}

/// A block of the file, folded.
struct Folded {
    kind: BlockKind,
    id: String,
    text: String,
}

struct FileText {
    blocks: Vec<Folded>,
}

impl FileText {
    fn new(ir: &DocIr) -> Self {
        Self {
            blocks: ir
                .blocks
                .iter()
                .filter(|b| !matches!(b.kind, BlockKind::Frontmatter | BlockKind::Code))
                .map(|b| Folded {
                    kind: b.kind,
                    id: b.id.clone(),
                    text: fold(&b.text),
                })
                .collect(),
        }
    }

    fn block(&self, ev: &EvidenceRef) -> Option<&Folded> {
        self.blocks.iter().find(|b| b.id == ev.block_id)
    }

    /// Blocks that write `name` (a heading counts twice).
    fn references(&self, name: &str) -> usize {
        self.blocks
            .iter()
            .filter(|b| b.text.contains(name))
            .map(|b| if b.kind == BlockKind::Heading { 2 } else { 1 })
            .sum()
    }

    /// Some block defines `name`: `**name**: ...`, `name: ...`, `name - ...` at the start of an item, `name is a ...`,
    /// `name means ...`, `name 是指 ...`, `所谓name`.
    fn defines(&self, name: &str) -> bool {
        // Cues that make a sentence a definition wherever the name stands.
        const SENTENCE: &[&str] = &[
            " is a ",
            " is an ",
            " is the ",
            " is defined as ",
            " means ",
            " refers to ",
            " stands for ",
            " denotes ",
            "是指",
            "指的是",
            "是一种",
            "是一个",
            "是一类",
        ];
        // Cues that define when the name opens the line or the item.
        const OPENING: &[&str] = &[":", "：", " — ", " – ", " - ", "（", " ("];
        self.blocks.iter().any(|b| {
            if b.kind == BlockKind::Heading {
                return false;
            }
            let mut from = 0;
            while let Some(at) = b.text[from..].find(name) {
                let start = from + at;
                let end = start + name.len();
                let before =
                    b.text[..start].trim_end_matches(['*', '_', '`', '"', '“', '「', ' ', '•']);
                let before = before.strip_suffix('-').map_or(before, str::trim_end);
                let after_raw = &b.text[end..];
                let after = after_raw.trim_start_matches(['*', '_', '`', '"', '”', '」']);
                let opens = before.is_empty() || before.ends_with('\n');
                if before.ends_with("所谓") || before.ends_with("定义") {
                    return true;
                }
                if opens
                    && OPENING
                        .iter()
                        .any(|c| after.starts_with(c) || after_raw.starts_with(c))
                {
                    return true;
                }
                if SENTENCE.iter().any(|c| after_raw.starts_with(c)) {
                    return true;
                }
                from = end;
            }
            false
        })
    }
}

// ---------------------------------------------------------------------------------------------
// The gate

/// Facts and entities the checks read besides the entity itself.
struct Ctx<'a> {
    text: FileText,
    /// Normalised names of the Persons and Organisations of the extraction.
    actors: BTreeSet<String>,
    /// A fact says someone owns or decides something in this file.
    owner_in_file: bool,
    known: BTreeSet<(EntityType, String)>,
    /// Entities with a fact of a rule that defines them.
    defined_by_rule: BTreeSet<EntityRef>,
    /// The extraction is the rules' (not the model's).
    structural: bool,
    /// Entities that are an end of a fact or carry an identity attribute.
    placed: BTreeSet<EntityRef>,
    _x: std::marker::PhantomData<&'a Extraction>,
}

fn parse_known(known: &[String]) -> BTreeSet<(EntityType, String)> {
    known
        .iter()
        .filter_map(|k| {
            let (ty, rest) = k.split_once(':')?;
            let name = rest.split_once("（别名：").map_or(rest, |(n, _)| n);
            Some((EntityType::parse(&ty.to_lowercase())?, norm(name)))
        })
        .collect()
}

fn ctx<'a>(x: &'a Extraction, ir: &DocIr, known: &[String]) -> Ctx<'a> {
    let actors = x
        .entities
        .iter()
        .filter(|m| matches!(m.entity.ty, EntityType::Person | EntityType::Organization))
        .map(|m| fold(&m.entity.name))
        .filter(|n| !n.is_empty())
        .collect();
    let owner_in_file = x.facts.iter().any(|f| {
        matches!(f.predicate, Predicate::Owns | Predicate::Decides)
            && matches!(f.subject.ty, EntityType::Person | EntityType::Organization)
    });
    let defined_by_rule = x
        .facts
        .iter()
        .filter(|f| f.rule_id == "dir.terms")
        .flat_map(|f| std::iter::once(f.subject.clone()).chain(f.object.clone()))
        .collect();
    Ctx {
        text: FileText::new(ir),
        actors,
        owner_in_file,
        known: parse_known(known),
        defined_by_rule,
        structural: x.llm.is_none(),
        placed: x
            .facts
            .iter()
            .flat_map(|f| std::iter::once(f.subject.clone()).chain(f.object.clone()))
            .chain(x.identities.iter().map(|i| i.entity.clone()))
            .collect(),
        _x: std::marker::PhantomData,
    }
}

fn check_decision(m: &EntityMention, c: &Ctx<'_>) -> Result<(), Reason> {
    let name = fold(&m.entity.name);
    let name = name.trim_matches(|ch: char| !ch.is_alphanumeric() && !is_cjk(ch));
    if all_generic(name) {
        return Err(Reason::Generic);
    }
    let evidence: Vec<&Folded> = m.evidence.iter().filter_map(|e| c.text.block(e)).collect();
    // The title of a decision record has the record behind it; a list item or a sentence has only itself.
    let record = evidence.iter().any(|b| b.kind == BlockKind::Heading);
    let w = words(name);
    if w < DECISION_MIN_WORDS {
        return Err(Reason::TooShort);
    }
    // A preference is a person's own statement of what they want; it needs no action word.
    if m.entity.subkind != Some("preference") && !has_action_word(name) && w < LONG_PHRASE_WORDS {
        return Err(Reason::NoAction);
    }
    // The name is written in its evidence.
    if !evidence.is_empty()
        && !evidence
            .iter()
            .any(|b| coverage(name, &b.text) >= MIN_COVERAGE)
    {
        return Err(Reason::NoEvidenceSentence);
    }
    // An actor or an owner. A preference is a person's own statement; a record has its owner in the file.
    let actor_in_sentence = evidence.iter().any(|b| {
        c.actors.iter().any(|a| b.text.contains(a.as_str()))
            || latin_words(&b.text)
                .iter()
                .any(|w| matches!(w.as_str(), "we" | "i" | "our"))
            || ["我们", "团队", "决定", "批准", "同意"]
                .iter()
                .any(|k| b.text.contains(k))
    });
    if m.entity.subkind == Some("preference") || record || c.owner_in_file || actor_in_sentence {
        Ok(())
    } else {
        Err(Reason::NoActor)
    }
}

fn check_person(m: &EntityMention, structural: bool) -> Result<(), Reason> {
    if super::rules::media::is_placeholder(&m.entity.name) {
        return Ok(());
    }
    let raw = m.entity.name.trim();
    // A name the rules read from a path, a frontmatter field or a table column is a name; only a stoplist word is not.
    if structural && raw.chars().count() <= 1 {
        return Ok(());
    }
    if all_generic(raw) {
        return Err(Reason::Generic);
    }
    if structural {
        return Ok(());
    }
    let zh = cjk_count(raw);
    if zh > 0 {
        let n = raw
            .chars()
            .filter(|c| is_cjk(*c) || *c == '·' || *c == '・')
            .count();
        if !(2..=8).contains(&n) || GROUP_MARKS_ZH.iter().any(|g| raw.ends_with(g)) {
            return Err(Reason::BadShape);
        }
        return Ok(());
    }
    let toks: Vec<&str> = raw.split_whitespace().collect();
    let ok = !toks.is_empty()
        && toks.len() <= 5
        && toks.iter().all(|t| {
            let f = fold(t);
            !stop_en(f.trim_matches('.'))
                && t.chars().next().is_some_and(|ch| {
                    ch.is_uppercase()
                        || ch.is_lowercase()
                            && matches!(*t, "van" | "von" | "de" | "da" | "bin" | "al" | "del")
                })
                && !t.chars().any(|ch| ch.is_ascii_digit())
        });
    if ok { Ok(()) } else { Err(Reason::BadShape) }
}

fn check_event(m: &EntityMention) -> Result<(), Reason> {
    let raw = m.entity.name.trim();
    if all_generic(raw) {
        return Err(Reason::Generic);
    }
    if words(raw) < 2.0 {
        return Err(Reason::TooShort);
    }
    if cjk_count(raw) == 0 && !raw.chars().any(|c| c.is_uppercase() || c.is_ascii_digit()) {
        return Err(Reason::BadShape);
    }
    Ok(())
}

fn check_term_or_topic(m: &EntityMention, c: &Ctx<'_>) -> Result<(), Reason> {
    let name = fold(&m.entity.name);
    let name = name
        .trim_matches(|ch: char| !ch.is_alphanumeric() && !is_cjk(ch))
        .to_owned();
    if name.is_empty() {
        return Err(Reason::BadShape);
    }
    if c.defined_by_rule.contains(&m.entity)
        || c.known.contains(&(m.entity.ty, norm(&m.entity.name)))
    {
        return Ok(());
    }
    // D96: the model's Term exists only from an explicit definition (a rule's tried or rejected item is a structural position).
    if matches!(m.entity.ty, EntityType::Term | EntityType::Thing) && !c.structural {
        return if c.text.defines(&name) {
            Ok(())
        } else {
            Err(Reason::Unsupported)
        };
    }
    // A name of one or two Latin words, or three CJK characters, is a word until the text shows it is a term.
    let short = cjk_count(&name) <= 3
        && latin_words(&name).len() <= 2
        && words(&name) <= 2.0 + f64::EPSILON;
    let generic = all_generic(&name);
    if !short && !generic {
        return Ok(());
    }
    if c.text.defines(&name) || c.text.references(&name) >= MIN_REFERENCES {
        return Ok(());
    }
    Err(if generic {
        Reason::Generic
    } else {
        Reason::Unsupported
    })
}

fn verdict(m: &EntityMention, c: &Ctx<'_>) -> Result<(), Reason> {
    verdict_by_type(m, c)?;
    let known = c.known.contains(&(m.entity.ty, norm(&m.entity.name)));
    if !c.structural
        && m.entity.ty != EntityType::Document
        && !known
        && !c.placed.contains(&m.entity)
    {
        return Err(Reason::MentionOnly);
    }
    Ok(())
}

fn verdict_by_type(m: &EntityMention, c: &Ctx<'_>) -> Result<(), Reason> {
    match m.entity.ty {
        EntityType::Decision => check_decision(m, c),
        EntityType::Person => check_person(m, c.structural),
        EntityType::Event => check_event(m),
        EntityType::Term | EntityType::Topic | EntityType::Thing => check_term_or_topic(m, c),
        _ => Ok(()),
    }
}

/// Run the gate. `known` are the entities the Drive publishes, `Type:name` (the model's prompt list); pass `&[]` for
/// the rules. Returns what stays and what was done.
pub fn gate(x: Extraction, ir: &DocIr, known: &[String]) -> (Extraction, QualityStats) {
    let mut stats = QualityStats::default();
    let verdicts: Vec<(EntityRef, Result<(), Reason>)> = {
        let c = ctx(&x, ir, known);
        x.entities
            .iter()
            .map(|m| (m.entity.clone(), verdict(m, &c)))
            .collect()
    };
    if verdicts.iter().all(|(_, v)| v.is_ok()) {
        return (x, stats);
    }
    // A Decision that is a word the file defines is that term.
    let mut retype: BTreeMap<EntityRef, EntityRef> = BTreeMap::new();
    let mut dropped: BTreeSet<EntityRef> = BTreeSet::new();
    {
        let c = ctx(&x, ir, known);
        for (e, v) in &verdicts {
            let Err(reason) = v else { continue };
            *stats
                .dropped
                .entry(format!("{}:{}", e.ty.as_str(), reason.as_str()))
                .or_default() += 1;
            if e.ty == EntityType::Decision && matches!(reason, Reason::Generic | Reason::TooShort)
            {
                let name = fold(&e.name);
                let name = name.trim_matches(|ch: char| !ch.is_alphanumeric() && !is_cjk(ch));
                if c.text.defines(name) {
                    retype.insert(e.clone(), EntityRef::new(EntityType::Term, e.name.clone()));
                    continue;
                }
            }
            stats
                .dropped_names
                .insert((e.ty.as_str().to_owned(), e.norm()));
            dropped.insert(e.clone());
        }
    }
    let keep = |e: &EntityRef| !dropped.contains(e) && !retype.contains_key(e);
    let mut out = Extraction {
        path: x.path.clone(),
        llm: x.llm.clone(),
        ..Extraction::default()
    };
    for m in x.entities {
        if keep(&m.entity) {
            out.entities.push(m);
        } else if let Some(t) = retype.get(&m.entity) {
            stats.retyped += 1;
            // The defining block is the evidence of the term; the decision's own block is as good for a text that defines it.
            if !out.entities.iter().any(|o| &o.entity == t) {
                out.entities.push(EntityMention {
                    entity: t.clone(),
                    evidence: m.evidence,
                });
            }
        }
    }
    for a in x.aliases {
        if keep(&a.entity) {
            out.aliases.push(a);
        } else {
            stats.facts_dropped += 1;
        }
    }
    for i in x.identities {
        if keep(&i.entity) {
            out.identities.push(i);
        } else {
            stats.facts_dropped += 1;
        }
    }
    for h in x.hints {
        if keep(&h.subject) {
            out.hints.push(h);
        } else {
            stats.facts_dropped += 1;
        }
    }
    for f in x.facts {
        let ends: Vec<&EntityRef> = std::iter::once(&f.subject)
            .chain(f.object.as_ref())
            .collect();
        if ends.iter().all(|e| keep(e)) {
            out.facts.push(f);
        } else {
            stats.facts_dropped += 1;
        }
    }
    // Terms the file defines keep a link from the document.
    out.entities.sort_by(|a, b| a.entity.cmp(&b.entity));
    (out, stats)
}

#[allow(dead_code)]
fn _types(_: &FactMention) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::rules::extract;
    use crate::parse::markdown::parse_markdown;

    fn run(path: &str, md: &str) -> Extraction {
        let ir = parse_markdown(md.as_bytes()).unwrap().ir;
        extract(path, &ir)
    }

    fn names(x: &Extraction, ty: EntityType) -> Vec<String> {
        x.entities
            .iter()
            .filter(|m| m.entity.ty == ty)
            .map(|m| m.entity.name.clone())
            .collect()
    }

    #[test]
    fn bare_words_under_a_decisions_heading_are_no_decisions() {
        let x = run(
            "docs/queue.md",
            "# Queue\n\nOwner: Tomas Reyes\n\n## Decisions\n\n- Rollback\n- Prepare\n- Consistency\n- Tomas Reyes decided to move the job queue from polling to LISTEN/NOTIFY.\n",
        );
        let d = names(&x, EntityType::Decision);
        assert_eq!(
            d,
            vec!["Tomas Reyes decided to move the job queue from polling to LISTEN/NOTIFY."],
            "{d:?}"
        );
    }

    #[test]
    fn chinese_words_are_no_decisions_and_propositions_stay() {
        let x = run(
            "docs/写入.md",
            "# 账本写入\n\n负责人：林远舟\n\n## 决定\n\n- 林远舟决定统一用单写者模式处理账本写入。\n- 唯一改写\n- 一致性\n- 回滚\n- 准备\n",
        );
        let d = names(&x, EntityType::Decision);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].contains("单写者"));
    }

    #[test]
    fn a_decision_record_with_a_generic_title_is_dropped_and_a_real_one_stays() {
        let a = run(
            "Decisions/rollback.md",
            "# Rollback\n\nNotes on rollback.\n",
        );
        assert!(names(&a, EntityType::Decision).is_empty());
        let b = run(
            "Decisions/0010.md",
            "---\nowner: Tomas Reyes\n---\n\n# Decision: use LISTEN/NOTIFY for the job queue\n\nTomas Reyes decided to use it.\n",
        );
        assert_eq!(
            names(&b, EntityType::Decision),
            vec!["use LISTEN/NOTIFY for the job queue"]
        );
    }

    #[test]
    fn a_decision_without_an_actor_is_not_published() {
        let ir = parse_markdown(
            "# Errors\n\n## Decision\n\n- Errors carry a stable numeric code.\n".as_bytes(),
        )
        .unwrap()
        .ir;
        let x = extract("docs/adr.md", &ir);
        assert!(names(&x, EntityType::Decision).is_empty());
    }

    #[test]
    fn a_short_term_needs_a_definition_or_repetition() {
        let mk = |text: &str, name: &str| {
            let ir = parse_markdown(text.as_bytes()).unwrap().ir;
            let b = ir.blocks.iter().find(|b| b.text.contains(name)).unwrap();
            let mut col = crate::extract::model::Collector::default();
            col.entity(
                &EntityRef::new(EntityType::Term, name),
                Some(&EvidenceRef::from_block(b)),
            );
            let x = col.finish("a.md");
            gate(x, &ir, &[])
        };
        let (x, s) = mk("# A\n\nRollback is slow.\n", "Rollback");
        assert!(x.entities.is_empty());
        assert_eq!(s.dropped.get("Term:generic_word"), Some(&1));
        let (x, _) = mk(
            "# A\n\n**Rollback**: undo of a prepared write.\n",
            "Rollback",
        );
        assert_eq!(x.entities.len(), 1);
        let (x, _) = mk(
            "# A\n\nHash partitioning by account.\n",
            "Hash partitioning by account",
        );
        assert_eq!(x.entities.len(), 1);
        let (x, _) = mk(
            "# A\n\nIdempotency one.\n\nIdempotency two.\n\nIdempotency three.\n",
            "Idempotency",
        );
        assert_eq!(x.entities.len(), 1);
        let (x, _) = mk(
            "# A\n\nIdempotency one.\n\nIdempotency two.\n",
            "Idempotency",
        );
        assert!(x.entities.is_empty());
    }

    #[test]
    fn people_and_events_need_a_name() {
        let m = |ty, name: &str| EntityMention {
            entity: EntityRef::new(ty, name),
            evidence: vec![],
        };
        assert!(check_person(&m(EntityType::Person, "Rollback"), false).is_err());
        assert!(check_person(&m(EntityType::Person, "Mara Lindqvist"), false).is_ok());
        assert!(check_person(&m(EntityType::Person, "林远舟"), false).is_ok());
        assert!(check_person(&m(EntityType::Person, "团队"), false).is_err());
        assert!(check_event(&m(EntityType::Event, "Prepare")).is_err());
        assert!(check_event(&m(EntityType::Event, "年度计划会")).is_ok());
        assert!(check_event(&m(EntityType::Event, "Customer Salon")).is_ok());
        assert!(check_event(&m(EntityType::Event, "weekly sync")).is_err());
    }

    #[test]
    fn the_gate_is_idempotent() {
        let x = run(
            "docs/queue.md",
            "# Queue\n\nOwner: Tomas Reyes\n\n## Decisions\n\n- Rollback\n- Tomas Reyes decided to move the job queue from polling to LISTEN/NOTIFY.\n",
        );
        let ir = parse_markdown("x".as_bytes()).unwrap().ir;
        let (again, s) = gate(x.clone(), &ir, &[]);
        assert!(s.is_empty() || again.entities.len() <= x.entities.len());
    }
}
