//! `text.email`, `doc.meta` helpers, `name.version` and `dir.terms` text
//! parsing (10 §5.1). Pure.

use unicode_normalization::UnicodeNormalization;

fn local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// Whether `s` is one plain email address (`local@domain.tld`).
pub fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && local.chars().all(local_char)
        && !local.starts_with('.')
        && !local.ends_with('.')
        && domain_ok(domain)
}

fn domain_ok(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    labels.len() >= 2
        && domain.len() <= 253
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        })
        && labels
            .last()
            .is_some_and(|t| t.len() >= 2 && t.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Email addresses in `text`, left to right, lowercase, de-duplicated.
pub fn emails(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        if *c != '@' {
            continue;
        }
        let mut a = i;
        while a > 0 && local_char(chars[a - 1]) {
            a -= 1;
        }
        let mut b = i + 1;
        while b < chars.len() && (chars[b].is_ascii_alphanumeric() || matches!(chars[b], '.' | '-'))
        {
            b += 1;
        }
        // A trailing dot belongs to the sentence.
        while b > i + 1 && chars[b - 1] == '.' {
            b -= 1;
        }
        let cand: String = chars[a..b].iter().collect();
        if is_email(&cand) {
            let cand = cand.to_lowercase();
            if !out.contains(&cand) {
                out.push(cand);
            }
        }
    }
    out
}

/// Authors that are tool defaults, not people.
pub fn generic_author(name: &str) -> bool {
    let n: String = name.nfkc().collect::<String>().trim().to_lowercase();
    n.is_empty()
        || n.chars().count() < 2
        || [
            "admin",
            "administrator",
            "user",
            "owner",
            "author",
            "unknown",
            "anonymous",
            "microsoft office user",
            "microsoft office",
            "microsoft",
            "wps",
            "wps office",
            "python-docx",
            "python-pptx",
            "openpyxl",
            "docling",
            "作者",
            "未知",
            "匿名",
            "管理员",
            "用户",
            "windows 用户",
            "windows用户",
        ]
        .contains(&n.as_str())
}

/// A version marker found at the end of a file stem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionMark {
    /// The stem without the marker, canonical form.
    pub trunk: String,
    /// Higher is newer; the unmarked file is 0.
    pub rank: u32,
}

/// Split a file stem into trunk and version rank (`方案-v2`, `方案_final`,
/// `方案（修订）`, `方案-rev3`). `None` when the stem carries no marker.
pub fn version_mark(stem: &str) -> Option<VersionMark> {
    let s: String = stem.nfkc().collect::<String>().trim().to_lowercase();
    let cut = |t: &str, suffix: &str| -> Option<String> {
        t.strip_suffix(suffix).map(|x| {
            x.trim_end_matches(['-', '_', ' ', '(', '（'])
                .trim()
                .to_owned()
        })
    };
    for (suffix, rank) in [
        ("_final", 1000u32),
        ("-final", 1000),
        (" final", 1000),
        ("最终版", 1000),
        ("终版", 1000),
        ("(修订)", 500),
        ("（修订）", 500),
        ("修订版", 500),
        ("(revised)", 500),
    ] {
        // `（修订）` was NFKC-folded to `(修订)`.
        if let Some(trunk) = cut(&s, suffix)
            && !trunk.is_empty()
        {
            return Some(VersionMark { trunk, rank });
        }
    }
    for tag in ["rev", "v"] {
        if let Some(pos) = s.rfind(tag) {
            let digits = &s[pos + tag.len()..];
            if !digits.is_empty()
                && digits.len() <= 3
                && digits.bytes().all(|b| b.is_ascii_digit())
                && let Ok(n) = digits.parse::<u32>()
            {
                let trunk = s[..pos]
                    .trim_end_matches(['-', '_', ' ', '.'])
                    .trim()
                    .to_owned();
                if !trunk.is_empty() && (pos == 0 || !s.as_bytes()[pos - 1].is_ascii_alphabetic()) {
                    return Some(VersionMark { trunk, rank: n });
                }
            }
        }
    }
    None
}

/// `term：definition` of one glossary line (`: `, `：`, ` — `, ` - `). Both
/// sides must be non-empty; the term is at most 60 characters.
pub fn definition_line(text: &str) -> Option<(String, String)> {
    let t = text
        .trim()
        .trim_start_matches(['-', '*', '+', ' '])
        .trim()
        .replace("**", "");
    let cut = t
        .char_indices()
        .find(|&(i, c)| {
            matches!(c, ':' | '：')
                || (c == '—' || c == '-' || c == '–') && i > 0 && t[..i].ends_with(' ')
        })
        .map(|(i, c)| (i, c.len_utf8()))?;
    let term = t[..cut.0].trim().trim_matches(['*', '_', '`']).trim();
    let def = t[cut.0 + cut.1..].trim();
    if term.is_empty() || def.is_empty() || term.chars().count() > 60 {
        return None;
    }
    Some((term.to_owned(), def.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_found_and_lowercased() {
        assert_eq!(
            emails("联系 Chen.Zimo@Example.com，或 ops@a.b.cn。"),
            ["chen.zimo@example.com", "ops@a.b.cn"]
        );
        assert!(emails("a@b").is_empty());
        assert!(emails("@x.com and y@.com").is_empty());
        assert!(is_email("a_b@x.io"));
        assert!(!is_email("a b@x.io"));
    }

    #[test]
    fn version_marks() {
        assert_eq!(
            version_mark("星图方案-v2"),
            Some(VersionMark {
                trunk: "星图方案".into(),
                rank: 2
            })
        );
        assert_eq!(version_mark("plan_final").unwrap().rank, 1000);
        assert_eq!(version_mark("方案（修订）").unwrap().trunk, "方案");
        assert_eq!(version_mark("方案-rev3").unwrap().rank, 3);
        assert_eq!(version_mark("review"), None);
        assert_eq!(version_mark("canvas"), None);
        assert_eq!(version_mark("方案"), None);
    }

    #[test]
    fn definition_lines() {
        assert_eq!(
            definition_line("- **ARR**：年度经常性收入"),
            Some(("ARR".into(), "年度经常性收入".into()))
        );
        assert_eq!(
            definition_line("SLA - 服务等级协议"),
            Some(("SLA".into(), "服务等级协议".into()))
        );
        assert_eq!(definition_line("没有分隔"), None);
        assert_eq!(definition_line("a-b"), None);
    }

    #[test]
    fn generic_authors_are_not_people() {
        assert!(generic_author("Microsoft Office User"));
        assert!(generic_author("admin"));
        assert!(!generic_author("陈子墨"));
    }
}
