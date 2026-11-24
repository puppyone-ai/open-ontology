//! Titles and honorifics of person names (10 §5.4 step 1). Pure.
//!
//! A mention such as `陈总`, `老陈` or `陈子墨老师` gets *candidate* forms
//! with the title removed. They are never a canonical name and always score
//! [`crate::resolve::exact::Via::Honorific`] (5000), which stays below the
//! binding threshold, so a title alone can never merge two people; with
//! context bonuses it only ranks candidates for review and for
//! `POST /resolve`.

use crate::resolve::normalize::norm;

/// Suffix titles, longest first.
pub const SUFFIXES: &[&str] = &[
    "老师", "经理", "主任", "同学", "老板", "博士", "先生", "女士", "总", "姐", "哥",
];
/// Prefix titles.
pub const PREFIXES: &[&str] = &["老", "小", "阿"];

fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
}

/// The name with one title removed, or `None` when it carries no title or
/// nothing would be left.
fn strip_once(name: &str) -> Option<String> {
    if SUFFIXES.contains(&name) {
        return None;
    }
    for s in SUFFIXES {
        if let Some(rest) = name.strip_suffix(s)
            && !rest.is_empty()
        {
            return Some(rest.to_owned());
        }
    }
    for p in PREFIXES {
        if let Some(rest) = name.strip_prefix(p)
            && !rest.is_empty()
        {
            return Some(rest.to_owned());
        }
    }
    None
}

/// Title-free forms of a person name, canonical (`norm`) form, without the
/// name itself, in a fixed order. A title is removed at most twice (`老陈总`).
pub fn stripped_forms(name: &str) -> Vec<String> {
    let n = norm(name);
    let mut out = Vec::new();
    let mut cur = n.clone();
    for _ in 0..2 {
        match strip_once(&cur) {
            Some(rest) => {
                if rest != n && !out.contains(&rest) {
                    out.push(rest.clone());
                }
                cur = rest;
            }
            None => break,
        }
    }
    out
}

/// The surname form (`陈`, `欧阳`) of a stripped form, when it is one or two
/// Han characters: stored people whose canonical name starts with it are
/// candidates.
pub fn surname_form(stripped: &str) -> Option<String> {
    let count = stripped.chars().count();
    if (1..=2).contains(&count) && stripped.chars().all(is_han) {
        Some(stripped.to_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_and_prefix_titles() {
        assert_eq!(stripped_forms("陈总"), ["陈"]);
        assert_eq!(stripped_forms("老陈"), ["陈"]);
        assert_eq!(stripped_forms("陈子墨老师"), ["陈子墨"]);
        assert_eq!(stripped_forms("小陈"), ["陈"]);
        assert_eq!(stripped_forms("陈子墨"), Vec::<String>::new());
        // Nothing left of the name: not a title.
        assert_eq!(stripped_forms("总"), Vec::<String>::new());
        assert_eq!(stripped_forms("老师"), Vec::<String>::new());
        // `老陈总`: prefix and suffix are both removed, one at a time.
        assert_eq!(stripped_forms("老陈总"), ["老陈", "陈"]);
    }

    #[test]
    fn surnames_are_one_or_two_han_characters() {
        assert_eq!(surname_form("陈"), Some("陈".into()));
        assert_eq!(surname_form("欧阳"), Some("欧阳".into()));
        assert_eq!(surname_form("陈子墨"), None);
        assert_eq!(surname_form("Li"), None);
    }
}
