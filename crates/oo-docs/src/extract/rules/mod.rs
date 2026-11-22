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
pub mod media;
pub mod media_class;
pub mod tables;
pub mod text;

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
use crate::profile::{Class, Profile};

/// Version tag stored with every rule fact.
pub const RULESET_VERSION: &str = "rules/2";

const MAX_NAME_CHARS: usize = 120;

struct Ctx<'a> {
    col: Collector,
    doc: EntityRef,
    mains: Vec<EntityRef>,
    default_from: Option<(String, &'static str)>,
    /// First date written in the block being read (`text.date`), the most
    /// specific `valid_from` there is.
    block_date: Option<ParsedDate>,
    /// The file is a csv (rule ids `csv.*`).
    csv: bool,
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
        let from = from.or(self.block_date.as_ref());
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
            Field::Email | Field::Github | Field::EmployeeId => {
                let kind = match f {
                    Field::Email => "email",
                    Field::Github => "github",
                    _ => "employee_id",
                };
                // Identity of the one Person the file stands for.
                if let [m] = mains.as_slice()
                    && m.ty == EntityType::Person
                    && let Some(v) = values.first()
                {
                    let v = v.trim().trim_matches(['<', '>']);
                    let ok = match kind {
                        "email" => text::is_email(v),
                        _ => {
                            !v.is_empty()
                                && v.chars().count() <= 64
                                && !v.contains(char::is_whitespace)
                        }
                    };
                    if ok {
                        self.col.identity(m, kind, v, conf, rule, ev);
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

/// Other files of the same Drive, as far as `name.version` needs them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Siblings<'a> {
    /// Drive-relative NFC paths of live files (any directory).
    pub paths: &'a [String],
    /// The profile stored on the file (the user's override and the model's say are in it); `None`: classify here.
    pub profile: Option<Profile>,
    /// Paths of the project marker files of the Drive (manifests, AGENTS.md, CODEOWNERS), for the project of a file.
    pub markers: &'a [String],
    /// What the Drive already decided about a recording (a model's or a person's verdict stored for its blob);
    /// `None` leaves it to the rules' own classification.
    pub media: Option<media_class::MediaClass>,
}

/// The default `valid_from` of the facts of one file, with its precision:
/// the frontmatter date, else a date in the file name, else the document's
/// creation date. `stem` is the file name without extension, `fm_date` the
/// frontmatter `date` value.
pub fn default_valid_from(
    stem: &str,
    ir: &DocIr,
    fm_date: Option<&String>,
) -> Option<(String, &'static str)> {
    fm_date
        .and_then(|v| kv::date_value(v))
        .or_else(|| {
            dates::scan(stem, false)
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
        })
}

/// [`default_valid_from`] of a file as the rules see it, for facts made later
/// from the same file (`doc.link`).
pub fn file_default_valid_from(path: &str, ir: &DocIr) -> Option<(String, &'static str)> {
    let path = nfc(path);
    let info = dirs::path_info(&path);
    let is_text = matches!(ir.format, DocFormat::Md | DocFormat::Txt);
    let fm_map = ir.meta.frontmatter.clone().unwrap_or_default();
    let fm_fields = if is_text {
        frontmatter::fields(&fm_map)
    } else {
        Vec::new()
    };
    let fm_date = fm_fields
        .iter()
        .find(|(k, _)| *k == Field::Date)
        .and_then(|(_, v)| v.first());
    default_valid_from(&info.stem, ir, fm_date)
}

/// Run `rules/1` on one parsed file. `path` is the drive-relative path.
pub fn extract(path: &str, ir: &DocIr) -> Extraction {
    extract_in(path, ir, &Siblings::default())
}

/// [`extract`] with the sibling files, which `name.version` needs to find an
/// older version of the same document.
pub fn extract_in(path: &str, ir: &DocIr, siblings: &Siblings<'_>) -> Extraction {
    extract_with_quality(path, ir, siblings).0
}

/// [`extract_in`] with what the entity quality gate (D90) dropped, for the diagnostics of the file.
pub fn extract_with_quality(
    path: &str,
    ir: &DocIr,
    siblings: &Siblings<'_>,
) -> (Extraction, super::quality::QualityStats) {
    super::quality::gate(extract_ungated(path, ir, siblings), ir, &[])
}

/// What the rules find before the quality gate.
pub fn extract_ungated(path: &str, ir: &DocIr, siblings: &Siblings<'_>) -> Extraction {
    let path = nfc(path);
    let info: PathInfo = dirs::path_info(&path);
    // D98: a recording is classified first. Meetings, interviews, lectures and voice memos are transcribed and their statements
    // extracted; a promo, an edited video, a film or music is a media asset: one typed object with title, duration and class and
    // nothing read from its sound track.
    let media_class = matches!(ir.format, DocFormat::Audio | DocFormat::Video).then(|| {
        siblings
            .media
            .unwrap_or_else(|| media_class::classify(ir, &path).class)
    });
    let asset_ir;
    let ir = if media_class.is_some_and(|c| !c.identifies_speakers()) {
        let mut shadow = ir.clone();
        shadow.blocks.truncate(1);
        for b in &mut shadow.blocks {
            b.text.clear();
        }
        asset_ir = shadow;
        &asset_ir
    } else {
        ir
    };
    let blocks = &ir.blocks;
    // D89: the profile decides what is read. Code and unknown text have no prose to read.
    let class = match siblings.profile {
        Some(profile) => crate::profile::Classified {
            profile,
            reason: "stored",
            decided: true,
        },
        None => crate::profile::classify(&path, ir),
    };
    // D94: only records (and media) are read for entities by the core rules; instruction, reference, code and other files are not.
    let structured = !matches!(class.profile.class(), Class::Record | Class::Media);
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
    // md.kv and md.headings read paragraphs and lists of every prose format.
    let is_prose = !matches!(ir.format, DocFormat::Csv | DocFormat::Xlsx) && !structured;
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
    let default_from = default_valid_from(
        &info.stem,
        ir,
        fm_value(Field::Date).and_then(|v| v.first()),
    );

    let year: Option<u32> = default_from
        .as_ref()
        .and_then(|(iso, _)| iso.get(..4))
        .and_then(|y| y.parse().ok());
    let doc = EntityRef::new(EntityType::Document, path.clone());
    let mut ctx = Ctx {
        col: Collector::default(),
        doc: doc.clone(),
        mains: Vec::new(),
        default_from,
        block_date: None,
        csv: ir.format == DocFormat::Csv,
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
    let kind: Option<Kind> = if structured {
        None
    } else if is_text || info.dir != Dir::None {
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
                let name = kv::preference_name(&b.text, MAX_NAME_CHARS);
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

    if is_prose {
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
            ctx.block_date = block_date(&b.text, year);
            // md.kv and the dated ownership sentence.
            // A row of a table (docx, pptx, md) describes the item in that row, a
            // task or a person, never the document that holds the table.
            let table_row = b.kind == BlockKind::Kv && b.parent.is_some();
            for line in b.text.split('\n') {
                if table_row {
                    break;
                }
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
                // A date in "本决定取代 2026年1月10日 的决定" belongs to the replaced
                // decision; the replacement starts on this decision's own date.
                let block_date = ctx.block_date.take();
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
                ctx.block_date = block_date;
            }
        }
    }

    ctx.block_date = None;

    // The profile extractors (instruction, code, paper): `Thing` candidates registered by the profile.
    crate::profile::apply(&class, &path, ir, siblings.markers, &mut ctx.col);

    // dir.terms: definition lists and two column tables.
    if info.dir == Dir::Terms {
        terms(&mut ctx, blocks);
    }
    // xlsx.* / csv.* / table.header_map.
    if matches!(ir.format, DocFormat::Csv | DocFormat::Xlsx) {
        tables::apply(&mut ctx, blocks);
    }
    // doc.meta, pptx.title, name.version, text.email.
    if !structured {
        doc_meta(&mut ctx, ir, &anchor_ev);
    }
    if ir.format == DocFormat::Pptx {
        pptx_title(&mut ctx, blocks);
    }
    if !structured {
        name_version(&mut ctx, &info, &path, siblings, &anchor_ev);
    }
    if !structured {
        text_email(&mut ctx, blocks);
    }
    // What speakers say about what they own (the subject is a provisional Person until the speaker is confirmed).
    if let Some(class) = media_class {
        media::apply(&mut ctx, ir, class);
        media::typed_objects(&mut ctx, ir, class, &path);
    }

    // doc.entity: the Document mentions every other entity found in the file. A
    // Decision is a record the document states or defines, not a name it mentions.
    let found: Vec<(EntityRef, EvidenceRef)> = ctx
        .col
        .entities
        .iter()
        .filter(|(e, _)| {
            !matches!(e.ty, EntityType::Document | EntityType::Decision)
                && !media::is_placeholder(&e.name)
        })
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

/// `text.date`: the first date written in `text`; `M月D日` takes its year from
/// the document date.
fn block_date(text: &str, year: Option<u32>) -> Option<ParsedDate> {
    let full = dates::scan(text, false).into_iter().next();
    let short = year.and_then(|y| dates::scan_month_day(text, y).into_iter().next());
    match (full, short) {
        (Some(f), Some(sh)) => Some(if f.1 <= sh.1 { f.0 } else { sh.0 }),
        (Some(f), None) => Some(f.0),
        (None, Some(sh)) => Some(sh.0),
        (None, None) => None,
    }
}

/// `dir.terms`: `term：definition` lines and two column tables.
fn terms(ctx: &mut Ctx<'_>, blocks: &[oo_core::docir::Block]) {
    let put = |ctx: &mut Ctx<'_>, term: &str, def: &str, ev: &EvidenceRef| {
        let term = clip(term, MAX_NAME_CHARS);
        let def = clip(def, 500);
        if term.is_empty() || def.is_empty() {
            return;
        }
        let t = EntityRef::new(EntityType::Term, term);
        ctx.fact(
            &t,
            Predicate::Attr("definition".into()),
            None,
            Some(json!({"text": def})),
            None,
            None,
            9500,
            "dir.terms",
            ev,
        );
    };
    for b in blocks {
        let ev = EvidenceRef::from_block(b);
        match b.kind {
            BlockKind::ListItem | BlockKind::Paragraph => {
                if let Some((t, d)) = text::definition_line(&b.text) {
                    put(ctx, &t, &d, &ev);
                }
            }
            BlockKind::Table | BlockKind::SheetTable => {
                let Some(g) = &b.table else { continue };
                if g.cols != 2 {
                    continue;
                }
                for r in g.header_rows..g.rows {
                    let cell = |c: u32| {
                        g.cells
                            .iter()
                            .find(|x| x.r == r && x.c == c)
                            .map(|x| x.text.trim().to_owned())
                            .unwrap_or_default()
                    };
                    put(ctx, &cell(0), &cell(1), &ev);
                }
            }
            _ => {}
        }
    }
}

/// `doc.meta`: authors and creation date of the Document.
fn doc_meta(ctx: &mut Ctx<'_>, ir: &DocIr, ev: &EvidenceRef) {
    let doc = ctx.doc.clone();
    for author in &ir.meta.authors {
        let a = clip(author, MAX_NAME_CHARS);
        if text::generic_author(&a) {
            continue;
        }
        ctx.fact(
            &doc,
            Predicate::Attr("author".into()),
            None,
            Some(json!({"text": a})),
            None,
            None,
            9000,
            "doc.meta",
            ev,
        );
        ctx.col.hint(
            &doc,
            Predicate::Mentions,
            EntityType::Person,
            &a,
            9000,
            "doc.meta",
            ev,
        );
    }
    if let Some(created) = ir
        .meta
        .created
        .as_deref()
        .and_then(|c| dates::parse_exact(c.get(..10).unwrap_or(c)))
    {
        ctx.fact(
            &doc,
            Predicate::Attr("created".into()),
            None,
            Some(json!({"date": created.iso})),
            None,
            None,
            9000,
            "doc.meta",
            ev,
        );
    }
}

/// Words a deck or report title may end with before the project name.
const TITLE_TAILS: &[&str] = &[
    "周报",
    "月报",
    "日报",
    "汇报",
    "复盘",
    "进展",
    "方案",
    "评审",
    "纪要",
    "同步",
    "汇总",
    "总结",
    "启动会",
    "周会",
];

/// `pptx.title`: the title of the first slide names the Document and hints at
/// a Project (resolve binds it only to an existing one).
fn pptx_title(ctx: &mut Ctx<'_>, blocks: &[oo_core::docir::Block]) {
    let Some(b) = blocks
        .iter()
        .find(|b| b.kind == BlockKind::SlideTitle && !b.text.trim().is_empty())
    else {
        return;
    };
    let ev = EvidenceRef::from_block(b);
    let title = clip(&b.text.replace('\n', " "), 256);
    let doc = ctx.doc.clone();
    ctx.col.alias(&doc, &title, 7000, "pptx.title", &ev);
    let mut candidates = vec![title.clone()];
    let mut t = title.as_str();
    loop {
        let before = t;
        for tail in TITLE_TAILS {
            if let Some(x) = t.trim().strip_suffix(tail) {
                t = x;
            }
        }
        t = t.trim_end_matches(|c: char| {
            c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' '
        });
        if t == before {
            break;
        }
    }
    let t = t.trim();
    if !t.is_empty() && t != title {
        candidates.push(t.to_owned());
    }
    for c in candidates {
        ctx.col.hint(
            &doc,
            Predicate::About,
            EntityType::Project,
            &c,
            7000,
            "pptx.title",
            &ev,
        );
    }
}

/// `name.version`: an older sibling with the same trunk is proposed as
/// superseded (never published without review).
fn name_version(
    ctx: &mut Ctx<'_>,
    info: &PathInfo,
    path: &str,
    siblings: &Siblings<'_>,
    ev: &EvidenceRef,
) {
    let Some(mine) = text::version_mark(&info.stem) else {
        return;
    };
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    let mut best: Option<(u32, String)> = None;
    for other in siblings.paths {
        if other == path || other.rsplit_once('/').map_or("", |(d, _)| d) != dir {
            continue;
        }
        let other_info = dirs::path_info(other);
        let rank = match text::version_mark(&other_info.stem) {
            Some(m) if m.trunk == mine.trunk => m.rank,
            Some(_) => continue,
            None if crate::resolve::normalize::norm(&other_info.stem) == mine.trunk => 0,
            None => continue,
        };
        if rank < mine.rank && best.as_ref().is_none_or(|(r, p)| (rank, other) > (*r, p)) {
            best = Some((rank, other.clone()));
        }
    }
    if let Some((_, old)) = best {
        let doc = ctx.doc.clone();
        let old = EntityRef::new(EntityType::Document, old);
        ctx.fact(
            &doc,
            Predicate::Supersedes,
            Some(old),
            None,
            None,
            None,
            7000,
            "name.version",
            ev,
        );
    }
}

/// `text.email`: one email and one known Person in the same block.
fn text_email(ctx: &mut Ctx<'_>, blocks: &[oo_core::docir::Block]) {
    let persons: Vec<EntityRef> = ctx
        .col
        .entities
        .keys()
        .filter(|e| e.ty == EntityType::Person)
        .cloned()
        .collect();
    for b in blocks {
        if matches!(b.kind, BlockKind::Frontmatter | BlockKind::Code) {
            continue;
        }
        let emails = text::emails(&b.text);
        let [email] = emails.as_slice() else { continue };
        let named: Vec<&EntityRef> = persons
            .iter()
            .filter(|p| b.text.contains(p.name.as_str()))
            .collect();
        if let [person] = named.as_slice() {
            let ev = EvidenceRef::from_block(b);
            ctx.col
                .identity(person, "email", email, 9000, "text.email", &ev);
        }
    }
}
