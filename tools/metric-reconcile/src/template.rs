//! `:name` template scan. Existing `$n`, dollar quotes, and `E'...'` are refused.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Templated {
    pub bound_sql: String,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateError {
    Unsupported,
}

pub fn compile_template(sql: &str) -> Result<Templated, TemplateError> {
    let bytes = sql.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut names: Vec<String> = Vec::new();
    let mut mode = Mode::Normal;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match mode {
            Mode::Normal => {
                if byte == b'\'' {
                    mode = Mode::String;
                    out.push(byte);
                    index += 1;
                } else if byte == b'-' && bytes.get(index + 1) == Some(&b'-') {
                    mode = Mode::Line;
                    out.push(byte);
                    out.push(b'-');
                    index += 2;
                } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    mode = Mode::Block;
                    out.push(byte);
                    out.push(b'*');
                    index += 2;
                } else if byte == b'$' || is_escape_string(bytes, index) {
                    return Err(TemplateError::Unsupported);
                } else if byte == b':' {
                    if bytes.get(index + 1) == Some(&b':') {
                        out.push(b':');
                        out.push(b':');
                        index += 2;
                        continue;
                    }
                    let start = index + 1;
                    if start >= bytes.len() || !is_ident_start(bytes[start]) {
                        return Err(TemplateError::Unsupported);
                    }
                    let mut end = start + 1;
                    while end < bytes.len() && is_ident_cont(bytes[end]) {
                        end += 1;
                    }
                    if end - start > 64 {
                        return Err(TemplateError::Unsupported);
                    }
                    let name = std::str::from_utf8(&bytes[start..end])
                        .map_err(|_| TemplateError::Unsupported)?;
                    let slot = if let Some(found) = names.iter().position(|have| have == name) {
                        found + 1
                    } else {
                        if names.len() == 32 {
                            return Err(TemplateError::Unsupported);
                        }
                        names.push(name.to_owned());
                        names.len()
                    };
                    out.extend_from_slice(format!("${slot}").as_bytes());
                    index = end;
                } else {
                    out.push(byte);
                    index += 1;
                }
            }
            Mode::String => {
                out.push(byte);
                if byte == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        out.push(b'\'');
                        index += 2;
                    } else {
                        mode = Mode::Normal;
                        index += 1;
                    }
                } else {
                    index += 1;
                }
            }
            Mode::Line => {
                out.push(byte);
                index += 1;
                if byte == b'\n' {
                    mode = Mode::Normal;
                }
            }
            Mode::Block => {
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    out.push(b'*');
                    out.push(b'/');
                    index += 2;
                    mode = Mode::Normal;
                } else {
                    out.push(byte);
                    index += 1;
                }
            }
        }
    }
    if mode != Mode::Normal {
        return Err(TemplateError::Unsupported);
    }
    let bound_sql = String::from_utf8(out).map_err(|_| TemplateError::Unsupported)?;
    Ok(Templated { bound_sql, names })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    String,
    Line,
    Block,
}

pub fn negative_code(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut mode = Mode::Normal;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match mode {
            Mode::Normal => {
                if byte == b'\'' {
                    mode = Mode::String;
                    index += 1;
                } else if byte == b'-' && bytes.get(index + 1) == Some(&b'-') {
                    mode = Mode::Line;
                    index += 2;
                } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    mode = Mode::Block;
                    index += 2;
                } else if (byte == b'<' && bytes.get(index + 1) == Some(&b'>'))
                    || (byte == b'!' && bytes.get(index + 1) == Some(&b'='))
                    || word_not_in(bytes, index)
                {
                    return true;
                } else {
                    index += 1;
                }
            }
            Mode::String => {
                if byte == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        index += 2;
                    } else {
                        mode = Mode::Normal;
                        index += 1;
                    }
                } else {
                    index += 1;
                }
            }
            Mode::Line => {
                index += 1;
                if byte == b'\n' {
                    mode = Mode::Normal;
                }
            }
            Mode::Block => {
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    index += 2;
                    mode = Mode::Normal;
                } else {
                    index += 1;
                }
            }
        }
    }
    false
}

fn word_not_in(bytes: &[u8], index: usize) -> bool {
    if !word_at(bytes, index, b"not") {
        return false;
    }
    let mut cursor = index + 3;
    if cursor >= bytes.len() || !bytes[cursor].is_ascii_whitespace() {
        return false;
    }
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    word_at(bytes, cursor, b"in")
}

fn word_at(bytes: &[u8], index: usize, word: &[u8]) -> bool {
    if index > 0 && is_ident_cont(bytes[index - 1]) {
        return false;
    }
    let end = index + word.len();
    if end > bytes.len() {
        return false;
    }
    if !bytes[index..end].eq_ignore_ascii_case(word) {
        return false;
    }
    end == bytes.len() || !is_ident_cont(bytes[end])
}

fn is_escape_string(bytes: &[u8], index: usize) -> bool {
    let byte = bytes[index];
    if byte != b'E' && byte != b'e' {
        return false;
    }
    if bytes.get(index + 1) != Some(&b'\'') {
        return false;
    }
    index == 0 || !is_ident_cont(bytes[index - 1])
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_cont(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

pub fn normalize_ws(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut pending = false;
    for character in sql.chars() {
        if character.is_whitespace() {
            pending = !out.is_empty();
        } else {
            if pending {
                out.push(' ');
                pending = false;
            }
            out.push(character);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_in_first_seen_order_and_ignores_casts_comments_and_strings() {
        let sql = "SELECT amount FROM src.receipt WHERE status = :status AND note = ':skip' AND received_on >= :start -- :no\n AND flag::bool";
        let templated = compile_template(sql).expect("template");
        assert_eq!(templated.names, vec!["status".to_owned(), "start".to_owned()]);
        assert!(templated.bound_sql.contains("$1"));
        assert!(templated.bound_sql.contains("$2"));
        assert!(templated.bound_sql.contains("':skip'"));
        assert!(templated.bound_sql.contains("::bool"));
        assert!(templated.bound_sql.contains("-- :no"));
        assert!(!templated.names.iter().any(|name| name == "no"));
    }

    #[test]
    fn unsupported_forms() {
        assert!(compile_template("SELECT amount FROM src.receipt WHERE id = $1").is_err());
        assert!(compile_template("SELECT $$x$$").is_err());
        assert!(compile_template("SELECT $tag$x$tag$").is_err());
        assert!(compile_template("SELECT E'x'").is_err());
        assert!(compile_template("SELECT 'unterminated").is_err());
        assert!(compile_template("SELECT :").is_err());
    }

    #[test]
    fn negative_codes_stay_outside_strings() {
        assert!(negative_code("SELECT a FROM s.t WHERE a <> 'x'"));
        assert!(negative_code("SELECT a FROM s.t WHERE a != b"));
        assert!(negative_code("SELECT a FROM s.t WHERE a NOT IN ('x')"));
        assert!(negative_code("SELECT a FROM s.t WHERE a not\nin ('x')"));
        assert!(!negative_code("SELECT a FROM s.t WHERE a IS NOT NULL"));
        assert!(!negative_code("SELECT a FROM s.t WHERE note = '<>'"));
        assert!(!negative_code("SELECT a FROM s.t WHERE note = 'NOT IN'"));
    }
}
