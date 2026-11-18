//! Client side of `media-parser` (10 §17.1 to §17.2, §17.8, board B-S10-05).
//!
//! `POST /v1/parse` answers `202 {task}`; the task is polled for `{done_ms, total_ms}` (reported as
//! `file.progress` by the caller) until `done`, `failed` or `cancelled`. A caller that wants to stop returns
//! `false` from the progress callback; the client then `DELETE`s the task, which kills the worker.
//! Size is checked before upload (4 GiB); duration and codec are the sidecar's verdicts.

use std::time::Duration;

use oo_core::docir::DocFormat;
use serde_json::{Value, json};

use super::blob_b3;
use super::sidecar::{DEADLINE_MS, SidecarClient, SidecarError, map_error, sha256_hex};
use crate::jobs::media_pool;
use crate::normalize::media::assemble;
use crate::normalize::{Normalized, finish};

/// Audio or video, as the engine decided from the sniffed container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Audio,
    Video,
}

impl MediaKind {
    pub fn format(self) -> DocFormat {
        match self {
            Self::Audio => DocFormat::Audio,
            Self::Video => DocFormat::Video,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }
}

/// Request options (`oo.docs/v1` `media.*` and the Drive's hot words).
#[derive(Clone, Debug, Default)]
pub struct MediaOptions {
    /// Entity names and aliases of this Drive; they only bias recognition and are never written to a Pack.
    pub hotwords: Vec<String>,
    /// `media.external_ffmpeg`: absolute path of the customer's FFmpeg (Linux).
    pub external_ffmpeg: Option<String>,
    /// Directory of the host decode helper's output for this blob (macOS Desktop).
    pub host_decode_dir: Option<String>,
    /// Number of speakers when the meeting metadata says so.
    pub num_speakers: Option<u32>,
}

impl MediaOptions {
    fn to_json(&self) -> Value {
        let mut o = serde_json::Map::new();
        if !self.hotwords.is_empty() {
            o.insert("hotwords".into(), json!(self.hotwords));
        }
        if let Some(p) = &self.external_ffmpeg {
            o.insert("external_ffmpeg".into(), json!(p));
        }
        if let Some(d) = &self.host_decode_dir {
            o.insert("host_decode_dir".into(), json!(d));
        }
        if let Some(n) = self.num_speakers {
            o.insert("num_speakers".into(), json!(n));
        }
        Value::Object(o)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaProgress {
    pub done_ms: u64,
    pub total_ms: u64,
}

/// Map the `error` object of a failed task.
pub fn task_error(err: &Value) -> SidecarError {
    let code = err.get("code").and_then(Value::as_str).unwrap_or("");
    let msg = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    match code {
        "PARSER_CODEC_UNAVAILABLE" => {
            let d = err.get("details");
            SidecarError::CodecUnavailable {
                codecs: d
                    .and_then(|d| d.get("codecs"))
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|c| c.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                how_to: d
                    .and_then(|d| d.get("how_to"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            }
        }
        "PARSER_MEDIA_TOO_LONG" => SidecarError::MediaTooLong,
        "PARSER_HOST_DECODE_MISMATCH" => SidecarError::HostDecodeMismatch,
        "PARSER_ASR_FAILED" => SidecarError::AsrFailed,
        "PARSER_DIARIZATION_FAILED" => SidecarError::DiarizationFailed,
        "PARSER_DISK_LOW" => SidecarError::DiskLow,
        "PARSER_ENCRYPTED" => SidecarError::Encrypted,
        "PARSER_CORRUPT" => SidecarError::Corrupt(msg),
        "PARSER_UNSUPPORTED" => SidecarError::Unsupported(msg),
        "PARSER_OUTPUT_LIMIT" => SidecarError::OutputLimit,
        "PARSER_TIMEOUT" => SidecarError::Timeout,
        "PARSER_OOM" => SidecarError::Oom,
        "PARSER_CRASH" => SidecarError::Crash(msg),
        _ => SidecarError::Crash(format!("task failed: {code}")),
    }
}

impl SidecarClient {
    /// Parse an audio or video file through the media-parser this client points at.
    ///
    /// `poll` is the task polling period; `progress` returns `false` to cancel.
    pub fn parse_media(
        &self,
        bytes: &[u8],
        kind: MediaKind,
        opts: &MediaOptions,
        poll: Duration,
        progress: &mut dyn FnMut(MediaProgress) -> bool,
    ) -> Result<Normalized, SidecarError> {
        media_pool::check_size(bytes.len() as u64).map_err(|_| SidecarError::MediaTooLong)?;
        let sha = sha256_hex(bytes);
        self.put_blob(&sha, bytes)?;
        let result = self.run_media_task(&sha, bytes, kind, opts, poll, progress);
        self.delete_blob(&sha);
        result
    }

    fn run_media_task(
        &self,
        sha: &str,
        bytes: &[u8],
        kind: MediaKind,
        opts: &MediaOptions,
        poll: Duration,
        progress: &mut dyn FnMut(MediaProgress) -> bool,
    ) -> Result<Normalized, SidecarError> {
        let req = json!({"blob": sha, "format": kind.name(), "options": opts.to_json(),
                         "deadline_ms": DEADLINE_MS});
        let r = self.call(
            "POST",
            "/v1/parse",
            req.to_string().as_bytes(),
            true,
            true,
            Duration::from_secs(30),
        )?;
        if r.status != 202 {
            return Err(map_error(&r));
        }
        let task = serde_json::from_slice::<Value>(&r.body)
            .ok()
            .and_then(|v| v.get("task").and_then(Value::as_str).map(str::to_owned))
            .ok_or_else(|| SidecarError::Protocol("parse did not return a task".into()))?;
        let path = format!("/v1/tasks/{task}");
        loop {
            let r = self.call("GET", &path, b"", false, true, Duration::from_secs(30))?;
            if r.status != 200 {
                return Err(map_error(&r));
            }
            let v: Value = serde_json::from_slice(&r.body)
                .map_err(|_| SidecarError::Protocol("task response is not JSON".into()))?;
            match v.get("state").and_then(Value::as_str) {
                Some("running") => {
                    let p = MediaProgress {
                        done_ms: v.get("done_ms").and_then(Value::as_u64).unwrap_or(0),
                        total_ms: v.get("total_ms").and_then(Value::as_u64).unwrap_or(0),
                    };
                    if !progress(p) {
                        let _ =
                            self.call("DELETE", &path, b"", false, true, Duration::from_secs(10));
                        return Err(SidecarError::Cancelled);
                    }
                    std::thread::sleep(poll);
                }
                Some("done") => {
                    let result = v
                        .get("result")
                        .ok_or_else(|| SidecarError::Protocol("done without result".into()))?;
                    if result.get("protocol").and_then(Value::as_str)
                        != Some(super::sidecar::PROTOCOL)
                    {
                        return Err(SidecarError::Protocol("unexpected protocol".into()));
                    }
                    let fragment = assemble(kind.format(), blob_b3(bytes), result)
                        .map_err(|e| SidecarError::Normalize(e.to_string()))?;
                    return finish(fragment).map_err(|e| SidecarError::Normalize(format!("{e:?}")));
                }
                Some("failed") => {
                    return Err(task_error(v.get("error").unwrap_or(&Value::Null)));
                }
                Some("cancelled") => return Err(SidecarError::Cancelled),
                _ => return Err(SidecarError::Protocol("unknown task state".into())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, Read, Write};
    use std::sync::{Arc, Mutex};

    use super::super::sidecar::SidecarConfig;
    use super::*;

    /// A tiny media sidecar: answers by `(method, path)` from a script; GET task answers the queued states.
    fn mock(states: Vec<Value>, post_status: u16) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let queue = Arc::new(Mutex::new(
            states
                .into_iter()
                .collect::<std::collections::VecDeque<_>>(),
        ));
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { break };
                let mut reader = std::io::BufReader::new(conn);
                let mut first = String::new();
                if reader.read_line(&mut first).unwrap_or(0) == 0 {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0u8; len];
                reader.read_exact(&mut body).unwrap();
                let mut parts = first.split_whitespace();
                let (method, path) = (
                    parts.next().unwrap().to_owned(),
                    parts.next().unwrap().to_owned(),
                );
                log.lock().unwrap().push(format!("{method} {path}"));
                let (status, out) = match (method.as_str(), path.as_str()) {
                    ("PUT", _) => (201, "{}".to_owned()),
                    ("DELETE", p) if p.starts_with("/v1/blobs/") => (204, String::new()),
                    ("DELETE", _) => (204, String::new()),
                    ("POST", "/v1/parse") if post_status == 202 => {
                        (202, json!({"task": "ab".repeat(16)}).to_string())
                    }
                    ("POST", "/v1/parse") => (
                        post_status,
                        json!({"error": {"code": "PARSER_BUSY", "message": "b"}}).to_string(),
                    ),
                    ("GET", p) if p.starts_with("/v1/tasks/") => {
                        let mut q = queue.lock().unwrap();
                        let s = if q.len() > 1 {
                            q.pop_front().unwrap()
                        } else {
                            q.front().cloned().unwrap()
                        };
                        (200, s.to_string())
                    }
                    _ => (500, "{}".into()),
                };
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    out.len()
                );
                let mut conn = reader.into_inner();
                let _ = conn.write_all(head.as_bytes());
                let _ = conn.write_all(out.as_bytes());
            }
        });
        (addr, seen)
    }

    fn done_result() -> Value {
        json!({"state": "done", "result": {
            "protocol": super::super::sidecar::PROTOCOL, "parser": {"id": "media", "version": "1"},
            "kind": "audio", "duration_ms": 9000, "speakers": ["S1"], "codec": "opus",
            "profile": {"tier": "cpu", "isa": "x", "models": []},
            "items": [{"kind": "utterance", "speaker": "S1", "t_start_ms": 100, "t_end_ms": 900, "text": "你好",
                       "word_start": 0, "word_end": 1, "confidence": 0.9}]}})
    }

    fn client(addr: String) -> SidecarClient {
        SidecarClient::new(SidecarConfig::new(addr, "t"))
    }

    #[test]
    fn a_media_task_reports_progress_and_returns_the_ir() {
        let (addr, seen) = mock(
            vec![
                json!({"state": "running", "done_ms": 1000, "total_ms": 9000}),
                json!({"state": "running", "done_ms": 6000, "total_ms": 9000}),
                done_result(),
            ],
            202,
        );
        let mut seen_progress = Vec::new();
        let n = client(addr)
            .parse_media(
                b"fake audio",
                MediaKind::Audio,
                &MediaOptions {
                    hotwords: vec!["灯塔".into()],
                    ..Default::default()
                },
                Duration::from_millis(5),
                &mut |p| {
                    seen_progress.push(p);
                    true
                },
            )
            .unwrap();
        assert_eq!(n.ir.format, DocFormat::Audio);
        assert_eq!(n.ir.blocks[0].anchor.fragment(), "#t=00:00:00-00:00:01@S1");
        assert_eq!(
            seen_progress,
            vec![
                MediaProgress {
                    done_ms: 1000,
                    total_ms: 9000
                },
                MediaProgress {
                    done_ms: 6000,
                    total_ms: 9000
                }
            ]
        );
        let log = seen.lock().unwrap();
        assert!(
            log.iter().any(|l| l.starts_with("DELETE /v1/blobs/")),
            "the scratch copy is deleted"
        );
    }

    #[test]
    fn cancelling_deletes_the_task() {
        let (addr, seen) = mock(
            vec![json!({"state": "running", "done_ms": 1, "total_ms": 9})],
            202,
        );
        let e = client(addr)
            .parse_media(
                b"x",
                MediaKind::Video,
                &MediaOptions::default(),
                Duration::from_millis(5),
                &mut |_| false,
            )
            .unwrap_err();
        assert_eq!(e, SidecarError::Cancelled);
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .any(|l| l.starts_with("DELETE /v1/tasks/"))
        );
    }

    #[test]
    fn typed_task_errors_map_to_state_codes() {
        let failed = |code: &str, details: Value| json!({"state": "failed", "error": {"code": code, "message": "m", "details": details}});
        let (addr, _) = mock(
            vec![failed(
                "PARSER_CODEC_UNAVAILABLE",
                json!({"codecs": ["h264", "aac"], "how_to": "set media.external_ffmpeg"}),
            )],
            202,
        );
        let e = client(addr)
            .parse_media(
                b"x",
                MediaKind::Video,
                &MediaOptions::default(),
                Duration::from_millis(5),
                &mut |_| true,
            )
            .unwrap_err();
        assert_eq!(
            e,
            SidecarError::CodecUnavailable {
                codecs: vec!["h264".into(), "aac".into()],
                how_to: "set media.external_ffmpeg".into()
            }
        );
        assert_eq!(e.code(), Some("DOCS_PARSE_CODEC_UNAVAILABLE"));
        assert!(e.consumes_attempt());
        for (code, want) in [
            ("PARSER_MEDIA_TOO_LONG", Some("DOCS_SOURCE_MEDIA_TOO_LONG")),
            (
                "PARSER_HOST_DECODE_MISMATCH",
                Some("DOCS_PARSE_MEDIA_HOST_DECODE_MISMATCH"),
            ),
            ("PARSER_ASR_FAILED", Some("DOCS_PARSE_ASR_FAILED")),
            (
                "PARSER_DIARIZATION_FAILED",
                Some("DOCS_PARSE_DIARIZATION_FAILED"),
            ),
            ("PARSER_DISK_LOW", None),
            ("PARSER_CRASH", Some("DOCS_PARSE_CRASH")),
        ] {
            assert_eq!(
                task_error(&json!({"code": code, "message": "m"})).code(),
                want,
                "{code}"
            );
        }
        assert_eq!(
            task_error(&json!({"code": "PARSER_DISK_LOW"})),
            SidecarError::DiskLow
        );
    }

    #[test]
    fn a_busy_media_sidecar_is_retried_later_and_an_oversize_file_never_uploads() {
        let (addr, seen) = mock(vec![], 429);
        let e = client(addr)
            .parse_media(
                b"x",
                MediaKind::Audio,
                &MediaOptions::default(),
                Duration::from_millis(5),
                &mut |_| true,
            )
            .unwrap_err();
        assert!(matches!(e, SidecarError::Busy { .. }) && !e.consumes_attempt());
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .any(|l| l.starts_with("DELETE /v1/blobs/"))
        );
        let e = media_pool::check_size((4u64 << 30) + 1);
        assert!(e.is_err());
    }

    #[test]
    fn options_serialize_only_what_is_set() {
        assert_eq!(MediaOptions::default().to_json(), json!({}));
        let o = MediaOptions {
            hotwords: vec!["a".into()],
            external_ffmpeg: Some("/usr/bin/ffmpeg".into()),
            host_decode_dir: None,
            num_speakers: Some(3),
        };
        assert_eq!(
            o.to_json(),
            json!({"hotwords": ["a"], "external_ffmpeg": "/usr/bin/ffmpeg", "num_speakers": 3})
        );
    }
}
