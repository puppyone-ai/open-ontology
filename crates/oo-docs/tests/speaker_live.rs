//! Speaker identity against one throwaway PostgreSQL (B-S10-08, 10 17.4, D74): facts from a speaker nobody has
//! confirmed do not publish, a `speaker_identity` item per label is queued, and confirming merges the
//! placeholder into the named Person so the facts publish with the right subject.
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
        "kind": "audio", "container": "wav", "codec": "pcm", "duration_ms": 60_000, "start_ms": 0,
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
async fn an_unconfirmed_speaker_publishes_nothing_and_a_confirmation_fixes_the_subject() {
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

    // The statement "我负责星火计划" is a draft owned by the placeholder and is not published.
    let (PublishOutcome::Published(_) | PublishOutcome::NoChange { .. }) = fx.publish().await
    else {
        panic!("publish")
    };
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND s.canonical_name LIKE '说话人 S1%' AND f.status = 'draft'"
        )
        .await,
        1
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact WHERE predicate = 'owns' AND status = 'published'"
        )
        .await,
        0,
        "nothing from an unconfirmed speaker is published"
    );
    // The Document does not "mention" the placeholder either.
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity o ON o.id = f.object_id \
              WHERE f.predicate = 'mentions' AND o.canonical_name LIKE '说话人 %'"
        )
        .await,
        0
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
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("the confirmed speaker's fact publishes")
    };
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND f.status = 'published' AND s.canonical_name = '小王' \
                AND s.type = 'Person'"
        )
        .await,
        1,
        "the subject is the confirmed Person"
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

/// A8 (L6): after a parser bump the recording is extracted again. The speaker a person already confirmed keeps
/// the Person (the new placeholder is merged at once) and a recording whose only drafts are statements of
/// speakers nobody has confirmed still returns to `published` (those wait in the review queue, they do not hold
/// the file back).
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
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("confirmed publish")
    };
    assert_eq!(file_state(&mut fx).await, "published");

    // The parser is bumped and the recording is extracted again. A new placeholder row for the confirmed label S1
    // appears (the first one was merged away): it is merged into 小王 at once, nothing of S1 is held.
    let ph = oo_docs::extract::rules::media::placeholder_name(&planted.b3, "S1");
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
    let merged = oo_docs::review::speaker::apply_confirmed(&mut fx.rt, &planted.ir)
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
    assert_eq!(held(&mut fx).await, 0, "no statement of S1 is held");
    // And once more: nothing left to merge.
    assert_eq!(
        oo_docs::review::speaker::apply_confirmed(&mut fx.rt, &planted.ir)
            .await
            .unwrap(),
        0
    );

    // What the rerun leaves is a draft statement on a speaker's placeholder row (here the merged row of S1, the
    // row of an unconfirmed speaker works the same). It waits for a person in the review queue; it must not keep
    // the recording from `published`, which is what happened to the meeting wav in the compose run.
    fx.admin_sql(&format!(
        "WITH f AS (INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, value, status, origin, confidence_bp) \
            SELECT gen_random_uuid(), repeat('a', 64), e.id, e.id, 'owns', '\"x\"'::jsonb, 'draft', 'rule', 9000 FROM _oo_docs.entity e \
             WHERE e.canonical_name = '{ph}' AND e.key <> '{k1}' RETURNING id) \
         INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, quote_b3, origin, block_id, anchor) \
            SELECT gen_random_uuid(), 'fact', f.id, fv.id, fv.b3, fv.b3, 'rule', 'blk_' || repeat('b', 20), '{{}}'::jsonb FROM f, _oo_docs.file_version fv \
             WHERE fv.id = '{}'::uuid",
        planted.version_id
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
        "one held draft with live evidence on this version"
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
          WHERE f.predicate = 'owns' AND f.status = 'draft' AND s.canonical_name LIKE '说话人 S1%'",
    )
    .await
}
