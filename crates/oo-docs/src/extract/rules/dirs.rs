//! `dir.*` and `fm.type`: which entity a file stands for (10 §5.1). Pure.

use unicode_normalization::UnicodeNormalization;

use crate::extract::model::EntityType;

/// Directory convention matched by the first path segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    People,
    Projects,
    Decisions,
    Preferences,
    Terms,
    None,
}

const PEOPLE: &[&str] = &["people", "人员", "成员", "团队"];
const PROJECTS: &[&str] = &["projects", "项目"];
const DECISIONS: &[&str] = &["decisions", "决策", "adr"];
const PREFERENCES: &[&str] = &["preferences", "偏好"];
const TERMS: &[&str] = &["glossary", "术语"];

/// Path pieces the rules work from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathInfo {
    pub segments: Vec<String>,
    /// File name without its last extension.
    pub stem: String,
    pub dir: Dir,
}

/// Split a drive-relative path (NFC) and classify its first segment
/// (case-insensitive).
pub fn path_info(path: &str) -> PathInfo {
    let segments: Vec<String> = path.split('/').map(str::to_owned).collect();
    let file = segments.last().cloned().unwrap_or_default();
    let stem = match file.rfind('.') {
        Some(i) if i > 0 => file[..i].to_owned(),
        _ => file,
    };
    let dir = if segments.len() >= 2 {
        let first: String = segments[0].nfkc().collect::<String>().to_lowercase();
        if PEOPLE.contains(&first.as_str()) {
            Dir::People
        } else if PROJECTS.contains(&first.as_str()) {
            Dir::Projects
        } else if DECISIONS.contains(&first.as_str()) {
            Dir::Decisions
        } else if PREFERENCES.contains(&first.as_str()) {
            Dir::Preferences
        } else if TERMS.contains(&first.as_str()) {
            Dir::Terms
        } else {
            Dir::None
        }
    } else {
        Dir::None
    };
    PathInfo {
        segments,
        stem,
        dir,
    }
}

/// `ADR-<digits>` and `决策-` file-name prefixes mark a decision anywhere.
pub fn decision_file_name(stem: &str) -> bool {
    let s: String = stem.nfkc().collect::<String>().to_lowercase();
    if let Some(rest) = s.strip_prefix("adr-") {
        return rest.bytes().next().is_some_and(|b| b.is_ascii_digit());
    }
    s.starts_with("决策-")
}

/// What a file stands for, and which rule says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub ty: EntityType,
    pub subkind: Option<&'static str>,
    pub rule: &'static str,
    pub conf: u16,
}

/// `fm.type` values (English and Chinese) that name a core type. Anything else
/// (`memo`, `report`) is an open type and is ignored by `rules/1`.
pub fn fm_type(value: &str) -> Option<(EntityType, Option<&'static str>)> {
    let v: String = value.nfkc().collect::<String>().trim().to_lowercase();
    Some(match v.as_str() {
        "person" | "人物" | "人员" | "成员" => (EntityType::Person, None),
        "organization" | "organisation" | "org" | "组织" | "公司" | "部门" => {
            (EntityType::Organization, None)
        }
        "project" | "项目" => (EntityType::Project, None),
        "decision" | "决策" | "决定" | "adr" => (EntityType::Decision, None),
        "preference" | "偏好" => (EntityType::Decision, Some("preference")),
        "topic" | "主题" | "话题" => (EntityType::Topic, None),
        "event" | "事件" | "活动" | "会议" => (EntityType::Event, None),
        "term" | "术语" | "名词" => (EntityType::Term, None),
        _ => return None,
    })
}

/// Decide the file's kind: `fm.type` wins, then the directory convention, then
/// the decision file-name prefixes. `None` means the file only stands for its
/// `Document`.
pub fn classify(info: &PathInfo, fm_type_value: Option<&str>) -> Option<Kind> {
    if let Some((ty, subkind)) = fm_type_value.and_then(fm_type) {
        return Some(Kind {
            ty,
            subkind,
            rule: "fm.type",
            conf: 10000,
        });
    }
    let kind = |ty, subkind, rule, conf| {
        Some(Kind {
            ty,
            subkind,
            rule,
            conf,
        })
    };
    match info.dir {
        Dir::People => kind(EntityType::Person, None, "dir.people", 10000),
        Dir::Projects => kind(EntityType::Project, None, "dir.projects", 10000),
        Dir::Decisions => kind(EntityType::Decision, None, "dir.decisions", 10000),
        Dir::Preferences => kind(
            EntityType::Decision,
            Some("preference"),
            "dir.preferences",
            9500,
        ),
        Dir::Terms => None,
        Dir::None if decision_file_name(&info.stem) => {
            kind(EntityType::Decision, None, "dir.decisions", 10000)
        }
        Dir::None => None,
    }
}

/// Remove the `决定：` / `决策：` / `Decision:` / `ADR-12:` lead-in of a
/// decision title; the original is returned when nothing would be left.
pub fn strip_decision_prefix(title: &str) -> String {
    let t = title.trim();
    let lower: String = t.nfkc().collect::<String>().to_lowercase();
    let mut rest = t;
    let mut stripped = false;
    for prefix in ["决定", "决策", "decision"] {
        if lower.starts_with(prefix) {
            let skip: usize = t
                .chars()
                .take(prefix.chars().count())
                .map(char::len_utf8)
                .sum();
            rest = &t[skip..];
            stripped = true;
            break;
        }
    }
    if !stripped && t.len() >= 4 && t.as_bytes()[..4].eq_ignore_ascii_case(b"adr-") {
        let after = &t[4..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        rest = &after[digits..];
    }
    let rest = rest.trim_start_matches([':', '：', '-', '—', ' ', '\u{3000}']);
    let rest = rest.trim();
    if rest.is_empty() {
        t.to_owned()
    } else {
        rest.to_owned()
    }
}

/// Entity name of the file's main entity. People use the file name, projects
/// the first sub-directory (or the file name), decisions the title.
pub fn main_name(kind: &Kind, info: &PathInfo, title: Option<&str>) -> String {
    let title_or_stem = || {
        title
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(&info.stem)
            .to_owned()
    };
    match (kind.ty, info.dir) {
        (EntityType::Person, Dir::People) => info.stem.clone(),
        (EntityType::Project, Dir::Projects) => {
            if info.segments.len() >= 3 {
                info.segments[1].clone()
            } else {
                info.stem.clone()
            }
        }
        (EntityType::Decision, _) => strip_decision_prefix(&title_or_stem()),
        _ => title_or_stem(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_are_case_insensitive_and_need_a_file() {
        assert_eq!(path_info("People/陈子墨.md").dir, Dir::People);
        assert_eq!(path_info("people/x.md").dir, Dir::People);
        assert_eq!(path_info("团队/a/b.md").dir, Dir::People);
        assert_eq!(path_info("PROJECTS/x.md").dir, Dir::Projects);
        assert_eq!(path_info("ADR/0001.md").dir, Dir::Decisions);
        assert_eq!(path_info("偏好/a.md").dir, Dir::Preferences);
        assert_eq!(path_info("People").dir, Dir::None, "a bare name is a file");
        assert_eq!(path_info("Notes/a.md").dir, Dir::None);
        let i = path_info("Projects/星图/规格.v2.md");
        assert_eq!(i.stem, "规格.v2");
        assert_eq!(i.segments.len(), 3);
    }

    #[test]
    fn decision_file_names() {
        assert!(decision_file_name("ADR-12-use-pg"));
        assert!(decision_file_name("adr-3"));
        assert!(decision_file_name("决策-周报"));
        assert!(!decision_file_name("ADR-xx"));
        assert!(!decision_file_name("adr"));
        assert!(!decision_file_name("D-2026-01"));
    }

    #[test]
    fn fm_type_wins_over_directory() {
        let i = path_info("People/x.md");
        assert_eq!(
            classify(&i, Some("project")).unwrap().ty,
            EntityType::Project
        );
        assert_eq!(classify(&i, Some("memo")).unwrap().ty, EntityType::Person);
        assert_eq!(classify(&i, None).unwrap().rule, "dir.people");
        let k = classify(&path_info("a.md"), Some("preference")).unwrap();
        assert_eq!(
            (k.ty, k.subkind),
            (EntityType::Decision, Some("preference"))
        );
        assert_eq!(classify(&path_info("a.md"), None), None);
        assert_eq!(
            classify(&path_info("misc/ADR-7.md"), None).unwrap().rule,
            "dir.decisions"
        );
    }

    #[test]
    fn decision_titles() {
        assert_eq!(
            strip_decision_prefix("决定：周报改为每周四发送"),
            "周报改为每周四发送"
        );
        assert_eq!(strip_decision_prefix("Decision: Use PG"), "Use PG");
        assert_eq!(strip_decision_prefix("ADR-12: Use PG"), "Use PG");
        assert_eq!(strip_decision_prefix("决定"), "决定");
        assert_eq!(strip_decision_prefix("周报"), "周报");
    }

    #[test]
    fn names_follow_the_rules() {
        let p = path_info("People/林远舟.md");
        let k = classify(&p, None).unwrap();
        assert_eq!(main_name(&k, &p, Some("别的标题")), "林远舟");
        let p = path_info("Projects/星图/规格.md");
        let k = classify(&p, None).unwrap();
        assert_eq!(main_name(&k, &p, None), "星图");
        let p = path_info("Projects/灯塔迁移.md");
        let k = classify(&p, None).unwrap();
        assert_eq!(main_name(&k, &p, None), "灯塔迁移");
        let p = path_info("Decisions/D-2026-04-x.md");
        let k = classify(&p, None).unwrap();
        assert_eq!(
            main_name(&k, &p, Some("决定：周报改为每周四发送")),
            "周报改为每周四发送"
        );
        assert_eq!(main_name(&k, &p, None), "D-2026-04-x");
    }
}
