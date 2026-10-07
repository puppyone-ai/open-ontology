use oo_control::{AdmissionContext, FileHost, ObjectKey, UploadConfig};

use crate::deploy::Deployment;
use crate::text::{self, domain_id, hex_encode};
use crate::{Error, Now, RetentionClass, check_ttl, expiry_millis, resolve_now};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationKind {
    Published,
    Open,
}

impl RegistrationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Open => "open",
        }
    }

    fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "published" => Ok(Self::Published),
            "open" => Ok(Self::Open),
            _ => Err(Error::Corrupt),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowState {
    Active,
    AbortAttempted,
}

impl RowState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::AbortAttempted => "abort_attempted",
        }
    }

    fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "active" => Ok(Self::Active),
            "abort_attempted" => Ok(Self::AbortAttempted),
            _ => Err(Error::Corrupt),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub registration_id: String,
    pub kind: RegistrationKind,
    pub class: RetentionClass,
    pub object_key: String,
    pub session_id: String,
    pub object_digest_hex: String,
    pub size: u64,
    pub part_count: Option<u32>,
    pub project: String,
    pub principal: String,
    pub role: String,
    pub policy_id: String,
    pub grant_hex: String,
    pub created_unix_ms: u64,
    pub expiry_unix_ms: u64,
    pub ttl_seconds: u64,
    pub explicit_workspace: bool,
    pub manifest_contract: String,
    pub lineage_digest_hex: String,
    pub idempotency_key: String,
    pub(crate) state: RowState,
}

pub struct RegisterPublished<'a> {
    pub object_key: &'a ObjectKey,
    pub class: RetentionClass,
    pub role: &'a str,
    pub policy_id: &'a str,
    pub ttl_seconds: u64,
    pub explicit_workspace: bool,
    pub now: Now,
}

pub struct RegisterOpen<'a> {
    pub config: &'a UploadConfig,
    pub class: RetentionClass,
    pub role: &'a str,
    pub policy_id: &'a str,
    pub ttl_seconds: u64,
    pub explicit_workspace: bool,
    pub now: Now,
}

impl Deployment {
    pub async fn register_published(
        &self,
        host: &FileHost,
        ctx: &AdmissionContext,
        request: RegisterPublished<'_>,
    ) -> Result<Registration, Error> {
        check_policy(request.role, request.policy_id)?;
        check_ttl(
            request.class,
            request.ttl_seconds,
            request.explicit_workspace,
        )?;
        let manifest = host
            .read_manifest(ctx, request.object_key)
            .await
            .map_err(map_host)?;
        if manifest.contract != crate::FILEHOST_CONTRACT {
            return Err(Error::Corrupt);
        }
        if manifest.project_id.as_str() != ctx.project_id.as_str()
            || manifest.principal_id.as_str() != ctx.principal_id.as_str()
        {
            return Err(Error::Denied);
        }
        let created = resolve_now(request.now)?;
        let row = Registration {
            registration_id: domain_id(&[
                manifest.object_key.as_str().as_bytes(),
                manifest.session_id.as_str().as_bytes(),
                &manifest.object_digest,
            ]),
            kind: RegistrationKind::Published,
            class: request.class,
            object_key: manifest.object_key.as_str().to_owned(),
            session_id: manifest.session_id.as_str().to_owned(),
            object_digest_hex: hex_encode(&manifest.object_digest),
            size: manifest.size,
            part_count: Some(manifest.part_count),
            project: manifest.project_id.as_str().to_owned(),
            principal: manifest.principal_id.as_str().to_owned(),
            role: request.role.to_owned(),
            policy_id: request.policy_id.to_owned(),
            grant_hex: hex_encode(manifest.grant_evidence.as_str().as_bytes()),
            created_unix_ms: created,
            expiry_unix_ms: expiry_millis(created, request.ttl_seconds)?,
            ttl_seconds: request.ttl_seconds,
            explicit_workspace: request.explicit_workspace,
            manifest_contract: manifest.contract.to_owned(),
            lineage_digest_hex: hex_encode(&manifest.lineage_digest),
            idempotency_key: manifest.idempotency_key.as_str().to_owned(),
            state: RowState::Active,
        };
        insert(self, row)
    }

    pub async fn register_open(
        &self,
        host: &FileHost,
        ctx: &AdmissionContext,
        request: RegisterOpen<'_>,
    ) -> Result<Registration, Error> {
        check_policy(request.role, request.policy_id)?;
        check_ttl(
            request.class,
            request.ttl_seconds,
            request.explicit_workspace,
        )?;
        let session = host.begin(ctx, request.config).await.map_err(map_host)?;
        let created = resolve_now(request.now)?;
        let row = Registration {
            registration_id: domain_id(&[
                session.object_key.as_str().as_bytes(),
                session.session_id.as_str().as_bytes(),
                &session.expected_sha256,
            ]),
            kind: RegistrationKind::Open,
            class: request.class,
            object_key: session.object_key.as_str().to_owned(),
            session_id: session.session_id.as_str().to_owned(),
            object_digest_hex: hex_encode(&session.expected_sha256),
            size: session.expected_size,
            part_count: None,
            project: ctx.project_id.as_str().to_owned(),
            principal: ctx.principal_id.as_str().to_owned(),
            role: request.role.to_owned(),
            policy_id: request.policy_id.to_owned(),
            grant_hex: hex_encode(ctx.grant_evidence.as_str().as_bytes()),
            created_unix_ms: created,
            expiry_unix_ms: expiry_millis(created, request.ttl_seconds)?,
            ttl_seconds: request.ttl_seconds,
            explicit_workspace: request.explicit_workspace,
            manifest_contract: "absent".to_owned(),
            lineage_digest_hex: "absent".to_owned(),
            idempotency_key: session.idempotency_key.as_str().to_owned(),
            state: RowState::Active,
        };
        insert(self, row)
    }
}

pub(crate) fn load(deploy: &Deployment) -> Result<Vec<Registration>, Error> {
    let bytes = text::read_hashed(&deploy.inventory_dir(), "registrations.txt")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::Corrupt)?;
    decode_inventory(text)
}

pub(crate) fn save(deploy: &Deployment, rows: &[Registration]) -> Result<(), Error> {
    let mut ordered = rows.to_vec();
    ordered.sort_by(|left, right| left.registration_id.cmp(&right.registration_id));
    let body = encode_inventory(&ordered);
    let parsed = decode_inventory(&body)?;
    if parsed != ordered {
        return Err(Error::Corrupt);
    }
    text::write_hashed(
        &deploy.inventory_dir(),
        "registrations.txt",
        body.as_bytes(),
    )
}

pub(crate) fn find_key<'a>(rows: &'a [Registration], object_key: &str) -> Option<&'a Registration> {
    rows.iter().find(|row| row.object_key == object_key)
}

pub(crate) fn set_state(
    deploy: &Deployment,
    registration_id: &str,
    state: RowState,
) -> Result<(), Error> {
    let mut rows = load(deploy)?;
    let Some(row) = rows
        .iter_mut()
        .find(|row| row.registration_id == registration_id)
    else {
        return Err(Error::NotFound);
    };
    row.state = state;
    save(deploy, &rows)
}

fn insert(deploy: &Deployment, row: Registration) -> Result<Registration, Error> {
    let mut rows = load(deploy)?;
    if let Some(existing) = rows
        .iter()
        .find(|stored| stored.object_key == row.object_key)
    {
        if same_binding(existing, &row) {
            return Ok(existing.clone());
        }
        return Err(Error::Conflict);
    }
    rows.push(row.clone());
    save(deploy, &rows)?;
    Ok(row)
}

fn same_binding(stored: &Registration, incoming: &Registration) -> bool {
    stored.registration_id == incoming.registration_id
        && stored.kind == incoming.kind
        && stored.class == incoming.class
        && stored.role == incoming.role
        && stored.policy_id == incoming.policy_id
        && stored.ttl_seconds == incoming.ttl_seconds
        && stored.explicit_workspace == incoming.explicit_workspace
        && stored.object_digest_hex == incoming.object_digest_hex
        && stored.project == incoming.project
        && stored.principal == incoming.principal
}

fn check_policy(role: &str, policy_id: &str) -> Result<(), Error> {
    if !text::identity_ok(role) || !text::identity_ok(policy_id) {
        return Err(Error::Admission);
    }
    Ok(())
}

fn map_host(error: oo_control::FileHostError) -> Error {
    match error {
        oo_control::FileHostError::Admission => Error::Admission,
        oo_control::FileHostError::Denied => Error::Denied,
        oo_control::FileHostError::NotFound => Error::GuessedKey,
        oo_control::FileHostError::Conflict => Error::Conflict,
        oo_control::FileHostError::Quota => Error::Quota,
        oo_control::FileHostError::Corrupt => Error::Corrupt,
        oo_control::FileHostError::Unsupported => Error::Unsupported {
            seam: "filehost_unlocked_store",
        },
        _ => Error::Store,
    }
}

fn encode_inventory(rows: &[Registration]) -> String {
    let mut out = format!("oo.artifact-retention/v1\ncount {}\n", rows.len());
    for row in rows {
        out.push_str(&encode_row(row));
    }
    out
}

fn encode_row(row: &Registration) -> String {
    let part_count = match row.part_count {
        Some(count) => count.to_string(),
        None => "unknown".to_owned(),
    };
    let workspace = if row.explicit_workspace {
        "true"
    } else {
        "false"
    };
    format!(
        "\
registration {id}
kind {kind}
class {class}
object_key {object_key}
session_id {session_id}
object_digest {digest}
size {size}
part_count {part_count}
project {project}
principal {principal}
role {role}
policy_id {policy}
grant_hex {grant}
created_unix_ms {created}
expiry_unix_ms {expiry}
ttl_seconds {ttl}
explicit_workspace {workspace}
manifest_contract {contract}
lineage_digest {lineage}
idempotency_key {idem}
version absent
version_reason filehost_manifest_has_no_version
state {state}
end
",
        id = row.registration_id,
        kind = row.kind.as_str(),
        class = row.class.as_str(),
        object_key = row.object_key,
        session_id = row.session_id,
        digest = row.object_digest_hex,
        size = row.size,
        part_count = part_count,
        project = row.project,
        principal = row.principal,
        role = row.role,
        policy = row.policy_id,
        grant = row.grant_hex,
        created = row.created_unix_ms,
        expiry = row.expiry_unix_ms,
        ttl = row.ttl_seconds,
        workspace = workspace,
        contract = row.manifest_contract,
        lineage = row.lineage_digest_hex,
        idem = row.idempotency_key,
        state = row.state.as_str(),
    )
}

fn decode_inventory(text: &str) -> Result<Vec<Registration>, Error> {
    let mut lines = text.lines().peekable();
    let Some(header) = lines.next() else {
        return Err(Error::Corrupt);
    };
    if header != "oo.artifact-retention/v1" {
        return Err(Error::Corrupt);
    }
    let Some(count_line) = lines.next() else {
        return Err(Error::Corrupt);
    };
    let Some(count_text) = count_line.strip_prefix("count ") else {
        return Err(Error::Corrupt);
    };
    let count: usize = count_text.parse().map_err(|_| Error::Corrupt)?;
    let mut rows = Vec::with_capacity(count);
    while lines.peek().is_some() {
        rows.push(decode_row(&mut lines)?);
    }
    if rows.len() != count {
        return Err(Error::Corrupt);
    }
    let mut ids = rows
        .iter()
        .map(|row| row.registration_id.as_str())
        .collect::<Vec<_>>();
    let before = ids.clone();
    ids.sort_unstable();
    if ids != before || ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::Corrupt);
    }
    Ok(rows)
}

fn decode_row<'a>(lines: &mut impl Iterator<Item = &'a str>) -> Result<Registration, Error> {
    let registration_id = field(lines, "registration")?;
    let kind = RegistrationKind::parse(&field(lines, "kind")?)?;
    let class = RetentionClass::parse(&field(lines, "class")?)?;
    let object_key = field(lines, "object_key")?;
    let session_id = field(lines, "session_id")?;
    let object_digest_hex = field(lines, "object_digest")?;
    let size = number(&field(lines, "size")?)?;
    let part_count = part_count(&field(lines, "part_count")?)?;
    let project = field(lines, "project")?;
    let principal = field(lines, "principal")?;
    let role = field(lines, "role")?;
    let policy_id = field(lines, "policy_id")?;
    let grant_hex = field(lines, "grant_hex")?;
    let created_unix_ms = number(&field(lines, "created_unix_ms")?)?;
    let expiry_unix_ms = number(&field(lines, "expiry_unix_ms")?)?;
    let ttl_seconds = number(&field(lines, "ttl_seconds")?)?;
    let explicit_workspace = match field(lines, "explicit_workspace")?.as_str() {
        "true" => true,
        "false" => false,
        _ => return Err(Error::Corrupt),
    };
    let manifest_contract = field(lines, "manifest_contract")?;
    let lineage_digest_hex = field(lines, "lineage_digest")?;
    let idempotency_key = field(lines, "idempotency_key")?;
    let version = field(lines, "version")?;
    let version_reason = field(lines, "version_reason")?;
    let state = RowState::parse(&field(lines, "state")?)?;
    let end = lines.next().ok_or(Error::Corrupt)?;
    if end != "end" {
        return Err(Error::Corrupt);
    }
    if version != "absent" || version_reason != "filehost_manifest_has_no_version" {
        return Err(Error::Corrupt);
    }
    if !text::token_ok(&registration_id)
        || oo_control::ObjectKey::parse(&object_key).is_err()
        || oo_control::SessionId::parse(&session_id).is_err()
        || !is_sha256_hex(&object_digest_hex)
        || !is_sha256_hex(&idempotency_key)
        || !text::identity_ok(&project)
        || !text::identity_ok(&principal)
        || !text::identity_ok(&role)
        || !text::identity_ok(&policy_id)
        || text::hex_decode(&grant_hex).is_err()
    {
        return Err(Error::Corrupt);
    }
    match kind {
        RegistrationKind::Published => {
            if manifest_contract != crate::FILEHOST_CONTRACT
                || !is_sha256_hex(&lineage_digest_hex)
                || part_count.is_none()
            {
                return Err(Error::Corrupt);
            }
        }
        RegistrationKind::Open => {
            if manifest_contract != "absent"
                || lineage_digest_hex != "absent"
                || part_count.is_some()
            {
                return Err(Error::Corrupt);
            }
        }
    }
    if expiry_millis(created_unix_ms, ttl_seconds)? != expiry_unix_ms {
        return Err(Error::Corrupt);
    }
    Ok(Registration {
        registration_id,
        kind,
        class,
        object_key,
        session_id,
        object_digest_hex,
        size,
        part_count,
        project,
        principal,
        role,
        policy_id,
        grant_hex,
        created_unix_ms,
        expiry_unix_ms,
        ttl_seconds,
        explicit_workspace,
        manifest_contract,
        lineage_digest_hex,
        idempotency_key,
        state,
    })
}

fn field<'a>(lines: &mut impl Iterator<Item = &'a str>, key: &str) -> Result<String, Error> {
    let line = lines.next().ok_or(Error::Corrupt)?;
    let value = line
        .strip_prefix(&format!("{key} "))
        .ok_or(Error::Corrupt)?;
    if !text::token_ok(value) {
        return Err(Error::Corrupt);
    }
    Ok(value.to_owned())
}

fn number(value: &str) -> Result<u64, Error> {
    if value.is_empty()
        || (value.starts_with('0') && value != "0")
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Error::Corrupt);
    }
    value.parse::<u64>().map_err(|_| Error::Corrupt)
}

fn part_count(value: &str) -> Result<Option<u32>, Error> {
    if value == "unknown" {
        return Ok(None);
    }
    let parsed = number(value)?;
    u32::try_from(parsed).map(Some).map_err(|_| Error::Corrupt)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
