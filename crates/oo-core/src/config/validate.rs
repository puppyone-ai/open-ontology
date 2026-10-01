//! Cross-field rules that JSON Schema cannot express.
//!
//! Schema and serde already agree on field shape. These checks are stricter:
//! duration caps, `per_table` acknowledgement, upsert keys, role coverage,
//! one-level model fallback, and hook placement.

use crate::diagnostic::{CONTRACT_CONFIG, Diagnostic};
use crate::duration::Duration;
use crate::text::{GlobPattern, QualifiedTable, RelativePath};

use super::model::{
    AgentConfig, ConsistencyMode, EvalSuite, FileMode, FilesFile, Grant, HookType, HooksFile,
    ModelConfig, ObjectBackend, OoConfig, RoleLimitOverride, RoleLimits, RolesFile, SourcesFile,
};

pub fn effective_role_limits(
    defaults: &RoleLimits,
    over: Option<&RoleLimitOverride>,
) -> RoleLimits {
    let Some(over) = over else {
        return defaults.clone();
    };
    RoleLimits {
        max_rows: over.max_rows.unwrap_or(defaults.max_rows),
        max_rows_cap: over.max_rows_cap.unwrap_or(defaults.max_rows_cap),
        export_rows: over.export_rows.unwrap_or(defaults.export_rows),
        export_bytes: over
            .export_bytes
            .clone()
            .unwrap_or_else(|| defaults.export_bytes.clone()),
        concurrency: over.concurrency.unwrap_or(defaults.concurrency),
        statement_timeout: over
            .statement_timeout
            .clone()
            .unwrap_or_else(|| defaults.statement_timeout.clone()),
        export_timeout: over
            .export_timeout
            .clone()
            .unwrap_or_else(|| defaults.export_timeout.clone()),
    }
}

pub fn validate_oo(config: &OoConfig) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    duration_ok(
        "/quality/reconcile_max_age",
        &config.quality.reconcile_max_age,
        &mut out,
    );
    if config.object_store.backend == ObjectBackend::S3 && config.object_store.s3.is_none() {
        out.push(cross(
            "/object_store/s3",
            "backend s3 requires endpoint, bucket, and region",
        ));
    }
    out
}

pub fn validate_sources(file: &SourcesFile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (name, source) in file.sources.iter() {
        let base = format!("/sources/{name}");
        if let Some(timeout) = &source.connection.connect_timeout {
            duration_ok(
                &format!("{base}/connection/connect_timeout"),
                timeout,
                &mut out,
            );
        }
        duration_ok(
            &format!("{base}/max_snapshot_txn_seconds"),
            &source.max_snapshot_txn_seconds,
            &mut out,
        );
        if let Some(freshness) = &source.freshness_expectation {
            duration_ok(
                &format!("{base}/freshness_expectation"),
                freshness,
                &mut out,
            );
        }
        duration_ok(
            &format!("{base}/incremental/every"),
            &source.incremental.every,
            &mut out,
        );
        duration_ok(
            &format!("{base}/incremental/overlap"),
            &source.incremental.overlap,
            &mut out,
        );
        if source.tables.include.is_empty() {
            out.push(cross(
                &format!("{base}/tables/include"),
                "include must contain at least one glob",
            ));
        }
        if source.consistency == ConsistencyMode::PerTable && !source.consistency_acknowledged {
            out.push(cross(
                &format!("{base}/consistency_acknowledged"),
                "consistency per_table requires consistency_acknowledged: true",
            ));
        }
        for (table, over) in source.overrides.iter() {
            if let Some(every) = &over.every {
                duration_ok(&format!("{base}/overrides/{table}/every"), every, &mut out);
            }
        }
    }
    out
}

pub fn validate_files(file: &FilesFile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (name, set) in file.files.iter() {
        if set.mode == FileMode::Upsert && set.key.as_ref().is_none_or(|key| key.is_empty()) {
            out.push(cross(
                &format!("/files/{name}/key"),
                "mode upsert requires a non-empty key",
            ));
        }
    }
    out
}

pub fn validate_roles(file: &RolesFile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    check_limits("/defaults/limits", &file.defaults.limits, &mut out);
    for (name, role) in file.roles.iter() {
        let base = format!("/roles/{name}");
        for (index, grant) in role.grants.iter().enumerate() {
            if let Grant::Schema(schema) = grant
                && schema.tables.is_empty()
            {
                out.push(cross(
                    &format!("{base}/grants/{index}/tables"),
                    "a schema grant needs at least one table glob",
                ));
            }
        }
        if let Some(limits) = &role.limits {
            check_limit_durations(&format!("{base}/limits"), limits, &mut out);
        }
        let effective = effective_role_limits(&file.defaults.limits, role.limits.as_ref());
        if effective.max_rows > effective.max_rows_cap {
            out.push(cross(
                &format!("{base}/limits/max_rows"),
                "max_rows cannot exceed max_rows_cap",
            ));
        }
        for (table, _) in role.row_filters.iter() {
            if !grant_covers(&role.grants, table) {
                out.push(cross(
                    &format!("{base}/row_filters/{table}"),
                    "row filter table is not covered by this role's grants",
                ));
            }
        }
    }
    for (index, binding) in file.bindings.iter().enumerate() {
        let base = format!("/bindings/{index}/match");
        match (
            binding.match_rule.idp_group.is_some(),
            binding.match_rule.any,
        ) {
            (true, None) | (false, Some(true)) => {}
            (false, Some(false)) => out.push(cross(&base, "match.any false is not a binding")),
            (true, Some(_)) => out.push(cross(&base, "idp_group and any are mutually exclusive")),
            (false, None) => out.push(cross(&base, "a binding matches idp_group or any: true")),
        }
        if let Some(role) = &binding.role
            && file.roles.get(role).is_none()
        {
            out.push(cross(
                &format!("/bindings/{index}/role"),
                format!("role `{role}` is not defined in this file"),
            ));
        }
    }
    out
}

pub fn validate_agent(config: &AgentConfig) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if config
        .harness
        .versions
        .get(&config.harness.default)
        .is_none()
    {
        out.push(cross(
            "/harness/versions",
            "versions must pin the default harness",
        ));
    }
    if config.model.credential.is_none() {
        out.push(cross(
            "/model/credential",
            "the primary model credential is required",
        ));
    }
    check_fallback(
        "/model/fallback",
        config.model.fallback.as_deref(),
        &mut out,
    );
    duration_ok(
        "/session/idle_timeout",
        &config.session.idle_timeout,
        &mut out,
    );
    duration_ok(
        "/session/max_lifetime",
        &config.session.max_lifetime,
        &mut out,
    );
    duration_ok(
        "/sandbox/pool/max_idle_age",
        &config.sandbox.pool.max_idle_age,
        &mut out,
    );
    if config.sandbox.pool.min_idle > config.sandbox.pool.max_pool {
        out.push(cross(
            "/sandbox/pool/min_idle",
            "min_idle cannot exceed max_pool",
        ));
    }
    if config.channels.feishu.enabled
        && (config.channels.feishu.app_id.is_none() || config.channels.feishu.credential.is_none())
    {
        out.push(cross(
            "/channels/feishu",
            "enabled feishu requires app_id and credential",
        ));
    }
    out
}

pub fn validate_hooks(file: &HooksFile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (name, hook) in file.hooks.iter() {
        let base = format!("/hooks/{name}");
        if let Some(debounce) = &hook.debounce {
            duration_ok(&format!("{base}/debounce"), debounce, &mut out);
        }
        match hook.kind {
            HookType::OnBatch => {
                if hook.builtin.is_none() {
                    out.push(cross(
                        &format!("{base}/builtin"),
                        "on_batch requires builtin",
                    ));
                }
                if hook.sql_dir.is_some() {
                    out.push(cross(
                        &format!("{base}/sql_dir"),
                        "on_batch does not take sql_dir",
                    ));
                }
            }
            HookType::AfterLoad => match &hook.sql_dir {
                Some(path) if after_load_dir(path) => {}
                Some(_) => out.push(cross(
                    &format!("{base}/sql_dir"),
                    "after_load sql_dir must stay under hooks/after_load/",
                )),
                None => out.push(cross(
                    &format!("{base}/sql_dir"),
                    "after_load requires sql_dir",
                )),
            },
        }
    }
    out
}

pub fn validate_eval(suite: &EvalSuite) -> Vec<Diagnostic> {
    let _ = suite;
    Vec::new()
}

fn check_fallback(path: &str, fallback: Option<&ModelConfig>, out: &mut Vec<Diagnostic>) {
    let Some(fallback) = fallback else {
        return;
    };
    if fallback.fallback.is_some() {
        out.push(cross(path, "model fallback is one level only"));
    }
}

fn check_limits(path: &str, limits: &RoleLimits, out: &mut Vec<Diagnostic>) {
    duration_ok(
        &format!("{path}/statement_timeout"),
        &limits.statement_timeout,
        out,
    );
    duration_ok(
        &format!("{path}/export_timeout"),
        &limits.export_timeout,
        out,
    );
    if limits.max_rows > limits.max_rows_cap {
        out.push(cross(
            &format!("{path}/max_rows"),
            "max_rows cannot exceed max_rows_cap",
        ));
    }
}

fn check_limit_durations(path: &str, limits: &RoleLimitOverride, out: &mut Vec<Diagnostic>) {
    if let Some(timeout) = &limits.statement_timeout {
        duration_ok(&format!("{path}/statement_timeout"), timeout, out);
    }
    if let Some(timeout) = &limits.export_timeout {
        duration_ok(&format!("{path}/export_timeout"), timeout, out);
    }
}

fn duration_ok(path: &str, duration: &Duration, out: &mut Vec<Diagnostic>) {
    if !duration.within_cap() {
        out.push(cross(path, "duration exceeds the ten-year cap"));
    }
}

fn after_load_dir(path: &RelativePath) -> bool {
    path.as_str().starts_with("hooks/after_load/")
}

fn grant_covers(grants: &[Grant], table: &QualifiedTable) -> bool {
    let text = table.as_str();
    let Some((schema, name)) = text.split_once('.') else {
        return false;
    };
    for grant in grants {
        match grant {
            Grant::Table(item) if item.table.as_str() == text => return true,
            Grant::Schema(item) if item.schema.as_str() == schema => {
                let included = item
                    .tables
                    .iter()
                    .any(|pattern| glob_match(pattern, name) || glob_match(pattern, text));
                let excluded = item
                    .exclude
                    .iter()
                    .any(|pattern| glob_match(pattern, name) || glob_match(pattern, text));
                if included && !excluded {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn glob_match(pattern: &GlobPattern, text: &str) -> bool {
    let parts: Vec<&str> = pattern.as_str().split('*').collect();
    if parts.len() == 1 {
        return pattern.as_str() == text;
    }
    if !text.starts_with(parts[0]) {
        return false;
    }
    let mut rest = &text[parts[0].len()..];
    for (index, part) in parts.iter().enumerate().skip(1) {
        if index + 1 == parts.len() {
            return rest.ends_with(part);
        }
        let Some(found) = rest.find(part) else {
            return false;
        };
        rest = &rest[found + part.len()..];
    }
    true
}

fn cross(path: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(CONTRACT_CONFIG, "cross_field", path, message)
}
