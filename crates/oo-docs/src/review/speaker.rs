//! Speaker identity (10 17.4, D74, board B-S10-08): `speaker_identity` review items and what a decision does.
//!
//! Three layers feed the queue (`extract::rules::media`): attendee names from meeting metadata, self
//! introductions and name calls, and the person who decides here. A label (`S1`) means nothing outside its file.
//! Extraction never waits for them, but nothing is auto-confirmed (D74). What a speaker's statements produced belongs
//! to a provisional Person (`Speaker 1 · <blob prefix>`) and stays draft ([`HELD_SQL`] is the condition the
//! publisher's classifier uses): not published, not in the BRIEF. The rest of the file proceeds. What the transcript
//! or the roster says ("我是小王", "I am Wanqing", the one attendee left) is only a candidate on the
//! `speaker_identity` item; confirming merges the provisional Person into the named one, so the facts are
//! re-attributed and judged like any other draft (a later re-extraction keeps the choice, [`apply_confirmed`]).
//! Nothing about a voice is stored: no voiceprint, no embedding, no comparison across recordings.

use oo_core::docir::DocIr;
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, Row};

use super::overlay;
use crate::extract::model::EntityType;
use crate::extract::rules::media::{self, SpeakerCandidate};
use crate::resolve::merge::{MergeError, merge_entities};
use crate::resolve::normalize::{entity_key, norm};
use crate::store::StoreError;

/// `review_item.kind`.
pub const KIND: &str = "speaker_identity";
/// `review_item.reason_code` of an unconfirmed speaker.
pub const REASON: &str = "speaker_unconfirmed";
/// `DOCS_REVIEW_SPEAKER_UNCONFIRMED`: a person tried to accept a fact whose speaker is still unknown.
pub const CODE_UNCONFIRMED: &str = "DOCS_REVIEW_SPEAKER_UNCONFIRMED";

/// SQL condition on the alias `s` (the fact's subject entity): the subject is a speaker placeholder.
pub const HELD_SQL: &str = "(s.type = 'Person' AND s.canonical_name LIKE '% · %' \
     AND (s.canonical_name LIKE 'Speaker %' OR s.canonical_name LIKE '说话人 S%'))";

/// Overlay `speaker_map` binds: one speaker of one recording (by blob hash, so a renamed copy shares it).
pub fn binds(blob_b3: &str, speaker: &str) -> String {
    format!("speaker:{blob_b3}:{speaker}")
}

fn candidate_json(c: &SpeakerCandidate) -> Value {
    json!({
        "name": c.name, "source": c.source.as_str(),
        "block_id": c.block_id, "quote": c.quote,
    })
}

/// The `subject` of the review item of one speaker: the attendee set and the candidates pointing at this label.
pub fn subject(
    ir: &DocIr,
    speaker: &str,
    attendees: &[SpeakerCandidate],
    intros: &[SpeakerCandidate],
) -> Value {
    let mut cands: Vec<Value> = intros
        .iter()
        .filter(|c| c.speaker.as_deref() == Some(speaker))
        .map(candidate_json)
        .collect();
    cands.extend(attendees.iter().map(candidate_json));
    json!({
        "b3": ir.blob_b3,
        "speaker": speaker,
        "placeholder": media::placeholder_name(&ir.blob_b3, speaker),
        "candidates": cands,
    })
}

/// Queue one `speaker_identity` item per speaker label of `ir` that has no confirmed mapping.
/// Returns how many items were added.
pub async fn propose(
    conn: &mut PgConnection,
    version_id: &str,
    ir: &DocIr,
    path: &str,
    minutes: Option<&str>,
) -> Result<u32, StoreError> {
    let attendees = media::attendees(path, minutes);
    let mut intros = media::introductions(ir);
    // The one attendee nobody else claims is a candidate for the one label left (never confirmed by itself).
    for (spk, name) in media::resolved(ir, &attendees, &intros) {
        if !intros
            .iter()
            .any(|c| c.speaker.as_deref() == Some(&spk) && c.name == name)
        {
            intros.push(SpeakerCandidate {
                speaker: Some(spk),
                name,
                source: media::CandidateSource::Attendees,
                block_id: None,
                quote: None,
            });
        }
    }
    let mut added = 0;
    for spk in media::speakers(ir) {
        let confirmed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM _oo_docs.overlay_current \
              WHERE kind = 'speaker_map' AND binds = $1 AND status = 'active')",
        )
        .bind(binds(&ir.blob_b3, &spk))
        .fetch_one(&mut *conn)
        .await?;
        if confirmed {
            continue;
        }
        let done = sqlx::query(
            "INSERT INTO _oo_docs.review_item \
               (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, \
                confidence_bp, reason_code) \
             SELECT gen_random_uuid(), 'speaker_identity', $1, f.drive_id, f.id, fv.id, $2::jsonb, NULL, $4 \
               FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id \
              WHERE fv.id = $3::uuid \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(format!("speaker_identity:{}:{spk}", ir.blob_b3))
        .bind(subject(ir, &spk, &attendees, &intros).to_string())
        .bind(version_id)
        .bind(REASON)
        .execute(&mut *conn)
        .await?;
        added += u32::try_from(done.rows_affected()).unwrap_or(0);
    }
    Ok(added)
}

/// A recording that is extracted again (a parser bump, a prompt cycle) gets a fresh placeholder Person
/// for every label, because the one that was merged is gone. A label a person has already confirmed
/// (overlay `speaker_map`) must not fall back to a placeholder: merge the new placeholder into the
/// mapped Person at once, so the statements are judged like any other draft and the file is not held
/// back by speakers that nobody has to decide again. Returns how many placeholders were merged.
pub async fn apply_confirmed(conn: &mut PgConnection, ir: &DocIr) -> Result<u32, StoreError> {
    let mut merged = 0;
    for spk in media::speakers(ir) {
        let mapped: Option<String> = sqlx::query_scalar(
            "SELECT payload->>'entity_key' FROM _oo_docs.overlay_current \
              WHERE kind = 'speaker_map' AND binds = $1 AND status = 'active' LIMIT 1",
        )
        .bind(binds(&ir.blob_b3, &spk))
        .fetch_optional(&mut *conn)
        .await?
        .flatten();
        let Some(person_key) = mapped else { continue };
        let placeholder: Option<String> = sqlx::query_scalar(
            "SELECT key FROM _oo_docs.entity WHERE type = 'Person' AND canonical_name = $1 AND status = 'active'",
        )
        .bind(media::placeholder_name(&ir.blob_b3, &spk))
        .fetch_optional(&mut *conn)
        .await?;
        let Some(from) = placeholder.filter(|f| *f != person_key) else {
            continue;
        };
        let mut sp = conn.begin().await?;
        match merge_entities(&mut sp, &from, &person_key, "human", None).await {
            Ok(_) => {
                sp.commit().await?;
                merged += 1;
            }
            Err(MergeError::Store(e)) => return Err(e),
            // An identity conflict: the statements stay held and a person looks.
            Err(_) => sp.rollback().await?,
        }
    }
    Ok(merged)
}

/// Result of a confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirmed {
    /// The mapping is written; `merged` says whether a placeholder with facts was merged into the Person.
    Done { person_key: String, merged: bool },
    /// More than one Person has this name: a person has to pick the entity.
    Ambiguous,
    /// The name is empty or is itself a placeholder.
    BadName,
    /// The merge was refused (identity conflict).
    Conflict,
}

/// Confirm (or correct) who a speaker is. `name` is the Person's name; the Person is found by normalized name,
/// or created when nobody has it yet. Writes overlay `speaker_map` and merges the speaker's placeholder.
pub async fn confirm(
    conn: &mut PgConnection,
    item_id: &str,
    item_subject: &Value,
    name: &str,
    by: &str,
) -> Result<Confirmed, StoreError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || media::is_placeholder(name) {
        return Ok(Confirmed::BadName);
    }
    let (Some(b3), Some(spk)) = (
        item_subject.get("b3").and_then(Value::as_str),
        item_subject.get("speaker").and_then(Value::as_str),
    ) else {
        return Ok(Confirmed::BadName);
    };
    let n = norm(name);
    let rows = sqlx::query(
        "SELECT key FROM _oo_docs.entity WHERE type = 'Person' AND norm_name = $1 AND status = 'active' \
          ORDER BY key LIMIT 2",
    )
    .bind(&n)
    .fetch_all(&mut *conn)
    .await?;
    let person_key = match rows.as_slice() {
        [] => {
            let key = entity_key(EntityType::Person, None, &n, None);
            sqlx::query(
                "INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) \
                 VALUES (gen_random_uuid(), $1, 'Person', $2, $3) ON CONFLICT (key) DO NOTHING",
            )
            .bind(&key)
            .bind(name)
            .bind(&n)
            .execute(&mut *conn)
            .await?;
            key
        }
        [one] => one.try_get::<String, _>("key")?,
        _ => return Ok(Confirmed::Ambiguous),
    };
    let placeholder: Option<String> = sqlx::query_scalar(
        "SELECT key FROM _oo_docs.entity WHERE type = 'Person' AND canonical_name = $1 AND status = 'active'",
    )
    .bind(media::placeholder_name(b3, spk))
    .fetch_optional(&mut *conn)
    .await?;
    let mut merged = false;
    if let Some(from) = placeholder {
        let mut sp = conn.begin().await?;
        match merge_entities(&mut sp, &from, &person_key, "human", Some(item_id)).await {
            Ok(_) => {
                sp.commit().await?;
                merged = true;
            }
            Err(MergeError::Store(e)) => return Err(e),
            Err(_) => {
                sp.rollback().await?;
                return Ok(Confirmed::Conflict);
            }
        }
    }
    overlay::insert(
        conn,
        "speaker_map",
        &binds(b3, spk),
        json!({"name": name, "entity_key": person_key}),
        by,
        Some(item_id),
        "active",
        None,
    )
    .await?;
    Ok(Confirmed::Done { person_key, merged })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::rules::media::CandidateSource;

    #[test]
    fn binds_are_per_recording_and_label() {
        assert_eq!(binds("ab", "S2"), "speaker:ab:S2");
    }

    #[test]
    fn the_item_subject_lists_the_candidates_of_that_label_and_the_attendee_set() {
        let ir = DocIr {
            protocol: "oo.doc-ir/v1.1".into(),
            blob_b3: "c".repeat(64),
            format: oo_core::docir::DocFormat::Audio,
            parser: oo_core::docir::ParserInfo {
                id: oo_core::docir::ParserId::Media,
                version: "1".into(),
                adapter_version: None,
                profile_sha256: "0".repeat(64),
                profile: None,
            },
            meta: Default::default(),
            blocks: vec![],
            warnings: vec![],
        };
        let intro = |s: &str, n: &str| SpeakerCandidate {
            speaker: Some(s.into()),
            name: n.into(),
            source: CandidateSource::SelfIntro,
            block_id: Some("blk_x".into()),
            quote: Some("我是".into()),
        };
        let att = SpeakerCandidate {
            speaker: None,
            name: "林川".into(),
            source: CandidateSource::Attendees,
            block_id: None,
            quote: None,
        };
        let v = subject(
            &ir,
            "S1",
            &[att],
            &[intro("S1", "小王"), intro("S2", "小李")],
        );
        let names: Vec<&str> = v["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["小王", "林川"]);
        assert_eq!(v["speaker"], "S1");
        assert!(media::is_placeholder(v["placeholder"].as_str().unwrap()));
    }

    #[test]
    fn a_provisional_speaker_is_held_by_name_and_a_real_person_is_not() {
        assert!(HELD_SQL.contains("Speaker %") && HELD_SQL.contains("说话人 S%"));
        assert!(media::is_placeholder(&media::placeholder_name(
            &"d".repeat(64),
            "S3"
        )));
        assert!(!media::is_placeholder("Speaker Lee"));
    }
}
