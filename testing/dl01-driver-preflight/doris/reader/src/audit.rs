use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use serde_json::{json, Value as Json};

use crate::fail::Fail;

pub struct Audit {
    file: Mutex<File>,
    secrets: Vec<String>,
}

impl Audit {
    pub fn create(path: &Path, secrets: Vec<String>) -> Result<Self, Fail> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| Fail::new("audit", err.to_string()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| Fail::new("audit", err.to_string()))?;
        Ok(Self {
            file: Mutex::new(file),
            secrets,
        })
    }

    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in &self.secrets {
            if secret.len() >= 8 {
                out = out.replace(secret, "[redacted]");
            }
        }
        out
    }

    pub fn handshake(&self, user: &str, connection_id: u32) {
        self.write(json!({
            "phase": "handshake",
            "user": user,
            "connection_id": connection_id,
        }));
    }

    pub fn statement(&self, user: &str, connection_id: u32, sql: &str, ok: bool, error: Option<String>) {
        let mut body = json!({
            "phase": "statement",
            "user": user,
            "connection_id": connection_id,
            "sql": self.redact(sql),
            "ok": ok,
        });
        if let Some(error) = error {
            body["error"] = Json::String(self.redact(&error));
        }
        self.write(body);
    }

    pub fn quit(&self, user: &str, connection_id: u32) {
        self.write(json!({
            "phase": "quit",
            "user": user,
            "connection_id": connection_id,
        }));
    }

    fn write(&self, body: Json) {
        let line = self.redact(&body.to_string());
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}
