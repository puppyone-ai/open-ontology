use serde_json::Value;
use tokio_postgres::config::SslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::config::{Config, PgAuth};
use crate::error::{csrf, malformed, unavailable, unauthenticated, Fault};
use crate::httputil::{header, status_line};
use crate::sha::{hex_encode, sha256};
use crate::tlsutil::{https_exchange, pg_tls_config};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Current {
    pub subject: String,
    pub csrf_token: String,
    pub project_id: String,
    pub role: String,
    pub data_grants: String,
    pub install_state: String,
}

pub async fn fetch_current(config: &Config, cookie: &str) -> Result<Current, Fault> {
    let origin = config.identity_origin.trim_end_matches('/');
    let rest = origin.strip_prefix("https://").ok_or_else(|| malformed("identity"))?;
    let (host, port) = split_host_port(rest)?;
    let request = format!(
        "GET /control/v1/identity/current HTTP/1.1\r\nhost: {host}:{port}\r\ncookie: __Host-oo-session={cookie}\r\naccept: application/json\r\nconnection: close\r\n\r\n"
    );
    let bytes = https_exchange(host, port, &config.identity_server_name, config.identity_ca_pem.as_bytes(), request.as_bytes()).await?;
    let (status, headers, body) = status_line(&bytes).ok_or_else(|| unavailable("identity"))?;
    if status == 401 {
        return Err(unauthenticated());
    }
    if status != 200 {
        return Err(unavailable("identity"));
    }
    if header(&headers, "content-type").is_none_or(|value| !value.starts_with("application/json")) {
        return Err(unavailable("identity"));
    }
    parse_current(&body)
}

fn parse_current(body: &[u8]) -> Result<Current, Fault> {
    let value: Value = serde_json::from_slice(body).map_err(|_| unavailable("identity"))?;
    if value.get("data_grants").and_then(Value::as_str) != Some("none") {
        return Err(malformed("data_grants"));
    }
    if value.get("authority").and_then(Value::as_str) != Some("management_session") {
        return Err(malformed("authority"));
    }
    let subject = text(&value, "subject")?;
    let csrf_token = text(&value, "csrf_token")?;
    let install_state = text(&value, "install_state")?;
    if csrf_token.len() < 20 || csrf_token.len() > 128 {
        return Err(malformed("csrf"));
    }
    let projects = value.get("projects").and_then(Value::as_array).ok_or_else(|| malformed("projects"))?;
    let mut matched = None;
    for project in projects {
        let project_id = project.get("project_id").and_then(Value::as_str).unwrap_or("");
        let role = project.get("role").and_then(Value::as_str).unwrap_or("");
        if project_id == "shop" {
            if matched.is_some() {
                return Err(malformed("projects"));
            }
            matched = Some(role.to_owned());
        }
    }
    let role = matched.ok_or_else(|| malformed("projects"))?;
    Ok(Current { subject, csrf_token, project_id: "shop".to_owned(), role, data_grants: "none".to_owned(), install_state })
}

fn text(value: &Value, key: &str) -> Result<String, Fault> {
    value.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(|| malformed("identity"))
}

fn split_host_port(rest: &str) -> Result<(&str, u16), Fault> {
    let (host, port) = rest.rsplit_once(':').ok_or_else(|| malformed("identity"))?;
    let port = port.parse().map_err(|_| malformed("identity"))?;
    if host.is_empty() || port == 0 {
        return Err(malformed("identity"));
    }
    Ok((host, port))
}

pub async fn confirm_presented_csrf(config: &Config, cookie: &str, presented: &str) -> Result<(), Fault> {
    if !token_ok(presented) {
        return Err(csrf());
    }
    let raw = b64url_32(cookie).ok_or_else(unauthenticated)?;
    let cookie_hash = hex_encode(&sha256(&raw));
    let presented_hash = hex_encode(&sha256(presented.as_bytes()));
    let Some(pg) = config.identity_pg.as_ref() else {
        return Err(unavailable("identity_pg"));
    };
    let stored = read_csrf_hash(pg, &cookie_hash).await?;
    if !same_hash(&presented_hash, &stored) {
        return Err(csrf());
    }
    Ok(())
}

async fn read_csrf_hash(pg: &PgAuth, cookie_hash: &str) -> Result<String, Fault> {
    let tls = MakeRustlsConnect::new(pg_tls_config(pg.ca_pem.as_bytes())?);
    let mut config = tokio_postgres::Config::new();
    config.user(&pg.user);
    config.password(&pg.password);
    config.host(&pg.host);
    config.port(pg.port);
    config.dbname(&pg.database);
    config.ssl_mode(SslMode::Require);
    let (client, connection) = config.connect(tls).await.map_err(|_| unavailable("identity_pg"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let row = client
        .query_opt(
            "SELECT csrf_hash FROM oo_identity_control.session WHERE cookie_hash = $1",
            &[&cookie_hash],
        )
        .await
        .map_err(|_| unavailable("identity_pg"))?;
    let Some(row) = row else {
        return Err(unauthenticated());
    };
    Ok(row.get(0))
}

fn token_ok(value: &str) -> bool {
    value.len() == 43 && value.bytes().all(b64url_byte)
}

fn b64url_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

fn b64url_32(value: &str) -> Option<[u8; 32]> {
    if !token_ok(value) {
        return None;
    }
    let mut alphabet = [0_u8; 128];
    for (index, symbol) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_".iter().enumerate() {
        alphabet[usize::from(*symbol)] = u8::try_from(index).ok()?;
    }
    let bytes = value.as_bytes();
    let mut raw = [0_u8; 32];
    let mut out = 0;
    let mut index = 0;
    while index + 4 <= 40 {
        let chunk = ((u32::from(alphabet[usize::from(bytes[index])])) << 18)
            | ((u32::from(alphabet[usize::from(bytes[index + 1])])) << 12)
            | ((u32::from(alphabet[usize::from(bytes[index + 2])])) << 6)
            | u32::from(alphabet[usize::from(bytes[index + 3])]);
        raw[out] = u8::try_from((chunk >> 16) & 0xff).ok()?;
        raw[out + 1] = u8::try_from((chunk >> 8) & 0xff).ok()?;
        raw[out + 2] = u8::try_from(chunk & 0xff).ok()?;
        out += 3;
        index += 4;
    }
    let chunk = ((u32::from(alphabet[usize::from(bytes[40])])) << 18)
        | ((u32::from(alphabet[usize::from(bytes[41])])) << 12)
        | ((u32::from(alphabet[usize::from(bytes[42])])) << 6);
    raw[30] = u8::try_from((chunk >> 16) & 0xff).ok()?;
    raw[31] = u8::try_from((chunk >> 8) & 0xff).ok()?;
    Some(raw)
}

fn same_hash(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (lhs, rhs) in left.bytes().zip(right.bytes()) {
        diff |= lhs ^ rhs;
    }
    diff == 0
}
