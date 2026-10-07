//! Call the fixed `role-pack-bundle` CLI on public compiler inputs.
//!
//! The gateway file is the bundle's raw `pack` span. This tool does not
//! recompute `pack_digest` and does not write a pack when the compiler refuses.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("gateway-pack-fixture: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or("usage")?;
    let mut compiler = None;
    let mut effective = None;
    let mut visibility = None;
    let mut output = None;
    let mut digest = None;
    let mut stderr_path = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("usage")?;
        match flag.as_str() {
            "--compiler" => compiler = Some(value),
            "--effective" => effective = Some(value),
            "--visibility" => visibility = Some(value),
            "--output" => output = Some(value),
            "--digest" => digest = Some(value),
            "--stderr" => stderr_path = Some(value),
            _ => return Err("usage".to_owned()),
        }
    }
    let compiler = compiler.ok_or("usage")?;
    let effective = effective.ok_or("usage")?;
    let visibility = visibility.ok_or("usage")?;
    match mode.as_str() {
        "compile" => {
            let output = output.ok_or("usage")?;
            let digest = digest.ok_or("usage")?;
            compile(&compiler, &effective, &visibility, Path::new(&output), Path::new(&digest))
        }
        "refuse" => {
            let stderr_path = stderr_path.ok_or("usage")?;
            refuse(&compiler, &effective, &visibility, Path::new(&stderr_path))
        }
        _ => Err("usage".to_owned()),
    }
}

fn compile(compiler: &str, effective: &str, visibility: &str, output: &Path, digest_path: &Path) -> Result<(), String> {
    let parent = output.parent().ok_or("output parent")?;
    let bundle = parent.join(format!(
        ".{}.bundle",
        output.file_name().and_then(|name| name.to_str()).ok_or("output name")?
    ));
    let command = command_text(effective, visibility)?;
    let produced = invoke(compiler, &command, &bundle)?;
    if !produced.status.success() {
        let _ = fs::remove_file(&bundle);
        return Err(format!(
            "compiler refused a pack that should seal: {}",
            command_note(&produced)
        ));
    }
    let text = fs::read_to_string(&bundle).map_err(|_| "bundle read".to_owned())?;
    let pack = raw_field(&text, "pack")?;
    let digest_raw = raw_field(&text, "pack_digest")?;
    let digest = json_string(digest_raw)?;
    if !pack.contains(&format!("\"pack_digest\":\"{digest}\"")) {
        return Err("bundle pack span does not carry the compiler digest".to_owned());
    }
    fs::write(output, pack).map_err(|_| "artifact write".to_owned())?;
    fs::write(digest_path, format!("{digest}\n")).map_err(|_| "digest write".to_owned())?;
    let _ = fs::remove_file(&bundle);
    Ok(())
}

fn refuse(compiler: &str, effective: &str, visibility: &str, stderr_path: &Path) -> Result<(), String> {
    let parent = stderr_path.parent().unwrap_or_else(|| Path::new("."));
    let bundle = parent.join(".hidden.bundle");
    let command = command_text(effective, visibility)?;
    let produced = invoke(compiler, &command, &bundle)?;
    let _ = fs::remove_file(&bundle);
    let stderr = String::from_utf8_lossy(&produced.stderr).into_owned();
    fs::write(stderr_path, &stderr).map_err(|_| "stderr write".to_owned())?;
    if produced.status.success() {
        return Err("compiler sealed a pack for unclassified PII".to_owned());
    }
    if !stderr.contains("classification.unknown_pii") {
        return Err("compiler refusal was not unknown PII".to_owned());
    }
    Ok(())
}

fn command_note(output: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let mut start = text.len().saturating_sub(1500);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

fn command_text(effective: &str, visibility: &str) -> Result<String, String> {
    let effective_text = fs::read_to_string(effective).map_err(|_| "effective read".to_owned())?;
    let visibility_text = fs::read_to_string(visibility).map_err(|_| "visibility read".to_owned())?;
    let effective = one_json(&effective_text)?;
    let visibility = one_json(&visibility_text)?;
    Ok(format!(
        "{{\"effective_oos\":{effective},\"protocol\":\"oo.builder.role-pack-command/v1\",\"visibility\":{visibility}}}"
    ))
}

fn invoke(compiler: &str, command: &str, bundle: &Path) -> Result<std::process::Output, String> {
    let request = bundle.with_extension("request.json");
    let _ = fs::remove_file(bundle);
    let _ = fs::remove_file(&request);
    fs::write(&request, command).map_err(|_| "request write".to_owned())?;
    let output = Command::new(compiler)
        .arg("--request")
        .arg(&request)
        .arg("--output")
        .arg(bundle)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "compiler spawn".to_owned())?;
    let _ = fs::remove_file(&request);
    Ok(output)
}

fn one_json(text: &str) -> Result<&str, String> {
    let trimmed = text.trim();
    let bytes = trimmed.as_bytes();
    let mut index = 0;
    skip_value(bytes, &mut index)?;
    skip_ws(bytes, &mut index);
    if index == bytes.len() {
        Ok(trimmed)
    } else {
        Err("fixture json has trailing data".to_owned())
    }
}

fn raw_field<'a>(document: &'a str, want: &str) -> Result<&'a str, String> {
    let bytes = document.as_bytes();
    let mut index = 0;
    skip_ws(bytes, &mut index);
    if bytes.get(index) != Some(&b'{') {
        return Err("bundle object".to_owned());
    }
    index += 1;
    loop {
        skip_ws(bytes, &mut index);
        if bytes.get(index) == Some(&b'}') {
            return Err("bundle field missing".to_owned());
        }
        let key = parse_key(bytes, &mut index)?;
        skip_ws(bytes, &mut index);
        if bytes.get(index) != Some(&b':') {
            return Err("bundle colon".to_owned());
        }
        index += 1;
        skip_ws(bytes, &mut index);
        let start = index;
        skip_value(bytes, &mut index)?;
        if key == want {
            return document.get(start..index).ok_or_else(|| "bundle span".to_owned());
        }
        skip_ws(bytes, &mut index);
        match bytes.get(index) {
            Some(b',') => index += 1,
            _ => return Err("bundle separator".to_owned()),
        }
    }
}

fn json_string(raw: &str) -> Result<String, String> {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') && !raw[1..raw.len() - 1].contains('\\') {
        Ok(raw[1..raw.len() - 1].to_owned())
    } else {
        Err("digest string".to_owned())
    }
}

fn parse_key(bytes: &[u8], index: &mut usize) -> Result<String, String> {
    let start = *index;
    skip_string(bytes, index)?;
    let raw = std::str::from_utf8(&bytes[start..*index]).map_err(|_| "key".to_owned())?;
    json_string(raw)
}

fn skip_value(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    skip_ws(bytes, index);
    match bytes.get(*index) {
        Some(b'{') => skip_container(bytes, index, b'{', b'}'),
        Some(b'[') => skip_container(bytes, index, b'[', b']'),
        Some(b'"') => skip_string(bytes, index),
        Some(b't') => skip_literal(bytes, index, b"true"),
        Some(b'f') => skip_literal(bytes, index, b"false"),
        Some(b'n') => skip_literal(bytes, index, b"null"),
        Some(b'-') | Some(b'0'..=b'9') => skip_number(bytes, index),
        _ => Err("json value".to_owned()),
    }
}

fn skip_container(bytes: &[u8], index: &mut usize, open: u8, close: u8) -> Result<(), String> {
    if bytes.get(*index) != Some(&open) {
        return Err("container".to_owned());
    }
    *index += 1;
    loop {
        skip_ws(bytes, index);
        if bytes.get(*index) == Some(&close) {
            *index += 1;
            return Ok(());
        }
        if open == b'{' {
            skip_string(bytes, index)?;
            skip_ws(bytes, index);
            if bytes.get(*index) != Some(&b':') {
                return Err("object colon".to_owned());
            }
            *index += 1;
        }
        skip_value(bytes, index)?;
        skip_ws(bytes, index);
        match bytes.get(*index) {
            Some(b',') => *index += 1,
            Some(byte) if *byte == close => {}
            _ => return Err("container separator".to_owned()),
        }
    }
}

fn skip_string(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    if bytes.get(*index) != Some(&b'"') {
        return Err("string".to_owned());
    }
    *index += 1;
    while *index < bytes.len() {
        match bytes[*index] {
            b'"' => {
                *index += 1;
                return Ok(());
            }
            b'\\' => {
                *index += 1;
                if *index >= bytes.len() {
                    return Err("string escape".to_owned());
                }
                if bytes[*index] == b'u' {
                    *index += 5;
                    if *index > bytes.len() {
                        return Err("string escape".to_owned());
                    }
                } else {
                    *index += 1;
                }
            }
            _ => *index += 1,
        }
    }
    Err("string end".to_owned())
}

fn skip_literal(bytes: &[u8], index: &mut usize, literal: &[u8]) -> Result<(), String> {
    if bytes.get(*index..*index + literal.len()) == Some(literal) {
        *index += literal.len();
        Ok(())
    } else {
        Err("literal".to_owned())
    }
}

fn skip_number(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    let start = *index;
    if bytes.get(*index) == Some(&b'-') {
        *index += 1;
    }
    while matches!(bytes.get(*index), Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
        *index += 1;
    }
    if *index == start {
        Err("number".to_owned())
    } else {
        Ok(())
    }
}

fn skip_ws(bytes: &[u8], index: &mut usize) {
    while matches!(bytes.get(*index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        *index += 1;
    }
}
