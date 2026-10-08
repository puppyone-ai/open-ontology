//! Holds the fixed control-identity binary and the fixed test issuer.
//! This file is copied into a build snapshot. It is not a product command.

#![forbid(unsafe_code)]

#[path = "../tests/live/idp.rs"]
mod idp;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use control_identity::{canonical_commit, load_config, Binding, Role};
use idp::{IdpParts, Mode};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let bin = std::env::var("CONTROL_IDENTITY_BIN").expect("bin");
    let origin = "https://oo-fe01-m14-edge:8443";
    let issuer = "https://oo-fe01-m14-idp:9443";
    let callback = format!("{origin}/control/v1/auth/callback");
    let bindings = bindings();
    let admin = bindings[0].clone();
    let redirects = vec![callback.clone()];
    let commit = canonical_commit(issuer, "oo-control", "oo-control-client", &redirects, &bindings, &admin, 1).expect("commit");
    let config_path = "/run/fe01/identity.json";
    fs::create_dir_all("/run/fe01").expect("run dir");
    let body = serde_json::json!({
        "https_origin": origin,
        "listen_host": "0.0.0.0",
        "listen_port": 7443,
        "tls_cert_file": "/secrets/svc.crt",
        "tls_key_file": "/secrets/svc.key",
        "issuer": issuer,
        "audience": "oo-control",
        "client_id": "oo-control-client",
        "client_secret_file": "/secrets/client.secret",
        "redirect_uris": redirects,
        "config_commit": commit,
        "revision": 1,
        "idp_ca_file": "/secrets/ca.crt",
        "pg": {
            "host": "oo-fe01-m14-pg",
            "port": 5432,
            "database": "identity",
            "user": "oo_identity_admin",
            "password_file": "/secrets/login.pass",
            "ca_file": "/secrets/ca.crt"
        },
        "bindings": bindings.iter().map(|item| serde_json::json!({
            "subject": item.subject,
            "project_id": item.project_id,
            "role": item.role.as_str()
        })).collect::<Vec<_>>(),
        "bootstrap": {
            "subject": admin.subject,
            "project_id": admin.project_id,
            "role": admin.role.as_str()
        },
        "idle_seconds": 600,
        "absolute_seconds": 600,
        "oauth_ttl_seconds": 180,
        "bootstrap_ttl_seconds": 600,
        "clock_skew_seconds": 0
    });
    fs::write(config_path, serde_json::to_vec_pretty(&body).expect("json")).expect("config");
    load_config(Path::new(config_path)).expect("config loads");
    let install = Command::new(&bin).args(["install", "--config", config_path]).output().expect("install");
    if !install.status.success() {
        fs::write("/evidence/raw/install.txt", format!("fail {}\n", install.status.code().unwrap_or(1))).ok();
        let _ = fs::write("/evidence/raw/install.stderr", install.stderr);
        std::process::exit(1);
    }
    fs::write("/evidence/raw/install.txt", "committed\n").expect("install record");
    let token_path = "/secrets/bootstrap.token";
    let _ = fs::remove_file(token_path);
    let issued = Command::new(&bin)
        .args(["bootstrap-issue", "--config", config_path, "--out", token_path])
        .output()
        .expect("issue");
    if !issued.status.success() || !Path::new(token_path).is_file() {
        fs::write("/evidence/raw/bootstrap-issue.txt", "fail\n").ok();
        std::process::exit(1);
    }
    fs::write("/evidence/raw/bootstrap-issue.txt", "issued\n").expect("issue record");
    let cert = fs::read("/secrets/idp.crt").expect("idp cert");
    let key = fs::read("/secrets/idp.key").expect("idp key");
    let der = fs::read("/secrets/k1.der").expect("der");
    let secret = read_trimmed("/secrets/client.secret");
    let listener = TcpListener::bind("0.0.0.0:9443").await.expect("idp bind");
    let idp = idp::start(
        listener,
        IdpParts {
            cert_pem: &cert,
            key_pem: &key,
            issuer,
            client_id: "oo-control-client",
            configured_audience: "oo-control",
            client_secret: &secret,
            redirect_uri: &callback,
            der_keys: &[der],
        },
    )
    .await;
    idp.set_mode(Mode::Ok);
    idp.set_subject("fixture-admin");
    fs::write("/evidence/subject.applied", "fixture-admin\n").expect("subject");
    let mut child = Command::new(&bin)
        .args(["serve", "--config", config_path])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("serve");
    let ready = wait_port("127.0.0.1:7443", Duration::from_secs(30));
    if !ready {
        let _ = child.kill();
        fs::write("/evidence/raw/serve.txt", "fail\n").ok();
        std::process::exit(1);
    }
    fs::write("/evidence/raw/serve.txt", "listening\n").expect("serve record");
    fs::write(
        "/evidence/ready.json",
        format!(
            "{{\"ok\":true,\"origin\":\"{origin}\",\"issuer\":\"{issuer}\",\"customer_integration\":\"not_claimed\",\"subjects\":[\"fixture-admin\",\"fixture-reviewer\",\"fixture-steward\",\"fixture-auditor\",\"fixture-owner\"],\"config_commit\":\"{commit}\",\"service_commit\":\"a6f92da8a0bf872f8a30efb116c2c7b14463505b\",\"contract_commit\":\"089b00d7f70da1bc42f7e201dfa070748c651d5b\",\"b04\":\"schema_frozen\"}}\n"
        ),
    )
    .expect("ready");
    let mut applied = String::from("fixture-admin");
    loop {
        if let Ok(next) = fs::read_to_string("/evidence/subject.request") {
            let next = next.trim();
            if next != applied && allowed(next) {
                idp.set_subject(next);
                applied = next.to_string();
                fs::write("/evidence/subject.applied", format!("{next}\n")).ok();
            }
        }
        if Path::new("/evidence/stop").exists() {
            break;
        }
        if let Some(status) = child.try_wait().expect("wait") {
            fs::write("/evidence/raw/serve.txt", format!("exit {}\n", status.code().unwrap_or(1))).ok();
            std::process::exit(1);
        }
        thread::sleep(Duration::from_secs(1));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn allowed(subject: &str) -> bool {
    matches!(
        subject,
        "fixture-admin" | "fixture-reviewer" | "fixture-steward" | "fixture-auditor" | "fixture-owner"
    )
}

fn wait_port(address: &str, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if std::net::TcpStream::connect(address).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(200));
    }
    false
}

fn bindings() -> Vec<Binding> {
    vec![
        row("fixture-admin", Role::Admin),
        row("fixture-reviewer", Role::Reviewer),
        row("fixture-steward", Role::Steward),
        row("fixture-auditor", Role::Auditor),
        row("fixture-owner", Role::BusinessOwner),
    ]
}

fn row(subject: &str, role: Role) -> Binding {
    Binding {
        subject: subject.into(),
        project_id: "alpha".into(),
        role,
    }
}

fn read_trimmed(path: &str) -> String {
    let mut text = fs::read_to_string(path).expect("secret");
    if text.ends_with('\n') {
        text.pop();
    }
    text
}
