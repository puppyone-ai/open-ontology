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
    QuotaReleased,
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

struct Target {
    digest: [u8; 32],
    size: u64,
    lineage: [u8; 32],
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
            Some("pre_delete")
                | Some("metadata_api_calling")
                | Some("metadata_api_done")
                | Some("quota_released")
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
        let deletion_success = outcomes
            .iter()
            .all(|item| item.deleted && item.absence_verified);
        Ok(ReapReport {
            deletion_success,
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
    if !matches!(seen, Seen::Present(_) | Seen::NotFound) {
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
    if let Seen::Present(manifest) = &seen
        && !manifest_matches(row, manifest)
    {
        return Ok(Step {
            outcome: outcome(row, "refused_drift", "none", "found"),
            paused: false,
        });
    }
    if row.kind == RegistrationKind::Open && !matches!(seen, Seen::Present(_)) {
        return abort_open(deploy, host, row, &ctx, &key, pause).await;
    }
    let target = if let Seen::Present(manifest) = &seen {
        target_from_manifest(manifest)
    } else {
        let target = target_from_row(row)?;
        match held_absence(host, row, &ctx, &key, &target).await? {
            Held::Done => {
                return Ok(Step {
                    outcome: deleted_outcome(row),
                    paused: false,
                });
            }
            Held::Continue => target,
            Held::Stop(outcome) => {
                return Ok(Step {
                    outcome,
                    paused: false,
                });
            }
        }
    };
    if pause == PauseAfter::MetadataApi {
        deploy::write_checkpoint(deploy, "metadata_api_calling", &row.registration_id)?;
        deploy::write_checkpoint(deploy, "metadata_api_done", &row.registration_id)?;
        let after = if matches!(seen, Seen::Present(_)) {
            "found"
        } else {
            "not_found"
        };
        return Ok(Step {
            outcome: outcome(row, "paused_after_metadata_api", "none", after),
            paused: true,
        });
    }
    delete_published(deploy, host, row, &ctx, &key, &target, pause).await
}

async fn abort_open(
    deploy: &Deployment,
    host: &FileHost,
    row: &Registration,
    ctx: &AdmissionContext,
    key: &ObjectKey,
    pause: PauseAfter,
) -> Result<Step, Error> {
    let session = SessionId::parse(&row.session_id).map_err(|_| Error::Corrupt)?;
    deploy::write_checkpoint(deploy, "metadata_api_calling", &row.registration_id)?;
    let abort = host.abort(ctx, &session).await;
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
    if matches!(abort, Err(FileHostError::Closed)) {
        let again = observe(host, ctx, key).await;
        if let Seen::Present(manifest) = &again
            && manifest_matches(row, manifest)
        {
            let target = target_from_manifest(manifest);
            return delete_published(deploy, host, row, ctx, key, &target, PauseAfter::None).await;
        }
    }
    finish_abort(deploy, host, row, ctx, key, abort).await
}

async fn finish_abort(
    deploy: &Deployment,
    host: &FileHost,
    row: &Registration,
    ctx: &AdmissionContext,
    key: &ObjectKey,
    abort: Result<(), FileHostError>,
) -> Result<Step, Error> {
    let after = observe(host, ctx, key).await;
    let after_label = seen_label(&after);
    match abort {
        Ok(()) => {
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
        Err(FileHostError::NotFound | FileHostError::Closed) => Ok(Step {
            outcome: outcome(
                row,
                "aborted_parts_unverified",
                "filehost_part_absence_observation",
                after_label,
            ),
            paused: false,
        }),
        Err(error) => Ok(Step {
            outcome: host_failure(row, error),
            paused: false,
        }),
    }
}

async fn delete_published(
    deploy: &Deployment,
    host: &FileHost,
    row: &Registration,
    ctx: &AdmissionContext,
    key: &ObjectKey,
    target: &Target,
    pause: PauseAfter,
) -> Result<Step, Error> {
    deploy::write_checkpoint(deploy, "metadata_api_calling", &row.registration_id)?;
    let stopped = host
        .delete_completed(ctx, key, target.digest, target.size, target.lineage, true)
        .await;
    deploy::write_checkpoint(deploy, "metadata_api_done", &row.registration_id)?;
    match stopped {
        Ok(None) => {
            deploy::write_checkpoint(deploy, "quota_released", &row.registration_id)?;
            if pause == PauseAfter::QuotaReleased {
                return Ok(Step {
                    outcome: outcome(row, "paused_after_quota_release", "none", "found"),
                    paused: true,
                });
            }
        }
        Ok(Some(_)) => {}
        Err(error) => {
            return Ok(Step {
                outcome: host_failure(row, error),
                paused: false,
            });
        }
    }
    let finished = host
        .delete_completed(ctx, key, target.digest, target.size, target.lineage, false)
        .await;
    let observed = host
        .observe_completed_absence(ctx, key, target.digest, target.size, target.lineage)
        .await;
    let count = host.completed_quota_release_count(ctx, key).await;
    Ok(Step {
        outcome: deletion_outcome(row, finished, observed, count),
        paused: false,
    })
}

fn deletion_outcome(
    row: &Registration,
    finished: Result<Option<ObjectManifest>, FileHostError>,
    observed: Result<bool, FileHostError>,
    count: Result<u8, FileHostError>,
) -> ObjectOutcome {
    match (finished, observed, count) {
        (Ok(Some(_)), Ok(true), Ok(1)) => deleted_outcome(row),
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => host_failure(row, error),
        _ => outcome(row, "read_error_not_absence", "none", "not_found"),
    }
}

enum Held {
    Done,
    Continue,
    Stop(ObjectOutcome),
}

async fn held_absence(
    host: &FileHost,
    row: &Registration,
    ctx: &AdmissionContext,
    key: &ObjectKey,
    target: &Target,
) -> Result<Held, Error> {
    match host
        .observe_completed_absence(ctx, key, target.digest, target.size, target.lineage)
        .await
    {
        Ok(true) => match host.completed_quota_release_count(ctx, key).await {
            Ok(1) => Ok(Held::Done),
            Ok(_) => Ok(Held::Stop(outcome(
                row,
                "read_error_not_absence",
                "none",
                "not_found",
            ))),
            Err(error) => Ok(Held::Stop(host_failure(row, error))),
        },
        Ok(false) | Err(FileHostError::Conflict) => Ok(Held::Continue),
        Err(FileHostError::NotFound) => Ok(Held::Stop(outcome(
            row,
            "observed_not_found_unverified",
            "filehost_part_absence_observation",
            "not_found",
        ))),
        Err(error) => Ok(Held::Stop(host_failure(row, error))),
    }
}

fn target_from_manifest(manifest: &ObjectManifest) -> Target {
    Target {
        digest: manifest.object_digest,
        size: manifest.size,
        lineage: manifest.lineage_digest,
    }
}

fn target_from_row(row: &Registration) -> Result<Target, Error> {
    Ok(Target {
        digest: decode_digest(&row.object_digest_hex)?,
        size: row.size,
        lineage: decode_digest(&row.lineage_digest_hex)?,
    })
}

fn decode_digest(hex_text: &str) -> Result<[u8; 32], Error> {
    let bytes = hex_decode(hex_text)?;
    bytes.try_into().map_err(|_| Error::Corrupt)
}

fn abort_seam(abort: &Result<(), FileHostError>) -> &'static str {
    match abort {
        Err(FileHostError::Closed) => "filehost_completed_object_delete",
        Ok(()) | Err(FileHostError::NotFound) => "filehost_part_absence_observation",
        _ => "none",
    }
}

fn host_failure(row: &Registration, error: FileHostError) -> ObjectOutcome {
    let (status, after) = match error {
        FileHostError::Denied => ("refused_foreign", "denied"),
        FileHostError::Corrupt => ("refused_tamper_or_symlink", "corrupt"),
        FileHostError::NotFound => ("observed_not_found_unverified", "not_found"),
        FileHostError::Conflict => ("refused_conflict", "found"),
        _ => ("read_error_not_absence", "store_error"),
    };
    outcome(row, status, "none", after)
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

fn deleted_outcome(row: &Registration) -> ObjectOutcome {
    ObjectOutcome {
        registration_id: row.registration_id.clone(),
        status: "deleted",
        seam: "none",
        filehost_after: "absent",
        absence_verified: true,
        deleted: true,
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
