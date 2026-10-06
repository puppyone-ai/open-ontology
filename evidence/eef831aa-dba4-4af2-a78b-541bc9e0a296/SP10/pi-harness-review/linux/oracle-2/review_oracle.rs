//! Independent H-D02 review oracle. Std only. Not a product crate.
//! Hash checks use the FIPS 180-4 function in this file. Nothing shells out.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const NPM_TARBALL_SHA: &str = "04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5";
const GITHUB_TARBALL_SHA: &str = "a95c2ded6a2d8b56247f4205c99e3bef6241b5b51044086d4185d75c2822c521";
const LOCK_SHA: &str = "35fd666029cb44d3cceb5a372cbacd2aecc478c69daa14898f006af291d4d869";
const PI_LICENSE_SHA: &str = "0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48";
const NODE_LICENSE_SHA: &str = "c738ae413cf561f174e34f6961f8ca458aae2369a73640dda6234c629b98bcc4";
const PI_COMMIT: &str = "7c10bd4337495ee613f2224843ecdf349b80d1df";
const PUBLISHED_DAY: &str = "2026-10-05";
const MARKER: &str = "public-proxy-marker";

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("review_oracle: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), String> {
    check_sha256()?;
    check_calendar()?;
    let mut args = env::args().skip(1);
    let cmd = args.next().ok_or("usage: registry|source|pin|live")?;
    match cmd.as_str() {
        "registry" => {
            let child = take_path(&mut args)?;
            let triples = take_path(&mut args)?;
            let root = take_path(&mut args)?;
            let manifest = take_path(&mut args)?;
            registry(&child, &triples, &root, &manifest)?;
        }
        "source" => source(&take_path(&mut args)?)?,
        "pin" => {
            let evidence = take_path(&mut args)?;
            let packument = take_path(&mut args)?;
            pin(&evidence, &packument)?;
        }
        "live" => live(&take_path(&mut args)?)?,
        _ => return Err("usage: registry|source|pin|live".to_owned()),
    }
    if args.next().is_some() {
        return Err("unexpected extra argument".to_owned());
    }
    Ok(())
}

fn take_path(args: &mut impl Iterator<Item = String>) -> Result<PathBuf, String> {
    args.next().map(PathBuf::from).ok_or_else(|| "missing path".to_owned())
}

fn registry(child_lock: &Path, triples_path: &Path, root_lock: &Path, manifest: &Path) -> Result<(), String> {
    let child = registry_triples(&read_bytes(child_lock)?)?;
    let root = registry_triples(&read_bytes(root_lock)?)?;
    let mut expected = Vec::new();
    for (index, line) in read_string(triples_path)?.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(' ');
        let name = parts.next().ok_or("triple name")?;
        let version = parts.next().ok_or("triple version")?;
        let checksum = parts.next().ok_or("triple checksum")?;
        if parts.next().is_some() || checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("bad triple line {}", index + 1));
        }
        expected.push(format!("{name} {version} {checksum}"));
    }
    expected.sort();
    let mut child_sorted = child.clone();
    child_sorted.sort();
    if child_sorted != expected {
        return Err(format!(
            "child lock registry set differs from registry-triples.txt child={} file={}",
            child_sorted.len(),
            expected.len()
        ));
    }
    let mut missing = Vec::new();
    for triple in &expected {
        if !root.iter().any(|item| item == triple) {
            missing.push(triple.clone());
        }
    }
    if !missing.is_empty() {
        return Err(format!("child triples absent from fixed root lock: {}", missing.join(", ")));
    }
    let manifest_text = read_string(manifest)?;
    if manifest_text.contains("signal-hook") || manifest_text.contains("\"process\"") || manifest_text.contains("\"net\"") {
        return Err("Cargo.toml enables process, net, or signal-hook".to_owned());
    }
    for pin in ["=1.53.1", "=1.0.229", "=1.0.151"] {
        if !manifest_text.contains(pin) {
            return Err(format!("manifest is missing {pin}"));
        }
    }
    let child_text = read_string(child_lock)?;
    if child_text.contains("signal-hook-registry") {
        return Err("child lock contains signal-hook-registry".to_owned());
    }
    println!("registry_triples={}", expected.len());
    println!("root_registry_packages={}", root.len());
    println!("registry_ok");
    Ok(())
}

fn registry_triples(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "lock is not utf-8".to_owned())?;
    let mut out = Vec::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut checksum = String::new();
    let mut registry = false;
    let flush = |registry: &mut bool, name: &mut String, version: &mut String, checksum: &mut String, out: &mut Vec<String>| -> Result<(), String> {
        if *registry {
            if name.is_empty() || version.is_empty() || checksum.len() != 64 {
                return Err(format!("incomplete registry package {name} {version}"));
            }
            out.push(format!("{name} {version} {checksum}"));
        }
        *registry = false;
        name.clear();
        version.clear();
        checksum.clear();
        Ok(())
    };
    for line in text.lines() {
        if line == "[[package]]" {
            flush(&mut registry, &mut name, &mut version, &mut checksum, &mut out)?;
            continue;
        }
        if let Some(value) = line.strip_prefix("name = \"") {
            name = value.trim_end_matches('"').to_owned();
        } else if let Some(value) = line.strip_prefix("version = \"") {
            version = value.trim_end_matches('"').to_owned();
        } else if let Some(value) = line.strip_prefix("checksum = \"") {
            checksum = value.trim_end_matches('"').to_owned();
        } else if line.starts_with("source = \"registry+") {
            registry = true;
        }
    }
    flush(&mut registry, &mut name, &mut version, &mut checksum, &mut out)?;
    if out.is_empty() {
        return Err("no registry packages".to_owned());
    }
    Ok(out)
}

fn source(work: &Path) -> Result<(), String> {
    let lib = read_string(&work.join("src/lib.rs"))?;
    let child = read_string(&work.join("src/child.rs"))?;
    let live = read_string(&work.join("src/bin/live.rs"))?;
    for (label, text, needle) in [
        ("lib prompt", lib.as_str(), ".command(\"prompt\""),
        ("lib get_messages", lib.as_str(), ".command(\"get_messages\""),
        ("lib switch_session", lib.as_str(), ".command(\"switch_session\""),
        ("child abort", child.as_str(), "command_inner(inner, \"abort\""),
        ("child refusal", child.as_str(), "kind == \"export\" || kind == \"restore\""),
        ("child refusal text", child.as_str(), "export and restore are not commands"),
    ] {
        if !text.contains(needle) {
            return Err(format!("{label} missing"));
        }
    }
    if lib.contains(".command(\"export\"") || lib.contains(".command(\"restore\"") || live.contains(".command(\"export\"") || live.contains(".command(\"restore\"") {
        return Err("a literal export or restore command call is present".to_owned());
    }
    if !lib.contains("RunscApproval::Absent") || !lib.contains("RunscApproval::Presented") || !lib.contains("ErrorKind::Unsupported") {
        return Err("runsc unsupported arms missing".to_owned());
    }
    println!("source_commands=prompt,get_messages,switch_session,abort");
    println!("literal_export_restore_calls=absent");
    println!("source_ok");
    Ok(())
}

fn pin(evidence: &Path, packument: &Path) -> Result<(), String> {
    expect_hash(&evidence.join("pi-LICENSE"), PI_LICENSE_SHA, "pi-license")?;
    expect_hash(&evidence.join("node-LICENSE"), NODE_LICENSE_SHA, "node-license")?;
    expect_hash(&evidence.join("package-lock.json"), LOCK_SHA, "package-lock")?;
    expect_sha_file(&evidence.join("npm-tarball.sha256"), NPM_TARBALL_SHA, "npm-tarball")?;
    expect_sha_file(&evidence.join("github-tarball.sha256"), GITHUB_TARBALL_SHA, "github-tarball")?;
    expect_sha_file(&evidence.join("package-lock.sha256"), LOCK_SHA, "package-lock-sidecar")?;
    expect_sha_file(&evidence.join("node-LICENSE.sha256"), NODE_LICENSE_SHA, "node-license-sidecar")?;
    expect_sha_file(&evidence.join("pi-LICENSE.sha256"), PI_LICENSE_SHA, "pi-license-sidecar")?;
    let version = read_string(&evidence.join("pi-version.stdout"))?;
    if version.trim() != "1.0.4" {
        return Err(format!("pi version stdout {:?}", version.trim()));
    }
    let node = read_string(&evidence.join("node-version.txt"))?;
    if node.trim() != "v22.23.3" {
        return Err(format!("node version {:?}", node.trim()));
    }
    let commit = read_string(&evidence.join("pi-git-commit.txt"))?;
    if commit.trim() != PI_COMMIT {
        return Err(format!("pi commit {:?}", commit.trim()));
    }
    let meta = read_bytes(&evidence.join("npm-registry.json"))?;
    expect_field(&meta, "version", "1.0.4")?;
    expect_field(&meta, "license", "MIT")?;
    expect_field(&meta, "gitHead", PI_COMMIT)?;
    if json_string_field(&meta, "integrity")? != "sha512-+956nfMFHr5lDUVY/2Q4k+YzojzBuCaBXFgj0eSlXVGr7QVliVddKdc1Pz6yVg1dOlJQmb67doOVrlMsIcIdaw==" {
        return Err("npm integrity changed".to_owned());
    }
    let published = published_day(&read_bytes(packument)?)?;
    if published != PUBLISHED_DAY {
        return Err(format!("published day changed to {published}"));
    }
    let run_day = read_string(&evidence.join("run-date.txt"))?;
    let run_day = run_day.trim();
    let age = days_from_civil_str(run_day)? - days_from_civil_str(&published)?;
    if !(0..7).contains(&age) {
        return Err(format!("prototype age {age} days is outside the unchanged floor"));
    }
    println!("published_day={published}");
    println!("run_day={run_day}");
    println!("prototype_age_days={age}");
    println!("pin_ok");
    Ok(())
}

fn live(proof: &Path) -> Result<(), String> {
    for name in ["tool.exit", "abort.exit", "disconnect.exit", "restore.exit", "build.exit"] {
        let text = read_string(&proof.join(name))?;
        if text.trim() != "0" {
            return Err(format!("{name} is {:?}", text.trim()));
        }
    }
    let ifaces = proof.join("net-ifaces.txt");
    let host = proof.join("hostconfig.txt");
    if !ifaces.exists() && !host.exists() {
        return Err("live network evidence is missing".to_owned());
    }
    if ifaces.exists() {
        let text = read_string(&ifaces)?;
        let names: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
        if names.len() != 1 || names[0] != "lo" {
            return Err(format!("live interfaces {names:?}"));
        }
        println!("net_ifaces=lo");
    }
    if host.exists() {
        let text = read_string(&host)?;
        if !text.lines().any(|line| line == "NetworkMode=none") {
            return Err("live network is not none".to_owned());
        }
        if text.lines().any(|line| line == "Privileged=true") {
            return Err("live container was privileged".to_owned());
        }
        println!("hostconfig_network=none");
    }
    let tool_events = read_string(&proof.join("tool/events.json"))?;
    ordered(
        &tool_events,
        &[
            "sequence: 0,",
            "TurnStarted",
            "sequence: 1,",
            "AssistantDelta { text: \"fixture-reply\" }",
            "sequence: 2,",
            "ToolCallStarted { name: \"bash\", call_id: \"call_fixed\"",
            "sequence: 3,",
            "ToolCallFinished { call_id: \"call_fixed\", status: \"ok\"",
            "pi-harness-fixture",
            "sequence: 4,",
            "TurnStarted",
            "sequence: 5,",
            "AssistantDelta { text: \"fixture-reply-after-tool\" }",
            "sequence: 6,",
            "TurnFinished { stop_reason: \"stop\"",
        ],
    )?;
    if tool_events.contains("Error {") {
        return Err("tool events contain Error".to_owned());
    }
    let tool_raw = read_bytes(&proof.join("tool/raw.ndjson"))?;
    require_bytes(&tool_raw, b"printf 'pi-harness-fixture")?;
    require_bytes(&tool_raw, b"\"stopReason\":\"toolUse\"")?;
    require_bytes(&tool_raw, b"\"stopReason\":\"stop\"")?;
    require_bytes(&tool_raw, b"tool_execution_end")?;
    require_bytes(&tool_raw, b"toolcall_end")?;
    scan_response_commands(&tool_raw, &["prompt"])?;
    let abort_events = read_string(&proof.join("abort/events.json"))?;
    if abort_events.contains("TurnFinished") {
        return Err("abort events contain TurnFinished".to_owned());
    }
    if !abort_events.contains("fixture-reply") {
        return Err("abort events missed the text delta".to_owned());
    }
    let abort = read_string(&proof.join("abort/abort.txt"))?;
    if !abort.contains("ack=true") || !abort.contains("waited=true") {
        return Err(format!("abort record {abort}"));
    }
    let pid = field_after(&abort, "pid=")?;
    if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("abort pid {pid}"));
    }
    let disconnect = read_string(&proof.join("disconnect/events.json"))?;
    if disconnect.contains("TurnFinished") || !disconnect.contains("stdout-eof") {
        return Err("disconnect was not stdout-eof without turn_finished".to_owned());
    }
    let exported = read_bytes(&proof.join("restore/export-messages.json"))?;
    let restored = read_bytes(&proof.join("restore/restore-messages.json"))?;
    let left = messages_array(&exported)?;
    let right = messages_array(&restored)?;
    if left != right {
        return Err("restored messages differ from exported messages".to_owned());
    }
    let restore_raw = read_bytes(&proof.join("restore/raw.ndjson"))?;
    scan_response_commands(&restore_raw, &["prompt", "get_messages"])?;
    if !restore_raw.windows(b"\"command\":\"get_messages\"".len()).any(|window| window == b"\"command\":\"get_messages\"") {
        return Err("restore raw is missing get_messages".to_owned());
    }
    let session = one_jsonl(&proof.join("restore/session"))?;
    let session_bytes = read_bytes(&session)?;
    if session_bytes.is_empty() {
        return Err("session jsonl is empty".to_owned());
    }
    println!("session_sha256={}", hex(&sha256(&session_bytes)));
    println!("messages_sha256={}", hex(&sha256(&left)));
    for mode in ["tool", "abort", "disconnect", "restore"] {
        let canary = read_bytes(&proof.join(mode).join("pack/canary"))?;
        if canary != b"pack-canary" {
            return Err(format!("{mode} pack canary changed"));
        }
        let log = read_string(&proof.join(mode).join("provider.log"))?;
        if log.contains("Bearer ") || log.to_ascii_lowercase().contains("authorization:") {
            return Err(format!("{mode} provider log contains authorization material"));
        }
        if !log.contains("auth_ok=true") {
            return Err(format!("{mode} provider did not accept the public marker"));
        }
        if log.contains("auth_other=true") {
            return Err(format!("{mode} provider saw a different authorization"));
        }
        let models = read_string(&proof.join(mode).join("agent/models.json"))?;
        if models.matches(MARKER).count() != 1 {
            return Err(format!("{mode} models.json marker count"));
        }
        if !models.contains("\"apiKey\": \"public-proxy-marker\"") && !models.contains("\"apiKey\":\"public-proxy-marker\"") {
            return Err(format!("{mode} apiKey is not the public marker"));
        }
    }
    println!("switch_session_stdout=not_captured_by_frozen_second_process");
    println!("live_oracle_ok");
    Ok(())
}

fn ordered(text: &str, needles: &[&str]) -> Result<(), String> {
    let mut from = 0;
    for needle in needles {
        let Some(found) = text[from..].find(needle) else {
            return Err(format!("missing {needle}"));
        };
        from += found + needle.len();
    }
    Ok(())
}

fn field_after(text: &str, key: &str) -> Result<String, String> {
    let Some(start) = text.find(key) else {
        return Err(format!("missing {key}"));
    };
    let rest = &text[start + key.len()..];
    let end = rest.find(|ch: char| ch.is_whitespace()).unwrap_or(rest.len());
    Ok(rest[..end].to_owned())
}

fn scan_response_commands(bytes: &[u8], allowed: &[&str]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "raw ndjson is not utf-8".to_owned())?;
    let mut seen = 0_u32;
    for line in text.lines() {
        if !line.contains("\"type\":\"response\"") {
            continue;
        }
        let command = json_string_field(line.as_bytes(), "command")?;
        if command == "export" || command == "restore" {
            return Err(format!("response command {command}"));
        }
        if !allowed.contains(&command.as_str()) {
            return Err(format!("unexpected response command {command}"));
        }
        seen = seen.saturating_add(1);
    }
    if seen == 0 {
        return Err("no response records".to_owned());
    }
    println!("response_commands_checked={seen}");
    Ok(())
}

fn messages_array(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let command = json_string_field(bytes, "command")?;
    if command != "get_messages" {
        return Err(format!("transcript command is {command}"));
    }
    let key_at = find_key(bytes, "messages")?;
    let mut index = key_at;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) != Some(&b':') {
        return Err("messages colon".to_owned());
    }
    index += 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) != Some(&b'[') {
        return Err("messages is not an array".to_owned());
    }
    let end = match_bracket(bytes, index)?;
    Ok(bytes[index..=end].to_vec())
}

fn one_jsonl(dir: &Path) -> Result<PathBuf, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let meta = fs::symlink_metadata(&current).map_err(|err| format!("meta {}: {err}", current.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!("symlink refused {}", current.display()));
        }
        if meta.is_file() {
            if current.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                found.push(current);
            }
            continue;
        }
        if meta.is_dir() {
            let entries = fs::read_dir(&current).map_err(|err| format!("read {}: {err}", current.display()))?;
            for entry in entries {
                let entry = entry.map_err(|err| format!("dirent: {err}"))?;
                pending.push(entry.path());
            }
        }
    }
    if found.len() != 1 {
        return Err(format!("session jsonl count {}", found.len()));
    }
    Ok(found.remove(0))
}

fn published_day(bytes: &[u8]) -> Result<String, String> {
    let object = object_after_key(bytes, "time")?;
    let value = json_string_field(object, "1.0.4")?;
    let day = value.get(..10).ok_or("short published time")?;
    if !day.as_bytes().iter().enumerate().all(|(index, byte)| match index {
        4 | 7 => *byte == b'-',
        _ => byte.is_ascii_digit(),
    }) {
        return Err(format!("published time {value}"));
    }
    Ok(day.to_owned())
}

fn object_after_key<'a>(bytes: &'a [u8], key: &str) -> Result<&'a [u8], String> {
    let mut from = 0;
    while from < bytes.len() {
        let Some(relative) = find_key_from(&bytes[from..], key) else {
            return Err(format!("missing object {key}"));
        };
        let mut index = from + relative;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) != Some(&b':') {
            return Err(format!("{key} colon"));
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) == Some(&b'{') {
            let end = match_bracket(bytes, index)?;
            return Ok(&bytes[index..=end]);
        }
        from += relative + key.len() + 2;
    }
    Err(format!("missing object {key}"))
}

fn expect_field(bytes: &[u8], key: &str, want: &str) -> Result<(), String> {
    let got = json_string_field(bytes, key)?;
    if got != want {
        return Err(format!("{key} is {got}"));
    }
    Ok(())
}

fn json_string_field(bytes: &[u8], key: &str) -> Result<String, String> {
    let mut index = find_key(bytes, key)?;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) != Some(&b':') {
        return Err(format!("{key} colon"));
    }
    index += 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) != Some(&b'"') {
        return Err(format!("{key} is not a string"));
    }
    index += 1;
    let mut out = String::new();
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            let next = *bytes.get(index + 1).ok_or("truncated escape")?;
            out.push(next as char);
            index += 2;
            continue;
        }
        if byte == b'"' {
            return Ok(out);
        }
        out.push(byte as char);
        index += 1;
    }
    Err(format!("unterminated {key}"))
}

fn find_key(bytes: &[u8], key: &str) -> Result<usize, String> {
    find_key_from(bytes, key).ok_or_else(|| format!("missing {key}"))
}

fn find_key_from(bytes: &[u8], key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let needle = needle.as_bytes();
    let mut index = 0;
    while index + needle.len() <= bytes.len() {
        if &bytes[index..index + needle.len()] == needle && !in_string(bytes, index) {
            return Some(index + needle.len());
        }
        index += 1;
    }
    None
}

fn in_string(bytes: &[u8], pos: usize) -> bool {
    let mut index = 0;
    let mut inside = false;
    while index < pos && index < bytes.len() {
        if inside && bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == b'"' {
            inside = !inside;
        }
        index += 1;
    }
    inside
}

fn match_bracket(bytes: &[u8], open_at: usize) -> Result<usize, String> {
    let open = bytes[open_at];
    let close = if open == b'[' { b']' } else { b'}' };
    let mut depth = 0_i32;
    let mut index = open_at;
    let mut inside = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if inside {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == b'"' {
                inside = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            inside = true;
            index += 1;
            continue;
        }
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth -= 1;
            if depth == 0 {
                return Ok(index);
            }
        }
        index += 1;
    }
    Err("unclosed value".to_owned())
}

fn require_bytes(bytes: &[u8], needle: &[u8]) -> Result<(), String> {
    if bytes.windows(needle.len()).any(|window| window == needle) {
        Ok(())
    } else {
        Err(format!("missing {}", String::from_utf8_lossy(needle)))
    }
}

fn expect_hash(path: &Path, want: &str, label: &str) -> Result<(), String> {
    let got = hex(&sha256(&read_bytes(path)?));
    if got != want {
        return Err(format!("{label} sha256 {got}"));
    }
    println!("{label}={got}");
    Ok(())
}

fn expect_sha_file(path: &Path, want: &str, label: &str) -> Result<(), String> {
    let text = read_string(path)?;
    let got = text.split_whitespace().next().unwrap_or("");
    if got != want {
        return Err(format!("{label} sidecar {got}"));
    }
    Ok(())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))
}

fn read_string(path: &Path) -> Result<String, String> {
    let bytes = read_bytes(path)?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not utf-8", path.display()))
}

fn check_calendar() -> Result<(), String> {
    if days_from_civil(1970, 1, 1) != 0 {
        return Err("calendar epoch".to_owned());
    }
    if days_from_civil_str("2026-10-06")? - days_from_civil_str("2026-10-05")? != 1 {
        return Err("calendar day step".to_owned());
    }
    Ok(())
}

fn days_from_civil_str(text: &str) -> Result<i64, String> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(format!("bad date {text}"));
    }
    let y: i32 = text[0..4].parse().map_err(|_| format!("bad year {text}"))?;
    let m: u32 = text[5..7].parse().map_err(|_| format!("bad month {text}"))?;
    let d: u32 = text[8..10].parse().map_err(|_| format!("bad day {text}"))?;
    Ok(days_from_civil(y, m, d))
}

fn days_from_civil(mut year: i32, month: u32, day: u32) -> i64 {
    year -= i32::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = (year - era * 400) as u32;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era as i64 * 146097 + doe as i64 - 719468
}

fn check_sha256() -> Result<(), String> {
    if hex(&sha256(b"")) != EMPTY_SHA || hex(&sha256(b"abc")) != ABC_SHA {
        return Err("sha256 self-check failed".to_owned());
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(input.len()).expect("len").checked_mul(8).expect("bits");
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut word = [0_u32; 64];
        for index in 0..16 {
            let start = index * 4;
            word[index] = u32::from_be_bytes([chunk[start], chunk[start + 1], chunk[start + 2], chunk[start + 3]]);
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16].wrapping_add(s0).wrapping_add(word[index - 7]).wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(choose).wrapping_add(K[index]).wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        let next = [a, b, c, d, e, f, g, h];
        for (slot, value) in state.iter_mut().zip(next) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0_u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}
