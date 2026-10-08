//! Profile gates and journey checks. `ready` is never inferred from a TCP probe.

use std::collections::{BTreeMap, BTreeSet};

use crate::json::{escape, parse, string_list, Val};
use crate::sha256::sha256_hex;

const STANDARD_MEMORY: i64 = 8 * 1024 * 1024 * 1024;
const STANDARD_DISK: i64 = 20 * 1024 * 1024 * 1024;
const MINIMAL_MEMORY: i64 = 4 * 1024 * 1024 * 1024;
const MINIMAL_DISK: i64 = 8 * 1024 * 1024 * 1024;
const DEV_MEMORY: i64 = 2 * 1024 * 1024 * 1024;
const DEV_DISK: i64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Malformed(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub ready: bool,
    pub body: String,
}

pub fn preflight(input: &str) -> Result<Decision, Malformed> {
    let value = parse(input).map_err(Malformed)?;
    let profile = required_str(&value, "profile")?;
    if !matches!(profile, "standard" | "minimal" | "dev" | "observability") {
        return Err(Malformed("profile".to_owned()));
    }
    reject_fabricated(&value)?;
    let runsc = required_str(&value, "runsc")?;
    if !matches!(runsc, "registered" | "absent") {
        return Err(Malformed("runsc".to_owned()));
    }
    let runtimes = strings(value.arr("runtimes").ok_or_else(|| Malformed("runtimes".to_owned()))?)?;
    let arch = required_str(&value, "arch")?;
    if !matches!(arch, "aarch64" | "x86_64") {
        return Err(Malformed("arch".to_owned()));
    }
    let disk = value.int("disk_free_bytes").ok_or_else(|| Malformed("disk_free_bytes".to_owned()))?;
    let memory = value.int("memory_bytes").ok_or_else(|| Malformed("memory_bytes".to_owned()))?;
    let ports = value.bool("ports_free").ok_or_else(|| Malformed("ports_free".to_owned()))?;
    let secrets = value.bool("secret_mode_ok").ok_or_else(|| Malformed("secret_mode_ok".to_owned()))?;
    let docker = value.bool("docker_at_least_24").ok_or_else(|| Malformed("docker_at_least_24".to_owned()))?;
    let compose = value.bool("compose_v2").ok_or_else(|| Malformed("compose_v2".to_owned()))?;
    let images = value.bool("pinned_images_present").ok_or_else(|| Malformed("pinned_images_present".to_owned()))?;
    let observability = value.bool("observability_images_pinned").ok_or_else(|| Malformed("observability_images_pinned".to_owned()))?;
    let mut reasons = BTreeSet::new();
    if !docker || !compose {
        reasons.insert("docker".to_owned());
    }
    if !images {
        reasons.insert("images".to_owned());
    }
    if !ports {
        reasons.insert("ports".to_owned());
    }
    if !secrets {
        reasons.insert("secrets".to_owned());
    }
    let (memory_min, disk_min) = match profile {
        "standard" => (STANDARD_MEMORY, STANDARD_DISK),
        "minimal" => (MINIMAL_MEMORY, MINIMAL_DISK),
        "dev" => (DEV_MEMORY, DEV_DISK),
        _ => (DEV_MEMORY, DEV_DISK),
    };
    if profile != "observability" && (memory < memory_min || disk < disk_min) {
        reasons.insert("resources".to_owned());
    }
    if matches!(profile, "standard" | "minimal") && runsc != "registered" {
        reasons.insert("runtime_unavailable".to_owned());
    }
    if profile == "observability" && !observability {
        reasons.insert("images_unpinned".to_owned());
    }
    if runtimes.iter().any(|item| item == "runc") && runsc != "registered" && matches!(profile, "standard" | "minimal") {
        reasons.insert("runtime_unavailable".to_owned());
    }
    let allow = reasons.is_empty();
    let deployment = if profile == "dev" && allow { "localprocess" } else if allow { profile } else { "refused" };
    let replica = if profile == "minimal" { "not_claimed" } else { "not_this_profile" };
    let other_arch = if arch == "aarch64" { "x86_64_integration_node" } else { "aarch64_integration_node" };
    let reason_list = reasons.into_iter().collect::<Vec<_>>();
    let body = format!(
        concat!(
            "{{\"allow\":{allow},\"profile\":{profile},\"deployment\":{deployment},",
            "\"production_signed\":false,\"runc_substituted\":false,\"physical_replica\":false,",
            "\"replica\":{replica},\"runsc\":{runsc},\"other_arch\":{other},\"parent_accepted\":false,",
            "\"goal_accepted\":false,\"reasons\":{reasons}}}"
        ),
        allow = allow,
        profile = escape(profile),
        deployment = escape(deployment),
        replica = escape(replica),
        runsc = escape(runsc),
        other = escape(other_arch),
        reasons = string_list(&reason_list),
    );
    Ok(Decision { ready: allow, body })
}

pub fn health(input: &str) -> Result<Decision, Malformed> {
    let value = parse(input).map_err(Malformed)?;
    reject_fabricated(&value)?;
    let profile = required_str(&value, "profile")?;
    if !matches!(profile, "standard" | "minimal" | "dev") {
        return Err(Malformed("profile".to_owned()));
    }
    let exchanges = value.arr("exchanges").ok_or_else(|| Malformed("exchanges".to_owned()))?;
    let (missing, tcp) = exchange_gaps(exchanges)?;
    let ready = missing.is_empty() && tcp.is_empty() && profile_allows_ready(&value, profile)?;
    let body = format!(
        "{{\"ready\":{ready},\"production_signed\":false,\"parent_accepted\":false,\"goal_accepted\":false,\"tcp_only\":{tcp},\"missing\":{missing}}}",
        ready = ready,
        tcp = string_list(&tcp),
        missing = string_list(&missing),
    );
    Ok(Decision { ready, body })
}

pub fn journey(input: &str) -> Result<Decision, Malformed> {
    let value = parse(input).map_err(Malformed)?;
    reject_fabricated(&value)?;
    let profile = required_str(&value, "profile")?;
    if !matches!(profile, "standard" | "minimal" | "dev") {
        return Err(Malformed("profile".to_owned()));
    }
    let mut reasons = BTreeSet::new();
    let exchanges = value.arr("exchanges").ok_or_else(|| Malformed("exchanges".to_owned()))?;
    let (missing, tcp) = exchange_gaps(exchanges)?;
    if !missing.is_empty() {
        reasons.insert("journey_incomplete".to_owned());
    }
    if !tcp.is_empty() {
        reasons.insert("tcp_only".to_owned());
    }
    match profile_allows_ready(&value, profile) {
        Ok(true) => {}
        Ok(false) => {
            reasons.insert("profile".to_owned());
        }
        Err(error) => return Err(error),
    }
    compare_snapshot(&value, "before", "after_repeat", "repeat_drift", &mut reasons)?;
    compare_snapshot(&value, "before", "after_restart", "restart_drift", &mut reasons)?;
    secret_modes(&value, &mut reasons)?;
    leaks(&value, &mut reasons)?;
    let ready = reasons.is_empty();
    let reason_list = reasons.into_iter().collect::<Vec<_>>();
    let body = format!(
        "{{\"ready\":{ready},\"production_signed\":false,\"parent_accepted\":false,\"goal_accepted\":false,\"deployment\":{deployment},\"reasons\":{reasons},\"missing\":{missing}}}",
        ready = ready,
        deployment = escape(if profile == "dev" { "localprocess" } else { profile }),
        reasons = string_list(&reason_list),
        missing = string_list(&missing),
    );
    Ok(Decision { ready, body })
}

pub fn identity_commit(input: &str) -> Result<Decision, Malformed> {
    let value = parse(input).map_err(Malformed)?;
    let text = canonical_preimage(&value)?;
    let commit = sha256_hex(text.as_bytes()).ok_or_else(|| Malformed("hash".to_owned()))?;
    Ok(Decision { ready: true, body: format!("{{\"config_commit\":{}}}", escape(&commit)) })
}

pub fn sbom(input: &str) -> Result<Decision, Malformed> {
    let value = parse(input).map_err(Malformed)?;
    let units = value.arr("units").ok_or_else(|| Malformed("units".to_owned()))?;
    let mut packages = String::from("[");
    let mut first = true;
    for unit in units {
        let name = required_unit_str(unit, "name")?;
        let commit = required_unit_str(unit, "commit")?;
        let status = required_unit_str(unit, "status")?;
        if !matches!(status, "fixed" | "recipe_only" | "image_built" | "not_started") {
            return Err(Malformed("status".to_owned()));
        }
        let lock_text = unit.string("lock_text").unwrap_or("");
        let parsed = if lock_text.is_empty() { Vec::new() } else { lock_packages(lock_text) };
        if !first {
            packages.push(',');
        }
        first = false;
        packages.push_str(&format!(
            "{{\"name\":{},\"commit\":{},\"status\":{},\"locked\":{},\"packages\":{}}}",
            escape(name),
            escape(commit),
            escape(status),
            !parsed.is_empty(),
            package_list(&parsed),
        ));
    }
    packages.push(']');
    let body = format!(
        "{{\"spdx_version\":\"SPDX-2.3\",\"name\":\"oo-product-stack-inputs\",\"data_license\":\"CC0-1.0\",\"parent_accepted\":false,\"note\":\"checksums are copied from the supplied Cargo.lock text; recipe_only units are not images\",\"packages\":{packages}}}"
    );
    Ok(Decision { ready: true, body })
}

fn profile_allows_ready(value: &Val, profile: &str) -> Result<bool, Malformed> {
    match profile {
        "dev" => {
            if required_str(value, "deployment")? != "localprocess" {
                return Ok(false);
            }
            if required_str(value, "runsc")? != "not_used" {
                return Ok(false);
            }
            Ok(true)
        }
        "standard" | "minimal" => Ok(required_str(value, "runsc")? == "registered"),
        _ => Err(Malformed("profile".to_owned())),
    }
}

fn exchange_gaps(exchanges: &[Val]) -> Result<(Vec<String>, Vec<String>), Malformed> {
    let mut seen = BTreeMap::new();
    let mut tcp = Vec::new();
    for exchange in exchanges {
        let name = required_unit_str(exchange, "name")?;
        if seen.insert(name.to_owned(), true).is_some() {
            return Err(Malformed("duplicate_exchange".to_owned()));
        }
        let http = exchange.int("http").ok_or_else(|| Malformed("http".to_owned()))?;
        let tcp_only = exchange.bool("tcp_only").ok_or_else(|| Malformed("tcp_only".to_owned()))?;
        if tcp_only {
            tcp.push(name.to_owned());
            continue;
        }
        let body = exchange.string("body").ok_or_else(|| Malformed("body".to_owned()))?;
        let parsed = parse(body).map_err(|_| Malformed("body".to_owned()))?;
        if !step_ok(name, http, &parsed) {
            seen.insert(name.to_owned(), false);
        }
    }
    let mut missing = Vec::new();
    for name in ["oidc_current", "catalog", "pack", "release", "session_close", "audit_ack"] {
        if seen.get(name) != Some(&true) {
            missing.push(name.to_owned());
        }
    }
    Ok((missing, tcp))
}

fn step_ok(name: &str, http: i64, body: &Val) -> bool {
    if http != 200 {
        return false;
    }
    match name {
        "oidc_current" => oidc_ok(body),
        "catalog" => catalog_ok(body),
        "pack" => pack_ok(body),
        "release" => release_ok(body),
        "session_close" => session_ok(body),
        "audit_ack" => audit_ok(body),
        _ => false,
    }
}

fn oidc_ok(body: &Val) -> bool {
    body.string("contract") == Some("oo.control-identity/v1")
        && body.string("operation") == Some("getIdentityCurrent")
        && body.string("install_state") == Some("initialized")
        && body.string("authority") == Some("management_session")
        && body.string("customer_integration") == Some("not_claimed")
        && body.string("data_grants") == Some("none")
}

fn catalog_ok(body: &Val) -> bool {
    let rows = body.get("catalog").and_then(|item| item.int("rows"));
    body.string("contract") == Some("oo.source-control.exchange/v1")
        && body.string("operation") == Some("getSourceCatalog")
        && rows == Some(100_000)
        && body.bool("published") == Some(false)
        && body.string("build_review") == Some("not_claimed")
        && body.string("release") == Some("not_claimed")
        && body.string("customer_integration") == Some("not_claimed")
}

fn pack_ok(body: &Val) -> bool {
    let packs = body.get("producer").and_then(|item| item.get("product")).and_then(|item| item.arr("packs"));
    let publication = body.get("producer").and_then(|item| item.get("statements")).and_then(|item| item.bool("model_publication"));
    body.string("contract") == Some("oo.selfbuild-control-api/v1")
        && matches!(body.string("operation"), Some("compileBuildPack" | "getBuildPack"))
        && body.bool("published") == Some(false)
        && body.bool("active") == Some(false)
        && body.string("compiled_pack") == Some("not_published")
        && body.string("data_grants") == Some("none")
        && body.string("management_identity") == Some("verified")
        && body.bool("producer_authenticated") == Some(false)
        && body.get("producer").and_then(|item| item.string("protocol")) == Some("oo.pg-builder.pack-combined/v1")
        && body.get("producer").and_then(|item| item.bool("authenticated")) == Some(false)
        && packs.is_some_and(|items| items.len() == 2)
        && publication != Some(true)
}

fn release_ok(body: &Val) -> bool {
    body.string("contract") == Some("oo.release-control-api/v1")
        && body.bool("production_ready") == Some(false)
        && body.bool("full_sp08_accepted") == Some(false)
        && body.string("certificate_class") == Some("test_scope_not_production")
        && body.bool("customer_gate_verified") == Some(false)
        && body.bool("runsc_verified") == Some(false)
}

fn session_ok(body: &Val) -> bool {
    if body.bool("listening") == Some(true) {
        return false;
    }
    let steps = body.arr("steps");
    body.string("contract") == Some("session-control-api-v1")
        && body.string("phase") == Some("closed")
        && body.string("deployment") == Some("localprocess")
        && body.bool("production_signed") == Some(false)
        && body.bool("retryable") == Some(false)
        && body.bool("reducer_physical_release_certified") == Some(false)
        && matches!(body.string("disposition"), Some("closed" | "replay"))
        && steps.is_some_and(steps_ok)
}

fn steps_ok(steps: &[Val]) -> bool {
    if steps.len() != 11 {
        return false;
    }
    const IDS: [&str; 11] = ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11"];
    for id in IDS {
        let Some(step) = steps.iter().find(|step| step.string("id") == Some(id)) else {
            return false;
        };
        let hash = step.string("evidence_sha256").unwrap_or("");
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return false;
        }
        let status = step.string("status").unwrap_or("");
        let absence = step.bool("actual_absence");
        let ok = match id {
            "s8" => status == "confirmed" && absence == Some(true),
            "s5" => matches!(status, "skipped" | "confirmed") && absence == Some(false),
            _ => status == "confirmed",
        };
        if !ok {
            return false;
        }
    }
    true
}

fn audit_ok(body: &Val) -> bool {
    let spool = body.string("spool_id").unwrap_or("");
    let request = body.string("request_id").unwrap_or("");
    let receipt = body.string("receipt_id").unwrap_or("");
    let hash = body.string("payload_hash").unwrap_or("");
    body.string("protocol") == Some("oo.audit.ingest/v1")
        && uuid_v4(spool)
        && uuid_v4(request)
        && uuid_v4(receipt)
        && hash.len() == 64
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && body.int("event_seq").is_some_and(|value| (1..=1_000_000).contains(&value))
}

fn uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let ok = match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            14 => *byte == b'4',
            19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase(),
        };
        if !ok {
            return false;
        }
    }
    true
}

fn compare_snapshot(value: &Val, left_name: &str, right_name: &str, reason: &str, reasons: &mut BTreeSet<String>) -> Result<(), Malformed> {
    let left = value.get(left_name).ok_or_else(|| Malformed(left_name.to_owned()))?;
    let right = value.get(right_name).ok_or_else(|| Malformed(right_name.to_owned()))?;
    let left_hash = required_unit_str(left, "secret_sha256")?;
    let right_hash = required_unit_str(right, "secret_sha256")?;
    if !is_hex64(left_hash) || left_hash != right_hash || rows_of(left)? != rows_of(right)? {
        reasons.insert(reason.to_owned());
    }
    Ok(())
}

fn rows_of(value: &Val) -> Result<BTreeMap<String, i64>, Malformed> {
    let rows = value.get("rows").and_then(Val::obj).ok_or_else(|| Malformed("rows".to_owned()))?;
    let mut out = BTreeMap::new();
    for (key, item) in rows {
        let Val::Int(count) = item else {
            return Err(Malformed("rows".to_owned()));
        };
        if *count < 0 {
            return Err(Malformed("rows".to_owned()));
        }
        out.insert(key.clone(), *count);
    }
    Ok(out)
}

fn secret_modes(value: &Val, reasons: &mut BTreeSet<String>) -> Result<(), Malformed> {
    let files = value.arr("secret_files").ok_or_else(|| Malformed("secret_files".to_owned()))?;
    if files.is_empty() {
        reasons.insert("secrets".to_owned());
        return Ok(());
    }
    for file in files {
        let path = required_unit_str(file, "path")?;
        let mode = required_unit_str(file, "mode")?;
        if path.contains("..") || path.starts_with('/') || path.contains("HOME") || !matches!(mode, "0600" | "0400") {
            reasons.insert("secrets".to_owned());
        }
    }
    Ok(())
}

fn leaks(value: &Val, reasons: &mut BTreeSet<String>) -> Result<(), Malformed> {
    let owned = strings(value.arr("resources_after_cleanup").ok_or_else(|| Malformed("resources_after_cleanup".to_owned()))?)?;
    if !owned.is_empty() {
        reasons.insert("leaked_resources".to_owned());
    }
    let before = strings(value.arr("foreign_before").ok_or_else(|| Malformed("foreign_before".to_owned()))?)?;
    let after = strings(value.arr("foreign_after").ok_or_else(|| Malformed("foreign_after".to_owned()))?)?;
    let mut left = before;
    let mut right = after;
    left.sort();
    right.sort();
    if left != right {
        reasons.insert("foreign_touched".to_owned());
    }
    Ok(())
}

fn canonical_preimage(value: &Val) -> Result<String, Malformed> {
    let audience = required_str(value, "audience")?;
    let client = required_str(value, "client_id")?;
    let issuer = required_str(value, "issuer")?;
    let revision = value.int("revision").ok_or_else(|| Malformed("revision".to_owned()))?;
    let redirects = strings(value.arr("redirect_uris").ok_or_else(|| Malformed("redirect_uris".to_owned()))?)?;
    let mut redirects = redirects;
    redirects.sort();
    let bindings = binding_list(value.arr("bindings").ok_or_else(|| Malformed("bindings".to_owned()))?)?;
    let bootstrap = one_binding(value.get("bootstrap").ok_or_else(|| Malformed("bootstrap".to_owned()))?)?;
    let mut binding_json = String::from("[");
    for (index, item) in bindings.iter().enumerate() {
        if index > 0 {
            binding_json.push(',');
        }
        binding_json.push_str(item);
    }
    binding_json.push(']');
    let mut redirect_json = String::from("[");
    for (index, item) in redirects.iter().enumerate() {
        if index > 0 {
            redirect_json.push(',');
        }
        redirect_json.push_str(&escape(item));
    }
    redirect_json.push(']');
    Ok(format!(
        "{{\"audience\":{audience},\"bindings\":{bindings},\"bootstrap\":{{\"customer_owner\":false,\"fixture_class\":\"explicit_test_fixture\",\"project_id\":{project},\"role\":{role},\"subject\":{subject}}},\"client_id\":{client},\"issuer\":{issuer},\"protocol\":\"oo.control-identity/config-commit/v1\",\"redirect_uris\":{redirects},\"revision\":{revision}}}",
        audience = escape(audience),
        bindings = binding_json,
        project = escape(&bootstrap.0),
        role = escape(&bootstrap.1),
        subject = escape(&bootstrap.2),
        client = escape(client),
        issuer = escape(issuer),
        redirects = redirect_json,
        revision = revision,
    ))
}

fn binding_list(items: &[Val]) -> Result<Vec<String>, Malformed> {
    let mut rows = Vec::new();
    for item in items {
        let (project, role, subject) = one_binding(item)?;
        rows.push((project, subject, role));
    }
    rows.sort();
    Ok(rows.into_iter().map(|(project, subject, role)| format!("{{\"project_id\":{},\"role\":{},\"subject\":{}}}", escape(&project), escape(&role), escape(&subject))).collect())
}

fn one_binding(value: &Val) -> Result<(String, String, String), Malformed> {
    Ok((
        required_unit_str(value, "project_id")?.to_owned(),
        required_unit_str(value, "role")?.to_owned(),
        required_unit_str(value, "subject")?.to_owned(),
    ))
}

fn lock_packages(text: &str) -> Vec<(String, String, Option<String>)> {
    let mut out = Vec::new();
    let mut name = None;
    let mut version = None;
    let mut checksum = None;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            push_package(&mut out, &mut name, &mut version, &mut checksum);
            in_package = true;
            continue;
        }
        if !in_package || line.starts_with('[') {
            if line.starts_with('[') && line != "[[package]]" {
                in_package = false;
            }
            continue;
        }
        if let Some(value) = quoted(line, "name") {
            name = Some(value);
        } else if let Some(value) = quoted(line, "version") {
            version = Some(value);
        } else if let Some(value) = quoted(line, "checksum") {
            checksum = Some(value);
        }
    }
    push_package(&mut out, &mut name, &mut version, &mut checksum);
    out
}

fn push_package(out: &mut Vec<(String, String, Option<String>)>, name: &mut Option<String>, version: &mut Option<String>, checksum: &mut Option<String>) {
    if let (Some(name_value), Some(version_value)) = (name.take(), version.take()) {
        out.push((name_value, version_value, checksum.take()));
    } else {
        *checksum = None;
    }
}

fn quoted(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"");
    let rest = line.strip_prefix(&prefix)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn package_list(items: &[(String, String, Option<String>)]) -> String {
    let mut out = String::from("[");
    for (index, (name, version, checksum)) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let sum = checksum.as_ref().map(|item| escape(item)).unwrap_or_else(|| "null".to_owned());
        out.push_str(&format!("{{\"name\":{},\"version\":{},\"checksum\":{sum}}}", escape(name), escape(version)));
    }
    out.push(']');
    out
}

fn reject_fabricated(value: &Val) -> Result<(), Malformed> {
    for key in ["production_signed", "parent_accepted", "goal_accepted", "runc_substituted"] {
        if value.bool(key) == Some(true) {
            return Err(Malformed(key.to_owned()));
        }
    }
    Ok(())
}

fn required_str<'a>(value: &'a Val, key: &str) -> Result<&'a str, Malformed> {
    value.string(key).ok_or_else(|| Malformed(key.to_owned()))
}

fn required_unit_str<'a>(value: &'a Val, key: &str) -> Result<&'a str, Malformed> {
    required_str(value, key)
}

fn strings(items: &[Val]) -> Result<Vec<String>, Malformed> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Val::Str(text) => out.push(text.clone()),
            _ => return Err(Malformed("string".to_owned())),
        }
    }
    Ok(out)
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
