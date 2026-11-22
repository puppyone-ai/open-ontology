//! The class decision with the real model (D74 revision, lane AL): the transcript of each recording, as the media-parser produced it, is
//! shown to the model with `media_class::prompt`; its one-word answer is compared with what the recording is. Needs the model egress on
//! loopback (`tools/model-egress/model_egress.py`, key and endpoint only by file path in its own config).
//!
//! The file name is withheld (the model sees the neutral title "recording"), so only the words decide.
//!
//! `OO_AL_EGRESS_PORT=18317 OO_AL_MODEL=cb/gpt-6.2-sol cargo test -p oo-docs --locked --test media_class_model_live -- --ignored --nocapture`

use oo_core::docir::DocFormat;
use oo_docs::extract::llm::egress::{EgressClient, LlmPort};
use oo_docs::extract::llm::prompt::ChatRequest;
use oo_docs::extract::rules::media_class::{MediaClass, parse_answer, prompt};
use oo_docs::normalize::{finish, media::assemble};
use serde_json::{Value, json};

fn ir(fixture: &str) -> oo_core::docir::DocIr {
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
    finish(assemble(DocFormat::Audio, "4".repeat(64), &resp).unwrap())
        .unwrap()
        .ir
}

#[tokio::test]
#[ignore = "needs the model egress (OO_AL_EGRESS_PORT, OO_AL_MODEL)"]
async fn the_model_tells_meetings_from_promos() {
    let port: u16 = std::env::var("OO_AL_EGRESS_PORT").unwrap().parse().unwrap();
    let model = std::env::var("OO_AL_MODEL").unwrap();
    let client = EgressClient::new(([127, 0, 0, 1], port).into(), model.clone());
    let mut cases: Vec<(String, MediaClass)> = [
        "acc-launch-sync-2027-01-12",
        "acc-budget-call-2027-01-26",
        "acc-supplier-call-2027-01-16",
        "acc-readiness-call-2027-02-02",
    ]
    .iter()
    .map(|f| ((*f).to_owned(), MediaClass::Conversation))
    .collect();
    for n in ["en2", "en2b", "en3", "en3b", "zh2", "zh2b", "zh3", "zh3b"] {
        cases.push((format!("mt-{n}"), MediaClass::Conversation));
    }
    cases.push(("promo-voice".into(), MediaClass::NonConversation));
    cases.push(("promo-over-music".into(), MediaClass::NonConversation));
    let (mut right, mut conv_right, mut conv_n) = (0, 0, 0);
    for (fixture, truth) in &cases {
        let (system, user) = prompt(&ir(fixture), "recording");
        let req = ChatRequest {
            model: client.model().to_owned(),
            system,
            user,
        };
        let answer = client.complete(&req).await.map(|r| r.content);
        let got = answer.as_ref().ok().and_then(|a| parse_answer(a));
        if got == Some(*truth) {
            right += 1;
        }
        if *truth == MediaClass::Conversation {
            conv_n += 1;
            if got == Some(MediaClass::Conversation) {
                conv_right += 1;
            }
        }
        eprintln!("MEASURE {fixture}: truth {truth:?}, model {got:?}");
    }
    eprintln!(
        "MEASURE real model {model}: {right}/{} correct; meetings and calls {conv_right}/{conv_n}",
        cases.len()
    );
    assert!(right * 10 >= cases.len() * 7, "at least 70 percent");
}
