//! Media pool and limits (10 section 17.8, D77, board B-S10-10).
//!
//! Audio and video parse jobs have their own job kind, [`JobKind::ParseMedia`], so they are claimed by their
//! own loop with their own slots (`concurrency.media`, default 1) and never hold one of the document slots.
//! Limits: one file at most 4 GiB and 4 hours (`too_large`, `DOCS_SOURCE_MEDIA_TOO_LONG`); the scratch guard
//! (`blocked_reason = disk_low`) is the sidecar's `PARSER_DISK_LOW`.

use super::JobKind;

/// `concurrency.media` default.
pub const DEFAULT_MEDIA_SLOTS: usize = 1;
/// Most slots a configuration may ask for (the media sidecar has `mem_limit` 4g and 2 cpus per worker).
pub const MAX_MEDIA_SLOTS: usize = 4;
/// One media file: size and duration limits (D77).
pub const MAX_MEDIA_BYTES: u64 = 4 << 30;
pub const MAX_MEDIA_MS: u64 = 4 * 3600 * 1000;
/// Error code of the over-limit state.
pub const CODE_TOO_LONG: &str = "DOCS_SOURCE_MEDIA_TOO_LONG";
/// `blocked_reason` of a job that waits for scratch space.
pub const BLOCKED_DISK_LOW: &str = "disk_low";

/// `concurrency.media` from the configuration, clamped to 1..=[`MAX_MEDIA_SLOTS`].
pub fn clamp_media_slots(n: u64) -> usize {
    (n as usize).clamp(1, MAX_MEDIA_SLOTS)
}

/// Audio and video containers by extension; the sniffer reports them as `unknown` until the sidecar probes them.
pub fn is_media_extension(ext: &str) -> bool {
    matches!(
        ext,
        "wav"
            | "mp3"
            | "flac"
            | "ogg"
            | "oga"
            | "opus"
            | "m4a"
            | "aac"
            | "mka"
            | "webm"
            | "mkv"
            | "mp4"
            | "mov"
            | "m4v"
            | "avi"
    )
}

/// The kind a parse job of this file extension is queued as.
pub fn parse_kind_for(ext: &str) -> JobKind {
    if is_media_extension(ext) {
        JobKind::ParseMedia
    } else {
        JobKind::Parse
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaLimit {
    TooLarge { bytes: u64 },
    TooLong { duration_ms: u64 },
}

impl MediaLimit {
    pub fn code(self) -> &'static str {
        CODE_TOO_LONG
    }
}

/// Size known before upload; duration known after the sidecar probe (`probe` or the task error).
pub fn check_size(bytes: u64) -> Result<(), MediaLimit> {
    if bytes > MAX_MEDIA_BYTES {
        Err(MediaLimit::TooLarge { bytes })
    } else {
        Ok(())
    }
}

pub fn check_duration(duration_ms: u64) -> Result<(), MediaLimit> {
    if duration_ms > MAX_MEDIA_MS {
        Err(MediaLimit::TooLong { duration_ms })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::scheduler::{SchedulerConfig, slot_pools, slots_for};
    use super::*;

    #[test]
    fn media_has_its_own_kind_and_database_text() {
        assert_ne!(JobKind::ParseMedia, JobKind::Parse);
        assert_eq!(JobKind::ParseMedia.as_str(), "parse_media");
        assert_eq!(JobKind::parse("parse_media"), Some(JobKind::ParseMedia));
        assert_eq!(
            JobKind::ParseMedia.policy().max_attempts,
            JobKind::Parse.policy().max_attempts
        );
        assert_eq!(parse_kind_for("mp4"), JobKind::ParseMedia);
        assert_eq!(
            parse_kind_for("PDF".to_ascii_lowercase().as_str()),
            JobKind::Parse
        );
        assert!(!is_media_extension("docx") && is_media_extension("m4a"));
    }

    #[test]
    fn media_defaults_to_one_slot_and_is_clamped() {
        let cfg = SchedulerConfig::new("t");
        assert_eq!(DEFAULT_MEDIA_SLOTS, 1);
        assert_eq!(clamp_media_slots(0), 1);
        assert_eq!(clamp_media_slots(2), 2);
        assert_eq!(clamp_media_slots(99), MAX_MEDIA_SLOTS);
        // Unset kinds get 2, the engine sets media explicitly (config.rs).
        assert_eq!(slots_for(&cfg, JobKind::Parse), 2);
    }

    #[tokio::test]
    async fn a_running_media_job_does_not_take_a_document_slot() {
        let mut cfg = SchedulerConfig::new("t");
        cfg.concurrency.insert(JobKind::Parse, 4);
        cfg.concurrency
            .insert(JobKind::ParseMedia, DEFAULT_MEDIA_SLOTS);
        let pools = slot_pools(&cfg, [JobKind::Parse, JobKind::ParseMedia]);
        let media = pools[&JobKind::ParseMedia].clone();
        let docs = pools[&JobKind::Parse].clone();
        // A recording is being parsed for hours.
        let long_job = media.clone().acquire_owned().await.unwrap();
        // A second recording waits; the four document slots are all free.
        assert!(media.clone().try_acquire_owned().is_err());
        let held: Vec<_> = (0..4)
            .map(|_| docs.clone().try_acquire_owned().expect("document slot"))
            .collect();
        assert_eq!(held.len(), 4);
        assert!(
            docs.clone().try_acquire_owned().is_err(),
            "the document pool is exactly 4"
        );
        // Releasing the recording lets the next one start; it never needed a document slot.
        drop(long_job);
        let next =
            tokio::time::timeout(Duration::from_millis(200), media.clone().acquire_owned()).await;
        assert!(next.is_ok());
        drop(held);
    }

    #[test]
    fn limits_name_the_media_error_code() {
        assert_eq!(check_size(MAX_MEDIA_BYTES), Ok(()));
        assert_eq!(
            check_size(MAX_MEDIA_BYTES + 1),
            Err(MediaLimit::TooLarge {
                bytes: MAX_MEDIA_BYTES + 1
            })
        );
        assert_eq!(check_duration(MAX_MEDIA_MS), Ok(()));
        let e = check_duration(MAX_MEDIA_MS + 1).unwrap_err();
        assert_eq!(
            e,
            MediaLimit::TooLong {
                duration_ms: MAX_MEDIA_MS + 1
            }
        );
        assert_eq!(e.code(), "DOCS_SOURCE_MEDIA_TOO_LONG");
        assert_eq!(BLOCKED_DISK_LOW, "disk_low");
    }
}
