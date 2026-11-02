//! Reuse a sealed semantic candidate through the existing gates and target fences.
use super::*;

pub struct DataOnlyParent<'a> {
    pub candidate_id: &'a str,
    pub source_hash: &'a str,
    pub mapping_sha256: &'a str,
}

pub async fn maintain_data_only(
    request: &MaintainRequest<'_>,
    parent: &DataOnlyParent<'_>,
) -> Result<Receipt, StoreError> {
    checked(request)?;
    if parent.candidate_id == request.candidate_id || request.native_mapping.is_none() {
        return Err(StoreError::Input);
    }
    let mut session = open(request.control.endpoint, request.control.user,
        request.control.password, request.control.ca_pem).await?;
    require_tls(&session.client).await?;
    session.client.query_one(sql::LOCK_SESSION, &[&MAINTAIN_LOCK]).await.map_err(query_error)?;
    let result = advance(&mut session, request, parent).await;
    let _ = session.client.query_one(sql::UNLOCK_SESSION, &[&MAINTAIN_LOCK]).await;
    result
}

async fn advance(
    session: &mut store::Session,
    request: &MaintainRequest<'_>,
    parent: &DataOnlyParent<'_>,
) -> Result<Receipt, StoreError> {
    let pointer = read_pointer(session).await?;
    let before = read_candidate(session, parent.candidate_id).await?.ok_or(StoreError::Input)?;
    let after = read_candidate(session, request.candidate_id).await?.ok_or(StoreError::Input)?;
    let old_mapping = known_mapping(session, parent.candidate_id).await?.ok_or(StoreError::Input)?;
    let next_mapping = request.native_mapping.ok_or(StoreError::Input)?;
    if before.source_hash != parent.source_hash
        || crate::canon::canonical_sha(&old_mapping) != parent.mapping_sha256
        || before.pack_digest != after.pack_digest || before.version == after.version
        || before.effective != after.effective || before.visibility != after.visibility
        || after.source_hash != request.source_hash
        || request.expected_version != Some(before.version.as_str())
    {
        return Ok(refused("data_only_semantic_binding", json!({"fence_touched":false})));
    }
    crate::native_library::data_only_successor(&old_mapping, next_mapping)?;
    // Release versions advance independently from immutable semantic versions.
    // Also bind the parent candidate, not just the version supplied by a caller.
    if pointer.candidate_id.as_deref() != Some(parent.candidate_id) {
        if pointer.candidate_id.as_deref() != Some(request.candidate_id)
            || known_mapping(session, request.candidate_id).await?.as_ref() != Some(next_mapping)
        {
            return Ok(refused("data_only_parent_conflict", json!({"fence_touched":false})));
        }
        // A replay must also repair a previously ignored retirement refusal.
        // Resolve both immutable vectors; never infer old physical names.
        let old_context = target::ValidatedNativeContext::new(&old_mapping)?;
        let next_context = target::ValidatedNativeContext::new(next_mapping)?;
        for logical in installed_visibility_bindings(request, &before.visibility)
            .map_err(|_| StoreError::Input)?
        {
            let old = resolve_validated_binding(&request.target, Some(&old_context), &logical).await?;
            let next = resolve_validated_binding(&request.target, Some(&next_context), &logical).await?;
            if (old.0.as_str(), old.1.as_str(), old.2.as_str())
                != (next.0.as_str(), next.1.as_str(), next.2.as_str())
            {
                let fence = FenceRef { schema_name: &old.0, relation_name: &old.1, role_name: &old.2 };
                let retired = target::retire_target(&request.target, &fence, &after.version).await?;
                if retired.code != 0 {
                    return Ok(retired);
                }
            }
        }
    }
    let mut result = activate_bound(session, request).await?;
    result.body["data_only"] = json!({
        "protocol":"oo.release-coordinator/data-only-use/v1",
        "parent_candidate_id":parent.candidate_id,
        "parent_source_hash":parent.source_hash,
        "parent_mapping_sha256":parent.mapping_sha256,
        "mapping_sha256":crate::canon::canonical_sha(next_mapping),
        "pack_digest":before.pack_digest,
        "semantic_candidate_unchanged":true
    });
    Ok(result)
}
