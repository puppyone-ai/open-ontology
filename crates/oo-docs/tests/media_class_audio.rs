//! The class of a recording on real pipeline output (D74 revision, lane AL): the files of
//! `tools/super-ontology/media_class/gen_audio.py` (macOS `say` voices, a synthesised chord bed) and the four audio
//! files of the acceptance corpus went through the media-parser's own VAD, ASR and speaker clustering
//! (`run_pipeline.py`); `tests/fixtures/media_class/*.json` is what came out. The meetings were clustered with the
//! two-speaker hint the engine sends when the metadata names the attendees; the promos and the acceptance files
//! with the default clustering (centred spectral count, Whisper for non-Chinese chunks).

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
fn the_acceptance_calls_are_conversations_now_that_the_voices_are_told_apart() {
    // With the centred spectral clustering every call has two or more voices taking turns, and English is transcribed by Whisper.
    for f in [
        "acc-launch-sync-2027-01-12",
        "acc-budget-call-2027-01-26",
        "acc-supplier-call-2027-01-16",
        "acc-readiness-call-2027-02-02",
    ] {
        let p = profile(f, &format!("meetings/audio/{}.wav", &f[4..]));
        eprintln!(
            "MEASURE {f}: {:?} voices={} turns={}",
            p.class, p.signals.voices, p.signals.turns
        );
        assert_eq!(p.class, MediaClass::Conversation, "{f}: {p:?}");
    }
}

/// D98 re-validation: eight synthetic meetings (macOS voices with their own rate and pitch; two and three speakers; Chinese and
/// English) through the media-parser's own clustering with its default settings. The speaker count and the DER come from
/// `tools/super-ontology/media_class/der.py` (numbers in the lane report); here the class of each is measured with and without a
/// word in the file name that says meeting.
#[test]
fn synthetic_meetings_are_conversations_when_the_clustering_finds_two_voices() {
    let names = ["en2", "en2b", "en3", "en3b", "zh2", "zh2b", "zh3", "zh3b"];
    let (mut bare, mut named) = (0, 0);
    for n in names {
        let fixture = format!("mt-{n}");
        if profile(&fixture, &format!("{n}.wav")).class == MediaClass::Conversation {
            bare += 1;
        }
        let p = profile(&fixture, &format!("meeting-{n}.wav"));
        if p.class.identifies_speakers() {
            named += 1;
        }
        eprintln!(
            "MEASURE {n}: {:?} {} voices={} turns={}",
            p.class, p.kind, p.signals.voices, p.signals.turns
        );
    }
    eprintln!(
        "MEASURE class accuracy on 8 meetings: {bare}/8 from the sound alone, {named}/8 with 'meeting' in the file name"
    );
    assert!(bare >= 7 && named >= 7);
}
