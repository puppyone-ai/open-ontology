//! Frontmatter: block detection and a deliberately small YAML subset.
//!
//! Supported: `key: scalar`, `key: [a, b]`, and block lists
//! (`key:` followed by `- item` lines). Scalars are quoted strings, `true`,
//! `false`, `null`/`~`, integers, decimals and plain strings (dates stay
//! strings). Not supported (the key is skipped and counted, the caller adds a
//! `DOCS_PARSE_FRONTMATTER_UNSUPPORTED` warning): nested maps, multi-line
//! scalars (`|`, `>`), anchors/aliases/tags, flow maps, nested flow lists.
//! A full YAML parser would need a dependency (oo-core has serde-saphyr,
//! reaching it needs a coordinator decision); this subset covers the golden
//! set and the usual Obsidian-style properties.

use std::collections::BTreeMap;

use serde_json::Value;

/// Location of a frontmatter block in the decoded text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    /// YAML between the fences.
    pub inner: (usize, usize),
    /// End of the closing fence line, without its line break.
    pub block_end: usize,
    /// First byte after the closing fence line (and its line break).
    pub body_start: usize,
}

fn is_fence(line: &str, closers: &[&str]) -> bool {
    let t = line.trim_end_matches(['\r', ' ', '\t']);
    closers.contains(&t)
}

/// A frontmatter block starts on line 1 with `---` and ends at the next line
/// that is exactly `---` or `...`. Without a closing fence there is none.
pub fn split(text: &str) -> Option<Split> {
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if !is_fence(first.trim_end_matches('\n'), &["---"]) || !first.ends_with('\n') {
        return None;
    }
    let mut pos = first.len();
    for line in lines {
        let start = pos;
        pos += line.len();
        let body = line.trim_end_matches('\n');
        if is_fence(body, &["---", "..."]) {
            return Some(Split {
                inner: (first.len(), start),
                block_end: start + body.trim_end_matches('\r').len(),
                body_start: pos,
            });
        }
    }
    None
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frontmatter {
    pub map: BTreeMap<String, Value>,
    /// Keys or lines skipped because they use unsupported YAML.
    pub unsupported: u32,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn parse_scalar(raw: &str) -> Value {
    let s = raw.trim();
    if let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        }
        return Value::String(out);
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        return Value::String(inner.replace("''", "'"));
    }
    match s {
        "true" | "True" => return Value::Bool(true),
        "false" | "False" => return Value::Bool(false),
        "null" | "Null" | "~" | "" => return Value::Null,
        _ => {}
    }
    let digits = s.strip_prefix('-').unwrap_or(s);
    if !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && let Ok(n) = s.parse::<i64>()
    {
        return Value::from(n);
    }
    if let Some((a, b)) = digits.split_once('.')
        && !a.is_empty()
        && !b.is_empty()
        && a.bytes().all(|c| c.is_ascii_digit())
        && b.bytes().all(|c| c.is_ascii_digit())
        && (a == "0" || !a.starts_with('0'))
        && let Ok(f) = s.parse::<f64>()
        && let Some(n) = serde_json::Number::from_f64(f)
    {
        return Value::Number(n);
    }
    Value::String(s.to_owned())
}

/// Remove a trailing ` # comment` from an unquoted value.
fn strip_comment(s: &str) -> &str {
    let t = s.trim();
    if t.starts_with(['"', '\'']) {
        return t;
    }
    match t.find(" #") {
        Some(i) => t[..i].trim_end(),
        None => t,
    }
}

/// Split `a, "b, c", d` on top-level commas. `None` for nested flow syntax.
fn flow_items(inner: &str) -> Option<Vec<String>> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c);
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                cur.push(c);
            }
            (None, '[' | ']' | '{' | '}') => return None,
            (None, ',') => items.push(std::mem::take(&mut cur)),
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if !cur.trim().is_empty() || !items.is_empty() {
        items.push(cur);
    }
    Some(items.into_iter().map(|s| s.trim().to_owned()).collect())
}

pub fn parse_simple_yaml(yaml: &str) -> Frontmatter {
    let lines: Vec<&str> = yaml.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut out = Frontmatter::default();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        // Stray indentation or list items without a key: unsupported.
        let Some((key_raw, rest)) = split_key(line) else {
            out.unsupported += 1;
            continue;
        };
        let key = match parse_scalar(key_raw) {
            Value::String(s) if !s.is_empty() => s,
            _ => {
                out.unsupported += 1;
                continue;
            }
        };
        let rest = strip_comment(rest);
        let value = if rest.is_empty() {
            // Block list, nested map, or null: look at the indented lines.
            let mut items = Vec::new();
            let mut nested = false;
            let mut saw_item = false;
            while i < lines.len() {
                let next = lines[i];
                let nt = next.trim();
                if nt.is_empty() || nt.starts_with('#') {
                    i += 1;
                    continue;
                }
                if indent_of(next) == 0 && !nt.starts_with("- ") && nt != "-" {
                    break;
                }
                i += 1;
                match nt.strip_prefix("- ") {
                    Some(item) if !nested => {
                        let item = strip_comment(item);
                        if item.starts_with(['[', '{', '&', '*', '!', '|', '>'])
                            || split_key(item).is_some()
                        {
                            nested = true;
                        } else {
                            items.push(parse_scalar(item));
                            saw_item = true;
                        }
                    }
                    _ => nested = true,
                }
            }
            if nested {
                out.unsupported += 1;
                continue;
            }
            if saw_item {
                Value::Array(items)
            } else {
                Value::Null
            }
        } else if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            match flow_items(inner) {
                Some(items) => Value::Array(items.iter().map(|s| parse_scalar(s)).collect()),
                None => {
                    out.unsupported += 1;
                    continue;
                }
            }
        } else if rest.starts_with(['{', '[', '|', '>', '&', '*', '!', '%', '@', '`']) {
            // Skip a multi-line scalar body too.
            while i < lines.len() && (lines[i].trim().is_empty() || indent_of(lines[i]) > 0) {
                i += 1;
            }
            out.unsupported += 1;
            continue;
        } else {
            parse_scalar(rest)
        };
        out.map.insert(key, value);
    }
    out
}

/// `key: rest` at column 0. The colon must be followed by a space or end the
/// line, so `http://x` and times in plain scalars do not split.
fn split_key(line: &str) -> Option<(&str, &str)> {
    if line.starts_with([' ', '\t', '#']) || line.starts_with("- ") || line == "-" {
        return None;
    }
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (quote, b) {
            (Some(q), b) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') if i == 0 => quote = Some(b),
            (None, b':')
                if i + 1 == bytes.len() || bytes[i + 1] == b' ' || bytes[i + 1] == b'\t' =>
            {
                return Some((line[..i].trim(), line[i + 1..].trim()));
            }
            _ => {}
        }
    }
    None
}
