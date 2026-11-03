//! Install, issue, or revoke an opaque client token through the public harness.
//!
//! The admin password is read from a file and wiped. The plaintext token is
//! written to the named file and is not printed. Stdout is the token hash.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use oo_gateway::http_gateway::token_lifecycle::{AdminRequest, Control, Grant, Operation};
use serde_json::Value;
use zeroize::Zeroize;

use oo_gateway::http_gateway::{
    CurrentTuple, Endpoint, IssueGrant, TokenKind, harness_install, harness_revoke_token,
};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("issue-token: {reason}");
            std::process::ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), &'static str> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err("usage");
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime")?;
    match args[0].as_str() {
        "install" => {
            let (endpoint, mut password, ca) = material(&args, 4)?;
            let result = runtime.block_on(harness_install(endpoint, &password, &ca));
            password.zeroize();
            result.map_err(|_| "install")
        }
        "issue" => {
            let (endpoint, mut password, ca) = material(&args, 6)?;
            let grant = read_grant(Path::new(&args[4]))?;
            let out = Path::new(&args[5]);
            let value: Value = serde_json::from_slice(&fs::read(&args[4]).map_err(|_| "grant")?)
                .map_err(|_| "grant")?;
            let purpose = match value.get("purpose") {
                None => String::new(),
                Some(Value::String(text)) => text.clone(),
                _ => return Err("purpose"),
            };
            let grant = Grant::from_legacy(grant, purpose)?;
            let control = Control::new(endpoint, std::mem::take(&mut password), ca);
            let result =
                runtime.block_on(control.execute(Operation::Issue(grant), "oo_token_admin"))?;
            let token = result.secret.ok_or("issue")?;
            write_token(out, &token)?;
            println!(
                "hash {}",
                result.metadata["token_sha256"].as_str().ok_or("hash")?
            );
            Ok(())
        }
        "revoke" => {
            let (endpoint, mut password, ca) = material(&args, 6)?;
            let project = args[4].clone();
            let hash = args[5].clone();
            if project.is_empty() || hash.len() != 64 {
                password.zeroize();
                return Err("revoke");
            }
            let result = runtime.block_on(harness_revoke_token(
                endpoint, &password, &ca, project, hash,
            ));
            password.zeroize();
            result.map_err(|_| "revoke")
        }
        "list" | "renew" | "rotate" | "enroll" | "policy" => {
            let secret_output = matches!(args[0].as_str(), "rotate" | "enroll");
            let (endpoint, mut password, ca) = material(&args, if secret_output { 6 } else { 5 })?;
            let mut value: Value =
                serde_json::from_slice(&fs::read(&args[4]).map_err(|_| "request")?)
                    .map_err(|_| "request")?;
            let object = value.as_object_mut().ok_or("request")?;
            object.insert("operation".into(), Value::String(args[0].clone()));
            let request: AdminRequest = serde_json::from_value(value).map_err(|_| "request")?;
            let control = Control::new(endpoint, std::mem::take(&mut password), ca);
            let result = runtime.block_on(control.execute(request.into(), "oo_token_admin"))?;
            if let Some(secret) = result.secret {
                write_token(Path::new(&args[5]), &secret)?;
            }
            println!("{}", result.metadata);
            Ok(())
        }
        _ => Err("usage"),
    }
}

fn material(args: &[String], count: usize) -> Result<(Endpoint, String, Vec<u8>), &'static str> {
    if args.len() != count {
        return Err("usage");
    }
    let endpoint = read_endpoint(Path::new(&args[1]))?;
    let password = read_password(Path::new(&args[2]))?;
    let ca = fs::read(&args[3]).map_err(|_| "ca")?;
    if ca.is_empty() {
        return Err("ca");
    }
    Ok((endpoint, password, ca))
}

fn read_endpoint(path: &Path) -> Result<Endpoint, &'static str> {
    let text = fs::read_to_string(path).map_err(|_| "endpoint")?;
    let value: Value = serde_json::from_str(&text).map_err(|_| "endpoint")?;
    let object = value.as_object().ok_or("endpoint")?;
    for key in object.keys() {
        if key != "host" && key != "port" && key != "database" {
            return Err("endpoint");
        }
    }
    let host = object
        .get("host")
        .and_then(Value::as_str)
        .ok_or("endpoint")?;
    let database = object
        .get("database")
        .and_then(Value::as_str)
        .ok_or("endpoint")?;
    let port = object
        .get("port")
        .and_then(Value::as_u64)
        .ok_or("endpoint")?;
    let port = u16::try_from(port).map_err(|_| "endpoint")?;
    if host.is_empty() || database.is_empty() || port == 0 {
        return Err("endpoint");
    }
    Ok(Endpoint {
        host: host.to_owned(),
        port,
        database: database.to_owned(),
    })
}

fn read_password(path: &Path) -> Result<String, &'static str> {
    let mut password = fs::read_to_string(path).map_err(|_| "password")?;
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    if password.is_empty() || password.contains('\0') {
        password.zeroize();
        return Err("password");
    }
    Ok(password)
}

fn read_grant(path: &Path) -> Result<IssueGrant, &'static str> {
    let text = fs::read_to_string(path).map_err(|_| "grant")?;
    let value: Value = serde_json::from_str(&text).map_err(|_| "grant")?;
    let object = value.as_object().ok_or("grant")?;
    let kind = match object.get("kind").and_then(Value::as_str) {
        Some("client") => TokenKind::Client,
        _ => return Err("grant"),
    };
    let claims = match object.get("claims") {
        None => Vec::new(),
        Some(Value::Array(items)) if items.is_empty() => Vec::new(),
        _ => return Err("grant"),
    };
    let ttl = match object.get("client_ttl_ms") {
        None => None,
        Some(Value::Number(number)) => Some(number.as_i64().ok_or("grant")?),
        _ => return Err("grant"),
    };
    Ok(IssueGrant {
        kind,
        current: CurrentTuple {
            project_id: text_field(object, "project_id")?,
            principal_id: text_field(object, "principal_id")?,
            role: text_field(object, "role")?,
            claims,
            policy_version: text_field(object, "policy_version")?,
            knowledge_version: text_field(object, "knowledge_version")?,
            audience_url: text_field(object, "audience_url")?,
            sandbox_id: None,
            session_id: None,
        },
        client_ttl_ms: ttl,
    })
}

fn text_field(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, &'static str> {
    match object.get(key).and_then(Value::as_str) {
        Some(text) if !text.is_empty() => Ok(text.to_owned()),
        _ => Err("grant"),
    }
}

fn write_token(path: &Path, token: &str) -> Result<(), &'static str> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| "token_file")?;
    file.write_all(token.as_bytes()).map_err(|_| "token_file")?;
    file.write_all(b"\n").map_err(|_| "token_file")?;
    file.sync_all().map_err(|_| "token_file")?;
    Ok(())
}
