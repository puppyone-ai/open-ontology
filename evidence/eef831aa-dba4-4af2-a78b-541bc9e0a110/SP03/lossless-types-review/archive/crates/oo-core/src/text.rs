//! String newtypes whose JSON Schema pattern and deserializer accept the same
//! language. Cross-field checks that need another field live outside this module.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::schema_util::string_schema;

macro_rules! string_newtype {
    ($name:ident, $min:expr, $max:expr, $pattern:expr, $check:path) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                if $check(&value) {
                    Ok(Self(value))
                } else {
                    Err(format!("invalid {}", stringify!($name)))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <String as serde::Deserialize>::deserialize(deserializer)?;
                $check(&value)
                    .then_some(Self(value))
                    .ok_or_else(|| serde::de::Error::custom(concat!("invalid ", stringify!($name))))
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(stringify!($name))
            }

            fn inline_schema() -> bool {
                true
            }

            fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                $crate::schema_util::string_schema($pattern, $min, $max)
            }
        }

        impl utoipa::PartialSchema for $name {
            fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                utoipa::openapi::schema::ObjectBuilder::new()
                    .schema_type(utoipa::openapi::schema::Type::String)
                    .pattern(Some($pattern))
                    .min_length(Some($min as usize))
                    .max_length(Some($max as usize))
                    .into()
            }
        }

        impl utoipa::ToSchema for $name {}
    };
}

pub(crate) use string_newtype as string_newtype_export;

fn matches_source_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

fn matches_connector(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
}

fn matches_table(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn matches_secret(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("secret://") else {
        return false;
    };
    let bytes = rest.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'.' | b'_' | b'-'))
}

fn matches_timezone(value: &str) -> bool {
    if value == "UTC" || value == "Etc/UTC" {
        return true;
    }
    let mut parts = value.split('/');
    let Some(first) = parts.next() else {
        return false;
    };
    if parts.clone().count() == 0 || parts.clone().count() > 2 {
        return false;
    }
    let ok_first = {
        let bytes = first.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 32
            && bytes[0].is_ascii_alphabetic()
            && bytes[1..]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'_' | b'+' | b'-'))
    };
    ok_first
        && parts.all(|part| {
            let bytes = part.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 32
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'_' | b'+' | b'-'))
        })
}

fn matches_locale(value: &str) -> bool {
    let bytes = value.as_bytes();
    match bytes.len() {
        2 => bytes.iter().all(|b| b.is_ascii_lowercase()),
        5 => {
            bytes[2] == b'-'
                && bytes[..2].iter().all(|b| b.is_ascii_lowercase())
                && bytes[3..].iter().all(|b| b.is_ascii_uppercase())
        }
        _ => false,
    }
}

fn matches_host(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 253 {
        return false;
    }
    bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'.' | b'-'))
        && bytes.last().is_some_and(|b| b.is_ascii_alphanumeric())
}

fn matches_database(value: &str) -> bool {
    let count = value.chars().count();
    (1..=64).contains(&count)
        && value
            .chars()
            .all(|ch| !ch.is_control() && !ch.is_whitespace() && ch != '/')
}

fn matches_glob(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 128
        && !value
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch == '\0')
        && !value.contains("..")
}

fn matches_model_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'.' | b'_' | b':' | b'-'))
}

fn matches_version_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'.' | b'_' | b'+' | b'-'))
}

fn matches_plugin(value: &str) -> bool {
    matches_connector(value)
}

fn matches_deployment(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn matches_window(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 11 || bytes[5] != b'-' || bytes[2] != b':' || bytes[8] != b':' {
        return false;
    }
    valid_hhmm(&bytes[0..5]) && valid_hhmm(&bytes[6..11])
}

fn valid_hhmm(bytes: &[u8]) -> bool {
    bytes.len() == 5
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b':'
        && bytes[3].is_ascii_digit()
        && bytes[4].is_ascii_digit()
        && (bytes[0] == b'0' || bytes[0] == b'1' || (bytes[0] == b'2' && bytes[1] <= b'3'))
        && bytes[3] <= b'5'
}

fn matches_https_url(value: &str) -> bool {
    parse_https_endpoint(value).is_ok()
}

/// HTTPS endpoint for object-store and model base URLs.
///
/// Query, fragment, and userinfo are rejected. The path may not carry the
/// explicit credential spellings `password=`, `token=`, `secret=`, `api_key=`,
/// or `access_key=`. Other path text is not scanned for PII. Errors from
/// [`HttpsUrl::new`] do not include the input.
fn parse_https_endpoint(value: &str) -> Result<(), ()> {
    let Some(rest) = value.strip_prefix("https://") else {
        return Err(());
    };
    if rest.is_empty()
        || rest.len() > 2040
        || rest
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == 0)
        || rest.contains('@')
        || rest.contains('?')
        || rest.contains('#')
    {
        return Err(());
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, Some(path)),
        None => (rest, None),
    };
    parse_https_authority(authority)?;
    if let Some(path) = path {
        if path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
        {
            return Err(());
        }
        let lower = path.to_ascii_lowercase();
        for needle in ["password=", "token=", "secret=", "api_key=", "access_key="] {
            if lower.contains(needle) {
                return Err(());
            }
        }
    }
    Ok(())
}

fn parse_https_authority(authority: &str) -> Result<(), ()> {
    if authority.is_empty() {
        return Err(());
    }
    if let Some(body) = authority.strip_prefix('[') {
        let Some((ip, rest)) = body.split_once(']') else {
            return Err(());
        };
        if !ipv6_literal(ip) {
            return Err(());
        }
        if rest.is_empty() {
            return Ok(());
        }
        let Some(port) = rest.strip_prefix(':') else {
            return Err(());
        };
        return port_ok(port);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            (host, Some(port))
        }
        _ => (authority, None),
    };
    if !dns_or_ipv4(host) {
        return Err(());
    }
    if let Some(port) = port {
        port_ok(port)?;
    }
    Ok(())
}

fn port_ok(text: &str) -> Result<(), ()> {
    if text.is_empty() || text.len() > 5 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    let port: u32 = text.parse().map_err(|_| ())?;
    (1..=65_535).contains(&port).then_some(()).ok_or(())
}

fn dns_or_ipv4(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.is_empty()
        || labels
            .iter()
            .any(|label| label.is_empty() || label.len() > 63)
    {
        return false;
    }
    if labels.len() == 4
        && labels
            .iter()
            .all(|label| label.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return labels.iter().all(|label| {
            if *label == "0" {
                return true;
            }
            !label.starts_with('0') && label.parse::<u16>().is_ok_and(|octet| octet <= 255)
        });
    }
    labels.iter().all(|label| {
        let bytes = label.as_bytes();
        bytes[0].is_ascii_alphanumeric()
            && bytes
                .last()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
    })
}

fn ipv6_literal(text: &str) -> bool {
    if text.is_empty() || text.len() > 45 || text.contains(":::") {
        return false;
    }
    let (head, tail) = match text.split_once("::") {
        Some((head, tail)) => (head, Some(tail)),
        None => (text, None),
    };
    let mut groups = 0usize;
    if !head.is_empty() {
        for group in head.split(':') {
            if !ipv6_group(group) {
                return false;
            }
            groups += 1;
        }
    }
    if let Some(tail) = tail {
        if !tail.is_empty() {
            let parts: Vec<&str> = tail.split(':').collect();
            if parts.last().is_some_and(|part| part.contains('.')) {
                let ipv4 = parts.last().copied().unwrap_or("");
                if !dns_or_ipv4(ipv4) || ipv4.split('.').count() != 4 {
                    return false;
                }
                for group in &parts[..parts.len() - 1] {
                    if !ipv6_group(group) {
                        return false;
                    }
                    groups += 1;
                }
                groups += 2;
            } else {
                for group in parts {
                    if !ipv6_group(group) {
                        return false;
                    }
                    groups += 1;
                }
            }
        }
        groups < 8
    } else {
        groups == 8
    }
}

fn ipv6_group(text: &str) -> bool {
    (1..=4).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn matches_relative_path(value: &str) -> bool {
    if value.is_empty() || value.len() > 256 {
        return false;
    }
    let body = value.strip_suffix('/').unwrap_or(value);
    !body.is_empty()
        && body.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
        })
}

fn matches_schema_files(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("files_") else {
        return false;
    };
    !rest.is_empty()
        && rest.len() <= 57
        && rest
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && matches_source_name(value)
}

fn matches_query_id(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("q_") else {
        return false;
    };
    let len = rest.len();
    (6..=40).contains(&len) && rest.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn matches_operation_id(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("op_") else {
        return false;
    };
    let len = rest.len();
    (8..=64).contains(&len) && rest.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn matches_pass_rule(value: &str) -> bool {
    if value == "pass" {
        return true;
    }
    let Some(rest) = value.strip_prefix("pass^") else {
        return false;
    };
    let bytes = rest.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 3
        && bytes[0] != b'0'
        && bytes.iter().all(|b| b.is_ascii_digit())
}

fn matches_hex_digest(value: &str) -> bool {
    let bare = value.strip_prefix("sha256:").unwrap_or(value);
    bare.len() == 64
        && bare
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn matches_idp_template(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("{{ user.attributes.") else {
        return false;
    };
    let Some(field) = rest.strip_suffix(" }}") else {
        return false;
    };
    let bytes = field.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn matches_free_label(value: &str) -> bool {
    let count = value.chars().count();
    (1..=256).contains(&count)
        && value.chars().all(|ch| {
            let code = u32::from(ch);
            !matches!(code, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F)
        })
}

fn matches_bounded_text(value: &str, max_chars: usize) -> bool {
    !value.chars().any(|ch| ch == '\0') && value.chars().count() <= max_chars
}

string_newtype!(
    SourceName,
    1,
    63,
    r"^[a-z][a-z0-9_]{0,62}$",
    matches_source_name
);
string_newtype!(
    ConnectorName,
    1,
    64,
    r"^[a-z][a-z0-9_-]{0,63}$",
    matches_connector
);
string_newtype!(
    TableName,
    1,
    128,
    r"^[A-Za-z_][A-Za-z0-9_]{0,127}$",
    matches_table
);
string_newtype!(
    SecretRef,
    10,
    137,
    r"^secret://[A-Za-z][A-Za-z0-9._-]{0,127}$",
    matches_secret
);
string_newtype!(
    TimezoneName,
    3,
    98,
    r"^(?:UTC|Etc/UTC|[A-Za-z][A-Za-z0-9_+-]{0,31}(?:/[A-Za-z0-9_+-]{1,32}){1,2})$",
    matches_timezone
);
string_newtype!(
    LocaleCode,
    2,
    5,
    r"^[a-z]{2}(?:-[A-Z]{2})?$",
    matches_locale
);
string_newtype!(
    HostName,
    1,
    253,
    r"^[A-Za-z0-9](?:[A-Za-z0-9.-]{0,251}[A-Za-z0-9])?$",
    matches_host
);
string_newtype!(
    DatabaseName,
    1,
    64,
    r"^[^\p{Cc}\s/]{1,64}$",
    matches_database
);
string_newtype!(
    GlobPattern,
    1,
    128,
    r"^(?!.*\.\.)[^\u{0}/\\]{1,128}$",
    matches_glob
);
string_newtype!(
    ModelId,
    1,
    128,
    r"^[A-Za-z0-9._:-]{1,128}$",
    matches_model_id
);
string_newtype!(
    VersionToken,
    1,
    64,
    r"^[A-Za-z0-9._+-]{1,64}$",
    matches_version_token
);
string_newtype!(
    PluginName,
    1,
    64,
    r"^[a-z][a-z0-9_-]{0,63}$",
    matches_plugin
);
string_newtype!(
    DeploymentName,
    1,
    63,
    r"^[a-z][a-z0-9-]{0,62}$",
    matches_deployment
);
string_newtype!(
    DayWindow,
    11,
    11,
    r"^(?:[01][0-9]|2[0-3]):[0-5][0-9]-(?:[01][0-9]|2[0-3]):[0-5][0-9]$",
    matches_window
);
string_newtype!(
    HttpsUrl,
    9,
    2048,
    r"^https://(?:\[[0-9A-Fa-f:.]+\]|[A-Za-z0-9](?:[A-Za-z0-9.-]{0,251}[A-Za-z0-9])?)(?::[0-9]{1,5})?(?:/[^\s?#@]*)?$",
    matches_https_url
);
string_newtype!(
    RelativePath,
    1,
    256,
    r"^(?!.*(?:^|/)\.\.?(?:/|$))(?:[A-Za-z0-9._+-]+(?:/[A-Za-z0-9._+-]+)*)/?$",
    matches_relative_path
);
string_newtype!(
    FilesSchemaName,
    7,
    63,
    r"^files_[a-z0-9_]{1,57}$",
    matches_schema_files
);
string_newtype!(QueryId, 8, 42, r"^q_[0-9A-Za-z]{6,40}$", matches_query_id);
string_newtype!(
    OperationId,
    11,
    67,
    r"^op_[0-9A-Za-z]{8,64}$",
    matches_operation_id
);
string_newtype!(
    PassRule,
    4,
    8,
    r"^pass(?:\^[1-9][0-9]{0,2})?$",
    matches_pass_rule
);
string_newtype!(
    HexDigest,
    64,
    71,
    r"^(?:sha256:)?[0-9a-f]{64}$",
    matches_hex_digest
);
string_newtype!(
    IdpTemplate,
    24,
    90,
    r"^\{\{ user\.attributes\.[A-Za-z_][A-Za-z0-9_]{0,63} \}\}$",
    matches_idp_template
);
string_newtype!(
    ClaimName,
    1,
    64,
    r"^[A-Za-z_][A-Za-z0-9_]{0,63}$",
    matches_claim_name
);
string_newtype!(
    CharsetName,
    1,
    32,
    r"^[A-Za-z0-9_-]{1,32}$",
    matches_charset
);
string_newtype!(
    QualifiedTable,
    3,
    192,
    r"^[a-z][a-z0-9_]{0,62}\.[A-Za-z_][A-Za-z0-9_]{0,127}$",
    matches_qualified_table
);
string_newtype!(
    RowFilterExpr,
    1,
    2000,
    r"^[^;\u{0}`${}]{1,2000}$",
    matches_row_filter
);
string_newtype!(Delimiter, 1, 8, r"^[^\s\u{0}]{1,8}$", matches_delimiter);
string_newtype!(
    ColumnPath,
    5,
    321,
    r"^[a-z][a-z0-9_]{0,62}\.[A-Za-z_][A-Za-z0-9_]{0,127}\.[A-Za-z_][A-Za-z0-9_]{0,127}$",
    matches_column_path
);
string_newtype!(
    CharsetTarget,
    1,
    386,
    r"^[A-Za-z_][A-Za-z0-9_]{0,127}(?:\.[A-Za-z_][A-Za-z0-9_]{0,127}){0,2}$",
    matches_charset_target
);

fn matches_claim_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn matches_charset(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
}

fn matches_qualified_table(value: &str) -> bool {
    let Some((schema, table)) = value.split_once('.') else {
        return false;
    };
    !table.contains('.') && matches_source_name(schema) && matches_table(table)
}

fn matches_row_filter(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 2000
        && !value
            .chars()
            .any(|ch| matches!(ch, ';' | '\0' | '`' | '$' | '{' | '}'))
}

fn matches_delimiter(value: &str) -> bool {
    (1..=8).contains(&value.chars().count())
        && !value.chars().any(|ch| ch.is_whitespace() || ch == '\0')
}

fn matches_column_path(value: &str) -> bool {
    let mut parts = value.split('.');
    let Some(schema) = parts.next() else {
        return false;
    };
    let Some(table) = parts.next() else {
        return false;
    };
    let Some(column) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && matches_source_name(schema)
        && matches_table(table)
        && matches_table(column)
}

fn matches_charset_target(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    (1..=3).contains(&parts.len()) && parts.iter().all(|part| matches_table(part))
}

impl crate::pattern_map::PatternKey for SourceName {
    const PATTERN: &'static str = r"^[a-z][a-z0-9_]{0,62}$";
}
impl crate::pattern_map::PatternKey for TableName {
    const PATTERN: &'static str = r"^[A-Za-z_][A-Za-z0-9_]{0,127}$";
}
impl crate::pattern_map::PatternKey for PluginName {
    const PATTERN: &'static str = r"^[a-z][a-z0-9_-]{0,63}$";
}
impl crate::pattern_map::PatternKey for ClaimName {
    const PATTERN: &'static str = r"^[A-Za-z_][A-Za-z0-9_]{0,63}$";
}
impl crate::pattern_map::PatternKey for GlobPattern {
    const PATTERN: &'static str = r"^(?!.*\.\.)[^\u{0}/\\]{1,128}$";
}
impl crate::pattern_map::PatternKey for ColumnPath {
    const PATTERN: &'static str =
        r"^[a-z][a-z0-9_]{0,62}\.[A-Za-z_][A-Za-z0-9_]{0,127}\.[A-Za-z_][A-Za-z0-9_]{0,127}$";
}
impl crate::pattern_map::PatternKey for CharsetTarget {
    const PATTERN: &'static str =
        r"^[A-Za-z_][A-Za-z0-9_]{0,127}(?:\.[A-Za-z_][A-Za-z0-9_]{0,127}){0,2}$";
}
impl crate::pattern_map::PatternKey for QualifiedTable {
    const PATTERN: &'static str = r"^[a-z][a-z0-9_]{0,62}\.[A-Za-z_][A-Za-z0-9_]{0,127}$";
}
impl crate::pattern_map::PatternKey for VersionToken {
    const PATTERN: &'static str = r"^[A-Za-z0-9._+-]{1,64}$";
}

/// Non-empty label. Newlines are allowed. NUL and other controls are not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Label(String);

impl<'de> Deserialize<'de> for Label {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if matches_free_label(&value) {
            Ok(Self(value))
        } else {
            Err(serde::de::Error::custom("invalid Label"))
        }
    }
}

impl JsonSchema for Label {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Label")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        string_schema(r"^[^\u{0}-\u{8}\u{B}\u{C}\u{E}-\u{1F}\u{7F}]+$", 1, 256)
    }
}

impl PartialSchema for Label {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .pattern(Some(r"^[^\u{0}-\u{8}\u{B}\u{C}\u{E}-\u{1F}\u{7F}]+$"))
            .min_length(Some(1))
            .max_length(Some(256))
            .into()
    }
}

impl ToSchema for Label {}

impl Label {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if matches_free_label(&value) {
            Ok(Self(value))
        } else {
            Err("invalid Label".to_owned())
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Free text with a character cap and no NUL. The cap is part of both schema and serde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BoundedText<const MAX: usize>(String);

/// Error text cap. A named const cannot appear in a `ToSchema` field: utoipa
/// treats it as a type argument. The alias keeps the cap and the derive apart.
pub type ErrorText = BoundedText<{ crate::limits::ERROR_TEXT_MAX_CHARS }>;
/// Query SQL cap. `maxLength` is a codepoint ceiling. The byte budget is
/// `x-utf8-max-bytes` plus this constructor.
pub type QuerySql = ByteBoundedText<{ crate::limits::QUERY_SQL_MAX_BYTES }>;

impl<const MAX: usize> BoundedText<MAX> {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for BoundedText<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if matches_bounded_text(&value, MAX) {
            Ok(Self(value))
        } else {
            Err(serde::de::Error::custom(
                "text exceeds its budget or contains NUL",
            ))
        }
    }
}

impl<const MAX: usize> JsonSchema for BoundedText<MAX> {
    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("BoundedText{MAX}"))
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        crate::schema_util::schema_object(serde_json::json!({
            "type": "string",
            "maxLength": MAX,
            "pattern": r"^[^\u{0}]*$"
        }))
    }
}

impl<const MAX: usize> utoipa::__dev::ComposeSchema for BoundedText<MAX> {
    fn compose(
        _: Vec<utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>>,
    ) -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .pattern(Some(r"^[^\u{0}]*$"))
            .max_length(Some(MAX))
            .into()
    }
}

impl<const MAX: usize> ToSchema for BoundedText<MAX> {
    fn name() -> Cow<'static, str> {
        Cow::Owned(format!("BoundedText{MAX}"))
    }
}

/// Text bounded by UTF-8 bytes. JSON Schema `maxLength` is the same number and
/// is only a codepoint ceiling, because every scalar is at least one byte.
/// `x-utf8-max-bytes` and [`Self`] both enforce the byte count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ByteBoundedText<const MAX: usize>(String);

impl<const MAX: usize> ByteBoundedText<MAX> {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if byte_bounded(&value, MAX) {
            Ok(Self(value))
        } else {
            Err("text exceeds its UTF-8 byte budget or contains NUL".to_owned())
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const MAX: usize> AsRef<str> for ByteBoundedText<MAX> {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

fn byte_bounded(value: &str, max_bytes: usize) -> bool {
    !value.as_bytes().contains(&0) && value.len() <= max_bytes
}

impl<'de, const MAX: usize> Deserialize<'de> for ByteBoundedText<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl<const MAX: usize> JsonSchema for ByteBoundedText<MAX> {
    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("ByteBoundedText{MAX}"))
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        crate::schema_util::schema_object(serde_json::json!({
            "type": "string",
            "maxLength": MAX,
            "x-utf8-max-bytes": MAX,
            "pattern": r"^[^\u{0}]*$",
            "description": "maxLength is a Unicode scalar ceiling. x-utf8-max-bytes is the UTF-8 byte budget enforced by the custom keyword and by Rust."
        }))
    }
}

impl<const MAX: usize> utoipa::__dev::ComposeSchema for ByteBoundedText<MAX> {
    fn compose(
        _: Vec<utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>>,
    ) -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .pattern(Some(r"^[^\u{0}]*$"))
            .max_length(Some(MAX))
            .description(Some(
                "maxLength is a Unicode scalar ceiling. The UTF-8 byte budget is the same number and is enforced by Rust.",
            ))
            .into()
    }
}

impl<const MAX: usize> ToSchema for ByteBoundedText<MAX> {
    fn name() -> Cow<'static, str> {
        Cow::Owned(format!("ByteBoundedText{MAX}"))
    }
}

pub fn contains_forbidden_interpolation(value: &str) -> bool {
    value.contains("${")
        || value.contains("$(")
        || value.contains("{{")
        || value.contains("}}")
        || value.contains("%{")
        || value.contains('`')
        || value.contains('\0')
}

pub fn is_exact_idp_template(value: &str) -> bool {
    matches_idp_template(value)
}

/// Walk a JSON value. An exact IdP template is allowed only when `allow` says so.
/// Every other string rejects interpolation metacharacters and NUL.
pub fn audit_interpolation(
    value: &serde_json::Value,
    pointer: &str,
    allow_template: &impl Fn(&str) -> bool,
    contract: &str,
    out: &mut Vec<crate::diagnostic::Diagnostic>,
) {
    match value {
        serde_json::Value::String(text) => {
            if text.contains('\0') {
                out.push(crate::diagnostic::Diagnostic::error(
                    contract,
                    "interpolation",
                    pointer,
                    "NUL is not allowed in configuration text",
                ));
                return;
            }
            if allow_template(pointer) && is_exact_idp_template(text) {
                return;
            }
            if contains_forbidden_interpolation(text) {
                out.push(crate::diagnostic::Diagnostic::error(
                    contract,
                    "interpolation",
                    pointer,
                    "interpolation or an executable expression is only allowed as a whole IdP claim template",
                ));
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let next = format!("{pointer}/{index}");
                audit_interpolation(item, &next, allow_template, contract, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                if key.contains('\0') {
                    out.push(crate::diagnostic::Diagnostic::error(
                        contract,
                        "interpolation",
                        pointer,
                        "NUL is not allowed in field names",
                    ));
                }
                let next = format!("{pointer}/{key}");
                audit_interpolation(item, &next, allow_template, contract, out);
            }
        }
        _ => {}
    }
}

pub fn claim_template_pointer(pointer: &str) -> bool {
    let mut parts = pointer.split('/').skip(1);
    if parts.next() != Some("bindings") {
        return false;
    }
    let Some(index) = parts.next() else {
        return false;
    };
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if parts.next() != Some("claims") {
        return false;
    }
    let Some(claim) = parts.next() else {
        return false;
    };
    if claim.is_empty() || claim.contains('/') {
        return false;
    }
    match parts.next() {
        None => true,
        Some(item) => {
            !item.is_empty() && item.bytes().all(|b| b.is_ascii_digit()) && parts.next().is_none()
        }
    }
}
