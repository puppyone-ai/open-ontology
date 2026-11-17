//! Block level reuse (10 §16.1, D53 first half): a new version of a file whose
//! blocks are the same as the blocks of its previous version does not run the
//! extraction again. The facts it would produce are the facts that already
//! exist, so the evidence moves to the new version instead.
//!
//! A block id is `blk_` + a hash of its anchor and its text (10 §3.4), so an
//! equal sequence of ids means equal text at equal positions; formatting,
//! styles and document properties are not part of the IR. The previous
//! version must have been extracted by the current ruleset, otherwise the old
//! facts say nothing about what the rules would find now.

use sqlx::PgPool;

use crate::cas::{Cas, CasKind};
use crate::extract::rules::RULESET_VERSION;
use oo_core::docir::DocIr;

/// The previous version of `file_id` whose live evidence a new version with
/// the blocks of `ir` can take over. `None` when the blocks differ, there is
/// no such version, or its IR is not at hand.
pub async fn reusable_version(
    pool: &PgPool,
    cas: &Cas,
    file_id: &str,
    version_id: &str,
    ir: &DocIr,
) -> Result<Option<String>, sqlx::Error> {
    let prev: Option<(String, String)> = sqlx::query_as(
        "SELECT fv.id::text, fv.b3 FROM _oo_docs.file_version fv \
          WHERE fv.file_id = $1::uuid AND fv.id <> $2::uuid AND fv.superseded_at IS NOT NULL \
            AND fv.b3 <> (SELECT b3 FROM _oo_docs.file_version WHERE id = $2::uuid) \
            AND EXISTS (SELECT 1 FROM _oo_docs.evidence e \
                         WHERE e.version_id = fv.id AND e.withdrawn_at IS NULL) \
            AND EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'extract_rules' \
                         AND j.version_id = fv.id AND j.state = 'succeeded' \
                         AND j.dedupe_key LIKE '%:' || $3) \
          ORDER BY fv.created_at DESC, fv.id LIMIT 1",
    )
    .bind(file_id)
    .bind(version_id)
    .bind(RULESET_VERSION)
    .fetch_optional(pool)
    .await?;
    let Some((old_version, old_b3)) = prev else {
        return Ok(None);
    };
    let old_ir_b3: Option<String> = sqlx::query_scalar(
        "SELECT ir_b3 FROM _oo_docs.parse_result \
          WHERE b3 = $1 AND status = 'complete' AND ir_b3 IS NOT NULL \
          ORDER BY finished_at DESC NULLS LAST LIMIT 1",
    )
    .bind(&old_b3)
    .fetch_optional(pool)
    .await?;
    let Some(old_ir_b3) = old_ir_b3 else {
        return Ok(None);
    };
    let Ok(bytes) = cas.get(CasKind::Ir, &old_ir_b3) else {
        return Ok(None);
    };
    let Ok(old_ir) = serde_json::from_slice::<DocIr>(&bytes) else {
        return Ok(None);
    };
    Ok(same_blocks(&old_ir, ir).then_some(old_version))
}

/// Same number of blocks with the same ids in the same order.
pub fn same_blocks(a: &DocIr, b: &DocIr) -> bool {
    a.blocks.len() == b.blocks.len() && a.blocks.iter().zip(&b.blocks).all(|(x, y)| x.id == y.id)
}
