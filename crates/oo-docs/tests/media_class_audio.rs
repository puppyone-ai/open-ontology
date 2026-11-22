//! The class of a recording on real pipeline output (D74 revision, lane AL): the files of
//! `tools/super-ontology/media_class/gen_audio.py` (macOS `say` voices, a synthesised chord bed) and the four audio
//! files of the acceptance corpus went through the media-parser's own VAD, ASR and speaker clustering
//! (`run_pipeline.py`); `tests/fixtures/media_class/*.json` is what came out. The meetings were clustered with the
//! two-speaker hint the engine sends when the metadata names the attendees; the promos and the acceptance files
//! with the default clustering, which merges synthetic voices that sound alike (so most acceptance calls arrive as
//! one label and the rules ask the model instead of deciding).

use oo_core::docir::DocFormat;
use oo_docs::extract::rules::media_class::{MediaClass, MediaProfile, classify};
use oo_docs::normalize::{finish, media::assemble};
use serde_json::{Value, json};

fn profile(fixture: &str, path: &str) -> MediaProfile {
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
    let frag = assemble(DocFormat::Audio, "3".repeat(64), &resp).unwrap();
    classify(&finish(frag).unwrap().ir, path)
}

#[test]
fn a_two_speaker_meeting_is_a_conversation_in_both_languages() {
    for (f, path) in [("meeting-zh", "周会.m4a"), ("meeting-en", "weekly.m4a")] {
        let p = profile(f, path);
        assert_eq!(p.class, MediaClass::Conversation, "{f}: {p:?}");
        assert_eq!(p.signals.voices, 2, "{f}");
        assert!(p.signals.turns >= 4, "{f}: {:?}", p.signals);
    }
}

#[test]
fn music_alone_has_nobody_to_identify() {
    let p = profile("music-only", "bgm.m4a");
    assert_eq!(
        (p.class, p.kind),
        (MediaClass::NonConversation, "music_or_ambient"),
        "{p:?}"
    );
    assert_eq!(p.signals.utterances, 0);
}

#[test]
fn a_promo_read_by_one_voice_is_not_a_conversation_with_or_without_music() {
    for f in ["promo-voice", "promo-over-music"] {
        let p = profile(f, "素材/星火宣传.wav");
        assert_eq!(
            (p.class, p.kind),
            (MediaClass::NonConversation, "promo"),
            "{f}: {p:?}"
        );
        assert_eq!(p.signals.voices, 1);
    }
    // The music under the voice lowers the share of speech; the class does not move.
    assert!(
        profile("promo-over-music", "x.wav").signals.speech_ratio_bp
            < profile("promo-voice", "x.wav").signals.speech_ratio_bp
    );
}

#[test]
fn a_dialogue_that_ends_in_music_is_still_a_conversation() {
    let p = profile("dialogue-then-music", "sync.wav");
    assert_eq!(p.class, MediaClass::Conversation, "{p:?}");
    assert!(p.signals.speech_ratio_bp < 6000, "half of it is music");
}

#[test]
fn acceptance_calls_with_a_merged_voice_go_to_the_model_not_to_a_guess() {
    // The diarization gave these three one label each; the names say call and sync and the text has two people.
    for f in [
        "acc-launch-sync-2027-01-12",
        "acc-readiness-call-2027-02-02",
        "acc-supplier-call-2027-01-16",
    ] {
        let p = profile(f, &format!("meetings/audio/{}.wav", &f[4..]));
        assert_eq!(p.class, MediaClass::Unsure, "{f}: {p:?}");
        assert!(!p.class.identifies_speakers());
    }
}

#[test]
fn a_call_with_two_voices_is_a_conversation_and_its_stray_label_is_not_a_voice() {
    let name = "meetings/audio/budget-call-2027-01-26.wav";
    let p = profile("acc-budget-call-2027-01-26", name);
    // One voice holds the floor and the second label speaks for a second: the name and the two self introductions
    // leave it to the model.
    assert_eq!(p.signals.speakers, 2);
    assert_eq!(p.signals.voices, 1, "{:?}", p.signals);
    assert_eq!(p.class, MediaClass::Unsure, "{p:?}");
}
