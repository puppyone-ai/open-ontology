//! Quote-aware YAML pre-scan.
//!
//! `serde-saphyr` still accepts core schema tags such as `!!str` when
//! `reject_unsupported_tags` is set. This scan rejects every explicit tag,
//! alias, anchor, merge key, directive, and extra document before
//! deserialization. A `!` inside quotes, comments, block scalars, or the
//! middle of a plain scalar is data.

use crate::diagnostic::{CONTRACT_CONFIG, Diagnostic};

#[derive(Debug)]
struct ScanError {
    name: &'static str,
    line: u32,
    column: u32,
    message: &'static str,
}

pub fn scan_yaml(input: &str, contract: &str) -> Result<(), Diagnostic> {
    match scan(input) {
        Ok(()) => Ok(()),
        Err(error) => Err(Diagnostic::error(contract, error.name, "/", error.message)
            .at(error.line, error.column)),
    }
}

fn scan(input: &str) -> Result<(), ScanError> {
    if input.starts_with('\u{feff}') {
        return Err(err("yaml.bom", 1, 1, "UTF-8 BOM is rejected"));
    }
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut column = 1u32;
    let mut flow = 0i32;
    let mut seen_content = false;
    let mut closed = false;
    let mut line_indent = 0usize;
    let mut at_line_start = true;

    let fail = |name, line, column, message| Err(err(name, line, column, message));

    while i < chars.len() {
        if at_line_start && flow == 0 {
            let mut look = i;
            let mut indent = 0usize;
            let mut look_col = column;
            while look < chars.len() && chars[look].1 == ' ' {
                indent += 1;
                look += 1;
                look_col += 1;
            }
            if look >= chars.len() {
                break;
            }
            if chars[look].1 == '\n' {
                i = look;
                bump(&mut i, &mut line, &mut column, '\n');
                at_line_start = true;
                line_indent = 0;
                continue;
            }
            let rest: String = chars[look..]
                .iter()
                .map(|(_, c)| *c)
                .take_while(|c| *c != '\n')
                .collect();
            let trimmed = rest.trim_end();
            if trimmed == "---" || trimmed == "..." {
                if trimmed == "---" {
                    if seen_content || closed {
                        return fail(
                            "yaml.document",
                            line,
                            look_col,
                            "only one YAML document is allowed",
                        );
                    }
                } else {
                    closed = true;
                }
                i = look + trimmed.chars().count();
                column = look_col + u32::try_from(trimmed.chars().count()).unwrap_or(u32::MAX);
                if i < chars.len() && chars[i].1 == '\n' {
                    bump(&mut i, &mut line, &mut column, '\n');
                }
                at_line_start = true;
                continue;
            }
            if chars[look].1 == '%' {
                return fail(
                    "yaml.directive",
                    line,
                    look_col,
                    "YAML directives are rejected",
                );
            }
            if closed {
                return fail(
                    "yaml.document",
                    line,
                    look_col,
                    "content after the document end is rejected",
                );
            }
            line_indent = indent;
            i = look;
            column = look_col;
            at_line_start = false;
            continue;
        }

        let ch = chars[i].1;
        if ch == '\n' {
            bump(&mut i, &mut line, &mut column, ch);
            at_line_start = true;
            continue;
        }
        if ch == '"' {
            seen_content = true;
            bump(&mut i, &mut line, &mut column, ch);
            while i < chars.len() {
                let inner = chars[i].1;
                if inner == '\\' {
                    bump(&mut i, &mut line, &mut column, inner);
                    if i < chars.len() {
                        let escaped = chars[i].1;
                        bump(&mut i, &mut line, &mut column, escaped);
                    }
                    continue;
                }
                bump(&mut i, &mut line, &mut column, inner);
                if inner == '"' {
                    break;
                }
                if inner == '\n' {
                    at_line_start = true;
                }
            }
            continue;
        }
        if ch == '\'' {
            seen_content = true;
            bump(&mut i, &mut line, &mut column, ch);
            while i < chars.len() {
                let inner = chars[i].1;
                if inner == '\'' {
                    bump(&mut i, &mut line, &mut column, inner);
                    if i < chars.len() && chars[i].1 == '\'' {
                        bump(&mut i, &mut line, &mut column, '\'');
                        continue;
                    }
                    break;
                }
                bump(&mut i, &mut line, &mut column, inner);
                if inner == '\n' {
                    at_line_start = true;
                }
            }
            continue;
        }
        if ch == '#' && token_boundary(&chars, i) {
            while i < chars.len() && chars[i].1 != '\n' {
                let current = chars[i].1;
                bump(&mut i, &mut line, &mut column, current);
            }
            continue;
        }
        if flow == 0 && (ch == '|' || ch == '>') && token_boundary(&chars, i) {
            seen_content = true;
            let parent_indent = line_indent;
            let header_line = line;
            let header_col = column;
            bump(&mut i, &mut line, &mut column, ch);
            let mut explicit: Option<usize> = None;
            if i < chars.len() && (chars[i].1 == '+' || chars[i].1 == '-') {
                let current = chars[i].1;
                bump(&mut i, &mut line, &mut column, current);
            }
            if i < chars.len() && chars[i].1.is_ascii_digit() {
                explicit = Some(usize::from(chars[i].1 as u8 - b'0'));
                let current = chars[i].1;
                bump(&mut i, &mut line, &mut column, current);
            }
            while i < chars.len() && chars[i].1 != '\n' {
                let header_ch = chars[i].1;
                if header_ch == '#' && token_boundary(&chars, i) {
                    break;
                }
                if !header_ch.is_whitespace() {
                    return fail(
                        "yaml.syntax",
                        header_line,
                        header_col,
                        "invalid block scalar header",
                    );
                }
                bump(&mut i, &mut line, &mut column, header_ch);
            }
            if i < chars.len() && chars[i].1 == '\n' {
                bump(&mut i, &mut line, &mut column, '\n');
            }
            let min_indent = explicit.unwrap_or(parent_indent + 1);
            if min_indent == 0 || min_indent <= parent_indent {
                return fail(
                    "yaml.syntax",
                    header_line,
                    header_col,
                    "block scalar indent is ambiguous",
                );
            }
            loop {
                if i >= chars.len() {
                    break;
                }
                let mut look = i;
                let mut indent = 0usize;
                while look < chars.len() && chars[look].1 == ' ' {
                    indent += 1;
                    look += 1;
                }
                if look >= chars.len() {
                    i = look;
                    break;
                }
                if chars[look].1 == '\n' {
                    i = look;
                    bump(&mut i, &mut line, &mut column, '\n');
                    continue;
                }
                if indent < min_indent {
                    column = content_line_column(indent);
                    line_indent = 0;
                    break;
                }
                while i < chars.len() && chars[i].1 != '\n' {
                    let current = chars[i].1;
                    bump(&mut i, &mut line, &mut column, current);
                }
                if i < chars.len() && chars[i].1 == '\n' {
                    bump(&mut i, &mut line, &mut column, '\n');
                }
            }
            at_line_start = true;
            continue;
        }
        if ch == '{' || ch == '[' {
            seen_content = true;
            flow += 1;
            bump(&mut i, &mut line, &mut column, ch);
            continue;
        }
        if ch == '}' || ch == ']' {
            flow -= 1;
            if flow < 0 {
                return fail("yaml.syntax", line, column, "unbalanced flow collection");
            }
            bump(&mut i, &mut line, &mut column, ch);
            continue;
        }
        if token_boundary(&chars, i) && matches!(ch, '!' | '&' | '*' | '<') {
            if ch == '!' {
                return fail("yaml.tag", line, column, "YAML tags are rejected");
            }
            if ch == '&' {
                return fail("yaml.anchor", line, column, "YAML anchors are rejected");
            }
            if ch == '*' {
                return fail("yaml.alias", line, column, "YAML aliases are rejected");
            }
            if ch == '<' && i + 1 < chars.len() && chars[i + 1].1 == '<' {
                let after = i + 2;
                let ended = after >= chars.len()
                    || matches!(
                        chars[after].1,
                        ' ' | '\t' | '\n' | ':' | ',' | ']' | '}' | '#'
                    );
                if ended {
                    return fail(
                        "yaml.merge_key",
                        line,
                        column,
                        "YAML merge keys are rejected",
                    );
                }
            }
        }
        if !ch.is_whitespace() {
            seen_content = true;
        }
        bump(&mut i, &mut line, &mut column, ch);
        at_line_start = false;
    }
    let _ = CONTRACT_CONFIG;
    Ok(())
}

fn content_line_column(indent: usize) -> u32 {
    u32::try_from(indent + 1).unwrap_or(u32::MAX)
}

fn bump(i: &mut usize, line: &mut u32, column: &mut u32, ch: char) {
    *i += 1;
    if ch == '\n' {
        *line += 1;
        *column = 1;
    } else {
        *column = column.saturating_add(1);
    }
}

fn token_boundary(chars: &[(usize, char)], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let prev = chars[index - 1].1;
    if prev.is_whitespace() || matches!(prev, '{' | '[' | ',' | ':' | '?' | '-') {
        if prev == '-' {
            let dash_boundary =
                index == 1 || chars[index - 2].1.is_whitespace() || chars[index - 2].1 == '\n';
            return dash_boundary;
        }
        return true;
    }
    false
}

fn err(name: &'static str, line: u32, column: u32, message: &'static str) -> ScanError {
    ScanError {
        name,
        line,
        column,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str) {
        scan_yaml(input, CONTRACT_CONFIG)
            .unwrap_or_else(|err| panic!("{input:?} rejected: {err:?}"));
    }

    fn bad(input: &str, code_suffix: &str) {
        let err = scan_yaml(input, CONTRACT_CONFIG).expect_err(input);
        assert!(err.code.ends_with(code_suffix), "{} -> {}", input, err.code);
    }

    #[test]
    fn quoted_and_plain_bang_are_data() {
        ok("note: \"hello!\"\n");
        ok("note: 'a!b'\n");
        ok("note: watch!\n");
        ok("note: \"*notalias\"\n");
        ok("# !tag\nvalue: 1\n");
        ok("instructions_append: |\n  hello!\n  !still-data\nnext: 1\n");
    }

    #[test]
    fn tags_aliases_anchors_and_directives_fail() {
        bad("note: !!str hello\n", "yaml.tag");
        bad("note: !include x\n", "yaml.tag");
        bad("note: &anchor 1\n", "yaml.anchor");
        bad("note: *alias\n", "yaml.alias");
        bad("key:!!str\n", "yaml.tag");
        bad("<<: {a: 1}\n", "yaml.merge_key");
        bad("%YAML 1.2\nvalue: 1\n", "yaml.directive");
        bad("\u{feff}value: 1\n", "yaml.bom");
        bad("a: 1\n---\nb: 2\n", "yaml.document");
    }
}
