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
    // The embeddings and voice features have no place in the database (D63): no such column or table.
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM information_schema.columns WHERE table_schema = '_oo_docs' \
              AND (column_name ILIKE '%voiceprint%' OR column_name ILIKE '%embedding%')"
        )
        .await,
        0
    );
    fx.finish(db).await;
}
