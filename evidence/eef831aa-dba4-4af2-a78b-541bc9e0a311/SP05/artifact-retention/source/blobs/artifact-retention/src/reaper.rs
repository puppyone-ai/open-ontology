use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, ObjectKey, ObjectManifest,
    PrincipalId, ProjectId, SessionId,
};
use sha2::{Digest, Sha256};

use crate::deploy::{self, Deployment};
use crate::inventory::{self, Registration, RegistrationKind, RowState};
use crate::text::{self, hex_decode, hex_encode};
use crate::{Error, Now, resolve_now};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseAfter {
    None,
    PreDelete,
    MetadataApi,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectOutcome {
    pub registration_id: String,
    pub status: &'static str,
    pub seam: &'static str,
    pub filehost_after: &'static str,
    pub absence_verified: bool,
    pub deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ReapReport {
    pub deletion_success: bool,
    pub idle: bool,
    pub paused: bool,
    pub restarted_from_partial: bool,
    pub parent_complete: bool,
    pub outcomes: Vec<ObjectOutcome>,
}

impl Deployment {
    pub async fn reap(
        &self,
        host: &FileHost,
        now: Now,
        pause: PauseAfter,
    ) -> Result<ReapReport, Error> {
        let phase = deploy::read_checkpoint(self)?;
        let restarted_from_partial = matches!(
            phase.as_deref(),
            Some("pre_delete") | Some("metadata_api_calling") | Some("metadata_api_done")
        );
        let now_ms = resolve_now(now)?;
        let rows = inventory::load(self)?;
        let expired: Vec<Registration> = rows
            .iter()
            .filter(|row| row.expiry_unix_ms <= now_ms)
            .cloned()
            .collect();
        if expired.is_empty() {
            return Ok(ReapReport {
                deletion_success: false,
                idle: true,
                paused: false,
                restarted_from_partial,
                parent_complete: false,
                outcomes: Vec::new(),
            });
        }
        let mut outcomes = Vec::new();
        for row in expired {
            let step = reap_one(self, host, &row, now_ms, pause).await?;
            let paused = step.paused;
            outcomes.push(step.outcome);
            if paused {
                return Ok(ReapReport {
                    deletion_success: false,
                    idle: false,
                    paused: true,
                    restarted_from_partial,
                    parent_complete: false,
                    outcomes,
                });
            }
        }
        deploy::write_checkpoint(self, "observed", "none")?;
        Ok(ReapReport {
            deletion_success: false,
            idle: false,
            paused: false,
            restarted_from_partial,
            parent_complete: false,
            outcomes,
        })
    }
}

struct Step {
    outcome: ObjectOutcome,
    paused: bool,
}

async fn reap_one(
    deploy: &Deployment,
    host: &FileHost,
    row: &Registration,
    now_ms: u64,
    pause: PauseAfter,
) -> Result<Step, Error> {
    write_audit(deploy, "pre_delete", row, now_ms, "pre_delete")?;
    deploy::write_checkpoint(deploy, "pre_delete", &row.registration_id)?;
    if pause == PauseAfter::PreDelete {
        return Ok(Step {
            outcome: outcome(row, "paused_before_delete", "none", "not_observed"),
            paused: true,
        });
    }
    let ctx = admission(row)?;
    let key = ObjectKey::parse(&row.object_key).map_err(|_| Error::Corrupt)?;
    let seen = observe(host, &ctx, &key).await;
    if !matches!(seen, Seen::Present(_)) && !matches!(seen, Seen::NotFound) {
        let after = seen_label(&seen);
        let status = if matches!(seen, Seen::Corrupt) {
            "refused_tamper_or_symlink"
        } else if matches!(seen, Seen::Denied) {
            "refused_foreign"
        } else {
            "read_error_not_absence"
        };
        return Ok(Step {
            outcome: outcome(row, status, "none", after),
            paused: false,
        });
    }
    if let Seen::Present(manifest) = &seen {
        if !manifest_matches(row, manifest) {
            return Ok(Step {
                outcome: outcome(row, "refused_drift", "none", "found"),
                paused: false,
            });
        }
        if row.kind == RegistrationKind::Open {
            return Ok(Step {
                outcome: outcome(
                    row,
                    "unsupported_completed_object_delete",
                    "filehost_completed_object_delete",
                    "found",
                ),
                paused: false,
            });
        }
    }
    if row.kind == RegistrationKind::Published && matches!(seen, Seen::NotFound) {
        return Ok(Step {
            outcome: outcome(
                row,
                "observed_not_found_unverified",
                "filehost_part_absence_observation",
                "not_found",
            ),
            paused: false,
        });
    }
    let session = SessionId::parse(&row.session_id).map_err(|_| Error::Corrupt)?;
    deploy::write_checkpoint(deploy, "metadata_api_calling", &row.registration_id)?;
    let abort = host.abort(&ctx, &session).await;
    deploy::write_checkpoint(deploy, "metadata_api_done", &row.registration_id)?;
    if pause == PauseAfter::MetadataApi {
        return Ok(Step {
            outcome: outcome(
                row,
                "paused_after_metadata_api",
                abort_seam(&abort),
                "not_observed",
            ),
            paused: true,
        });
    }
    finish_after_abort(deploy, host, row, &ctx, &key, abort).await
}

async fn finish_after_abort(
    deploy: &Deployment,
    host: &FileHost,
    row: &Registration,
    ctx: &AdmissionContext,
    key: &ObjectKey,
    abort: Result<(), FileHostError>,
) -> Result<Step, Error> {
    let after = observe(host, ctx, key).await;
    let after_label = seen_label(&after);
    match (row.kind, abort, after) {
        (RegistrationKind::Published, Err(FileHostError::Closed), Seen::Present(manifest))
            if manifest_matches(row, &manifest) =>
        {
            Ok(Step {
                outcome: outcome(
                    row,
                    "unsupported_completed_object_delete",
                    "filehost_completed_object_delete",
                    "found",
                ),
                paused: false,
            })
        }
        (RegistrationKind::Published, Err(FileHostError::Closed), _) => Ok(Step {
            outcome: outcome(
                row,
                "unsupported_completed_object_delete",
                "filehost_completed_object_delete",
                after_label,
            ),
            paused: false,
        }),
        (RegistrationKind::Open, Ok(()), _) => {
            inventory::set_state(deploy, &row.registration_id, RowState::AbortAttempted)?;
            Ok(Step {
                outcome: outcome(
                    row,
                    "aborted_parts_unverified",
                    "filehost_part_absence_observation",
                    after_label,
                ),
                paused: false,
            })
        }
        (RegistrationKind::Open, Err(FileHostError::NotFound), _) => Ok(Step {
            outcome: outcome(
                row,
                "aborted_parts_unverified",
                "filehost_part_absence_observation",
                after_label,
            ),
            paused: false,
        }),
        (RegistrationKind::Open, Err(FileHostError::Closed), Seen::Present(_)) => Ok(Step {
            outcome: outcome(
                row,
                "unsupported_completed_object_delete",
                "filehost_completed_object_delete",
                "found",
            ),
            paused: false,
        }),
        (_, Err(FileHostError::Denied), _) => Ok(Step {
            outcome: outcome(row, "refused_foreign", "none", "denied"),
            paused: false,
        }),
        (_, Err(FileHostError::Store | FileHostError::Unavailable), _) => Ok(Step {
            outcome: outcome(row, "read_error_not_absence", "none", "store_error"),
            paused: false,
        }),
        _ => Ok(Step {
            outcome: outcome(row, "read_error_not_absence", "none", after_label),
            paused: false,
        }),
    }
}

fn abort_seam(abort: &Result<(), FileHostError>) -> &'static str {
    match abort {
        Err(FileHostError::Closed) => "filehost_completed_object_delete",
        Ok(()) | Err(FileHostError::NotFound) => "filehost_part_absence_observation",
        _ => "none",
    }
}

fn outcome(
    row: &Registration,
    status: &'static str,
    seam: &'static str,
    filehost_after: &'static str,
) -> ObjectOutcome {
    ObjectOutcome {
        registration_id: row.registration_id.clone(),
        status,
        seam,
        filehost_after,
        absence_verified: false,
        deleted: false,
    }
}

#[allow(clippy::large_enum_variant)]
enum Seen {
    Present(ObjectManifest),
    NotFound,
    Denied,
    Corrupt,
    Store,
}

fn seen_label(seen: &Seen) -> &'static str {
    match seen {
        Seen::Present(_) => "found",
        Seen::NotFound => "not_found",
        Seen::Denied => "denied",
        Seen::Corrupt => "corrupt",
        Seen::Store => "store_error",
    }
}

async fn observe(host: &FileHost, ctx: &AdmissionContext, key: &ObjectKey) -> Seen {
    let mut download = match host.open_download(ctx, key, 8192, None).await {
        Ok(download) => download,
        Err(FileHostError::NotFound) => return Seen::NotFound,
        Err(FileHostError::Denied) => return Seen::Denied,
        Err(FileHostError::Corrupt) => return Seen::Corrupt,
        Err(_) => return Seen::Store,
    };
    let manifest = download.manifest().clone();
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    loop {
        match download.next_chunk().await {
            Ok(Some(chunk)) => {
                hasher.update(&chunk);
                let Ok(len) = u64::try_from(chunk.len()) else {
                    return Seen::Corrupt;
                };
                let Some(next) = total.checked_add(len) else {
                    return Seen::Corrupt;
                };
                total = next;
            }
            Ok(None) => break,
            Err(FileHostError::Corrupt) => return Seen::Corrupt,
            Err(FileHostError::NotFound) => return Seen::Store,
            Err(_) => return Seen::Store,
        }
    }
    if !download.is_complete() {
        return Seen::Corrupt;
    }
    let digest = hasher.finalize();
    if total != manifest.size || AsRef::<[u8]>::as_ref(&digest) != manifest.object_digest {
        return Seen::Corrupt;
    }
    Seen::Present(manifest)
}

fn manifest_matches(row: &Registration, manifest: &ObjectManifest) -> bool {
    row.object_key == manifest.object_key.as_str()
        && row.session_id == manifest.session_id.as_str()
        && row.object_digest_hex == hex_encode(&manifest.object_digest)
        && row.size == manifest.size
        && row.project == manifest.project_id.as_str()
        && row.principal == manifest.principal_id.as_str()
        && row.idempotency_key == manifest.idempotency_key.as_str()
        && (row.kind == RegistrationKind::Open
            || (row.manifest_contract == manifest.contract
                && row.lineage_digest_hex == hex_encode(&manifest.lineage_digest)
                && row.part_count == Some(manifest.part_count)
                && row.grant_hex == hex_encode(manifest.grant_evidence.as_str().as_bytes())))
}

fn admission(row: &Registration) -> Result<AdmissionContext, Error> {
    let grant_bytes = hex_decode(&row.grant_hex)?;
    let grant = String::from_utf8(grant_bytes).map_err(|_| Error::Corrupt)?;
    Ok(AdmissionContext {
        project_id: ProjectId::new(&row.project).map_err(|_| Error::Corrupt)?,
        principal_id: PrincipalId::new(&row.principal).map_err(|_| Error::Corrupt)?,
        grant_evidence: GrantEvidence::new(grant).map_err(|_| Error::Corrupt)?,
    })
}

fn write_audit(
    deploy: &Deployment,
    event: &str,
    row: &Registration,
    now_ms: u64,
    phase: &str,
) -> Result<(), Error> {
    let dir = deploy.audit_dir();
    let mut numbers = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|_| Error::Store)? {
        let entry = entry.map_err(|_| Error::Store)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(".txt")
            && stem.bytes().all(|byte| byte.is_ascii_digit())
        {
            numbers.push(stem.to_owned());
        }
    }
    if numbers.len() >= 64 {
        return Err(Error::Limit);
    }
    numbers.sort();
    let next = numbers.len() + 1;
    let name = format!("{next:04}.txt");
    let body = format!(
        "oo.artifact-retention/v1\nevent {event}\nregistration_id {}\nobject_digest {}\nnow_unix_ms {now_ms}\nphase {phase}\n",
        row.registration_id, row.object_digest_hex
    );
    text::write_hashed(&dir, &name, body.as_bytes())
}

pub fn report_json(report: &ReapReport) -> String {
    let mut outcomes = String::from("[");
    for (index, outcome) in report.outcomes.iter().enumerate() {
        if index > 0 {
            outcomes.push(',');
        }
        outcomes.push_str(&format!(
            "{{\"registration_id\":\"{}\",\"status\":\"{}\",\"seam\":\"{}\",\"filehost_after\":\"{}\",\"absence_verified\":{},\"deleted\":{}}}",
            outcome.registration_id,
            outcome.status,
            outcome.seam,
            outcome.filehost_after,
            outcome.absence_verified,
            outcome.deleted
        ));
    }
    outcomes.push(']');
    format!(
        "{{\"contract\":\"oo.artifact-retention/v1\",\"deletion_success\":{},\"idle\":{},\"paused\":{},\"restarted_from_partial\":{},\"parent_complete\":{},\"outcomes\":{outcomes}}}",
        report.deletion_success,
        report.idle,
        report.paused,
        report.restarted_from_partial,
        report.parent_complete
    )
}
