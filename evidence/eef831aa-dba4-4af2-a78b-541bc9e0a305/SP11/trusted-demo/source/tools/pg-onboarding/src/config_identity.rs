//! Recompute `oo-pg-config-identity/v1` from non-secret request fields.
//!
//! Field lines are `key=value`. Table lines are `table`, schema, and table
//! separated by tabs. Password and CA bytes are not inputs. The five budget
//! numbers are the import CLI defaults used when the request omits `budget`.

use crate::sha256::sha256_hex;

pub const CONNECT_MS: u64 = 30_000;
pub const STATEMENT_MS: u64 = 60_000;
pub const LOCK_MS: u64 = 30_000;
pub const MAX_ROWS: u64 = 10_000;
pub const MAX_BYTES: u64 = 8_388_608;

pub struct IdentityInput<'a> {
    pub source_id: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub database: &'a str,
    pub username: &'a str,
    pub secret_ref: &'a str,
    pub tables: &'a [(&'a str, &'a str)],
}

pub fn config_identity_hex(
    source_id: &str,
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    secret_ref: &str,
    tables: &[(&str, &str)],
) -> String {
    sha256_hex(
        preimage(&IdentityInput {
            source_id,
            host,
            port,
            database,
            username,
            secret_ref,
            tables,
        })
        .as_bytes(),
    )
}

pub fn preimage(input: &IdentityInput<'_>) -> String {
    let mut body = String::from("oo-pg-config-identity/v1\n");
    push_line(&mut body, "source_id", input.source_id);
    push_line(&mut body, "host", input.host);
    push_line(&mut body, "port", &input.port.to_string());
    push_line(&mut body, "database", input.database);
    push_line(&mut body, "username", input.username);
    push_line(&mut body, "secret_ref", input.secret_ref);
    push_line(&mut body, "connect_ms", &CONNECT_MS.to_string());
    push_line(&mut body, "statement_ms", &STATEMENT_MS.to_string());
    push_line(&mut body, "lock_ms", &LOCK_MS.to_string());
    push_line(&mut body, "max_rows", &MAX_ROWS.to_string());
    push_line(&mut body, "max_bytes", &MAX_BYTES.to_string());
    for (schema, table) in input.tables {
        body.push_str("table\t");
        body.push_str(schema);
        body.push('\t');
        body.push_str(table);
        body.push('\n');
    }
    body
}

fn push_line(body: &mut String, key: &str, value: &str) {
    body.push_str(key);
    body.push('=');
    body.push_str(value);
    body.push('\n');
}
