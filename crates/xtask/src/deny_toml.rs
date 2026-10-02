//! Enough TOML to read `ci/deny.toml` and reject a hostile edit.
//!
//! Comments are discarded. A license name that appears only in a comment is
//! not an allow-list entry. Duplicate keys are errors. This is not a general
//! TOML library.

use serde_json::{Map, Value, json};

use crate::report::Issue;

pub fn parse_deny_toml(text: &str) -> Result<Value, Issue> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        index: 0,
        line: 1,
    };
    parser.document()
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    line: usize,
}

impl<'a> Parser<'a> {
    fn document(&mut self) -> Result<Value, Issue> {
        let mut root = Map::new();
        let mut current: Vec<String> = Vec::new();
        self.skip();
        while self.index < self.bytes.len() {
            if self.peek() == Some(b'[') {
                let (path, array) = self.table_header()?;
                current = path;
                if array {
                    self.array_of_tables(&current)?;
                } else {
                    self.ensure_table(&mut root, &current)?;
                }
                self.skip();
                continue;
            }
            let key = self.key()?;
            self.skip();
            if self.peek() != Some(b'=') {
                return self.err("expected '=' after a key");
            }
            self.bump();
            self.skip();
            let value = self.value()?;
            self.insert(&mut root, &current, &key, value)?;
            self.skip();
        }
        Ok(Value::Object(root))
    }

    fn table_header(&mut self) -> Result<(Vec<String>, bool), Issue> {
        self.bump();
        let array = if self.peek() == Some(b'[') {
            self.bump();
            true
        } else {
            false
        };
        self.skip();
        let mut path = Vec::new();
        loop {
            path.push(self.key()?);
            self.skip();
            if self.peek() == Some(b'.') {
                self.bump();
                self.skip();
                continue;
            }
            break;
        }
        if self.peek() != Some(b']') {
            return self.err("expected ']' after a table name");
        }
        self.bump();
        if array {
            if self.peek() != Some(b']') {
                return self.err("expected ']]' after an array-of-tables name");
            }
            self.bump();
        }
        if path.is_empty() {
            return self.err("empty table name");
        }
        Ok((path, array))
    }

    fn array_of_tables(&mut self, path: &[String]) -> Result<(), Issue> {
        let _ = path;
        Err(self.issue(
            "supply.exception",
            "an array-of-tables is not part of the reviewed deny config",
        ))
    }

    fn ensure_table(
        &mut self,
        root: &mut Map<String, Value>,
        path: &[String],
    ) -> Result<(), Issue> {
        let mut cursor = root;
        for (depth, part) in path.iter().enumerate() {
            let last = depth + 1 == path.len();
            if !cursor.contains_key(part) {
                cursor.insert(part.clone(), Value::Object(Map::new()));
            }
            let slot = cursor.get_mut(part).unwrap();
            if !slot.is_object() {
                return self.err("a table name replaces a value");
            }
            if last {
                return Ok(());
            }
            cursor = match slot {
                Value::Object(map) => map,
                _ => unreachable!(),
            };
        }
        Ok(())
    }

    fn insert(
        &mut self,
        root: &mut Map<String, Value>,
        table: &[String],
        key: &str,
        value: Value,
    ) -> Result<(), Issue> {
        let mut cursor = root;
        for part in table {
            let slot = cursor
                .get_mut(part)
                .ok_or_else(|| self.issue("supply.deny", "table was not opened"))?;
            cursor = match slot {
                Value::Object(map) => map,
                _ => return self.err("cannot insert into a non-table"),
            };
        }
        if cursor.contains_key(key) {
            return self.err(&format!("duplicate key {key}"));
        }
        cursor.insert(key.to_owned(), value);
        Ok(())
    }

    fn value(&mut self) -> Result<Value, Issue> {
        self.skip();
        match self.peek() {
            Some(b'"') => Ok(Value::String(self.basic_string()?)),
            Some(b'\'') => Ok(Value::String(self.literal_string()?)),
            Some(b'[') => self.array(),
            Some(b'{') => self.inline_table(),
            Some(b't') | Some(b'f') => self.bool(),
            Some(b'+') | Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => self.err("expected a value"),
        }
    }

    fn array(&mut self) -> Result<Value, Issue> {
        self.bump();
        let mut items = Vec::new();
        self.skip();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip();
                    if self.peek() == Some(b']') {
                        self.bump();
                        break;
                    }
                }
                Some(b']') => {
                    self.bump();
                    break;
                }
                _ => return self.err("expected ',' or ']' in an array"),
            }
        }
        Ok(Value::Array(items))
    }

    fn inline_table(&mut self) -> Result<Value, Issue> {
        self.bump();
        let mut map = Map::new();
        self.skip();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Value::Object(map));
        }
        loop {
            let key = self.key()?;
            self.skip();
            if self.peek() != Some(b'=') {
                return self.err("expected '=' in an inline table");
            }
            self.bump();
            let value = self.value()?;
            if map.contains_key(&key) {
                return self.err(&format!("duplicate key {key}"));
            }
            map.insert(key, value);
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip();
                    if self.peek() == Some(b'}') {
                        return self.err("trailing comma in an inline table");
                    }
                }
                Some(b'}') => {
                    self.bump();
                    break;
                }
                _ => return self.err("expected ',' or '}' in an inline table"),
            }
        }
        Ok(Value::Object(map))
    }

    fn bool(&mut self) -> Result<Value, Issue> {
        if self.consume(b"true") {
            return Ok(Value::Bool(true));
        }
        if self.consume(b"false") {
            return Ok(Value::Bool(false));
        }
        self.err("expected true or false")
    }

    fn number(&mut self) -> Result<Value, Issue> {
        let start = self.index;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            self.bump();
        }
        if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            return self.err("expected a number");
        }
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_digit() || byte == b'_')
        {
            self.bump();
        }
        let mut float = false;
        if self.peek() == Some(b'.') {
            float = true;
            self.bump();
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return self.err("expected a digit after a decimal point");
            }
            while self
                .peek()
                .is_some_and(|byte| byte.is_ascii_digit() || byte == b'_')
            {
                self.bump();
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            float = true;
            self.bump();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.bump();
            }
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return self.err("expected an exponent");
            }
            while self
                .peek()
                .is_some_and(|byte| byte.is_ascii_digit() || byte == b'_')
            {
                self.bump();
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index])
            .map_err(|_| self.issue("supply.deny", "number is not utf-8"))?
            .replace('_', "");
        if float {
            Ok(Value::String(text))
        } else {
            Ok(json!(text))
        }
    }

    fn basic_string(&mut self) -> Result<String, Issue> {
        self.bump();
        if self.starts_with(b"\"\"") {
            return self.err("multiline strings are rejected");
        }
        let mut out = String::new();
        while let Some(byte) = self.peek() {
            if byte == b'"' {
                self.bump();
                return Ok(out);
            }
            if byte == b'\\' {
                self.bump();
                let escaped = self
                    .peek()
                    .ok_or_else(|| self.issue("supply.deny", "truncated escape"))?;
                self.bump();
                match escaped {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    _ => return self.err("unsupported string escape"),
                }
                continue;
            }
            if byte == b'\n' {
                return self.err("unterminated string");
            }
            let ch = self.utf8()?;
            out.push(ch);
        }
        self.err("unterminated string")
    }

    fn literal_string(&mut self) -> Result<String, Issue> {
        self.bump();
        if self.starts_with(b"''") {
            return self.err("multiline strings are rejected");
        }
        let start = self.index;
        while let Some(byte) = self.peek() {
            if byte == b'\'' {
                let text = std::str::from_utf8(&self.bytes[start..self.index])
                    .map_err(|_| self.issue("supply.deny", "literal string is not utf-8"))?
                    .to_owned();
                self.bump();
                return Ok(text);
            }
            if byte == b'\n' {
                return self.err("unterminated literal string");
            }
            self.bump();
        }
        self.err("unterminated literal string")
    }

    fn key(&mut self) -> Result<String, Issue> {
        self.skip();
        if self.peek() == Some(b'"') {
            return self.basic_string();
        }
        if self.peek() == Some(b'\'') {
            return self.literal_string();
        }
        let start = self.index;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            self.bump();
        }
        if start == self.index {
            return self.err("expected a key");
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index])
            .map_err(|_| self.issue("supply.deny", "key is not utf-8"))?;
        Ok(text.to_owned())
    }

    fn skip(&mut self) {
        loop {
            while self
                .peek()
                .is_some_and(|byte| byte == b' ' || byte == b'\t' || byte == b'\r')
            {
                self.bump();
            }
            if self.peek() == Some(b'\n') {
                self.bump();
                continue;
            }
            if self.peek() == Some(b'#') {
                while self.peek().is_some_and(|byte| byte != b'\n') {
                    self.bump();
                }
                continue;
            }
            break;
        }
    }

    fn utf8(&mut self) -> Result<char, Issue> {
        let rest = &self.bytes[self.index..];
        let text =
            std::str::from_utf8(rest).map_err(|_| self.issue("supply.deny", "invalid utf-8"))?;
        let ch = text
            .chars()
            .next()
            .ok_or_else(|| self.issue("supply.deny", "truncated utf-8"))?;
        for _ in 0..ch.len_utf8() {
            self.bump();
        }
        Ok(ch)
    }

    fn consume(&mut self, needle: &[u8]) -> bool {
        if self.starts_with(needle) {
            for _ in needle {
                self.bump();
            }
            true
        } else {
            false
        }
    }

    fn starts_with(&self, needle: &[u8]) -> bool {
        self.bytes[self.index..].starts_with(needle)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn bump(&mut self) {
        if self.bytes.get(self.index) == Some(&b'\n') {
            self.line += 1;
        }
        self.index += 1;
    }

    fn err<T>(&self, message: &str) -> Result<T, Issue> {
        Err(self.issue("supply.deny", &format!("line {}: {message}", self.line)))
    }

    fn issue(&self, code: &'static str, message: &str) -> Issue {
        Issue::new(code, "ci/deny.toml", message)
    }
}

pub const REVIEWED_LICENSES: &[&str] = &[
    "Apache-2.0",
    "BSD-3-Clause",
    "ISC",
    "MIT",
    "MIT-0",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
];

pub const CONFIDENCE_FLOOR_MILLI: u32 = 800;

pub fn deny_policy_issues(text: &str) -> Vec<Issue> {
    let value = match parse_deny_toml(text) {
        Ok(value) => value,
        Err(issue) => return vec![issue],
    };
    let mut issues = reviewed_shape_issues(&value);
    if find_key(&value, "clarify").is_some() {
        issues.push(Issue::new(
            "supply.clarify",
            "ci/deny.toml",
            "an unreviewed clarify block is not allowed",
        ));
    }
    if find_key(&value, "exceptions").is_some() {
        issues.push(Issue::new(
            "supply.exception",
            "ci/deny.toml",
            "a license exception table is not allowed",
        ));
    }
    match value.pointer("/advisories/ignore") {
        None => {}
        Some(Value::Array(items)) if items.is_empty() => {}
        Some(_) => issues.push(Issue::new(
            "supply.advisory_ignore",
            "ci/deny.toml",
            "advisories.ignore must be absent or empty",
        )),
    }
    let Some(allow) = value.pointer("/licenses/allow").and_then(Value::as_array) else {
        issues.push(Issue::new(
            "supply.license",
            "ci/deny.toml",
            "licenses.allow is missing",
        ));
        return issues;
    };
    let mut seen = Vec::new();
    for item in allow {
        let Some(name) = item.as_str() else {
            issues.push(Issue::new(
                "supply.license",
                "ci/deny.toml",
                "licenses.allow contains a non-string",
            ));
            continue;
        };
        if name.contains('*') || name == "*" {
            issues.push(Issue::new(
                "supply.license",
                "ci/deny.toml",
                "a wildcard license allow is rejected",
            ));
        }
        let lower = name.to_ascii_lowercase();
        for banned in ["licenseref", "agpl", "busl", "sspl", "mpl-2.0", "lgpl"] {
            if lower.contains(banned) {
                issues.push(Issue::new(
                    "supply.license",
                    "ci/deny.toml",
                    format!("allow list contains {name}"),
                ));
            }
        }
        if seen.iter().any(|item: &String| item == name) {
            issues.push(Issue::new(
                "supply.license",
                "ci/deny.toml",
                format!("duplicate allow entry {name}"),
            ));
        }
        seen.push(name.to_owned());
    }
    let mut left: Vec<&str> = seen.iter().map(String::as_str).collect();
    left.sort_unstable();
    let mut right = REVIEWED_LICENSES.to_vec();
    right.sort_unstable();
    if left != right {
        issues.push(Issue::new(
            "supply.license",
            "ci/deny.toml",
            "licenses.allow is not the reviewed set",
        ));
    }
    match value.pointer("/licenses/confidence-threshold") {
        Some(Value::String(text)) => match decimal_milli(text) {
            Some(milli) if milli >= CONFIDENCE_FLOOR_MILLI => {}
            Some(milli) => issues.push(Issue::new(
                "supply.confidence",
                "ci/deny.toml",
                format!("confidence-threshold {milli} milli is below 800"),
            )),
            None => issues.push(Issue::new(
                "supply.confidence",
                "ci/deny.toml",
                "confidence-threshold is not a decimal",
            )),
        },
        _ => issues.push(Issue::new(
            "supply.confidence",
            "ci/deny.toml",
            "confidence-threshold is missing",
        )),
    }
    issues
}

fn reviewed_shape_issues(value: &Value) -> Vec<Issue> {
    let mut issues = Vec::new();
    let Some(root) = value.as_object() else {
        issues.push(shape("deny config must be a table"));
        return issues;
    };
    unknown_keys(
        root,
        &["graph", "advisories", "licenses", "bans", "sources"],
        &mut issues,
    );
    let graph = object_at(root, "graph", &mut issues);
    if let Some(graph) = graph {
        unknown_keys(graph, &["all-features", "no-default-features"], &mut issues);
        expect_bool(graph, "all-features", false, &mut issues);
        expect_bool(graph, "no-default-features", false, &mut issues);
    }
    if let Some(advisories) = object_at(root, "advisories", &mut issues) {
        unknown_keys(
            advisories,
            &["yanked", "unmaintained", "unsound", "ignore"],
            &mut issues,
        );
        expect_str(advisories, "yanked", "deny", &mut issues);
        expect_str(advisories, "unmaintained", "all", &mut issues);
        expect_str(advisories, "unsound", "all", &mut issues);
        match advisories.get("ignore") {
            Some(Value::Array(items)) if items.is_empty() => {}
            Some(_) => issues.push(shape("advisories.ignore must be an empty array")),
            None => issues.push(shape("advisories.ignore is missing")),
        }
    }
    if let Some(licenses) = object_at(root, "licenses", &mut issues) {
        unknown_keys(
            licenses,
            &["confidence-threshold", "allow", "unused-allowed-license"],
            &mut issues,
        );
        expect_str(licenses, "unused-allowed-license", "warn", &mut issues);
        match licenses.get("allow") {
            Some(Value::Array(items)) if items.iter().all(Value::is_string) => {}
            Some(_) => issues.push(shape("licenses.allow must be an array of strings")),
            None => issues.push(shape("licenses.allow is missing")),
        }
    }
    if let Some(bans) = object_at(root, "bans", &mut issues) {
        unknown_keys(
            bans,
            &[
                "multiple-versions",
                "wildcards",
                "allow-wildcard-paths",
                "highlight",
            ],
            &mut issues,
        );
        expect_str(bans, "multiple-versions", "warn", &mut issues);
        expect_str(bans, "wildcards", "deny", &mut issues);
        expect_bool(bans, "allow-wildcard-paths", true, &mut issues);
        expect_str(bans, "highlight", "all", &mut issues);
    }
    if let Some(sources) = object_at(root, "sources", &mut issues) {
        unknown_keys(
            sources,
            &["unknown-registry", "unknown-git", "allow-registry"],
            &mut issues,
        );
        expect_str(sources, "unknown-registry", "deny", &mut issues);
        expect_str(sources, "unknown-git", "deny", &mut issues);
        match sources.get("allow-registry") {
            Some(Value::Array(items))
                if items.len() == 1
                    && items[0].as_str()
                        == Some("https://github.com/rust-lang/crates.io-index") => {}
            Some(_) => issues.push(shape(
                "sources.allow-registry must be exactly the crates.io index URL",
            )),
            None => issues.push(shape("sources.allow-registry is missing")),
        }
    }
    issues
}

fn shape(message: &str) -> Issue {
    Issue::new("supply.deny", "ci/deny.toml", message)
}

fn object_at<'a>(
    root: &'a Map<String, Value>,
    key: &str,
    issues: &mut Vec<Issue>,
) -> Option<&'a Map<String, Value>> {
    match root.get(key) {
        Some(Value::Object(map)) => Some(map),
        Some(_) => {
            issues.push(shape(&format!("{key} must be a table")));
            None
        }
        None => {
            issues.push(shape(&format!("{key} is missing")));
            None
        }
    }
}

fn unknown_keys(map: &Map<String, Value>, allowed: &[&str], issues: &mut Vec<Issue>) {
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            issues.push(shape(&format!("unreviewed key {key}")));
        }
    }
}

fn expect_str(map: &Map<String, Value>, key: &str, expected: &str, issues: &mut Vec<Issue>) {
    match map.get(key).and_then(Value::as_str) {
        Some(value) if value == expected => {}
        Some(value) => issues.push(shape(&format!(
            "{key} is {value}, reviewed value is {expected}"
        ))),
        None => issues.push(shape(&format!("{key} must be the string {expected}"))),
    }
}

fn expect_bool(map: &Map<String, Value>, key: &str, expected: bool, issues: &mut Vec<Issue>) {
    match map.get(key).and_then(Value::as_bool) {
        Some(value) if value == expected => {}
        Some(value) => issues.push(shape(&format!(
            "{key} is {value}, reviewed value is {expected}"
        ))),
        None => issues.push(shape(&format!("{key} must be boolean {expected}"))),
    }
}

fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            for child in map.values() {
                if let Some(found) = find_key(child, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            for child in items {
                if let Some(found) = find_key(child, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn decimal_milli(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.starts_with('-') || text.starts_with('+') {
        return None;
    }
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let whole: u32 = whole.parse().ok()?;
    let mut digits = frac.to_owned();
    if digits.len() > 3 {
        return None;
    }
    while digits.len() < 3 {
        digits.push('0');
    }
    let frac: u32 = if digits.is_empty() {
        0
    } else {
        digits.parse().ok()?
    };
    whole.checked_mul(1000)?.checked_add(frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_does_not_populate_the_allow_list() {
        let text = "# Apache-2.0 MIT BSD-3-Clause Unicode-3.0 Zlib\n[licenses]\nallow = [\"*\"]\nconfidence-threshold = 0.8\n[advisories]\nignore = []\n";
        let issues = deny_policy_issues(text);
        assert!(
            issues.iter().any(|issue| issue.message.contains("wildcard")
                || issue.message.contains("reviewed set")),
            "{issues:?}"
        );
    }

    #[test]
    fn exception_and_low_confidence_fail() {
        let text = "[licenses]\nconfidence-threshold = 0.0\nallow = [\"Apache-2.0\", \"MIT\", \"ISC\", \"BSD-3-Clause\", \"Unicode-3.0\", \"Zlib\", \"Unlicense\", \"MIT-0\"]\nexceptions = [{ allow = [\"Unlicense\"] }]\n[advisories]\nignore = [\"RUSTSEC-0000-0000\"]\n";
        let issues = deny_policy_issues(text);
        assert!(issues.iter().any(|issue| issue.code == "supply.exception"));
        assert!(issues.iter().any(|issue| issue.code == "supply.confidence"));
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "supply.advisory_ignore")
        );
    }

    #[test]
    fn duplicate_key_fails() {
        let text =
            "[licenses]\nconfidence-threshold = 0.8\nconfidence-threshold = 0.0\nallow = []\n";
        let error = parse_deny_toml(text).unwrap_err();
        assert!(error.message.contains("duplicate"));
    }

    #[test]
    fn reviewed_shape_rejects_unknown_keys_and_weakened_values() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ci/deny.toml");
        let text = std::fs::read_to_string(path).expect("deny.toml");
        let clean = deny_policy_issues(&text);
        assert!(
            clean.is_empty(),
            "{}",
            clean
                .iter()
                .map(|issue| format!("{} {}", issue.code, issue.message))
                .collect::<Vec<_>>()
                .join("; ")
        );
        let weakened = text
            .replace("wildcards = \"deny\"", "wildcards = \"allow\"")
            .replace(
                "unknown-registry = \"deny\"",
                "unknown-registry = \"allow\"",
            )
            .replace("unknown-git = \"deny\"", "unknown-git = \"warn\"");
        let issues = deny_policy_issues(&weakened);
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("wildcards"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("unknown-registry"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("unknown-git"))
        );
        let extra = text.replace(
            "[bans]\n",
            "[bans]\nskip-tree = [\"target\"]\nprivate = true\n",
        );
        let issues = deny_policy_issues(&extra);
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("unreviewed key skip-tree"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("unreviewed key private"))
        );
        let git = text.replace(
            "allow-registry = [\"https://github.com/rust-lang/crates.io-index\"]\n",
            "allow-registry = [\"https://example.invalid/index\"]\nallow-git = [\"https://example.invalid/repo\"]\n",
        );
        let issues = deny_policy_issues(&git);
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("allow-registry"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("unreviewed key allow-git"))
        );
    }
}
