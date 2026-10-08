use serde::Deserialize;

use crate::error::{Error, Result};
use crate::sha256::sha256_hex;

pub const HEARTBEAT_SECONDS: i64 = 10;
pub const LEASE_SECONDS: i64 = 30;
pub const WAVE2_DELAY_SECONDS: i64 = 45;
pub const PROOF_PROJECT: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a390";
pub const CONFIG_VERSION: &str = "jw-payload-1";
pub const KIND: &str = "pg_effect";
pub const WAKE_CHANNEL: &str = "oo_jw_wake";
pub const DATABASE: &str = "oo_a390_control";
pub const RUNTIME_LOGIN: &str = "oo_a390_runtime_login";

pub const BOOT_SHA: &str = "445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52";
pub const CONTROL_SHA: &str = "b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75";
pub const EVENT_SHA: &str = "4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee";
pub const LEASE_SHA: &str = "5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034";
pub const ADAPT_SHA: &str = "8ba28e94b8b861f640a1aa5dc8021ba6bbd44cb64a7c8a13a1fefae421331bcd";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRequest {
    pub project_id: String,
    pub dedupe_key: String,
    pub units: u32,
    pub fail_attempts: u32,
    pub stop_after: Option<u32>,
    pub terminal_failure: bool,
    pub max_attempts: i32,
    pub run_delay_seconds: i64,
}

impl TaskRequest {
    pub fn check(&self) -> Result<()> {
        if !is_uuid(&self.project_id) {
            return Err(Error::new("input"));
        }
        let key_len = self.dedupe_key.len();
        if !(1..=256).contains(&key_len) || !self.dedupe_key.is_ascii() {
            return Err(Error::new("input"));
        }
        if !(1..=32).contains(&self.units) && self.units != 0 {
            return Err(Error::new("input"));
        }
        if self.fail_attempts > 32 || !(1..=32).contains(&self.max_attempts) {
            return Err(Error::new("input"));
        }
        if !(0..=120).contains(&self.run_delay_seconds) {
            return Err(Error::new("input"));
        }
        if self.stop_after.is_some_and(|stop| stop > 32) {
            return Err(Error::new("input"));
        }
        Ok(())
    }

    pub fn payload_text(&self) -> String {
        payload_text(
            self.units,
            self.fail_attempts,
            self.stop_after,
            self.terminal_failure,
        )
    }

    pub fn digest(&self) -> String {
        sha256_hex(self.payload_text().as_bytes())
    }
}

pub fn payload_text(
    units: u32,
    fail_attempts: u32,
    stop_after: Option<u32>,
    terminal_failure: bool,
) -> String {
    let mut text = format!(r#"{{"v":1,"units":{units},"fail_attempts":{fail_attempts}"#);
    if let Some(stop) = stop_after {
        text.push_str(&format!(r#","stop_after":{stop}"#));
    }
    if terminal_failure {
        text.push_str(r#","terminal_failure":true"#);
    }
    text.push('}');
    text
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub v: u32,
    pub units: u32,
    pub fail_attempts: u32,
    #[serde(default)]
    pub stop_after: Option<u32>,
    #[serde(default)]
    pub terminal_failure: bool,
}

impl Work {
    pub fn parse(text: &str) -> Result<Self> {
        let work: Self = serde_json::from_str(text).map_err(|_| Error::new("payload"))?;
        if work.v != 1 || work.fail_attempts > 32 {
            return Err(Error::new("payload"));
        }
        if work.stop_after.is_some_and(|stop| stop > 32) {
            return Err(Error::new("payload"));
        }
        if !(1..=32).contains(&work.units) {
            return Err(Error::new("payload"));
        }
        Ok(work)
    }
}

pub fn heartbeat_due(elapsed_seconds: i64) -> bool {
    elapsed_seconds >= HEARTBEAT_SECONDS
}

pub fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let dash = index == 8 || index == 13 || index == 18 || index == 23;
        let ok = if dash {
            *byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        };
        if !ok {
            return false;
        }
    }
    true
}

pub fn worker_id(index: u16) -> String {
    format!("w-{index:03}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseStatus {
    Enqueued,
    Replayed,
    Claimed,
    None,
    Extended,
    Succeeded,
    Failed,
    Retried,
    Cancelled,
    LostLease,
    Terminal,
}

impl LeaseStatus {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "enqueued" => Self::Enqueued,
            "replayed" => Self::Replayed,
            "claimed" => Self::Claimed,
            "none" => Self::None,
            "extended" => Self::Extended,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "retried" => Self::Retried,
            "cancelled" => Self::Cancelled,
            "lost_lease" => Self::LostLease,
            "terminal" => Self::Terminal,
            _ => return Err(Error::new("status")),
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enqueued => "enqueued",
            Self::Replayed => "replayed",
            Self::Claimed => "claimed",
            Self::None => "none",
            Self::Extended => "extended",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Retried => "retried",
            Self::Cancelled => "cancelled",
            Self::LostLease => "lost_lease",
            Self::Terminal => "terminal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseResult {
    pub status: LeaseStatus,
    pub task_id: Option<String>,
    pub project_id: Option<String>,
    pub kind: Option<String>,
    pub state: Option<String>,
    pub attempt: Option<i32>,
    pub generation: Option<i64>,
    pub lease_owner: Option<String>,
    pub run_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{heartbeat_due, payload_text, LeaseStatus, Work, HEARTBEAT_SECONDS, LEASE_SECONDS};

    #[test]
    fn heartbeat_boundary_matches_the_frozen_constant() {
        assert_eq!(HEARTBEAT_SECONDS, 10);
        assert_eq!(LEASE_SECONDS, 30);
        assert!(!heartbeat_due(9));
        assert!(heartbeat_due(10));
    }

    #[test]
    fn payload_parses_and_rejects_zero_units() {
        let text = payload_text(4, 0, Some(2), false);
        assert_eq!(text, r#"{"v":1,"units":4,"fail_attempts":0,"stop_after":2}"#);
        let work = Work::parse(&text).expect("payload");
        assert_eq!(work.units, 4);
        assert_eq!(work.stop_after, Some(2));
        assert!(Work::parse(r#"{"v":1,"units":0,"fail_attempts":0}"#).is_err());
    }

    #[test]
    fn statuses_are_the_published_set() {
        assert_eq!(LeaseStatus::parse("lost_lease").unwrap(), LeaseStatus::LostLease);
        assert!(LeaseStatus::parse("already_applied").is_err());
    }
}
