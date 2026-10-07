use oo_control::{AdmissionContext, FileHost, FileHostError, ObjectKey};
use sha2::{Digest, Sha256};

use crate::inventory::{self, Registration};
use crate::text::{hex_decode, hex_encode};
use crate::{DOWNLOAD_PERMIT_TTL_CAP_SECONDS, Deployment, Error, Now, resolve_now};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermitDecision {
    pub allowed: bool,
    pub reason: &'static str,
    pub filehost_admission: &'static str,
    pub gateway_url_revocation: &'static str,
    pub bearer_url: &'static str,
}

pub fn enforce_download_ttl(requested_seconds: u64) -> Result<(), Error> {
    if requested_seconds == 0 || requested_seconds > DOWNLOAD_PERMIT_TTL_CAP_SECONDS {
        return Err(Error::TtlCap);
    }
    Ok(())
}

impl Deployment {
    #[allow(clippy::too_many_arguments)]
    pub async fn permit(
        &self,
        host: &FileHost,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
        role: &str,
        policy_id: &str,
        now: Now,
        requested_permit_seconds: Option<u64>,
    ) -> Result<PermitDecision, Error> {
        if let Some(seconds) = requested_permit_seconds {
            enforce_download_ttl(seconds)?;
        }
        let decision = classify(self, host, ctx, object_key, role, policy_id, now).await?;
        Ok(decision)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn pull(
        &self,
        host: &FileHost,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
        role: &str,
        policy_id: &str,
        now: Now,
        requested_permit_seconds: Option<u64>,
    ) -> Result<Vec<u8>, Error> {
        if let Some(seconds) = requested_permit_seconds {
            enforce_download_ttl(seconds)?;
        }
        let rows = inventory::load(self)?;
        let Some(row) = inventory::find_key(&rows, object_key.as_str()) else {
            return Err(Error::GuessedKey);
        };
        let now_ms = resolve_now(now)?;
        if now_ms >= row.expiry_unix_ms {
            return Err(Error::Expired);
        }
        if ctx.project_id.as_str() != row.project || ctx.principal_id.as_str() != row.principal {
            if let Ok(download) = host.open_download(ctx, object_key, 8192, None).await {
                drop(download);
            }
            return Err(Error::Denied);
        }
        if role != row.role || policy_id != row.policy_id {
            if let Ok(download) = host.open_download(ctx, object_key, 8192, None).await {
                drop(download);
            }
            return Err(Error::TupleMismatch);
        }
        read_matching(host, ctx, object_key, row).await
    }
}

async fn classify(
    deploy: &Deployment,
    host: &FileHost,
    ctx: &AdmissionContext,
    object_key: &ObjectKey,
    role: &str,
    policy_id: &str,
    now: Now,
) -> Result<PermitDecision, Error> {
    let rows = inventory::load(deploy)?;
    let Some(row) = inventory::find_key(&rows, object_key.as_str()) else {
        return Err(Error::GuessedKey);
    };
    let now_ms = resolve_now(now)?;
    if now_ms >= row.expiry_unix_ms {
        return Ok(decision(false, "expired", "not_consulted"));
    }
    let opened = host.open_download(ctx, object_key, 8192, None).await;
    let admission = match &opened {
        Ok(_) => "project_principal_matched",
        Err(FileHostError::Denied) => "denied",
        Err(FileHostError::NotFound) => "not_found",
        Err(FileHostError::Corrupt) => "corrupt",
        Err(_) => "store_error",
    };
    drop(opened);
    if ctx.project_id.as_str() != row.project || ctx.principal_id.as_str() != row.principal {
        return Ok(decision(false, "filehost_denied", admission));
    }
    if role != row.role || policy_id != row.policy_id {
        return Ok(decision(false, "role_or_policy_mismatch", admission));
    }
    if admission != "project_principal_matched" {
        return Ok(decision(false, "filehost_refused", admission));
    }
    Ok(decision(true, "admitted", admission))
}

fn decision(
    allowed: bool,
    reason: &'static str,
    filehost_admission: &'static str,
) -> PermitDecision {
    PermitDecision {
        allowed,
        reason,
        filehost_admission,
        gateway_url_revocation: "not_implemented",
        bearer_url: "none",
    }
}

async fn read_matching(
    host: &FileHost,
    ctx: &AdmissionContext,
    object_key: &ObjectKey,
    row: &Registration,
) -> Result<Vec<u8>, Error> {
    let mut download = host
        .open_download(ctx, object_key, 8192, None)
        .await
        .map_err(map_read)?;
    let manifest = download.manifest().clone();
    if hex_encode(&manifest.object_digest) != row.object_digest_hex
        || manifest.size != row.size
        || manifest.session_id.as_str() != row.session_id
        || manifest.object_key.as_str() != row.object_key
    {
        return Err(Error::Drift);
    }
    let mut hasher = Sha256::new();
    let mut body = Vec::new();
    while let Some(chunk) = download.next_chunk().await.map_err(map_read)? {
        hasher.update(&chunk);
        body.extend_from_slice(&chunk);
    }
    if !download.is_complete() {
        return Err(Error::Corrupt);
    }
    let digest = hasher.finalize();
    if body.len() as u64 != manifest.size
        || AsRef::<[u8]>::as_ref(&digest) != manifest.object_digest
    {
        return Err(Error::Drift);
    }
    let _ = hex_decode(&row.grant_hex)?;
    Ok(body)
}

fn map_read(error: FileHostError) -> Error {
    match error {
        FileHostError::NotFound => Error::NotFound,
        FileHostError::Denied => Error::Denied,
        FileHostError::Corrupt => Error::Corrupt,
        FileHostError::Store | FileHostError::Stopped | FileHostError::Unavailable => {
            Error::ReadErrorNotAbsence
        }
        _ => Error::ReadErrorNotAbsence,
    }
}
