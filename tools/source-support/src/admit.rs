//! Target selection and registration admission. Explicit capabilities are not silently downgraded.

use std::path::Path;

use crate::evidence::{self, EvidenceFault};
use crate::json::Json;
use crate::matrix::{canonical_capability, level_ok, version_fit, Matrix, Target, VersionFit, LEVELS};
use crate::output::{strings, Outcome};
use crate::register::{self, schema_drift, RegisterError, FILE_FORMATS, NOT_CLAIMED_FORMATS};

#[derive(Clone, Debug)]
struct Detected {
    family: String,
    product: String,
    version: String,
    variant: String,
    driver_path: String,
    driver_impl: Option<String>,
    capability: Option<String>,
    profile: String,
    sync_auto: bool,
    experimental: bool,
    compat_mode: Option<String>,
    driver_present: bool,
    patch_evidence: Option<String>,
    format: Option<String>,
}

struct Hit<'a> {
    target: &'a Target,
    capability: String,
    level: String,
    fit: VersionFit,
}

pub fn check(matrix_text: &str, matrix: &Matrix, doc: &str, evidence: Option<&Path>, publication_public: bool) -> Outcome {
    let mut outcome = gate_matrix(matrix_text, matrix, "check");
    if outcome.exit_code != 0 {
        return outcome;
    }
    if publication_public && matrix.targets.iter().any(|target| target.scope.as_deref() == Some("deployment-local")) {
        return fail(matrix_text, "check", 3, "PRIVATE_SCOPE", "deployment_local");
    }
    if doc_drift(doc) {
        return fail(matrix_text, "check", 3, "DOC_DRIFT", "doc");
    }
    if let Some(fault) = evidence_faults(matrix, evidence, None) {
        return evidence_outcome(matrix_text, "check", fault);
    }
    let verified = verified_count(matrix);
    outcome = outcome
        .field("target_count", Json::Int(matrix.targets.len() as i64))
        .field("verified_count", Json::Int(verified))
        .field("levels", strings(&matrix.levels));
    outcome
}

pub fn supported(matrix_text: &str, matrix: &Matrix) -> Outcome {
    let outcome = gate_matrix(matrix_text, matrix, "supported");
    if outcome.exit_code != 0 {
        return outcome;
    }
    let mut supported_rows = Vec::new();
    let mut experimental_rows = Vec::new();
    for target in &matrix.targets {
        if target.scope.as_deref() == Some("deployment-local") {
            continue;
        }
        for (capability, level) in &target.caps {
            if level == "verified" || level == "certified" {
                supported_rows.push(row(target, capability, level));
            } else if level == "experimental" {
                experimental_rows.push(row(target, capability, level));
            }
        }
    }
    Outcome::new("supported", true, 0, "SUPPORTED", "listed", matrix_text)
        .field("target_count", Json::Int(matrix.targets.len() as i64))
        .field("verified_count", Json::Int(supported_rows.len() as i64))
        .field("supported", Json::Array(supported_rows))
        .field("experimental", Json::Array(experimental_rows))
        .field("production_note", Json::String("only verified and certified capabilities are supported".to_string()))
}

pub fn decide(
    operation: &'static str,
    matrix_text: &str,
    matrix: &Matrix,
    request: &Json,
    evidence: Option<&Path>,
    schema: Option<&Json>,
) -> Outcome {
    let outcome = gate_matrix(matrix_text, matrix, operation);
    if outcome.exit_code != 0 {
        return outcome;
    }
    if request.get("contract").and_then(Json::string) != Some("oo.source-support.admit/v1") {
        return fail(matrix_text, operation, 1, "malformed", "contract");
    }
    if unknown_keys(request, &["contract", "detected", "registration"]) {
        return fail(matrix_text, operation, 1, "malformed", "json");
    }
    let has_registration = request.get("registration").is_some();
    if operation == "select" && has_registration {
        return fail(matrix_text, operation, 1, "malformed", "registration");
    }
    if operation == "admit" && !has_registration {
        return fail(matrix_text, operation, 1, "malformed", "registration");
    }
    let Some(detected_json) = request.get("detected") else {
        return fail(matrix_text, operation, 1, "malformed", "detected");
    };
    let registration = match request.get("registration") {
        None => None,
        Some(value) => {
            if let Some(schema) = schema {
                if schema_drift(schema) {
                    return fail(matrix_text, operation, 3, "CONTRACT_DRIFT", "register_schema");
                }
            } else {
                return fail(matrix_text, operation, 1, "malformed", "register_schema");
            }
            match register::parse_registration(value) {
                Ok(parsed) => Some(parsed),
                Err(error) => return register_fail(matrix_text, operation, error),
            }
        }
    };
    let detected = match parse_detected(detected_json) {
        Ok(value) => value,
        Err(detail) => return fail(matrix_text, operation, 1, "malformed", detail),
    };
    let mut registration_state = "not_applicable";
    if let Some(parsed) = &registration {
        registration_state = "validated_not_submitted";
        if let Some(format) = detected.format.as_deref() {
            if NOT_CLAIMED_FORMATS.contains(&format) || !FILE_FORMATS.contains(&format) {
                return fail(matrix_text, operation, 2, "CONNECTOR_UNSUPPORTED", "source_control_not_claimed")
                    .field("source_control_registration", Json::String(registration_state.to_string()))
                    .field("source_ref", Json::String(parsed.source_ref.clone()))
                    .field("project_id", Json::String(parsed.project_id.clone()));
            }
        }
        if parsed.kind == "pg_local" && (detected.family != "postgres" || detected.product != "PostgreSQL") {
            return fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "registration_identity")
                .field("source_control_registration", Json::String(registration_state.to_string()));
        }
        if parsed.kind == "file_local" && detected.family != "file" {
            return fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "registration_identity")
                .field("source_control_registration", Json::String(registration_state.to_string()));
        }
        if parsed.kind == "file_local" && detected.format.as_deref().is_none_or(|format| !FILE_FORMATS.contains(&format)) {
            return fail(matrix_text, operation, 2, "CONNECTOR_UNSUPPORTED", "source_control_not_claimed")
                .field("source_control_registration", Json::String(registration_state.to_string()));
        }
    }
    match select(operation, matrix_text, matrix, &detected) {
        Ok(hit) => finish(operation, matrix_text, evidence, &detected, &hit, registration_state, registration.as_ref().map(|item| (item.project_id.as_str(), item.source_ref.as_str()))),
        Err(error) => error
            .field("source_control_registration", Json::String(registration_state.to_string()))
            .field("source_control_host_allowlist", Json::String("not_evaluated".to_string()))
            .field("secret_resolution", Json::String("not_evaluated".to_string())),
    }
}

fn finish(
    operation: &'static str,
    matrix_text: &str,
    evidence_root: Option<&Path>,
    detected: &Detected,
    hit: &Hit<'_>,
    registration_state: &str,
    identity: Option<(&str, &str)>,
) -> Outcome {
    let base = |outcome: Outcome| {
        let mut outcome = outcome
            .field("target_id", Json::String(hit.target.id.clone()))
            .field("capability", Json::String(hit.capability.clone()))
            .field("level", Json::String(hit.level.clone()))
            .field("profile", Json::String(detected.profile.clone()))
            .field("limits", strings(&hit.target.limits))
            .field("version_fit", Json::String(fit_name(hit.fit).to_string()))
            .field("existing_source_stopped", Json::Bool(false))
            .field("source_control_registration", Json::String(registration_state.to_string()))
            .field("source_control_host_allowlist", Json::String("not_evaluated".to_string()))
            .field("secret_resolution", Json::String("not_evaluated".to_string()));
        if let Some((project_id, source_ref)) = identity {
            outcome = outcome
                .field("project_id", Json::String(project_id.to_string()))
                .field("source_ref", Json::String(source_ref.to_string()));
        }
        outcome
    };
    if hit.target.scope.as_deref() == Some("deployment-local") {
        return base(fail(matrix_text, operation, 2, "CONNECTOR_UNSUPPORTED", "deployment_local"));
    }
    if hit.target.limits.iter().any(|limit| limit.contains("兼容模式"))
        && detected.compat_mode.as_deref().is_none_or(|mode| mode.is_empty() || mode == "unknown")
    {
        return base(fail(matrix_text, operation, 2, "COMPAT_MODE_UNKNOWN", "compat_mode"));
    }
    if hit.target.driver.as_ref().is_some_and(|driver| driver.path.contains("P2")) && !detected.driver_present {
        return base(fail(matrix_text, operation, 2, "DRIVER_MISSING", "driver"));
    }
    if !level_ok(&hit.level) {
        return base(fail(matrix_text, operation, 3, "MATRIX_LEVEL", "level"));
    }
    if matches!(hit.fit, VersionFit::Patch) {
        let Some(id) = detected.patch_evidence.as_deref() else {
            return base(fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "patch_unproven"));
        };
        let Some(root) = evidence_root else {
            return base(fail(matrix_text, operation, 3, "MISSING_EVIDENCE", "patch_evidence"));
        };
        if let Err(fault) = evidence::verify_patch(root, id) {
            return base(evidence_outcome(matrix_text, operation, fault));
        }
    }
    if hit.level == "candidate" || hit.level == "excluded" {
        let reason = if hit.level == "candidate" { "candidate" } else { "excluded" };
        return base(fail(matrix_text, operation, 2, "CONNECTOR_UNSUPPORTED", reason));
    }
    if hit.level == "designed" {
        let reason = if other_verified(hit.target) { "explicit_not_downgraded" } else { "designed" };
        return base(fail(matrix_text, operation, 2, "CAPABILITY_UNVERIFIED", reason));
    }
    if hit.level == "experimental" {
        if detected.profile == "production" || detected.sync_auto {
            let reason = if detected.profile == "production" { "experimental_production" } else { "sync_auto_empty" };
            return base(fail(matrix_text, operation, 2, "CAPABILITY_UNVERIFIED", reason));
        }
        if !detected.experimental {
            return base(fail(matrix_text, operation, 2, "CAPABILITY_UNVERIFIED", "experimental_not_enabled"));
        }
        let mut warnings = vec!["experimental".to_string()];
        warnings.extend(hit.target.limits.iter().cloned());
        return base(Outcome::new(operation, true, 0, "ADMITTED_EXPERIMENTAL", "experimental", matrix_text))
            .field("warnings", strings(&warnings))
            .field("production_allowed", Json::Bool(false));
    }
    let Some(root) = evidence_root else {
        return base(fail(matrix_text, operation, 3, "MISSING_EVIDENCE", "evidence"));
    };
    let Some(evidence_ref) = evidence::find(hit.target, &hit.capability) else {
        return base(fail(matrix_text, operation, 3, "MISSING_EVIDENCE", "evidence"));
    };
    let version = match hit.fit {
        VersionFit::Exact | VersionFit::Interval => Some(detected.version.as_str()),
        VersionFit::Patch | VersionFit::Absent => None,
    };
    if let Err(fault) = evidence::verify(root, hit.target, evidence_ref, version) {
        return base(evidence_outcome(matrix_text, operation, fault));
    }
    if hit.level == "certified" && (evidence_ref.kind != "customer" || evidence_ref.releases < 3) {
        return base(fail(matrix_text, operation, 3, "EVIDENCE_INCOMPLETE", "certified_customer"));
    }
    let reason = if hit.level == "certified" { "certified" } else { "verified" };
    base(Outcome::new(operation, true, 0, "ADMITTED", reason, matrix_text))
        .field("warnings", Json::Array(Vec::new()))
        .field("production_allowed", Json::Bool(true))
        .field("evidence_id", Json::String(evidence_ref.id.clone()))
}

fn select<'a>(operation: &'static str, matrix_text: &str, matrix: &'a Matrix, detected: &Detected) -> Result<Hit<'a>, Outcome> {
    let family: Vec<_> = matrix.targets.iter().filter(|target| target.family == detected.family).collect();
    if family.is_empty() {
        return Err(fail(matrix_text, operation, 2, "CONNECTOR_UNSUPPORTED", "family_absent"));
    }
    let product: Vec<_> = family.into_iter().filter(|target| target.product == detected.product).collect();
    if product.is_empty() {
        return Err(fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "product"));
    }
    let mut version: Vec<(&Target, VersionFit)> = Vec::new();
    for target in product {
        if let Some(fit) = version_fit(&target.versions, &detected.version) {
            version.push((target, fit));
        }
    }
    if version.is_empty() {
        return Err(fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "version"));
    }
    if version.iter().all(|(_, fit)| *fit == VersionFit::Patch) && detected.patch_evidence.is_none() {
        return Err(fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "patch_unproven"));
    }
    let variant: Vec<_> = version
        .into_iter()
        .filter(|(target, _)| match &target.variant {
            Some(value) => value == &detected.variant,
            None => detected.variant.is_empty(),
        })
        .collect();
    if variant.is_empty() {
        return Err(fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "variant"));
    }
    let driver: Vec<_> = variant
        .into_iter()
        .filter(|(target, _)| match &target.driver {
            Some(driver) => driver.path == detected.driver_path && driver.impl_name == detected.driver_impl,
            None => detected.driver_path.is_empty() && detected.driver_impl.is_none(),
        })
        .collect();
    if driver.is_empty() {
        return Err(fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "driver"));
    }
    if driver.len() != 1 {
        return Err(fail(matrix_text, operation, 2, "TARGET_VERSION_UNVERIFIED", "ambiguous"));
    }
    let (target, fit) = driver[0];
    if detected.sync_auto {
        let mut usable = Vec::new();
        for (capability, level) in &target.caps {
            if level == "verified" || level == "certified" {
                usable.push(capability.clone());
            }
        }
        if usable.len() != 1 {
            return Err(fail(matrix_text, operation, 2, "CAPABILITY_UNVERIFIED", "sync_auto_empty"));
        }
        let capability = usable.pop().unwrap_or_default();
        let level = target.caps.get(&capability).cloned().unwrap_or_default();
        return Ok(Hit { target, capability, level, fit });
    }
    let Some(capability) = detected.capability.clone() else {
        return Err(fail(matrix_text, operation, 1, "malformed", "capability"));
    };
    let Some(level) = target.caps.get(&capability).cloned() else {
        let reason = if other_verified(target) { "explicit_not_downgraded" } else { "capability_absent" };
        return Err(fail(matrix_text, operation, 2, "CAPABILITY_UNVERIFIED", reason).field("target_id", Json::String(target.id.clone())));
    };
    Ok(Hit { target, capability, level, fit })
}

fn parse_detected(value: &Json) -> Result<Detected, &'static str> {
    if unknown_keys(
        value,
        &[
            "family",
            "product",
            "version",
            "variant",
            "driver",
            "capability",
            "profile",
            "sync",
            "experimental",
            "compat_mode",
            "driver_present",
            "patch_evidence",
            "format",
        ],
    ) {
        return Err("detected");
    }
    let family = required_text(value, "family")?;
    let product = required_text(value, "product")?;
    let version = required_text(value, "version")?;
    let variant = required_text(value, "variant")?;
    let driver = value.get("driver").ok_or("driver")?;
    if unknown_keys(driver, &["path", "impl"]) {
        return Err("driver");
    }
    let driver_path = required_text(driver, "path")?;
    let driver_impl = match driver.get("impl") {
        None | Some(Json::Null) => None,
        Some(Json::String(text)) => Some(text.clone()),
        Some(_) => return Err("driver"),
    };
    let profile = required_text(value, "profile")?;
    if profile != "production" && profile != "non-production" {
        return Err("profile");
    }
    let sync = required_text(value, "sync")?;
    let sync_auto = sync == "auto";
    let capability = if sync_auto {
        if value.get("capability").is_some() {
            return Err("capability");
        }
        None
    } else {
        let name = canonical_capability(&sync).ok_or("capability")?;
        if let Some(extra) = value.get("capability") {
            let extra = canonical_capability(extra.string().ok_or("capability")?).ok_or("capability")?;
            if extra != name {
                return Err("capability");
            }
        }
        Some(name.to_string())
    };
    let experimental = match value.get("experimental") {
        None => false,
        Some(Json::Bool(value)) => *value,
        Some(_) => return Err("experimental"),
    };
    let compat_mode = match value.get("compat_mode") {
        None | Some(Json::Null) => None,
        Some(Json::String(text)) => Some(text.clone()),
        Some(_) => return Err("compat_mode"),
    };
    let driver_present = value.get("driver_present").and_then(Json::bool).ok_or("driver_present")?;
    let patch_evidence = match value.get("patch_evidence") {
        None | Some(Json::Null) => None,
        Some(Json::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(_) => return Err("patch_evidence"),
    };
    let format = match value.get("format") {
        None | Some(Json::Null) => None,
        Some(Json::String(text)) => Some(text.clone()),
        Some(_) => return Err("format"),
    };
    Ok(Detected {
        family,
        product,
        version,
        variant,
        driver_path,
        driver_impl,
        capability,
        profile,
        sync_auto,
        experimental,
        compat_mode,
        driver_present,
        patch_evidence,
        format,
    })
}

fn gate_matrix(matrix_text: &str, matrix: &Matrix, operation: &'static str) -> Outcome {
    if matrix.schema != crate::matrix::MATRIX_SCHEMA {
        return fail(matrix_text, operation, 3, "MATRIX_SCHEMA", "schema");
    }
    if matrix.levels != LEVELS {
        return fail(matrix_text, operation, 3, "MATRIX_LEVELS", "levels");
    }
    if matrix.targets.is_empty() {
        return fail(matrix_text, operation, 3, "MATRIX_EMPTY", "targets");
    }
    let mut ids = Vec::new();
    for target in &matrix.targets {
        if ids.contains(&target.id) {
            return fail(matrix_text, operation, 3, "DUPLICATE_TARGET", "id");
        }
        ids.push(target.id.clone());
        if !target.unknown_caps.is_empty() {
            return fail(matrix_text, operation, 3, "UNKNOWN_CAPABILITY", "capability");
        }
        for level in target.caps.values() {
            if !level_ok(level) {
                return fail(matrix_text, operation, 3, "MATRIX_LEVEL", "level");
            }
        }
        for grade in target.grade.values() {
            if !matches!(grade.as_str(), "T" | "S1" | "S0" | "B") {
                return fail(matrix_text, operation, 3, "MATRIX_GRADE", "grade");
            }
        }
    }
    Outcome::new(operation, true, 0, "CHECKED", "ok", matrix_text)
}

fn evidence_faults(matrix: &Matrix, evidence: Option<&Path>, _detected_version: Option<&str>) -> Option<EvidenceFault> {
    for target in &matrix.targets {
        for (capability, level) in &target.caps {
            if level != "verified" && level != "certified" {
                continue;
            }
            let Some(root) = evidence else {
                return Some(EvidenceFault::Missing);
            };
            let Some(item) = evidence::find(target, capability) else {
                return Some(EvidenceFault::Missing);
            };
            if let Err(fault) = evidence::verify(root, target, item, None) {
                return Some(fault);
            }
            if level == "certified" && (item.kind != "customer" || item.releases < 3) {
                return Some(EvidenceFault::Incomplete("certified_customer"));
            }

        }
    }
    None
}

fn evidence_outcome(matrix_text: &str, operation: &'static str, fault: EvidenceFault) -> Outcome {
    match fault {
        EvidenceFault::Missing => fail(matrix_text, operation, 3, "MISSING_EVIDENCE", "evidence"),
        EvidenceFault::Forged => fail(matrix_text, operation, 3, "FORGED_EVIDENCE", "sha256"),
        EvidenceFault::Handwritten => fail(matrix_text, operation, 3, "HANDWRITTEN_GRADE", "generated"),
        EvidenceFault::Scope => fail(matrix_text, operation, 3, "EVIDENCE_SCOPE", "managed_variant"),
        EvidenceFault::Incomplete(reason) => fail(matrix_text, operation, 3, "EVIDENCE_INCOMPLETE", reason),
    }
}

fn doc_drift(doc: &str) -> bool {
    const TOKENS: &[&str] = &[
        "oo-support-matrix/v1",
        "CONNECTOR_UNSUPPORTED",
        "TARGET_VERSION_UNVERIFIED",
        "CAPABILITY_UNVERIFIED",
        "COMPAT_MODE_UNKNOWN",
        "DRIVER_MISSING",
        "`candidate`",
        "`designed`",
        "`experimental`",
        "`verified`",
        "`certified`",
        "`excluded`",
    ];
    TOKENS.iter().any(|token| !doc.contains(token))
}

fn verified_count(matrix: &Matrix) -> i64 {
    matrix
        .targets
        .iter()
        .flat_map(|target| target.caps.values())
        .filter(|level| *level == "verified" || *level == "certified")
        .count() as i64
}

fn other_verified(target: &Target) -> bool {
    target.caps.values().any(|level| level == "verified" || level == "certified")
}

fn row(target: &Target, capability: &str, level: &str) -> Json {
    Json::Object(vec![
        ("target_id".to_string(), Json::String(target.id.clone())),
        ("family".to_string(), Json::String(target.family.clone())),
        ("product".to_string(), Json::String(target.product.clone())),
        ("variant".to_string(), Json::String(target.variant.clone().unwrap_or_default())),
        ("capability".to_string(), Json::String(capability.to_string())),
        ("level".to_string(), Json::String(level.to_string())),
        ("limits".to_string(), strings(&target.limits)),
    ])
}

fn fit_name(fit: VersionFit) -> &'static str {
    match fit {
        VersionFit::Exact => "exact",
        VersionFit::Interval => "interval",
        VersionFit::Patch => "patch",
        VersionFit::Absent => "absent",
    }
}

fn fail(matrix_text: &str, operation: &'static str, exit_code: i32, code: &'static str, reason: &'static str) -> Outcome {
    Outcome::new(operation, false, exit_code, code, reason, matrix_text)
}

fn register_fail(matrix_text: &str, operation: &'static str, error: RegisterError) -> Outcome {
    fail(matrix_text, operation, 1, error.code, error.detail)
        .field("source_control_registration", Json::String("rejected".to_string()))
        .field("source_control_host_allowlist", Json::String("not_evaluated".to_string()))
        .field("secret_resolution", Json::String("not_evaluated".to_string()))
}

fn required_text(value: &Json, key: &'static str) -> Result<String, &'static str> {
    match value.get(key).and_then(Json::string) {
        Some(text) => Ok(text.to_string()),
        None => Err(key),
    }
}

fn unknown_keys(value: &Json, allowed: &[&str]) -> bool {
    value.object().is_none_or(|items| items.iter().any(|(key, _)| !allowed.contains(&key.as_str())))
}
