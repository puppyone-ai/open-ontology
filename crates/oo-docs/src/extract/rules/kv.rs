//! Field names shared by `fm.fields` and `md.kv`, and the text helpers that
//! pull names and dates out of one value (10 §5.1). Pure.

use unicode_normalization::UnicodeNormalization;

use super::dates::{ParsedDate, parse_exact, scan};

/// Fields the rules understand, in frontmatter or as `key: value` lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Type,
    Owner,
    Members,
    Project,
    Status,
    Date,
    DecidedBy,
    Supersedes,
    Aliases,
    Rejected,
    Tags,
}

fn key_norm(key: &str) -> String {
    key.nfkc().collect::<String>().trim().to_lowercase()
}

/// Map a (frontmatter or line) key to a [`Field`].
pub fn field(key: &str) -> Option<Field> {
    Some(match key_norm(key).as_str() {
        "type" | "类型" => Field::Type,
        "owner" | "负责人" | "责任人" => Field::Owner,
        "members" | "member" | "成员" | "核心成员" | "团队成员" => Field::Members,
        "project" | "项目" => Field::Project,
        "status" | "状态" => Field::Status,
        "date" | "日期" => Field::Date,
        "decided_by" | "决策人" | "决定人" => Field::DecidedBy,
        "supersedes" | "取代" => Field::Supersedes,
        "aliases" | "alias" | "别名" => Field::Aliases,
        "被否决方案" | "被否决" | "否决方案" | "rejected" => Field::Rejected,
        "tags" | "tag" | "标签" => Field::Tags,
        _ => return None,
    })
}

/// Split one `key: value` line (`**负责人**：张三`, `Owner: 张三`). The key may
/// be wrapped in `**`/`__`, is at most 24 characters and ends at the first
/// ASCII or full-width colon.
pub fn split_kv(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    let line = line
        .trim_start_matches(['-', '*', '+', '>', ' '])
        .trim_start();
    let idx = line.find([':', '：'])?;
    let key = line[..idx]
        .trim()
        .trim_matches(['*', '_'])
        .trim()
        .to_owned();
    if key.is_empty() || key.chars().count() > 24 {
        return None;
    }
    let colon_len = line[idx..].chars().next()?.len_utf8();
    let mut value = line[idx + colon_len..].trim();
    value = value.trim_start_matches(['*', '_']).trim();
    Some((key, value.to_owned()))
}

/// One person-like name with an optional date from a parenthesis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamePart {
    pub name: String,
    pub from: Option<ParsedDate>,
}

/// Remove parenthesised notes; the first date inside one is returned.
fn strip_parens(segment: &str) -> (String, Option<ParsedDate>) {
    let mut out = String::new();
    let mut note = String::new();
    let mut depth = 0u32;
    let mut date = None;
    for c in segment.chars() {
        match c {
            '（' | '(' => depth += 1,
            '）' | ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if date.is_none() {
                        date = scan(&note, true).into_iter().next().map(|(d, _, _)| d);
                    }
                    note.clear();
                }
            }
            _ if depth > 0 => note.push(c),
            _ => out.push(c),
        }
    }
    (out, date)
}

/// Names from the value of a person field. Segments are separated by `；;` or
/// a line break. In a segment with `负责` only the text before it is read
/// (`吴启明负责数据库迁移，上线采用灰度发布` is one person, the clause after the
/// comma is not a name); otherwise `、，,/` separate names. A date inside a
/// parenthesis (`孙晓棠（2026-07-01 起）`) is attached when the segment holds a
/// single name. Pieces longer than 40 characters or without a letter or
/// ideograph are dropped.
pub fn split_names(value: &str) -> Vec<NamePart> {
    let mut out = Vec::new();
    for segment in value.split(['；', ';', '\n']) {
        let (clean, date) = strip_parens(segment);
        let mut text = clean.trim();
        if let Some(cut) = text.find("负责") {
            text = text[..cut].trim();
        }
        let names: Vec<&str> = text
            .split(['、', '，', ',', '/'])
            .map(|p| {
                p.trim()
                    .trim_end_matches(['。', '.'])
                    .trim_end_matches('等')
                    .trim()
            })
            .filter(|p| {
                !p.is_empty() && p.chars().count() <= 40 && p.chars().any(char::is_alphanumeric)
            })
            .collect();
        let single = names.len() == 1;
        for name in names {
            out.push(NamePart {
                name: name.to_owned(),
                from: if single { date.clone() } else { None },
            });
        }
    }
    out
}

/// `2026-01-01 至 2026-05-31 由陈子墨负责` and `2026-06-01 起由周若溪负责`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerSentence {
    pub name: String,
    pub from: ParsedDate,
    pub to: Option<ParsedDate>,
}

/// Recognise a dated ownership sentence. At least one date is required; the
/// first is `valid_from`, a second one (a range) is `valid_to`.
pub fn owner_sentence(line: &str) -> Option<OwnerSentence> {
    let by = line.find('由')?;
    let after = &line[by + '由'.len_utf8()..];
    let end = after.find("负责")?;
    let name = after[..end].trim();
    if name.is_empty()
        || name.chars().count() > 40
        || name.contains(['，', '。', ',', '；', ';', '：', ':'])
    {
        return None;
    }
    let dates: Vec<ParsedDate> = scan(&line[..by], false)
        .into_iter()
        .map(|(d, _, _)| d)
        .collect();
    let mut it = dates.into_iter();
    let from = it.next()?;
    Some(OwnerSentence {
        name: name.to_owned(),
        from,
        to: it.next(),
    })
}

/// Text before the first clause break, used as the name of a list item or of
/// a rejected alternative (`自研图表引擎，大数据量下渲染卡顿` → `自研图表引擎`).
pub fn clean_item(text: &str, max_chars: usize) -> String {
    let t = text.trim().trim_start_matches(['-', '*', '+', ' ']).trim();
    let cut = t
        .find(['：', ':', '，', ',', '。', '；', ';', '（', '('])
        .unwrap_or(t.len());
    let head = t[..cut].trim().trim_matches(['*', '_', '`']).trim();
    head.chars().take(max_chars).collect()
}

/// Date of a frontmatter value or a line value, if the whole value is one.
pub fn date_value(value: &str) -> Option<ParsedDate> {
    parse_exact(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_fields() {
        assert_eq!(field("负责人"), Some(Field::Owner));
        assert_eq!(field(" Owner "), Some(Field::Owner));
        assert_eq!(field("ＯＷＮＥＲ"), Some(Field::Owner));
        assert_eq!(field("被否决方案"), Some(Field::Rejected));
        assert_eq!(field("核心成员"), Some(Field::Members));
        assert_eq!(field("whatever"), None);
    }

    #[test]
    fn kv_lines() {
        assert_eq!(
            split_kv("负责人：孙晓棠（2026-07-01 起）。"),
            Some(("负责人".into(), "孙晓棠（2026-07-01 起）。".into()))
        );
        assert_eq!(
            split_kv("**Owner**: Evan Lin"),
            Some(("Owner".into(), "Evan Lin".into()))
        );
        assert_eq!(
            split_kv("- 状态: active"),
            Some(("状态".into(), "active".into()))
        );
        assert_eq!(split_kv("没有冒号的句子"), None);
        assert_eq!(split_kv(&format!("{}：x", "长".repeat(30))), None);
    }

    #[test]
    fn names_and_dates() {
        let v = split_names("孙晓棠（2026-07-01 起）");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "孙晓棠");
        assert_eq!(v[0].from.as_ref().unwrap().iso, "2026-07-01");
        let v = split_names("贺文博负责前端；韩立负责客户案例素材。");
        assert_eq!(
            v.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["贺文博", "韩立"]
        );
        let v = split_names("吴启明负责数据库迁移，上线采用灰度发布。");
        assert_eq!(
            v.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["吴启明"]
        );
        let v = split_names("吴启明、郑雨桐");
        assert_eq!(v.len(), 2);
        let v = split_names("甲（2026-01-01 起，兼职），乙");
        assert_eq!(
            v.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["甲", "乙"]
        );
        assert!(
            v.iter().all(|n| n.from.is_none()),
            "ambiguous date is dropped"
        );
        assert!(split_names("、，；").is_empty());
        assert!(split_names(&"x".repeat(41)).is_empty());
    }

    #[test]
    fn owner_sentences() {
        let s = owner_sentence("2026-01-01 至 2026-05-31 由陈子墨负责。").unwrap();
        assert_eq!(s.name, "陈子墨");
        assert_eq!(s.from.iso, "2026-01-01");
        assert_eq!(s.to.unwrap().iso, "2026-05-31");
        let s = owner_sentence("2026-06-01 起由周若溪负责。").unwrap();
        assert_eq!(s.name, "周若溪");
        assert!(s.to.is_none());
        assert_eq!(owner_sentence("由周若溪负责"), None, "date required");
        assert_eq!(owner_sentence("2026-06-01 起由周若溪，李四负责"), None);
    }

    #[test]
    fn items() {
        assert_eq!(
            clean_item("自研图表引擎，大数据量下渲染卡顿", 80),
            "自研图表引擎"
        );
        assert_eq!(clean_item("- **采用 PG**（原因见上）", 80), "采用 PG");
        assert_eq!(clean_item("abcdef", 3), "abc");
    }
}
