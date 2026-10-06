//! Stdin entry for one reviewed column-policy set.
//!
//! The operator document carries the administrative binding and the exact
//! reviewed-effective command bytes. This process calls
//! `compile_reviewed_policies` and writes a `ColumnAclPolicy` DTO.
//! `ColumnAclPolicy` has no serde implementation. The process does not open
//! a database, grant, publish, or write an audit record.

#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

use oo_core::json_scan::parse_json_value;
use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_gateway::policy::{ClassStatus, ColumnAclPolicy, ColumnSpec, PiiClass, RowFilter};
use oo_gateway::reviewed_policy::{
    ReviewedAllow, ReviewedBinding, ReviewedIdentity, ReviewedPolicyError, ReviewedSourceSeal,
    compile_reviewed_policies,
};
use serde::Deserialize;
use serde::Serialize;

const PROTOCOL: &str = "oo.gateway.reviewed-policy-cli/v1";

const EXIT_IO: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_BUDGET: u8 = 3;
const EXIT_SCAN: u8 = 4;
const EXIT_REQUEST: u8 = 5;
const EXIT_PROTOCOL: u8 = 6;
const EXIT_SHAPE: u8 = 7;
const EXIT_SEAL: u8 = 8;
const EXIT_IDENTITY: u8 = 9;
const EXIT_DECISION: u8 = 10;
const EXIT_SEMANTIC: u8 = 11;
const EXIT_PII: u8 = 12;
const EXIT_DUPLICATE: u8 = 13;
const EXIT_FORBIDDEN: u8 = 14;
const EXIT_RLS: u8 = 15;
const EXIT_POLICY: u8 = 16;

fn main() -> ExitCode {
    if std::env::args().nth(1).is_some() {
        return Fail::Usage.emit();
    }
    let text = match read_stdin() {
        Ok(text) => text,
        Err(fail) => return fail.emit(),
    };
    match compile(&text) {
        Ok(body) => {
            let mut out = io::stdout().lock();
            if out.write_all(body.as_bytes()).is_err() || out.flush().is_err() {
                return Fail::Io.emit();
            }
            ExitCode::SUCCESS
        }
        Err(fail) => fail.emit(),
    }
}

fn read_stdin() -> Result<String, Fail> {
    let mut stdin = io::stdin().lock();
    let mut buf = Vec::new();
    let max = DOCUMENT_MAX_BYTES.saturating_add(1);
    stdin
        .by_ref()
        .take(max as u64)
        .read_to_end(&mut buf)
        .map_err(|_| Fail::Io)?;
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(Fail::Budget);
    }
    let text = std::str::from_utf8(&buf).map_err(|_| Fail::Scan)?;
    Ok(text.to_string())
}

fn compile(text: &str) -> Result<String, Fail> {
    let value = parse_json_value(text, PROTOCOL).map_err(|_| Fail::Scan)?;
    let request: RequestDoc = serde_json::from_value(value).map_err(|_| Fail::Request)?;
    if request.protocol != PROTOCOL {
        return Err(Fail::Request);
    }
    let binding = binding_from(request.binding);
    let compiled = compile_reviewed_policies(
        &request.command,
        &request.artifact,
        &request.decisions,
        &binding,
    )
    .map_err(adapt)?;
    let output = OutputDoc {
        protocol: PROTOCOL,
        policies: compiled.policies.into_iter().map(policy_out).collect(),
    };
    serde_json::to_string(&output).map_err(|_| Fail::Io)
}

fn adapt(err: ReviewedPolicyError) -> Fail {
    match err {
        ReviewedPolicyError::Budget => Fail::Budget,
        ReviewedPolicyError::Scan => Fail::Scan,
        ReviewedPolicyError::Protocol => Fail::Protocol,
        ReviewedPolicyError::Shape => Fail::Shape,
        ReviewedPolicyError::Seal => Fail::Seal,
        ReviewedPolicyError::Identity => Fail::Identity,
        ReviewedPolicyError::Decision { reason } => Fail::Decision(known_reason(reason)),
        ReviewedPolicyError::Semantic => Fail::Semantic,
        ReviewedPolicyError::PiiAllow => Fail::PiiAllow,
        ReviewedPolicyError::Duplicate => Fail::Duplicate,
        ReviewedPolicyError::Forbidden => Fail::Forbidden,
        ReviewedPolicyError::Rls => Fail::Rls,
        ReviewedPolicyError::Policy(_) => Fail::Policy,
    }
}

fn known_reason(reason: &str) -> &'static str {
    match reason {
        "unknown" => "unknown",
        "missing" => "missing",
        "stale_schema" => "stale_schema",
        "stale_fact" => "stale_fact",
        "rejected" => "rejected",
        "unbound" => "unbound",
        "mismatch" => "mismatch",
        _ => "",
    }
}

fn binding_from(doc: BindingDoc) -> ReviewedBinding {
    ReviewedBinding {
        identity: ReviewedIdentity {
            project: doc.identity.project,
            ontology_version: doc.identity.ontology_version,
            catalog_version: doc.identity.catalog_version,
            policy_id: doc.identity.policy_id,
            policy_version: doc.identity.policy_version,
            knowledge_id: doc.identity.knowledge_id,
            knowledge_version: doc.identity.knowledge_version,
        },
        seal: ReviewedSourceSeal {
            source_id: doc.seal.source_id,
            schema: doc.seal.schema,
            schema_hash: doc.seal.schema_hash,
            artifact_sha256: doc.seal.artifact_sha256,
        },
        config_digest: doc.config_digest,
        generation: doc.generation,
        role: doc.role,
        login: doc.login,
        allow: doc
            .allow
            .into_iter()
            .map(|item| ReviewedAllow {
                dataset: item.dataset,
                column: item.column,
                explicit_allow: item.explicit_allow,
            })
            .collect(),
    }
}

fn policy_out(policy: ColumnAclPolicy) -> PolicyOut {
    PolicyOut {
        project: policy.project,
        config_digest: policy.config_digest,
        generation: policy.generation,
        role: policy.role,
        login: policy.login,
        table: TableOut {
            namespace: policy.table.namespace,
            name: policy.table.name,
            columns: policy.table.columns.into_iter().map(column_out).collect(),
        },
        row_filter: policy.row_filter.map(filter_out),
    }
}

fn column_out(column: ColumnSpec) -> ColumnOut {
    ColumnOut {
        name: column.name,
        status: status_name(column.status),
        semantic_type: column.semantic_type,
        pii: pii_out(column.pii),
    }
}

fn status_name(status: ClassStatus) -> &'static str {
    match status {
        ClassStatus::Confirmed => "confirmed",
        ClassStatus::AutoAccepted => "auto_accepted",
        ClassStatus::Unclassified => "unclassified",
    }
}

fn pii_out(pii: PiiClass) -> PiiOut {
    match pii {
        PiiClass::None => PiiOut {
            class: "none",
            explicit_allow: None,
        },
        PiiClass::Unknown => PiiOut {
            class: "unknown",
            explicit_allow: None,
        },
        PiiClass::Declared { explicit_allow } => PiiOut {
            class: "declared",
            explicit_allow: Some(explicit_allow),
        },
    }
}

fn filter_out(filter: RowFilter) -> RowFilterOut {
    let RowFilter::Claims { column, claim } = filter;
    RowFilterOut {
        kind: "claims".to_string(),
        column,
        claim,
    }
}

enum Fail {
    Io,
    Usage,
    Budget,
    Scan,
    Request,
    Protocol,
    Shape,
    Seal,
    Identity,
    Decision(&'static str),
    Semantic,
    PiiAllow,
    Duplicate,
    Forbidden,
    Rls,
    Policy,
}

impl Fail {
    fn emit(self) -> ExitCode {
        let (code, line) = match self {
            Self::Io => (EXIT_IO, "reviewed-policy: io".to_string()),
            Self::Usage => (
                EXIT_USAGE,
                "reviewed-policy: usage: reviewed-policy reads one JSON document from stdin and takes no arguments".to_string(),
            ),
            Self::Budget => (EXIT_BUDGET, "reviewed-policy: budget".to_string()),
            Self::Scan => (EXIT_SCAN, "reviewed-policy: scan".to_string()),
            Self::Request => (EXIT_REQUEST, "reviewed-policy: request".to_string()),
            Self::Protocol => (EXIT_PROTOCOL, "reviewed-policy: protocol".to_string()),
            Self::Shape => (EXIT_SHAPE, "reviewed-policy: shape".to_string()),
            Self::Seal => (EXIT_SEAL, "reviewed-policy: seal".to_string()),
            Self::Identity => (EXIT_IDENTITY, "reviewed-policy: identity".to_string()),
            Self::Decision("") => (EXIT_DECISION, "reviewed-policy: decision".to_string()),
            Self::Decision(reason) => {
                (EXIT_DECISION, format!("reviewed-policy: decision {reason}"))
            }
            Self::Semantic => (EXIT_SEMANTIC, "reviewed-policy: semantic".to_string()),
            Self::PiiAllow => (EXIT_PII, "reviewed-policy: pii_allow".to_string()),
            Self::Duplicate => (EXIT_DUPLICATE, "reviewed-policy: duplicate".to_string()),
            Self::Forbidden => (EXIT_FORBIDDEN, "reviewed-policy: forbidden".to_string()),
            Self::Rls => (EXIT_RLS, "reviewed-policy: rls".to_string()),
            Self::Policy => (EXIT_POLICY, "reviewed-policy: policy".to_string()),
        };
        eprintln!("{line}");
        ExitCode::from(code)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDoc {
    protocol: String,
    command: String,
    artifact: String,
    decisions: String,
    binding: BindingDoc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingDoc {
    identity: IdentityDoc,
    seal: SealDoc,
    config_digest: String,
    generation: i64,
    role: String,
    login: String,
    allow: Vec<AllowDoc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityDoc {
    project: String,
    ontology_version: String,
    catalog_version: String,
    policy_id: String,
    policy_version: String,
    knowledge_id: String,
    knowledge_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SealDoc {
    source_id: String,
    schema: String,
    schema_hash: String,
    artifact_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowDoc {
    dataset: String,
    column: String,
    explicit_allow: bool,
}

#[derive(Serialize)]
struct OutputDoc {
    protocol: &'static str,
    policies: Vec<PolicyOut>,
}

#[derive(Serialize)]
struct PolicyOut {
    project: String,
    config_digest: String,
    generation: i64,
    role: String,
    login: String,
    table: TableOut,
    row_filter: Option<RowFilterOut>,
}

#[derive(Serialize)]
struct TableOut {
    namespace: String,
    name: String,
    columns: Vec<ColumnOut>,
}

#[derive(Serialize)]
struct ColumnOut {
    name: String,
    status: &'static str,
    semantic_type: String,
    pii: PiiOut,
}

#[derive(Serialize)]
struct PiiOut {
    class: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    explicit_allow: Option<bool>,
}

#[derive(Serialize)]
struct RowFilterOut {
    kind: String,
    column: String,
    claim: String,
}
