//! Caller-supplied bearer token.
//!
//! `Debug` and `Display` are redacted. This slice does not link `secrecy` or
//! `zeroize`, and dropping the value does not wipe process memory.

use std::fmt;

use reqwest::header::HeaderValue;

use crate::error::{LocalError, QueryError};

const MAX_TOKEN_BYTES: usize = 4_096;

#[derive(Clone)]
pub struct BearerToken {
    header: HeaderValue,
}

impl BearerToken {
    pub fn new(token: &str) -> Result<Self, QueryError> {
        if token.is_empty()
            || token.len() > MAX_TOKEN_BYTES
            || !token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        {
            return Err(QueryError::Local(LocalError::Token));
        }
        let mut text = String::from("Bearer ");
        text.push_str(token);
        let header =
            HeaderValue::from_str(&text).map_err(|_| QueryError::Local(LocalError::Token))?;
        Ok(Self { header })
    }

    pub(crate) fn authorization(&self) -> HeaderValue {
        self.header.clone()
    }

    pub(crate) fn secret(&self) -> &str {
        self.header
            .to_str()
            .unwrap_or("")
            .strip_prefix("Bearer ")
            .unwrap_or("")
    }
}

impl fmt::Debug for BearerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BearerToken([redacted])")
    }
}

impl fmt::Display for BearerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BearerToken([redacted])")
    }
}
