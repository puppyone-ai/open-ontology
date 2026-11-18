//! `media-parser` output to `oo.doc-ir/v1.1` for audio and video (10 §17.3 to §17.5).
//!
//! The sidecar returns `items` already ordered by the file's time axis (milliseconds, container start offset
//! included): `utterance` (speaker, word range, confidence), `scene` (start, end) and `frame_text` (pts, bbox).
//! Blocks keep that order; ordinals come from `finish`. `hpath` is empty (spec §17.3). Speaker labels are
//! file-local `S1`, `S2`; nothing but the label reaches the IR (D63, D74).

use oo_core::docir::{
    Anchor, BlockFlag, BlockKind, ComputeTier, DocFormat, DocMeta, ParserId, ParserInfo,
    ParserModel, ParserProfile, is_speaker_label,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{IrFragment, IrWarning, RawBlock};

const PROFILE: &str =
    "media/1;vad=silero;chunk=30s;align=300s;diarization=cluster;frames=1fps;dedupe=dhash16;pts=ms";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    Shape(&'static str),
    Format,
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(w) => write!(f, "media output: {w}"),
            Self::Format => f.write_str("format is not audio or video"),
        }
    }
}

impl std::error::Error for MediaError {}

fn profile_sha256() -> String {
    Sha256::digest(PROFILE.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(Value::as_u64)
}

fn bbox(v: &Value) -> Option<[i64; 4]> {
    let a = v.get("bbox")?.as_array()?;
    (a.len() == 4).then(|| {
        let g = |i: usize| a[i].as_i64().unwrap_or(0);
        [g(0), g(1), g(2), g(3)]
    })
}

pub fn assemble(
    format: DocFormat,
    blob_b3: String,
    resp: &Value,
) -> Result<IrFragment, MediaError> {
    if !matches!(format, DocFormat::Audio | DocFormat::Video) {
        return Err(MediaError::Format);
    }
    let parser_v = resp.get("parser").unwrap_or(&Value::Null);
    let profile = resp.get("profile");
    let models: Vec<ParserModel> = profile
        .and_then(|p| p.get("models"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    Some(ParserModel {
                        name: m.get("name")?.as_str()?.chars().take(128).collect(),
                        sha256: m.get("sha256")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let parser = ParserInfo {
        id: ParserId::Media,
        version: parser_v
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .chars()
            .take(64)
            .collect(),
        adapter_version: parser_v
            .get("adapter_version")
            .and_then(Value::as_str)
            .map(str::to_owned),
        profile_sha256: profile_sha256(),
        profile: Some(ParserProfile {
            tier: ComputeTier::Cpu,
            models,
            isa: profile
                .and_then(|p| p.get("isa"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        }),
    };
    let items = resp
        .get("items")
        .and_then(Value::as_array)
        .ok_or(MediaError::Shape("items"))?;
    let mut blocks = Vec::with_capacity(items.len());
    for it in items {
        let kind = it.get("kind").and_then(Value::as_str).unwrap_or("");
        let text = it.get("text").and_then(Value::as_str).unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        let (kind, anchor, flags) = match kind {
            "utterance" => {
                let speaker = it.get("speaker").and_then(Value::as_str).unwrap_or("");
                if !is_speaker_label(speaker) {
                    return Err(MediaError::Shape("speaker"));
                }
                let (s, e) = (
                    u(it, "t_start_ms").ok_or(MediaError::Shape("t_start_ms"))?,
                    u(it, "t_end_ms").ok_or(MediaError::Shape("t_end_ms"))?,
                );
                let low = it
                    .get("low_text_confidence")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                (
                    BlockKind::Utterance,
                    Anchor::Media {
                        t_start_ms: s,
                        t_end_ms: e.max(s),
                        speaker: Some(speaker.to_owned()),
                        word_start: u(it, "word_start").map(|v| v as u32),
                        word_end: u(it, "word_end").map(|v| v as u32),
                    },
                    if low {
                        vec![BlockFlag::LowTextConfidence]
                    } else {
                        Vec::new()
                    },
                )
            }
            "scene" => {
                let (s, e) = (
                    u(it, "t_start_ms").ok_or(MediaError::Shape("t_start_ms"))?,
                    u(it, "t_end_ms").ok_or(MediaError::Shape("t_end_ms"))?,
                );
                (
                    BlockKind::Scene,
                    Anchor::Media {
                        t_start_ms: s,
                        t_end_ms: e.max(s),
                        speaker: None,
                        word_start: None,
                        word_end: None,
                    },
                    Vec::new(),
                )
            }
            "frame_text" => {
                let low = it.get("confidence").and_then(Value::as_f64).unwrap_or(1.0) < 0.8;
                (
                    BlockKind::FrameText,
                    Anchor::Frame {
                        t_ms: u(it, "t_ms").ok_or(MediaError::Shape("t_ms"))?,
                        bbox: bbox(it),
                    },
                    if low {
                        vec![BlockFlag::LowTextConfidence]
                    } else {
                        Vec::new()
                    },
                )
            }
            _ => return Err(MediaError::Shape("item kind")),
        };
        blocks.push(RawBlock {
            kind,
            level: None,
            text: text.to_owned(),
            hpath: Vec::new(),
            parent: None,
            anchor,
            table: None,
            flags,
            exact: false,
        });
    }
    let mut speakers: Vec<String> = resp
        .get("speakers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    s.as_str()
                        .filter(|s| is_speaker_label(s))
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    speakers.truncate(64);
    let mut warnings: Vec<IrWarning> = Vec::new();
    for w in resp
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(code) = w.as_str().filter(|c| c.starts_with("DOCS_PARSE_")) {
            warnings.push(IrWarning {
                code: code.to_owned(),
                block: None,
            });
        }
    }
    if blocks.is_empty() && !warnings.iter().any(|w| w.code == "DOCS_PARSE_EMPTY") {
        warnings.push(IrWarning {
            code: "DOCS_PARSE_EMPTY".to_owned(),
            block: None,
        });
    }
    let meta = DocMeta {
        duration_ms: u(resp, "duration_ms"),
        speakers,
        codec: resp
            .get("codec")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
            .map(|c| c.chars().take(64).collect()),
        width: u(resp, "width").filter(|w| *w > 0).map(|w| w as u32),
        height: u(resp, "height").filter(|h| *h > 0).map(|h| h as u32),
        ..DocMeta::default()
    };
    Ok(IrFragment {
        blob_b3,
        format,
        parser,
        meta,
        blocks,
        warnings,
    })
}
