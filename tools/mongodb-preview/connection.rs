//! Private endpoint input; secrets remain in the existing SecretRef resolver.
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth_database: String,
}
pub fn load() -> Result<Connection, String> {
    let path = std::env::var("OO_MONGO_CONNECTION").map_err(|_| "connection input absent")?;
    let bytes = std::fs::read(path).map_err(|_| "connection input unreadable")?;
    let value: Connection = serde_json::from_slice(&bytes).map_err(|_| "connection input invalid")?;
    if value.port == 0 || value.host.is_empty() || value.user.is_empty() || value.auth_database.is_empty()
        || value.host.chars().any(|c| c.is_whitespace() || "/?@#".contains(c)) {
        return Err("connection fields invalid".into());
    }
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_inline_secret_and_out_of_range_port() {
        assert!(serde_json::from_str::<Connection>(r#"{"host":"localhost","port":27017,"user":"reader","auth_database":"admin","password":"hidden"}"#).is_err());
        assert!(serde_json::from_str::<Connection>(r#"{"host":"localhost","port":65536,"user":"reader","auth_database":"admin"}"#).is_err());
    }
}
