//! `ruleset_version = rules/1` (10 §5.1), the L2 subset: `doc.entity`,
//! `dir.people|projects|decisions|preferences`, `fm.type`, `fm.fields`,
//! `md.kv`, `md.headings`, `name.date`. Rules only build core types, never
//! `Thing`, and only read structured positions (path, frontmatter, `key:
//! value` lines, list items under known headings), so no free-text person
//! names are invented. Everything here is pure and deterministic.
//!
//! Reading choices where the spec is silent (also in the B-S2-07 report):
//! * the evidence of a path-derived fact (`dir.*`) is the first level 1
//!   heading block, else the first block;
//! * a `Document` always exists per file (name = path, aliases = title and
//!   file stem) and `mentions` every entity another rule found in the file;
//! * `owner`/`负责人` means `decides` when the file's entity is a Decision
//!   (a preference too), `owns` otherwise;
//! * rejected / tried alternatives are `Term` entities named by the text
//!   before the first clause break of the item;
//! * a preference file without list items takes each paragraph as one
//!   preference.

pub mod dates;
pub mod dirs;
pub mod frontmatter;
pub mod headings;
pub mod kv;

use oo_core::docir::{BlockKind, DocFormat, DocIr};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use self::dates::ParsedDate;
use self::dirs::{Dir, Kind, PathInfo};
use self::headings::Section;
use self::kv::Field;
use super::model::{
    Collector, EntityRef, EntityType, EvidenceRef, Extraction, FactMention, Predicate,
};

/// Version tag stored with every rule fact.
pub const RULESET_VERSION: &str = "rules/1";

const MAX_NAME_CHARS: usize = 120;

struct Ctx<'a> {
    col: Collector,
    doc: EntityRef,
    mains: Vec<EntityRef>,
    default_from: Option<(String, &'static str)>,
    _ir: &'a DocIr,
}

impl Ctx<'_> {
    #[allow(clippy::too_many_arguments)]
    fn fact(
        &mut self,
        subject: &EntityRef,
        predicate: Predicate,
        object: Option<EntityRef>,
        value: Option<Value>,
        from: Option<&ParsedDate>,
        to: Option<&ParsedDate>,
        conf: u16,
        rule: &'static str,
        ev: &EvidenceRef,
    ) {
        let (valid_from, valid_precision) = match from {
            Some(d) => (Some(d.iso.clone()), Some(d.precision)),
            None => match &self.default_from {
                Some((iso, precision)) => (Some(iso.clone()), Some(*precision)),
                None => (None, None),
            },
        };
        self.col.fact(FactMention {
            subject: subject.clone(),
            predicate,
            object,
            value,
            valid_from,
            valid_to: to.map(|d| d.iso.clone()),
            valid_precision,
            confidence_bp: conf,
            rule_id: rule,
            evidence: vec![ev.clone()],
        });
    }

    /// Apply one recognised field (frontmatter or `key: value` line) to the
    /// file's main entities.
    fn apply_field(
        &mut self,
        f: Field,
        values: &[String],
        ev: &EvidenceRef,
        conf: u16,
        rule: &'static str,
    ) {
        let mains = self.mains.clone();
        match f {
            Field::Owner | Field::DecidedBy => {
                for part in values.iter().flat_map(|v| kv::split_names(v)) {
                    let person = EntityRef::new(EntityType::Person, part.name);
                    for m in &mains {
                        let pred = match (f, m.ty) {
                            (_, EntityType::Decision) => Predicate::Decides,
                            (Field::DecidedBy, _) => continue,
                            (_, EntityType::Person) => continue,
                            _ => Predicate::Owns,
                        };
                        self.fact(
                            &person,
                            pred,
                            Some(m.clone()),
                            None,
                            part.from.as_ref(),
                            None,
                            conf,
                            rule,
                            ev,
                        );
                    }
                }
            }
            Field::Members => {
                for part in values.iter().flat_map(|v| kv::split_names(v)) {
                    let person = EntityRef::new(EntityType::Person, part.name);
                    for m in &mains {
                        if matches!(
                            m.ty,
                            EntityType::Project
                                | EntityType::Organization
                                | EntityType::Event
                                | EntityType::Topic
                        ) {
                            self.fact(
                                &person,
                                Predicate::MemberOf,
                                Some(m.clone()),
                                None,
                                part.from.as_ref(),
                                None,
                                conf,
                                rule,
                                ev,
                            );
                        }
                    }
                }
            }
            Field::Project => {
                for part in values.iter().flat_map(|v| kv::split_names(v)) {
                    let project = EntityRef::new(EntityType::Project, part.name);
                    for m in &mains {
                        if matches!(
                            m.ty,
                            EntityType::Document
                                | EntityType::Decision
                                | EntityType::Event
                                | EntityType::Topic
                        ) {
                            self.fact(
                                m,
                                Predicate::About,
                                Some(project.clone()),
                                None,
                                None,
                                None,
                                conf,
                                rule,
                                ev,
                            );
                        }
                    }
                }
            }
            Field::Status => {
                let Some(text) = values.first().map(|v| v.trim().trim_end_matches('。')) else {
                    return;
                };
                if text.is_empty() || text.chars().count() > 200 {
                    return;
                }
                for m in &mains {
                    self.fact(
                        m,
                        Predicate::Attr("status".into()),
                        None,
                        Some(json!({"text": text})),
                        None,
                        None,
                        conf,
                        rule,
                        ev,
                    );
                }
            }
            Field::Supersedes => {
                for part in values.iter().flat_map(|v| kv::split_names(v)) {
                    let old = EntityRef::new(
                        EntityType::Decision,
                        dirs::strip_decision_prefix(&part.name),
                    );
                    for m in &mains {
                        if m.ty == EntityType::Decision {
                            self.fact(
                                m,
                                Predicate::Supersedes,
                                Some(old.clone()),
                                None,
                                None,
                                None,
                                conf,
                                rule,
                                ev,
                            );
                        }
                    }
                }
            }
            Field::Rejected => {
                for v in values {
                    let name = kv::clean_item(v, MAX_NAME_CHARS);
                    if name.is_empty() {
                        continue;
                    }
                    let term = EntityRef::new(EntityType::Term, name);
                    if let Some(m) = mains.first().filter(|m| m.ty != EntityType::Document) {
                        let m = m.clone();
                        self.fact(
                            &m,
                            Predicate::Rejects,
                            Some(term),
                            None,
                            None,
                            None,
                            conf,
                            rule,
                            ev,
                        );
                    }
                }
            }
            Field::Aliases => {
                if let [m] = mains.as_slice() {
                    for part in values.iter().flat_map(|v| kv::split_names(v)) {
                        self.col.alias(m, &part.name, conf, rule, ev);
                    }
                }
            }
            Field::Type | Field::Date | Field::Tags => {}
        }
    }
}

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

/// Run `rules/1` on one parsed file. `path` is the drive-relative path.
pub fn extract(path: &str, ir: &DocIr) -> Extraction {
    let path = nfc(path);
    let info: PathInfo = dirs::path_info(&path);
    let blocks = &ir.blocks;
    // Evidence anchors: heading (level 1 first), frontmatter block.
    let h1 = blocks
        .iter()
        .find(|b| b.kind == BlockKind::Heading && b.level == Some(1));
    let anchor_block = h1
        .or_else(|| blocks.iter().find(|b| b.kind == BlockKind::Heading))
        .or_else(|| blocks.iter().find(|b| b.kind != BlockKind::Frontmatter))
        .or_else(|| blocks.first());
    let Some(anchor_block) = anchor_block else {
        return Extraction {
            path,
            ..Extraction::default()
        };
    };
    let anchor_ev = EvidenceRef::from_block(anchor_block);
    let fm_ev = blocks
        .iter()
        .find(|b| b.kind == BlockKind::Frontmatter)
        .map_or_else(|| anchor_ev.clone(), EvidenceRef::from_block);
    let is_text = matches!(ir.format, DocFormat::Md | DocFormat::Txt);
    let fm_map = ir.meta.frontmatter.clone().unwrap_or_default();
    let fm_fields = if is_text {
        frontmatter::fields(&fm_map)
    } else {
        Vec::new()
    };
    let fm_value = |f: Field| fm_fields.iter().find(|(k, _)| *k == f).map(|(_, v)| v);

    let title: Option<String> = h1
        .map(|b| b.text.trim().to_owned())
        .or_else(|| ir.meta.title.clone())
        .or_else(|| {
            fm_map
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|t| !t.is_empty());

    // Default valid_from: frontmatter date > file name date > document date.
    let default_from: Option<(String, &'static str)> = fm_value(Field::Date)
        .and_then(|v| v.first())
        .and_then(|v| kv::date_value(v))
        .or_else(|| {
            dates::scan(&info.stem, false)
                .into_iter()
                .next()
                .map(|(d, _, _)| d)
        })
        .map(|d| (d.iso, d.precision))
        .or_else(|| {
            ir.meta
                .created
                .as_deref()
                .and_then(|c| dates::parse_exact(c.get(..10).unwrap_or(c)))
                .map(|d| (d.iso, "doc_date"))
        });

    let doc = EntityRef::new(EntityType::Document, path.clone());
    let mut ctx = Ctx {
        col: Collector::default(),
        doc: doc.clone(),
        mains: Vec::new(),
        default_from,
        _ir: ir,
    };

    // doc.entity
    ctx.col.entity(&doc, Some(&anchor_ev));
    if let Some(t) = &title {
        ctx.col
            .alias(&doc, &clip(t, 256), 10000, "doc.entity", &anchor_ev);
    }
    ctx.col.alias(
        &doc,
        &clip(&info.stem, 256),
        10000,
        "doc.entity",
        &anchor_ev,
    );

    // dir.* / fm.type: the entity the file stands for.
    let kind: Option<Kind> = if is_text || info.dir != Dir::None {
        dirs::classify(
            &info,
            fm_value(Field::Type)
                .and_then(|v| v.first())
                .map(String::as_str),
        )
    } else {
        None
    };
    if let Some(kind) = kind {
        let text_blocks = |pred: &dyn Fn(BlockKind) -> bool| -> Vec<&oo_core::docir::Block> {
            blocks
                .iter()
                .filter(|b| pred(b.kind) && !b.text.trim().is_empty())
                .collect()
        };
        if kind.subkind == Some("preference") {
            let mut items = text_blocks(&|k| k == BlockKind::ListItem);
            if items.is_empty() {
                items = text_blocks(&|k| k == BlockKind::Paragraph);
            }
            for b in items {
                let name = clip(
                    b.text.trim().trim_end_matches(['。', '.', '；', ';']),
                    MAX_NAME_CHARS,
                );
                if name.is_empty() {
                    continue;
                }
                let e = EntityRef::preference(name);
                ctx.col.entity(&e, Some(&EvidenceRef::from_block(b)));
                ctx.mains.push(e);
            }
        } else {
            let name = dirs::main_name(&kind, &info, title.as_deref());
            let mut e = EntityRef::new(kind.ty, clip(&name, MAX_NAME_CHARS));
            e.subkind = kind.subkind;
            ctx.col.entity(&e, Some(&anchor_ev));
            // Title as alias when the file name was used for the name.
            if let Some(t) = &title
                && kind.ty != EntityType::Decision
            {
                ctx.col
                    .alias(&e, &clip(t, 256), kind.conf, kind.rule, &anchor_ev);
            }
            ctx.mains.push(e);
        }
        // dir.people: sub-directory is the member's organization.
        if info.dir == Dir::People && info.segments.len() >= 3 && kind.ty == EntityType::Person {
            let org = EntityRef::new(EntityType::Organization, info.segments[1].clone());
            for m in ctx.mains.clone() {
                ctx.fact(
                    &m,
                    Predicate::MemberOf,
                    Some(org.clone()),
                    None,
                    None,
                    None,
                    10000,
                    "dir.people",
                    &anchor_ev,
                );
            }
        }
        // dir.projects: the file is a Document about the Project.
        if info.dir == Dir::Projects && kind.ty == EntityType::Project {
            for m in ctx.mains.clone() {
                let d = ctx.doc.clone();
                ctx.fact(
                    &d,
                    Predicate::About,
                    Some(m),
                    None,
                    None,
                    None,
                    10000,
                    "dir.projects",
                    &anchor_ev,
                );
            }
        }
    } else {
        // No convention: the Document is the only subject.
        ctx.mains.push(doc.clone());
    }

    if is_text {
        // fm.fields
        for (f, values) in &fm_fields {
            ctx.apply_field(*f, values, &fm_ev, 9800, "fm.fields");
        }
        let sections = headings::sections(blocks);
        let is_decision_file = ctx
            .mains
            .first()
            .is_some_and(|m| m.ty == EntityType::Decision);
        for (i, b) in blocks.iter().enumerate() {
            if matches!(
                b.kind,
                BlockKind::Frontmatter | BlockKind::Heading | BlockKind::Code
            ) {
                continue;
            }
            let ev = EvidenceRef::from_block(b);
            // md.kv and the dated ownership sentence.
            for line in b.text.split('\n') {
                if let Some((key, value)) = kv::split_kv(line)
                    && let Some(f) = kv::field(&key)
                    && !matches!(f, Field::Type | Field::Date | Field::Tags)
                {
                    ctx.apply_field(f, &[value], &ev, 9000, "md.kv");
                }
                if let Some(s) = kv::owner_sentence(line) {
                    let person = EntityRef::new(EntityType::Person, s.name.clone());
                    let is_project =
                        matches!(ctx.mains.first().map(|m| m.ty), Some(EntityType::Project));
                    if is_project {
                        let m = ctx.mains[0].clone();
                        ctx.fact(
                            &person,
                            Predicate::Owns,
                            Some(m),
                            None,
                            Some(&s.from),
                            s.to.as_ref(),
                            9000,
                            "md.kv",
                            &ev,
                        );
                    }
                }
            }
            // md.headings: items under decision / tried / rejected sections.
            if b.kind == BlockKind::ListItem {
                let name = kv::clean_item(&b.text, MAX_NAME_CHARS);
                match (sections[i], name.is_empty()) {
                    (_, true) | (None, _) => {}
                    (Some(Section::Decisions), false) => {
                        let d = EntityRef::new(EntityType::Decision, name);
                        ctx.col.entity(&d, Some(&ev));
                        let owners = fm_value(Field::Owner)
                            .or_else(|| fm_value(Field::DecidedBy))
                            .cloned()
                            .unwrap_or_default();
                        for part in owners.iter().flat_map(|v| kv::split_names(v)) {
                            let person = EntityRef::new(EntityType::Person, part.name);
                            ctx.fact(
                                &person,
                                Predicate::Decides,
                                Some(d.clone()),
                                None,
                                part.from.as_ref(),
                                None,
                                8500,
                                "md.headings",
                                &ev,
                            );
                        }
                    }
                    (Some(section), false) => {
                        let pred = if section == Section::Tried {
                            Predicate::TriedAndFailed
                        } else {
                            Predicate::Rejects
                        };
                        if let Some(m) = ctx
                            .mains
                            .first()
                            .filter(|m| m.ty != EntityType::Document)
                            .cloned()
                        {
                            let term = EntityRef::new(EntityType::Term, name);
                            ctx.fact(
                                &m,
                                pred,
                                Some(term),
                                None,
                                None,
                                None,
                                8500,
                                "md.headings",
                                &ev,
                            );
                        }
                    }
                }
            }
            // Replacement sentences in a decision file.
            if is_decision_file && let Some(title) = headings::replaced_title(&b.text) {
                let old = EntityRef::new(EntityType::Decision, dirs::strip_decision_prefix(&title));
                for m in ctx.mains.clone() {
                    if m.ty == EntityType::Decision && m.norm() != old.norm() {
                        ctx.fact(
                            &m,
                            Predicate::Supersedes,
                            Some(old.clone()),
                            None,
                            None,
                            None,
                            8500,
                            "md.headings",
                            &ev,
                        );
                    }
                }
            }
        }
    }

    // doc.entity: the Document mentions every other entity found in the file.
    let found: Vec<(EntityRef, EvidenceRef)> = ctx
        .col
        .entities
        .iter()
        .filter(|(e, _)| e.ty != EntityType::Document)
        .filter_map(|(e, evs)| evs.first().map(|ev| (e.clone(), ev.clone())))
        .collect();
    for (e, ev) in found {
        let d = ctx.doc.clone();
        ctx.fact(
            &d,
            Predicate::Mentions,
            Some(e),
            None,
            None,
            None,
            10000,
            "doc.entity",
            &ev,
        );
    }
    ctx.col.finish(&path)
}
