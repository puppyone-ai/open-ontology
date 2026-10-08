//! Local JSON helpers for the pack-dialogue loop. No secrets are printed.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

const DIGEST: &str = "e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return ExitCode::from(2);
    };
    let result = match command.as_str() {
        "extract-pack" => extract_pack(&arg(&mut args), &arg(&mut args)),
        "scan-secret" => scan_secret(&arg(&mut args), &arg(&mut args)),
        "prepare" => prepare(&arg(&mut args)),
        "gateway" => gateway(&arg(&mut args), &arg(&mut args), &arg(&mut args), &arg(&mut args)),
        _ => Err("usage"),
    };
    match result {
        Ok(()) => ExitCode::from(0),
        Err("secret_present") => ExitCode::from(3),
        Err(_) => ExitCode::from(1),
    }
}

fn arg(args: &mut impl Iterator<Item = String>) -> String {
    args.next().unwrap_or_default()
}

fn extract_pack(bundle_path: &str, pack_path: &str) -> Result<(), &'static str> {
    let text = fs::read_to_string(bundle_path).map_err(|_| "read")?;
    let raw = object_field(&text, "pack").ok_or("pack")?;
    if !raw.contains(DIGEST) || !raw.contains("know_a317") || raw.contains("know-a317") {
        return Err("identity");
    }
    fs::write(pack_path, raw.as_bytes()).map_err(|_| "write")?;
    println!("pack_bytes={}", raw.len());
    Ok(())
}

fn scan_secret(path: &str, bearer_path: &str) -> Result<(), &'static str> {
    let bearer = fs::read(bearer_path).map_err(|_| "bearer")?;
    let body = fs::read(path).map_err(|_| "read")?;
    if bearer.is_empty() {
        return Err("bearer");
    }
    if body.windows(bearer.len()).any(|window| window == bearer.as_slice()) {
        println!("secret_present");
        return Err("secret_present");
    }
    println!("secret_absent");
    Ok(())
}

fn prepare(out_dir: &str) -> Result<(), &'static str> {
    let (install, provision, _) = fixture_docs();
    fs::write(format!("{out_dir}/install.json"), install).map_err(|_| "write")?;
    fs::write(format!("{out_dir}/provision.json"), provision).map_err(|_| "write")?;
    Ok(())
}

fn fixture_docs() -> (String, String, String) {
    let column = "{\"name\":\"entry_count\",\"pii\":{\"class\":\"none\"},\"semantic_type\":\"value\",\"status\":\"confirmed\"}";
    let policy_doc = format!(
        "{{\"project\":\"shop\",\"config_digest\":{digest},\"generation\":1,\"role\":\"clerk\",\"login\":\"oo_a399_login\",\"table\":{{\"namespace\":\"public\",\"name\":\"entry\",\"columns\":[{column}]}},\"row_filter\":null}}",
        digest = quote(DIGEST),
        column = column,
    );
    let admin_endpoint = "{\"host\":\"localhost\",\"port\":5432,\"database\":\"oo_a399\",\"user\":\"oo_fixture_admin\"}";
    let install = format!(
        "{{\"protocol\":\"oo.gateway.install-column-acl-cli/v1\",\"policies\":[{policy}],\"endpoint\":{endpoint},\"password_file\":\"/secrets/admin.pass\",\"ca_file\":\"/secrets/ca.crt\"}}\n",
        policy = policy_doc,
        endpoint = admin_endpoint,
    );
    let provision = format!(
        "{{\"protocol\":\"oo.gateway.provision-mapping-reader-cli/v1\",\"policies\":[{policy}],\"login\":\"oo_a399_login\",\"endpoint\":{endpoint},\"password_file\":\"/secrets/admin.pass\",\"ca_file\":\"/secrets/ca.crt\"}}\n",
        policy = policy_doc,
        endpoint = admin_endpoint,
    );
    (install, provision, policy_doc)
}

fn gateway(proposal_path: &str, pack_path: &str, query_sha: &str, out_dir: &str) -> Result<(), &'static str> {
    let proposal = fs::read_to_string(proposal_path).map_err(|_| "read")?;
    let sql = json_string(&proposal, "sql").ok_or("sql")?;
    let role = json_string(&proposal, "role").ok_or("role")?;
    let project = json_string(&proposal, "project").ok_or("project")?;
    let policy = json_string(&proposal, "policy_id").ok_or("policy")?;
    let proposal_sha = json_string(&proposal, "proposal_sha256").ok_or("proposal_sha")?;
    let sql_sha = json_string(&proposal, "sql_sha256").ok_or("sql_sha")?;
    let tuple_sha = json_string(&proposal, "tuple_sha256").ok_or("tuple_sha")?;
    let pack_digest = json_string(&proposal, "pack_digest").ok_or("pack_digest")?;
    let slug = role.strip_prefix("oo_r_").ok_or("role")?;
    if policy != DIGEST || project != "shop" || role != "oo_r_clerk" {
        return Err("binding");
    }
    if query_sha.len() != 64 {
        return Err("sha");
    }
    let approval = format!(
        "{{\"protocol\":\"oo.ontology-dialogue.approval/v1\",\"trust\":\"local_operator_declared\",\"authenticated\":false,\"proposal_sha256\":{proposal},\"sql_sha256\":{sql_sha},\"tuple_sha256\":{tuple},\"pack_digest\":{pack}}}\n",
        proposal = quote(&proposal_sha),
        sql_sha = quote(&sql_sha),
        tuple = quote(&tuple_sha),
        pack = quote(&pack_digest),
    );
    let _ = slug;
    let (_, _, policy_doc) = fixture_docs();
    let endpoint = "{\"host\":\"localhost\",\"port\":5432,\"database\":\"oo_a399\",\"user\":\"oo_a399_login\"}";
    let request = format!(
        "{{\"version\":\"oo.gateway.query-pg/v1\",\"endpoint\":{endpoint},\"password_file\":\"/secrets/login.pass\",\"ca_file\":\"/secrets/ca.crt\",\"audit_file\":\"/work/audit.jsonl\",\"identity\":{{\"project\":\"shop\",\"role\":{role},\"policy_id\":{digest}}},\"policies\":[{policy}],\"catalog\":{{\"max_tables\":4,\"max_columns\":64,\"max_functions\":1,\"max_operators\":64,\"large_table_row_threshold\":1000000,\"unknown_rows\":\"refuse\"}},\"statement\":{{\"source\":\"request\",\"sql\":{sql},\"max_rows\":20,\"max_result_bytes\":4096,\"wall_clock_ms\":30000}}}}\n",
        endpoint = endpoint,
        role = quote(&role),
        digest = quote(DIGEST),
        policy = policy_doc,
        sql = quote(&sql),
    );
    let execute = format!(
        "{{\"protocol\":\"oo.ontology-dialogue.execute-config/v1\",\"proposal_file\":\"/work/proposal.json\",\"approval_file\":\"/work/approval.json\",\"pack_file\":{pack},\"request_file\":\"/work/request.json\",\"query_pg\":{{\"path\":\"/bins/query-pg\",\"sha256\":{sha}}},\"binding\":{{\"session_id\":\"sess-a399\",\"turn_id\":\"turn-a399\",\"declares_data\":true,\"render_rows\":true}},\"budget\":{{\"deadline_ms\":30000,\"max_stdout_bytes\":1048576}}}}\n",
        pack = quote(pack_path),
        sha = quote(query_sha),
    );
    fs::write(format!("{out_dir}/approval.json"), approval).map_err(|_| "write")?;
    fs::write(format!("{out_dir}/request.json"), request).map_err(|_| "write")?;
    fs::write(format!("{out_dir}/execute.json"), execute).map_err(|_| "write")?;
    let select = sql.trim_start().len() >= 6 && sql.trim_start()[..6].eq_ignore_ascii_case("select");
    println!("proposal_sql_bytes={}", sql.len());
    println!("proposal_starts_select={}", if select { "yes" } else { "no" });
    println!("policy_id={policy}");
    Ok(())
}

fn object_field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":");
    let start = text.find(&pattern)? + pattern.len();
    let rest = text[start..].trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    Some(take_value(rest)?.to_string())
}

fn json_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":");
    let mut search = text;
    while let Some(index) = search.find(&pattern) {
        let rest = search[index + pattern.len()..].trim_start();
        if rest.starts_with('"') {
            return unescape(rest);
        }
        search = &search[index + pattern.len()..];
    }
    None
}

fn take_value(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'{') && bytes.first() != Some(&b'[') {
        return None;
    }
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if *byte == b'\\' {
                escape = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn unescape(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut out = String::new();
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'"' {
            return Some(out);
        }
        if byte == b'\\' {
            index += 1;
            let escaped = *bytes.get(index)?;
            match escaped {
                b'"' | b'\\' | b'/' => out.push(escaped as char),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => return None,
                _ => return None,
            }
        } else {
            out.push(byte as char);
        }
        index += 1;
    }
    None
}

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for byte in value.bytes() {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\u{byte:04x}")),
        }
    }
    out.push('"');
    out
}
