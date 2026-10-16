//! The existing authenticated Control API contract. Credentials stay on stdin.
use crate::io::*;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn curl(
    url: &str,
    ca: &str,
    headers: &[(&str, String)],
    body: Option<&Value>,
) -> Result<(u16, Value)> {
    require(
        url.starts_with("https://") && !url.contains(['\r', '\n', '"']),
        "https_endpoint",
    )?;
    let quote = |s: &str| -> Result<String> {
        require(!s.contains(['\r', '\n']), "header_newline")?;
        Ok(s.replace('\\', "\\\\").replace('"', "\\\""))
    };
    let mut config = format!(
        "url = \"{}\"\ncacert = \"{}\"\nsilent\nshow-error\nmax-time = 30\nwrite-out = \"\\n%{{http_code}}\"\n",
        quote(url)?,
        quote(ca)?
    );
    for (key, val) in headers {
        config.push_str(&format!("header = \"{}: {}\"\n", quote(key)?, quote(val)?));
    }
    if let Some(body) = body {
        config.push_str("request = \"POST\"\nheader = \"Content-Type: application/json\"\n");
        config.push_str(&format!(
            "data = \"{}\"\n",
            quote(&String::from_utf8(bytes(body)?)?)?
        ));
    }
    let mut child = Command::new("curl")
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("curl_stdin")?
        .write_all(config.as_bytes())?;
    let output = child.wait_with_output()?;
    require(output.status.success(), "control_transport")?;
    let split = output
        .stdout
        .iter()
        .rposition(|b| *b == b'\n')
        .ok_or("http_status")?;
    Ok((
        std::str::from_utf8(&output.stdout[split + 1..])?.parse()?,
        serde_json::from_slice(&output.stdout[..split])?,
    ))
}

pub fn call(
    c: &Value,
    role: &str,
    path: &str,
    body: Option<Value>,
    calls: &mut Calls,
    name: &str,
) -> Result<Value> {
    require(
        path.starts_with("/control/v1/") && !path.contains(['\r', '\n']),
        "control_path",
    )?;
    let cookie = fs::read_to_string(private(text(&c["cookies"], role)?)?)?
        .trim()
        .to_owned();
    let mut headers = vec![
        ("Cookie", format!("__Host-oo-session={cookie}")),
        ("Origin", text(c, "origin")?.into()),
        ("Accept", "application/json".into()),
    ];
    let mut body = body;
    if let Some(ref mut b) = body {
        let (status, current) = curl(
            &format!(
                "{}/control/v1/identity/current",
                text(c, "identity_origin")?.trim_end_matches('/')
            ),
            text(c, "identity_ca")?,
            &headers[..1],
            None,
        )?;
        require(
            status == 200 && current["install_state"] == "initialized",
            "management_session",
        )?;
        b["identity_revision"] = current["revision"].clone();
        b["idempotency_key"] = json!(sha(format!(
            "{}:{}:{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            path
        )
        .as_bytes()));
        headers.push(("X-OO-CSRF", text(&current, "csrf_token")?.into()));
    }
    let label = format!("api-{name}-{}", fs::read_dir(&calls.out)?.count());
    save(
        calls.out.join(format!("{label}.started.json")),
        &json!({"path":path,"role":role,"request":body}),
    )?;
    let (status, response) = curl(
        &format!("{}{}", text(c, "endpoint")?.trim_end_matches('/'), path),
        text(c, "ca")?,
        &headers,
        body.as_ref(),
    )?;
    save(
        calls.out.join(format!("{label}.response.json")),
        &json!({"status":status,"request":body,"response":response}),
    )?;
    save(
        calls.out.join(format!("{label}.exit.json")),
        &json!({"exit":if status<400 {0}else{1}}),
    )?;
    require(status < 400, "actual_api_refused")?;
    Ok(response)
}

pub fn response_file(calls: &Calls, name: &str) -> Result<std::path::PathBuf> {
    let prefix = format!("api-{name}-");
    let found = fs::read_dir(&calls.out)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with(&prefix)
                && e.file_name().to_string_lossy().ends_with(".response.json")
        })
        .collect::<Vec<_>>();
    require(found.len() == 1, "ambiguous_api_response")?;
    Ok(found[0].path())
}
