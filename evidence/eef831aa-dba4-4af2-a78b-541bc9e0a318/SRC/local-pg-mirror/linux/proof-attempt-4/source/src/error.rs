use std::fmt;

/// Closed mirror failure. Display is a static code, a static fact, and an optional SQLSTATE.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MirrorError {
    pub code: &'static str,
    pub fact: &'static str,
    pub sqlstate: Option<[u8; 5]>,
}

impl MirrorError {
    pub fn new(code: &'static str, fact: &'static str) -> Self {
        Self {
            code,
            fact,
            sqlstate: None,
        }
    }

    pub fn with_sqlstate(code: &'static str, fact: &'static str, sqlstate: [u8; 5]) -> Self {
        Self {
            code,
            fact,
            sqlstate: Some(sqlstate),
        }
    }
}

impl fmt::Display for MirrorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.code, self.fact)?;
        if let Some(state) = self.sqlstate {
            let text = std::str::from_utf8(&state).unwrap_or("?????");
            write!(formatter, " {text}")?;
        }
        Ok(())
    }
}

impl std::error::Error for MirrorError {}

pub fn parse_sqlstate(raw: &str) -> Option<[u8; 5]> {
    let bytes = raw.as_bytes();
    if bytes.len() != 5 || !bytes.iter().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    let mut out = [0; 5];
    out.copy_from_slice(bytes);
    Some(out)
}
