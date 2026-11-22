//! Speaker identity against one throwaway PostgreSQL (B-S10-08, 10 17.4, D74 as revised 2026-11-21): a conversation
//! publishes at once under the placeholder speakers "Speaker A/B", a `speaker_identity` item per label is queued
//! without blocking anything, and naming a speaker later merges the placeholder into the named Person so the facts
//! are re-attributed. A recording that is not a conversation has no speakers and no items.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test speaker_live -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::Fx;
use oo_core::docir::DocFormat;
use oo_docs::normalize::{finish, media::assemble};
use oo_docs::publish::run::PublishOutcome;
use oo_docs::review::decide::{Decision, ItemDecision, decide_items};
use serde_json::json;

fn meeting(seed: &str) -> oo_docs::normalize::Normalized {
    let resp = json!({
        "protocol": "oo.doc-parser/v1", "parser": {"id": "media", "version": "1", "adapter_version": "1"},
        "kind": "audio", "container": "wav", "codec": "pcm", "duration_ms": 16_000, "start_ms": 0,
        "speakers": ["S1", "S2"],
        "profile": {"tier": "cpu", "isa": "aarch64:asimd", "models": []},
        "items": [
            {"kind": "utterance", "speaker": "S1", "t_start_ms": 0, "t_end_ms": 4000,
             "text": format!("大家好，我是小王。我负责星火计划。{seed}"), "word_start": 0, "word_end": 9},
            {"kind": "utterance", "speaker": "S2", "t_start_ms": 5000, "t_end_ms": 9000,
             "text": "好的，我们开始。", "word_start": 10, "word_end": 14},
            {"kind": "utterance", "speaker": "S1", "t_start_ms": 10000, "t_end_ms": 12000,
             "text": "小李你说一下进度。", "word_start": 15, "word_end": 19},
            {"kind": "utterance", "speaker": "S2", "t_start_ms": 13000, "t_end_ms": 16000,
             "text": "进度正常。", "word_start": 20, "word_end": 23}
        ],
        "warnings": []
    });
    let frag = assemble(
        DocFormat::Audio,
        "1".repeat(63) + seed.get(..1).unwrap_or("2"),
        &resp,
    )
    .unwrap();
    finish(frag).unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn an_unnamed_speaker_publishes_under_a_placeholder_and_naming_re_attributes_the_facts() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_speaker";
    let mut fx = Fx::new(db).await;
    let planted = fx
        .plant_normalized("会议/周会 参会=小王、小李.m4a", meeting("a"), 1000)
        .await;
    let doc = oo_docs::publish::write::DocumentInput {
        version_id: &planted.version_id,
        b3: &planted.b3,
        path: "会议/周会 参会=小王、小李.m4a",
        ir: &planted.ir,
    };
    oo_docs::publish::write::index_document(&mut fx.rt, &doc)
        .await
        .unwrap();
    let added = oo_docs::review::speaker::propose(
        &mut fx.rt,
        &planted.version_id,
        &planted.ir,
        "会议/周会 参会=小王、小李.m4a",
        None,
    )
    .await
    .unwrap();
    assert_eq!(added, 2, "one item per speaker label");
    // Proposing again adds nothing (dedupe).
    assert_eq!(
        oo_docs::review::speaker::propose(
            &mut fx.rt,
            &planted.version_id,
            &planted.ir,
            "x.m4a",
            None
        )
        .await
        .unwrap(),
        0
    );

    // The statement "我负责星火计划" is published at once, owned by the placeholder speaker (D74 as revised).
    let (PublishOutcome::Published(_) | PublishOutcome::NoChange { .. }) = fx.publish().await
    else {
        panic!("publish")
    };
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.media_profile WHERE class = 'conversation' AND source = 'rules'"
        )
        .await,
        1,
        "the recording was classified before its speakers were told apart"
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND s.canonical_name LIKE '《%》的说话人 A · %' AND f.status = 'published'"
        )
        .await,
        1,
        "published under the placeholder before anybody is asked"
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact WHERE predicate = 'owns' AND status = 'draft'"
        )
        .await,
        0,
        "nothing waits for the speaker to be named"
    );
    // The Document does not "mention" the placeholder.
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity o ON o.id = f.object_id \
              WHERE f.predicate = 'mentions' AND o.canonical_name LIKE '《%》的说话人 %'"
        )
        .await,
        0
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'published'")
            .await,
        1,
        "the file is published with its speakers still unnamed"
    );
    // Naming a speaker is not a review (D97): the items wait in the file's own list and count nowhere.
    oo_docs::review::queue::refresh_pending_counts(&mut fx.rt)
        .await
        .unwrap();
    assert_eq!(
        fx.scalar("SELECT COALESCE(sum(pending_review), 0) FROM _oo_docs.file")
            .await,
        0
    );
    // The typed objects (D93, D98): a Recording, a Meeting recorded in it, and its two participants.
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity WHERE type = 'Recording'")
            .await,
        1
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity m ON m.id = f.subject_id \
              WHERE m.type = 'Meeting' AND f.predicate = 'recorded_in' AND f.status = 'published'"
        )
        .await,
        1
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity p ON p.id = f.object_id \
              WHERE f.predicate = 'has_participant' AND f.status = 'published' \
                AND p.canonical_name LIKE '《%》的说话人 %'"
        )
        .await,
        2,
        "the participants are the placeholder speakers"
    );

    // The S1 item carries the self introduction and the attendee names as candidates.
    let (id, revision): (String, i64) = sqlx::query_as(
        "SELECT id::text, revision FROM _oo_docs.review_item WHERE kind = 'speaker_identity' \
          AND subject->>'speaker' = 'S1'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    let cands: String = sqlx::query_scalar(
        "SELECT (subject->'candidates')::text FROM _oo_docs.review_item WHERE id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    let cands: serde_json::Value = serde_json::from_str(&cands).unwrap();
    assert_eq!(cands[0]["name"], "小王");
    assert_eq!(cands[0]["source"], "self_intro");
    // The one attendee nobody else claims is a candidate for S2; nothing is confirmed by itself.
    let s2: String = sqlx::query_scalar(
        "SELECT (subject->'candidates'->0->>'name') FROM _oo_docs.review_item \
          WHERE kind = 'speaker_identity' AND subject->>'speaker' = 'S2'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    assert_eq!(s2, "小李");
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'speaker_map'")
            .await,
        0,
        "a candidate is never a confirmation"
    );

    // Confirm S1 = 小王.
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[ItemDecision {
            id: id.clone(),
            revision,
            decision: Decision::Accept,
            edit: Some(json!({"name": "小王"})),
            note: None,
        }],
    )
    .await
    .unwrap();
    assert_eq!(out.results[0].status, "ok", "{:?}", out.results[0]);
    let (PublishOutcome::Published(_) | PublishOutcome::NoChange { .. }) = fx.publish().await
    else {
        panic!("publish after naming")
    };
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND f.status = 'published' AND s.canonical_name = '小王' \
                AND s.type = 'Person'"
        )
        .await,
        1,
        "re-attributed: the subject is the named Person"
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND s.canonical_name LIKE '《%》的说话人 A · %'"
        )
        .await,
        0,
        "nothing is left on the placeholder"
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity p ON p.id = f.object_id \
              WHERE f.predicate = 'has_participant' AND f.status = 'published' AND p.canonical_name = '小王'"
        )
        .await,
        1,
        "the meeting's participant became the named Person"
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'speaker_map' AND status = 'active'"
        )
        .await,
        1
    );
    // The voice embeddings and voice features have no place in the database (D63): no such column or
    // table. The one embedding column allowed is the optional text vector of a block
    // (`block_vector.embedding`, 0144, B-S13-02): it is computed from block text, never from audio.
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM information_schema.columns WHERE table_schema = '_oo_docs' \
              AND (column_name ILIKE '%voiceprint%' \
                   OR (column_name ILIKE '%embedding%' AND table_name <> 'block_vector'))"
        )
        .await,
        0
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM information_schema.columns WHERE table_schema = '_oo_docs' \
              AND table_name = 'block_vector' \
              AND (column_name ILIKE '%speaker%' OR column_name ILIKE '%audio%' OR column_name ILIKE '%media%')"
        )
        .await,
        0,
        "the text vector table carries no speaker or audio link"
    );
    fx.finish(db).await;
}

/// A8 (L6): after a parser bump the recording is extracted again. The speaker a person already named keeps
/// the Person (the new placeholder is merged at once) and a recording whose only drafts wait in the review
/// queue still returns to `published`.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_reprocessed_recording_keeps_its_confirmed_speaker_and_returns_to_published() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_speaker_a8";
    let mut fx = Fx::new(db).await;
    let path = A8_PATH;
    let planted = fx.plant_normalized(path, meeting("a"), 1000).await;
    let doc = oo_docs::publish::write::DocumentInput {
        version_id: &planted.version_id,
        b3: &planted.b3,
        path,
        ir: &planted.ir,
    };
    oo_docs::publish::write::index_document(&mut fx.rt, &doc)
        .await
        .unwrap();
    oo_docs::review::speaker::propose(&mut fx.rt, &planted.version_id, &planted.ir, path, None)
        .await
        .unwrap();
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("first publish")
    };
    // Confirm S1 = 小王 and publish.
    let (id, revision): (String, i64) = sqlx::query_as(
        "SELECT id::\"text\", revision FROM _oo_docs.review_item WHERE kind = 'speaker_identity' AND subject->>'speaker' = 'S1'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[ItemDecision {
            id,
            revision,
            decision: Decision::Accept,
            edit: Some(json!({"name": "小王"})),
            note: None,
        }],
    )
    .await
    .unwrap();
    assert_eq!(out.results[0].status, "ok");
    let (PublishOutcome::Published(_) | PublishOutcome::NoChange { .. }) = fx.publish().await
    else {
        panic!("confirmed publish")
    };
    assert_eq!(file_state(&mut fx).await, "published");

    // The parser is bumped and the recording is extracted again. A new placeholder row for the confirmed label S1
    // appears (the first one was merged away): it is merged into 小王 at once, nothing of S1 is held.
    let ph =
        oo_docs::extract::rules::media::placeholder_name(&planted.b3, "S1", "周会 参会=小王、小李");
    let k1 = oo_docs::resolve::normalize::entity_key(
        oo_docs::extract::model::EntityType::Person,
        None,
        "a8-placeholder",
        None,
    );
    fx.admin_sql(&format!(
        "INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) \
         VALUES (gen_random_uuid(), '{k1}', 'Person', '{ph}', lower('{ph}'))"
    ))
    .await;
    let merged = oo_docs::review::speaker::apply_confirmed(&mut fx.rt, &planted.ir, path)
        .await
        .unwrap();
    assert_eq!(
        merged, 1,
        "the new placeholder of a confirmed label is merged into the Person"
    );
    assert_eq!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.entity WHERE key = '{k1}' AND status = 'merged'"
        ))
        .await,
        1
    );
    assert_eq!(
        held(&mut fx).await,
        0,
        "nothing of S1 stays on a placeholder"
    );
    // And once more: nothing left to merge.
    assert_eq!(
        oo_docs::review::speaker::apply_confirmed(&mut fx.rt, &planted.ir, path)
            .await
            .unwrap(),
        0
    );

    // What the rerun leaves can be a draft statement that waits in the review queue. It must not keep the recording
    // from `published`, which is what happened to the meeting wav in the compose run.
    fx.admin_sql(&format!(
        "WITH f AS (INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, value, status, origin, confidence_bp) \
            SELECT gen_random_uuid(), repeat('a', 64), e.id, e.id, 'owns', '\"x\"'::jsonb, 'draft', 'rule', 9000 FROM _oo_docs.entity e \
             WHERE e.canonical_name = '{ph}' AND e.key <> '{k1}' RETURNING id), \
          q AS (INSERT INTO _oo_docs.review_item (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, reason_code) \
            SELECT gen_random_uuid(), 'low_confidence_fact', 'a8-q-' || f.id::text, fl.drive_id, fl.id, fv.id, \
                   jsonb_build_object('fact_id', f.id::text), 'low_confidence' \
              FROM f, _oo_docs.file_version fv JOIN _oo_docs.file fl ON fl.id = fv.file_id WHERE fv.id = '{}'::uuid) \
         INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, quote_b3, origin, block_id, anchor) \
            SELECT gen_random_uuid(), 'fact', f.id, fv.id, fv.b3, fv.b3, 'rule', 'blk_' || repeat('b', 20), '{{}}'::jsonb FROM f, _oo_docs.file_version fv \
             WHERE fv.id = '{}'::uuid",
        planted.version_id, planted.version_id
    ))
    .await;
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.file SET state = 'indexed' WHERE path = '{path}'"
    ))
    .await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'draft'")
            .await,
        1,
        "one draft with live evidence on this version"
    );
    let pool = common::pool_runtime(db, 2).await;
    let h = oo_docs::jobs::stages::PublishHandler::new(
        pool,
        std::sync::Arc::new(oo_docs::cas::Cas::open(&fx.dir).unwrap()),
    );
    assert_eq!(h.promote(None).await.unwrap(), 1, "the file is promoted");
    assert_eq!(
        file_state(&mut fx).await,
        "published",
        "the recording returns to published after the rerun"
    );
    // The same for a new extraction that DOC-G10 holds for a person: its draft facts are listed as `added` in a
    // pending reprocess_diff item, the old extraction stays live, and the file is published (the item waits).
    fx.admin_sql(&format!(
        "WITH f AS (INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, value, status, origin, confidence_bp) \
            SELECT gen_random_uuid(), repeat('c', 64), e.id, e.id, 'owns', '\"y\"'::jsonb, 'draft', 'rule', 9000 FROM _oo_docs.entity e \
             WHERE e.canonical_name = '小王' RETURNING id), \
          ev AS (INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, quote_b3, origin, block_id, anchor) \
            SELECT gen_random_uuid(), 'fact', f.id, fv.id, fv.b3, fv.b3, 'rule', 'blk_' || repeat('c', 20), '{{}}'::jsonb FROM f, _oo_docs.file_version fv \
             WHERE fv.id = '{vid}'::uuid) \
         INSERT INTO _oo_docs.review_item (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, reason_code) \
            SELECT gen_random_uuid(), 'reprocess_diff', 'a8-held-diff', fl.drive_id, fl.id, fv.id, \
                   jsonb_build_object('gate', 'DOC-G10', 'added', jsonb_build_array(repeat('c', 64)), 'removed', '[]'::jsonb), 'diff_over_30_percent' \
              FROM _oo_docs.file_version fv JOIN _oo_docs.file fl ON fl.id = fv.file_id WHERE fv.id = '{vid}'::uuid",
        vid = planted.version_id
    ))
    .await;
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.file SET state = 'indexed' WHERE path = '{path}'"
    ))
    .await;
    assert_eq!(
        h.promote(None).await.unwrap(),
        1,
        "a held reprocess does not keep the file from published"
    );
    assert_eq!(file_state(&mut fx).await, "published");
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND f.status = 'published' AND s.canonical_name = '小王'"
        )
        .await,
        1
    );
    fx.finish(db).await;
}

fn recording(
    seed: &str,
    duration_ms: u64,
    items: serde_json::Value,
) -> oo_docs::normalize::Normalized {
    let resp = json!({
        "protocol": "oo.doc-parser/v1", "parser": {"id": "media", "version": "1", "adapter_version": "1"},
        "kind": "audio", "container": "wav", "codec": "pcm", "duration_ms": duration_ms, "start_ms": 0,
        "speakers": ["S1", "S2"],
        "profile": {"tier": "cpu", "isa": "aarch64:asimd", "models": []},
        "items": items, "warnings": []
    });
    let frag = assemble(DocFormat::Audio, "2".repeat(63) + seed, &resp).unwrap();
    finish(frag).unwrap()
}

/// A recording that is not a conversation has no speakers: nothing is attributed to a voice and nobody is asked.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_promo_has_no_speakers_and_asks_nobody() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_speaker_promo";
    let mut fx = Fx::new(db).await;
    let path = "素材/星火宣传片.wav";
    let promo = recording(
        "b",
        30_000,
        json!([
            {"kind": "utterance", "speaker": "S1", "t_start_ms": 0, "t_end_ms": 14000,
             "text": "星火计划，点亮每一个团队。我负责星火计划的全部体验。", "word_start": 0, "word_end": 20},
            {"kind": "utterance", "speaker": "S1", "t_start_ms": 15000, "t_end_ms": 28000,
             "text": "现在加入，立刻开始。", "word_start": 21, "word_end": 30}
        ]),
    );
    let planted = fx.plant_normalized(path, promo, 1000).await;
    let doc = oo_docs::publish::write::DocumentInput {
        version_id: &planted.version_id,
        b3: &planted.b3,
        path,
        ir: &planted.ir,
    };
    oo_docs::publish::write::index_document(&mut fx.rt, &doc)
        .await
        .unwrap();
    let added =
        oo_docs::review::speaker::propose(&mut fx.rt, &planted.version_id, &planted.ir, path, None)
            .await
            .unwrap();
    assert_eq!(
        added, 0,
        "no speaker item for a recording that is not a conversation"
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'speaker_identity'")
            .await,
        0
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.media_profile WHERE class = 'non_conversation' AND kind = 'promo'"
        )
        .await,
        1
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE canonical_name LIKE '《%》的说话人 %'"
        )
        .await,
        0,
        "no placeholder person is made"
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE predicate = 'owns'")
            .await,
        0,
        "no sentence is attributed to a voice"
    );
    // A media asset is one typed object: a Recording with its class and duration, and no other fact from its sound.
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity WHERE type = 'Recording'")
            .await,
        1
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact WHERE predicate NOT LIKE 'attr.%' AND predicate <> 'mentions'"
        )
        .await,
        0,
        "nothing but properties and the document's own mentions"
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity WHERE type = 'Meeting'")
            .await,
        0
    );
    fx.finish(db).await;
}

struct Says(&'static str);

impl oo_docs::extract::llm::egress::LlmPort for Says {
    fn model(&self) -> &str {
        "stub"
    }
    fn complete<'a>(
        &'a self,
        _req: &'a oo_docs::extract::llm::prompt::ChatRequest,
    ) -> oo_docs::extract::llm::egress::CallFuture<'a> {
        Box::pin(async move {
            Ok(oo_docs::extract::llm::egress::ChatReply {
                content: self.0.to_owned(),
                tokens_in: 1,
                tokens_out: 1,
            })
        })
    }
}

/// The rules cannot place two voices that barely answer each other; the model reads the text. Its verdict is stored
/// (a reprocess keeps it) and a conversation is then extracted again with its speakers.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn an_undecided_recording_is_settled_by_the_model_once() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_speaker_unsure";
    let mut fx = Fx::new(db).await;
    let path = "素材/录音三.wav";
    let rec = recording(
        "c",
        100_000,
        json!([
            {"kind": "utterance", "speaker": "S1", "t_start_ms": 0, "t_end_ms": 8000,
             "text": "我是小王，我负责星火计划。", "word_start": 0, "word_end": 8},
            {"kind": "utterance", "speaker": "S2", "t_start_ms": 8500, "t_end_ms": 16000,
             "text": "好的，请继续。", "word_start": 9, "word_end": 12},
            {"kind": "utterance", "speaker": "S1", "t_start_ms": 16500, "t_end_ms": 18000,
             "text": "谢谢。", "word_start": 13, "word_end": 14}
        ]),
    );
    let planted = fx.plant_normalized(path, rec, 1000).await;
    let doc = oo_docs::publish::write::DocumentInput {
        version_id: &planted.version_id,
        b3: &planted.b3,
        path,
        ir: &planted.ir,
    };
    oo_docs::publish::write::index_document(&mut fx.rt, &doc)
        .await
        .unwrap();
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.media_profile WHERE class = 'unsure' AND source = 'rules'")
            .await,
        1
    );
    assert_eq!(
        oo_docs::review::speaker::propose(&mut fx.rt, &planted.version_id, &planted.ir, path, None)
            .await
            .unwrap(),
        0,
        "undecided: nobody is asked"
    );
    // A model that answers with anything but one clear word leaves it undecided.
    let vague = oo_docs::publish::media_profile::ask_model(
        &mut fx.rt,
        &Says("maybe both"),
        &planted.ir,
        "录音三",
    )
    .await
    .unwrap();
    assert_eq!(vague, None);
    let said = oo_docs::publish::media_profile::ask_model(
        &mut fx.rt,
        &Says("conversation"),
        &planted.ir,
        "录音三",
    )
    .await
    .unwrap();
    assert_eq!(
        said,
        Some(oo_docs::extract::rules::media_class::MediaClass::Conversation)
    );
    oo_docs::publish::write::index_document(&mut fx.rt, &doc)
        .await
        .unwrap();
    assert_eq!(
        oo_docs::review::speaker::propose(&mut fx.rt, &planted.version_id, &planted.ir, path, None)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND s.canonical_name LIKE '《%》的说话人 A · %'"
        )
        .await,
        1
    );
    // The stored verdict survives the rules running again, and the model is not asked twice.
    oo_docs::publish::write::index_document(&mut fx.rt, &doc)
        .await
        .unwrap();
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.media_profile WHERE class = 'conversation' AND source = 'model'")
            .await,
        1
    );
    let again = oo_docs::publish::media_profile::ask_model(
        &mut fx.rt,
        &Says("other"),
        &planted.ir,
        "录音三",
    )
    .await
    .unwrap();
    assert_eq!(again, None);
    fx.finish(db).await;
}

/// "Not anyone known": the item closes and the speaker stays the placeholder, its facts stay published.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn not_anyone_known_keeps_the_placeholder_and_its_facts() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_speaker_none";
    let mut fx = Fx::new(db).await;
    let path = "会议/周会 参会=小王、小李.m4a";
    let planted = fx.plant_normalized(path, meeting("d"), 1000).await;
    let doc = oo_docs::publish::write::DocumentInput {
        version_id: &planted.version_id,
        b3: &planted.b3,
        path,
        ir: &planted.ir,
    };
    oo_docs::publish::write::index_document(&mut fx.rt, &doc)
        .await
        .unwrap();
    oo_docs::review::speaker::propose(&mut fx.rt, &planted.version_id, &planted.ir, path, None)
        .await
        .unwrap();
    let (id, revision): (String, i64) = sqlx::query_as(
        "SELECT id::\"text\", revision FROM _oo_docs.review_item WHERE kind = 'speaker_identity' AND subject->>'speaker' = 'S1'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[ItemDecision {
            id,
            revision,
            decision: Decision::Reject,
            edit: None,
            note: None,
        }],
    )
    .await
    .unwrap();
    assert_eq!(out.results[0].status, "ok");
    let (PublishOutcome::Published(_) | PublishOutcome::NoChange { .. }) = fx.publish().await
    else {
        panic!("publish")
    };
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND f.status = 'published' AND s.canonical_name LIKE '《%》的说话人 A · %'"
        )
        .await,
        1
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'speaker_map'")
            .await,
        0
    );
    fx.finish(db).await;
}

const A8_PATH: &str = "会议/周会 参会=小王、小李.m4a";

async fn file_state(fx: &mut Fx) -> String {
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
        "SELECT state FROM _oo_docs.file WHERE path = '{A8_PATH}'"
    )))
    .fetch_one(&mut fx.admin)
    .await
    .unwrap()
}

async fn held(fx: &mut Fx) -> i64 {
    fx.scalar(
        "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
          WHERE f.predicate = 'owns' AND f.status = 'draft' AND s.canonical_name LIKE '《%》的说话人 A · %'",
    )
    .await
}
