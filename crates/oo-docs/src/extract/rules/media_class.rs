//! What kind of recording is this (D74 revision, 10 17.4): a conversation (a meeting, an interview, a call, a
//! podcast with hosts) or not (a promo or an ad, an edited video, a film, music, a lecture with one speaker,
//! ambient sound). Only conversations get speakers told apart; the others publish their transcript facts with no
//! speaker attribution and never ask anybody who is speaking.
//!
//! Cheap signals first, all read from the parsed recording: how many voices carry the talk, how often the floor
//! changes hands, how much of the file is speech at all (the rest is music, silence or noise), the duration, the
//! scene cuts of a video, and a weak hint from the file name. Only a recording these leave undecided is shown to the
//! model (the transcript text, [`prompt`]); without a model it is treated as not a conversation, because a
//! missing speaker name costs less than a question nobody needed to be asked. Pure and deterministic.

use std::collections::BTreeMap;

use oo_core::docir::{Anchor, Block, BlockKind, DocFormat, DocIr};
use serde::Serialize;
use serde_json::{Value, json};

/// A voice carries the talk when it speaks at least this share of the speech (basis points) ...
const VOICE_SHARE_BP: u64 = 1000;
/// ... and at least this long.
const VOICE_MIN_MS: u64 = 1500;
/// Less speech than this (basis points of the file, or milliseconds) is music, silence or ambient sound.
const SPEECH_RATIO_MIN_BP: u64 = 800;
const SPEECH_MIN_MS: u64 = 3000;
/// A conversation changes speaker at least this often (per minute of talk) and at least this many times.
const TURN_RATE_MIN_X100: u64 = 200;
const TURNS_MIN: u64 = 2;
/// And a third of the file is speech.
const CONVERSATION_SPEECH_BP: u64 = 3000;
/// One voice for this long is a lecture or a talk rather than a clip.
const LECTURE_MS: u64 = 180_000;
/// A video with more cuts than this per minute and more voices than talk is an edited film.
const CUTS_PER_MIN_EDITED: u64 = 4;

/// The verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaClass {
    /// Several people talk to each other.
    Conversation,
    /// A promo, an edited video, a film, music, a lecture, ambient sound: no interview, nobody to name.
    NonConversation,
    /// The signals disagree; the model may look at the text.
    Unsure,
}

impl MediaClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::NonConversation => "non_conversation",
            Self::Unsure => "unsure",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "conversation" => Some(Self::Conversation),
            "non_conversation" => Some(Self::NonConversation),
            "unsure" => Some(Self::Unsure),
            _ => None,
        }
    }

    /// What extraction acts on: only a decided conversation gets speaker attribution.
    pub fn identifies_speakers(self) -> bool {
        self == Self::Conversation
    }
}

/// What the decision looked at (stored with it, shown on request).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Signals {
    pub duration_ms: u64,
    pub speech_ms: u64,
    /// Speech over duration, basis points.
    pub speech_ratio_bp: u64,
    pub utterances: u32,
    /// Distinct labels in the file.
    pub speakers: u32,
    /// Labels that carry the talk ([`VOICE_SHARE_BP`], [`VOICE_MIN_MS`]).
    pub voices: u32,
    /// Share of the loudest voice, basis points of the speech.
    pub top_share_bp: u64,
    /// Changes of voice between neighbouring utterances.
    pub turns: u32,
    /// Turns per minute of talk, times 100.
    pub turn_rate_x100: u64,
    pub scenes: u32,
    pub video: bool,
    /// Different names the speakers give themselves in the text ("我是小王", "I am Priya"): several of them under one
    /// or two voice labels mean the diarization merged people.
    pub text_voices: u32,
    /// `conversation`, `non_conversation` or `none`: what the file name suggests.
    pub name_hint: &'static str,
}

/// The decision, its reason, and how sure it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaProfile {
    pub class: MediaClass,
    /// `conversation`, `lecture`, `voice_memo`, `promo`, `music_or_ambient`, `edited_video`, `undecided`.
    pub kind: &'static str,
    pub confidence_bp: u16,
    pub signals: Signals,
}

impl MediaProfile {
    pub fn signals_json(&self) -> Value {
        serde_json::to_value(&self.signals).unwrap_or_else(|_| json!({}))
    }
}

fn utterance(b: &Block) -> Option<(u64, u64, &str)> {
    match (&b.kind, &b.anchor) {
        (
            BlockKind::Utterance,
            Anchor::Media {
                t_start_ms,
                t_end_ms,
                speaker: Some(s),
                ..
            },
        ) => Some((*t_start_ms, (*t_end_ms).max(*t_start_ms), s.as_str())),
        _ => None,
    }
}

fn name_hint(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&[
        "宣传",
        "广告",
        "片头",
        "片尾",
        "预告",
        "电影",
        "剧集",
        "混剪",
        "剪辑",
        "配乐",
        "歌曲",
        "音乐",
        "背景音",
        "promo",
        "trailer",
        "advert",
        "film",
        "movie",
        "clip",
        "bgm",
        "soundtrack",
        "song",
        "music",
        "ambient",
        "demo reel",
    ]) {
        "non_conversation"
    } else if has(&[
        "讲座", "课程", "授课", "演讲", "lecture", "keynote", "webinar", "course",
    ]) {
        "lecture"
    } else if has(&["备忘", "语音", "memo", "voice note", "voicenote"]) {
        "memo"
    } else if has(&[
        "会议",
        "访谈",
        "采访",
        "通话",
        "电话",
        "对话",
        "例会",
        "周会",
        "晨会",
        "评审",
        "面试",
        "播客",
        "纪要",
        "meeting",
        "interview",
        "call",
        "podcast",
        "standup",
        "sync",
        "review",
        "参会",
    ]) {
        "conversation"
    } else {
        "none"
    }
}

/// The voice labels that carry the talk, in order of first appearance: the ones a person is asked about. A label
/// that speaks for a moment (a clustering stray) is not a voice.
pub fn voices(ir: &DocIr) -> Vec<String> {
    let utts: Vec<(u64, u64, &str)> = ir.blocks.iter().filter_map(utterance).collect();
    let mut by: BTreeMap<&str, u64> = BTreeMap::new();
    for (s, e, spk) in &utts {
        *by.entry(spk).or_default() += e - s;
    }
    let total: u64 = by.values().sum::<u64>().max(1);
    let mut out: Vec<String> = Vec::new();
    for (_, _, spk) in &utts {
        let ms = by[spk];
        if ms * 10_000 / total >= VOICE_SHARE_BP
            && ms >= VOICE_MIN_MS
            && !out.iter().any(|o| o == spk)
        {
            out.push((*spk).to_owned());
        }
    }
    out
}

/// Measure a recording.
pub fn signals(ir: &DocIr, path: &str) -> Signals {
    let mut utts: Vec<(u64, u64, &str)> = ir.blocks.iter().filter_map(utterance).collect();
    utts.sort_by_key(|u| (u.0, u.1));
    let last_end = ir
        .blocks
        .iter()
        .filter_map(|b| match &b.anchor {
            Anchor::Media { t_end_ms, .. } => Some(*t_end_ms),
            Anchor::Frame { t_ms, .. } => Some(*t_ms),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let duration_ms = ir.meta.duration_ms.unwrap_or(last_end).max(last_end).max(1);
    let mut by_speaker: BTreeMap<&str, (u64, u32)> = BTreeMap::new();
    let mut speech_ms = 0u64;
    let mut cursor = 0u64;
    for (s, e, spk) in &utts {
        let from = (*s).max(cursor);
        if *e > from {
            speech_ms += e - from;
            cursor = *e;
        }
        let entry = by_speaker.entry(spk).or_default();
        entry.0 += e - s;
        entry.1 += 1;
    }
    let total: u64 = by_speaker.values().map(|v| v.0).sum::<u64>().max(1);
    let voices: Vec<&str> = by_speaker
        .iter()
        .filter(|(_, (ms, _))| ms * 10_000 / total >= VOICE_SHARE_BP && *ms >= VOICE_MIN_MS)
        .map(|(s, _)| *s)
        .collect();
    let top = by_speaker.values().map(|v| v.0).max().unwrap_or(0);
    // A turn is a change between two voices that carry the talk; a stray label in between does not break it.
    let mut turns = 0u32;
    let mut prev: Option<&str> = None;
    for (_, _, spk) in utts.iter().filter(|u| voices.contains(&u.2)) {
        if let Some(p) = prev
            && p != *spk
        {
            turns += 1;
        }
        prev = Some(spk);
    }
    let span_ms = match (utts.first(), utts.last()) {
        (Some(f), Some(l)) => l.1.saturating_sub(f.0).max(1),
        _ => 1,
    };
    let scenes = ir
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Scene)
        .count();
    Signals {
        duration_ms,
        speech_ms,
        speech_ratio_bp: speech_ms * 10_000 / duration_ms,
        utterances: u32::try_from(utts.len()).unwrap_or(u32::MAX),
        speakers: u32::try_from(by_speaker.len()).unwrap_or(u32::MAX),
        voices: u32::try_from(voices.len()).unwrap_or(u32::MAX),
        top_share_bp: top * 10_000 / total,
        turns,
        turn_rate_x100: u64::from(turns) * 6_000_000 / span_ms,
        scenes: u32::try_from(scenes).unwrap_or(u32::MAX),
        video: ir.format == DocFormat::Video,
        text_voices: u32::try_from(
            super::media::introductions(ir)
                .iter()
                .filter(|c| c.source == super::media::CandidateSource::SelfIntro)
                .map(|c| c.name.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
        )
        .unwrap_or(u32::MAX),
        name_hint: name_hint(path),
    }
}

/// Decide from the signals. The order matters: nothing to say first, then one voice, then the floor changing hands.
pub fn decide(s: &Signals) -> MediaProfile {
    let mk = |class, kind, confidence_bp| MediaProfile {
        class,
        kind,
        confidence_bp,
        signals: s.clone(),
    };
    use MediaClass::{Conversation, NonConversation, Unsure};
    // Music, silence, ambient sound: next to no speech.
    if s.utterances == 0 || s.speech_ms < SPEECH_MIN_MS || s.speech_ratio_bp < SPEECH_RATIO_MIN_BP {
        return mk(
            NonConversation,
            if s.video && s.scenes > 0 {
                "edited_video"
            } else {
                "music_or_ambient"
            },
            9000,
        );
    }
    // One voice (or one voice and a stray label): a promo read, a talk, a lecture, a narration. Unless the text says
    // several people spoke (they introduce themselves, or the name says call or meeting) and the diarization merged
    // them: then the model reads the text.
    if s.voices <= 1 {
        if s.speech_ms >= 8000
            && (s.text_voices >= 2 || (s.name_hint == "conversation" && s.speech_ms < LECTURE_MS))
        {
            return mk(Unsure, "undecided", 4000);
        }
        // D98: a lecture or a voice memo is transcribed (one speaker); a promo read, a narration over pictures or music
        // is a media asset and nothing is extracted from it.
        let conf = if s.top_share_bp >= 9000 { 9000 } else { 7500 };
        return match s.name_hint {
            "non_conversation" => mk(NonConversation, "promo", conf),
            "lecture" => mk(Conversation, "lecture", conf),
            "memo" => mk(Conversation, "voice_memo", conf),
            _ if s.speech_ms >= LECTURE_MS => mk(Conversation, "lecture", conf),
            _ => mk(NonConversation, "promo", conf),
        };
    }
    let minutes_x100 = s.duration_ms * 100 / 60_000;
    let cuts_per_min = (u64::from(s.scenes) * 100)
        .checked_div(minutes_x100)
        .unwrap_or(0);
    let talking = u64::from(s.turns) >= TURNS_MIN && s.turn_rate_x100 >= TURN_RATE_MIN_X100;
    // A cut-heavy video where the speech is a part of it: a film, a trailer, an edited piece.
    if s.video && cuts_per_min >= CUTS_PER_MIN_EDITED && s.speech_ratio_bp < 6000 {
        return mk(NonConversation, "edited_video", 8000);
    }
    if talking && s.speech_ratio_bp >= CONVERSATION_SPEECH_BP {
        return mk(
            Conversation,
            "conversation",
            if s.turns >= 6 { 9200 } else { 8200 },
        );
    }
    // Voices that hardly answer each other: two narrators, a presenter and a clip.
    if u64::from(s.turns) < 2 && s.speech_ms >= 20_000 {
        return mk(NonConversation, "promo", 7000);
    }
    // The name decides a close call, but only weakly.
    match s.name_hint {
        "conversation" if u64::from(s.turns) >= 2 => mk(Conversation, "conversation", 6500),
        "non_conversation" => mk(NonConversation, "promo", 6500),
        _ => mk(Unsure, "undecided", 4000),
    }
}

/// Classify a recording.
pub fn classify(ir: &DocIr, path: &str) -> MediaProfile {
    decide(&signals(ir, path))
}

/// The system and user message of the model's question for an undecided recording: the first and the last of the
/// transcript, no speaker names, nothing but the text. The answer is one word.
pub fn prompt(ir: &DocIr, title: &str) -> (String, String) {
    const SYSTEM: &str = "You classify a transcript of a recording. Answer with exactly one word: \
`conversation` if several people talk to each other (a meeting, an interview, a call, a podcast with hosts), \
or `other` if it is a promo or advertisement, an edited or produced video, a film, a lecture or talk by one \
speaker, a narration, or anything that is not people talking to each other. No other text.";
    let mut text = String::new();
    for b in ir.blocks.iter().filter(|b| b.kind == BlockKind::Utterance) {
        if text.chars().count() > 2400 {
            break;
        }
        text.push_str(b.text.trim());
        text.push('\n');
    }
    let text: String = text.chars().take(2400).collect();
    (
        SYSTEM.to_owned(),
        format!("File name: {title}\n\nTranscript:\n{text}"),
    )
}

/// The model's one-word answer. Anything else leaves the recording undecided.
pub fn parse_answer(reply: &str) -> Option<MediaClass> {
    let w = reply
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    match w.as_str() {
        "conversation" => Some(MediaClass::Conversation),
        "other" => Some(MediaClass::NonConversation),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_core::docir::{DocMeta, ParserId, ParserInfo};

    fn utt(ord: u32, speaker: &str, start: u64, end: u64) -> Block {
        Block {
            id: format!("blk_{ord}"),
            ordinal: ord,
            kind: BlockKind::Utterance,
            level: None,
            text: "x".into(),
            hpath: vec![],
            parent: None,
            anchor: Anchor::Media {
                t_start_ms: start,
                t_end_ms: end,
                speaker: Some(speaker.into()),
                word_start: None,
                word_end: None,
            },
            table: None,
            flags: vec![],
        }
    }

    fn scene(ord: u32, start: u64, end: u64) -> Block {
        let mut b = utt(ord, "S1", start, end);
        b.kind = BlockKind::Scene;
        b.anchor = Anchor::Media {
            t_start_ms: start,
            t_end_ms: end,
            speaker: None,
            word_start: None,
            word_end: None,
        };
        b
    }

    fn ir(format: DocFormat, duration_ms: u64, blocks: Vec<Block>) -> DocIr {
        DocIr {
            protocol: "oo.doc-ir/v1.1".into(),
            blob_b3: "0".repeat(64),
            format,
            parser: ParserInfo {
                id: ParserId::Media,
                version: "1".into(),
                adapter_version: None,
                profile_sha256: "0".repeat(64),
                profile: None,
            },
            meta: DocMeta {
                duration_ms: Some(duration_ms),
                ..DocMeta::default()
            },
            blocks,
            warnings: vec![],
        }
    }

    fn talk(pairs: u32, secs: u64) -> Vec<Block> {
        (0..pairs * 2)
            .map(|i| {
                let t = u64::from(i) * secs * 1000;
                utt(
                    i,
                    if i % 2 == 0 { "S1" } else { "S2" },
                    t,
                    t + secs * 1000 - 500,
                )
            })
            .collect()
    }

    #[test]
    fn two_voices_taking_turns_are_a_conversation() {
        let p = classify(&ir(DocFormat::Audio, 40_000, talk(4, 5)), "a.m4a");
        assert_eq!(p.class, MediaClass::Conversation, "{p:?}");
        assert!(p.class.identifies_speakers());
    }

    #[test]
    fn one_voice_is_a_promo_unless_it_is_a_lecture_or_a_memo() {
        let p = classify(
            &ir(DocFormat::Audio, 30_000, vec![utt(0, "S1", 0, 28_000)]),
            "a.m4a",
        );
        assert_eq!((p.class, p.kind), (MediaClass::NonConversation, "promo"));
        let long = classify(
            &ir(DocFormat::Audio, 600_000, vec![utt(0, "S1", 0, 590_000)]),
            "课程.m4a",
        );
        assert_eq!(
            (long.class, long.kind),
            (MediaClass::Conversation, "lecture")
        );
        let memo = classify(
            &ir(DocFormat::Audio, 40_000, vec![utt(0, "S1", 0, 38_000)]),
            "语音备忘 0412.m4a",
        );
        assert_eq!(
            (memo.class, memo.kind),
            (MediaClass::Conversation, "voice_memo")
        );
        let ad = classify(
            &ir(DocFormat::Audio, 600_000, vec![utt(0, "S1", 0, 590_000)]),
            "宣传片.m4a",
        );
        assert_eq!((ad.class, ad.kind), (MediaClass::NonConversation, "promo"));
    }

    #[test]
    fn a_stray_label_does_not_make_a_second_voice() {
        // A promo read by one voice that the clustering split once for a second.
        let mut b = vec![
            utt(0, "S1", 0, 20_000),
            utt(1, "S2", 20_000, 20_800),
            utt(2, "S1", 21_000, 30_000),
        ];
        b.push(utt(3, "S1", 30_500, 38_000));
        let p = classify(&ir(DocFormat::Audio, 40_000, b), "宣传.wav");
        assert_eq!(p.class, MediaClass::NonConversation, "{p:?}");
        assert_eq!(p.signals.voices, 1);
    }

    #[test]
    fn music_and_silence_have_nothing_to_identify() {
        let p = classify(&ir(DocFormat::Audio, 120_000, vec![]), "bgm.m4a");
        assert_eq!(
            (p.class, p.kind),
            (MediaClass::NonConversation, "music_or_ambient")
        );
        // A few words over two minutes of music.
        let p = classify(
            &ir(DocFormat::Audio, 120_000, vec![utt(0, "S1", 5_000, 7_000)]),
            "song.m4a",
        );
        assert_eq!(p.kind, "music_or_ambient");
    }

    #[test]
    fn a_cut_heavy_video_with_voices_is_an_edited_film() {
        let mut b = talk(3, 4);
        for i in 0..12 {
            b.push(scene(
                100 + i,
                u64::from(i) * 5_000,
                u64::from(i) * 5_000 + 5_000,
            ));
        }
        let p = classify(&ir(DocFormat::Video, 60_000, b), "片段.mp4");
        assert_eq!(
            (p.class, p.kind),
            (MediaClass::NonConversation, "edited_video"),
            "{p:?}"
        );
    }

    #[test]
    fn voices_that_hardly_answer_each_other_are_not_a_conversation() {
        let b = vec![utt(0, "S1", 0, 30_000), utt(1, "S2", 31_000, 60_000)];
        let p = classify(&ir(DocFormat::Audio, 65_000, b), "x.m4a");
        assert_eq!(p.class, MediaClass::NonConversation, "{p:?}");
    }

    #[test]
    fn a_close_call_goes_to_the_name_and_then_to_the_model() {
        let b = vec![
            utt(0, "S1", 0, 8_000),
            utt(1, "S2", 8_500, 16_000),
            utt(2, "S1", 16_500, 18_000),
        ];
        let near = |name: &str| classify(&ir(DocFormat::Audio, 100_000, b.clone()), name);
        assert_eq!(near("周会.m4a").class, MediaClass::Conversation);
        assert_eq!(near("trailer.m4a").class, MediaClass::NonConversation);
        assert_eq!(near("x.m4a").class, MediaClass::Unsure);
    }

    #[test]
    fn the_answer_of_the_model_is_one_word() {
        assert_eq!(
            parse_answer(" Conversation. "),
            Some(MediaClass::Conversation)
        );
        assert_eq!(parse_answer("`other`"), Some(MediaClass::NonConversation));
        assert_eq!(parse_answer("it is a podcast"), None);
        let (sys, user) = prompt(&ir(DocFormat::Audio, 1000, talk(1, 2)), "t");
        assert!(sys.contains("one word") && user.contains("Transcript"));
    }
}
