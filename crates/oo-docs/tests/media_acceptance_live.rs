//! The four audio files of the acceptance corpus through the engine (D74 revision, lane AL): what class each gets, how many
//! speaker items are asked before (every label, the old rule) and after, which entities are published before anybody is named,
//! and what a name does. The input is the media-parser's real output for the files (`tests/fixtures/media_class/acc-*.json`);
//! the model that settles an undecided recording is a stand-in that answers `conversation` (the real model is not called here).
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test media_acceptance_live -- --ignored --test-threads=1 --nocapture`

mod common;

use common::CLUSTER_LOCK;
use common::fx::Fx;
use oo_core::docir::DocFormat;
use oo_docs::normalize::{finish, media::assemble};
use oo_docs::review::decide::{Decision, ItemDecision, decide_items};
use serde_json::{Value, json};

const FILES: [(&str, &str); 4] = [
    (
        "acc-launch-sync-2027-01-12",
        "meetings/audio/launch-sync-2027-01-12.wav",
    ),
    (
        "acc-budget-call-2027-01-26",
        "meetings/audio/budget-call-2027-01-26.m4a",
    ),
    (
        "acc-supplier-call-2027-01-16",
        "meetings/audio/supplier-call-2027-01-16.wav",
    ),
    (
        "acc-readiness-call-2027-02-02",
        "meetings/audio/readiness-call-2027-02-02.m4a",
    ),
];

struct Says;
impl oo_docs::extract::llm::egress::LlmPort for Says {
    fn model(&self) -> &str {
        "stand-in"
    }
    fn complete<'a>(
        &'a self,
        _r: &'a oo_docs::extract::llm::prompt::ChatRequest,
    ) -> oo_docs::extract::llm::egress::CallFuture<'a> {
        Box::pin(async {
            Ok(oo_docs::extract::llm::egress::ChatReply {
                content: "conversation".into(),
                tokens_in: 1,
                tokens_out: 1,
            })
        })
    }
}

fn normalized(fixture: &str, n: usize) -> oo_docs::normalize::Normalized {
    let raw = std::fs::read_to_string(format!(
        "{}/tests/fixtures/media_class/{fixture}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let rec: Value = serde_json::from_str(&raw).unwrap();
    let resp = json!({
        "protocol": "oo.doc-parser/v1", "parser": {"id": "media", "version": "1", "adapter_version": "1"},
        "kind": "audio", "container": "wav", "codec": "pcm", "duration_ms": rec["duration_ms"], "start_ms": 0,
        "speakers": rec["speakers"], "profile": {"tier": "cpu", "isa": "aarch64:asimd", "models": []},
        "items": rec["items"], "warnings": []
    });
    let b3 = format!("{n:x}").repeat(64);
    finish(assemble(DocFormat::Audio, b3, &resp).unwrap()).unwrap()
}

async fn counts(fx: &mut Fx) -> (i64, i64, i64, i64) {
    let published = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
        .await;
    let entities = fx
        .scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE status = 'active' AND type <> 'Document'",
        )
        .await;
    let placeholders = fx
        .scalar("SELECT count(*) FROM _oo_docs.entity WHERE status = 'active' AND canonical_name LIKE '《%》的说话人 %'")
        .await;
    let items = fx
        .scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'speaker_identity' AND status = 'pending'")
        .await;
    (published, entities, placeholders, items)
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_acceptance_audio_by_class_items_and_naming() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_acc_audio";
    let mut fx = Fx::new(db).await;
    let mut planted = Vec::new();
    let mut before = 0;
    for (i, (fixture, path)) in FILES.iter().enumerate() {
        let n = normalized(fixture, i + 1);
        before += n.ir.meta.speakers.len();
        let p = fx.plant_normalized(path, n, 1000).await;
        fx.index_planted(path, &p).await;
        oo_docs::review::speaker::propose(&mut fx.rt, &p.version_id, &p.ir, path, None)
            .await
            .unwrap();
        planted.push((path, p));
    }
    fx.publish().await;
    let classes = fx
        .text("SELECT class || ':' || kind FROM _oo_docs.media_profile ORDER BY b3")
        .await;
    let a = counts(&mut fx).await;
    eprintln!("MEASURE labels(before, old rule: one item per label) = {before}");
    eprintln!(
        "MEASURE rules only: class {classes:?}; published facts/entities/placeholders/items = {a:?}"
    );
    assert_eq!(a.3, 0, "undecided recordings queue nothing");

    for (path, p) in &planted {
        oo_docs::publish::media_profile::ask_model(&mut fx.rt, &Says, &p.ir, path)
            .await
            .unwrap();
        fx.index_planted(path, p).await;
        oo_docs::review::speaker::propose(&mut fx.rt, &p.version_id, &p.ir, path, None)
            .await
            .unwrap();
    }
    fx.publish().await;
    let classes = fx
        .text("SELECT class || ':' || source FROM _oo_docs.media_profile ORDER BY b3")
        .await;
    let b = counts(&mut fx).await;
    eprintln!(
        "MEASURE after the model (stand-in) says conversation: class {classes:?}; published facts/entities/placeholders/items = {b:?}"
    );
    assert_eq!(
        b.3 as usize, 4,
        "one item per voice, none for the stray label"
    );

    let items: Vec<(String, i64, String)> = sqlx::query_as(
        "SELECT id::text, revision, COALESCE(subject->'candidates'->0->>'name', 'Test Person ' || (subject->>'speaker')) \
           FROM _oo_docs.review_item WHERE kind = 'speaker_identity' AND status = 'pending' ORDER BY id",
    )
    .fetch_all(&mut fx.admin)
    .await
    .unwrap();
    let decisions: Vec<ItemDecision> = items
        .into_iter()
        .map(|(id, revision, name)| ItemDecision {
            id,
            revision,
            decision: Decision::Accept,
            edit: Some(json!({ "name": name })),
            note: None,
        })
        .collect();
    let out = decide_items(&mut fx.rt, "tester", &decisions)
        .await
        .unwrap();
    assert!(
        out.results.iter().all(|r| r.status == "ok"),
        "{:?}",
        out.results
    );
    fx.publish().await;
    let c = counts(&mut fx).await;
    eprintln!(
        "MEASURE after naming through the decide API: published facts/entities/placeholders/items = {c:?}"
    );
    assert_eq!(c.3, 0);
    fx.finish(db).await;
}

/// The eight synthetic meetings (real pipeline output, default clustering): class, what is published before anybody is named
/// (Recording, Meeting, placeholder participants and what they state), and what naming every speaker does.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_synthetic_meetings_publish_before_naming_and_move_to_the_named_person() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_mt_audio";
    let mut fx = Fx::new(db).await;
    for (i, n) in ["en2", "en2b", "en3", "en3b", "zh2", "zh2b", "zh3", "zh3b"]
        .iter()
        .enumerate()
    {
        let path = format!("会议/meeting-{n}.wav");
        let p = fx
            .plant_normalized(&path, normalized(&format!("mt-{n}"), i + 1), 1000)
            .await;
        fx.index_planted(&path, &p).await;
        oo_docs::review::speaker::propose(&mut fx.rt, &p.version_id, &p.ir, &path, None)
            .await
            .unwrap();
    }
    fx.publish().await;
    let recordings = fx
        .scalar("SELECT count(*) FROM _oo_docs.entity WHERE type = 'Recording'")
        .await;
    let meetings = fx
        .scalar("SELECT count(*) FROM _oo_docs.entity WHERE type = 'Meeting'")
        .await;
    let classes = fx
        .text("SELECT class FROM _oo_docs.media_profile ORDER BY b3")
        .await;
    let before = counts(&mut fx).await;
    let participants = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE predicate = 'has_participant' AND status = 'published'")
        .await;
    eprintln!("MEASURE meetings: recordings={recordings} meetings={meetings} classes={classes:?}");
    eprintln!(
        "MEASURE meetings before naming: published facts/entities/placeholders/items = {before:?}; published has_participant = {participants}"
    );
    let items: Vec<(String, i64, String)> = sqlx::query_as(
        "SELECT id::text, revision, 'Person ' || md5(id::text) FROM _oo_docs.review_item \
          WHERE kind = 'speaker_identity' AND status = 'pending' ORDER BY id",
    )
    .fetch_all(&mut fx.admin)
    .await
    .unwrap();
    let n_items = items.len();
    let decisions: Vec<ItemDecision> = items
        .into_iter()
        .map(|(id, revision, name)| ItemDecision {
            id,
            revision,
            decision: Decision::Accept,
            edit: Some(json!({ "name": &name[..14] })),
            note: None,
        })
        .collect();
    let out = decide_items(&mut fx.rt, "tester", &decisions)
        .await
        .unwrap();
    assert!(out.results.iter().all(|r| r.status == "ok"));
    fx.publish().await;
    let after = counts(&mut fx).await;
    let moved = fx
        .scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity p ON p.id = f.object_id \
              WHERE f.predicate = 'has_participant' AND f.status = 'published' AND p.canonical_name LIKE 'Person %'",
        )
        .await;
    eprintln!(
        "MEASURE meetings after naming {n_items} items: published facts/entities/placeholders/items = {after:?}; participants now on named Persons = {moved}"
    );
    assert_eq!(
        after.2, 0,
        "no placeholder is left once every speaker is named"
    );
    assert_eq!(moved, participants);
    fx.finish(db).await;
}
