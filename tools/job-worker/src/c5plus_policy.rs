use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub period_ms: [u64; 4],
    pub duty_per_mille: u16,
    pub threads_running_max: u32,
    pub execution_window_utc: [u16; 2],
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub completed_at_ms: [Option<u64>; 4],
}

impl Config {
    pub fn validate(&self) -> Result<(), &'static str> {
        let [start, end] = self.execution_window_utc;
        if self.period_ms.contains(&0)
            || !(1..=1000).contains(&self.duty_per_mille)
            || self.threads_running_max == 0
            || start >= 1440
            || end > 1440
            || start == end
        {
            return Err("c5plus_config");
        }
        Ok(())
    }

    pub fn due(
        &self,
        now_ms: u64,
        utc_minute: u16,
        threads_running: u32,
        progress: &Progress,
    ) -> Result<[bool; 4], &'static str> {
        self.validate()?;
        if utc_minute >= 1440
            || progress
                .completed_at_ms
                .iter()
                .flatten()
                .any(|completed| *completed > now_ms)
        {
            return Err("c5plus_clock");
        }
        let [start, end] = self.execution_window_utc;
        let in_window = if start < end {
            utc_minute >= start && utc_minute < end
        } else {
            utc_minute >= start || utc_minute < end
        };
        Ok(std::array::from_fn(|layer| {
            threads_running <= self.threads_running_max
                && (layer < 2 || in_window)
                && progress.completed_at_ms[layer]
                    .is_none_or(|last| now_ms - last >= self.period_ms[layer])
        }))
    }

    pub fn rest_ms(&self, elapsed_ms: u64) -> Result<u64, &'static str> {
        self.validate()?;
        let duty = u128::from(self.duty_per_mille);
        let rest = (u128::from(elapsed_ms) * (1000 - duty)).div_ceil(duty);
        u64::try_from(rest).map_err(|_| "c5plus_duration")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayeredConfig {
    pub policy: Config,
    #[serde(default, skip_serializing_if = "is_false")]
    pub incremental_only: bool,
    pub window_ms: u64,
    pub overlap_ms: u64,
    pub recent_column: Option<String>,
    pub server_timeout_ms: u32,
    pub client_timeout_ms: u64,
    pub execution_timezone: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl LayeredConfig {
    pub fn due(
        &self,
        now_ms: u64,
        utc_minute: u16,
        threads_running: u32,
        progress: &Progress,
    ) -> Result<[bool; 4], &'static str> {
        self.validate()?;
        let mut due = self
            .policy
            .due(now_ms, utc_minute, threads_running, progress)?;
        // Suppression is not completion: leave the persisted L4 checkpoint untouched.
        if self.incremental_only {
            due[3] = false;
        }
        Ok(due)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        self.policy.validate()?;
        if self.execution_timezone != "UTC"
            || self.window_ms == 0
            || self.window_ms.checked_add(self.overlap_ms).is_none()
            || self.server_timeout_ms == 0
            || self.client_timeout_ms <= u64::from(self.server_timeout_ms)
            || self.recent_column.as_ref().is_some_and(|name| {
                name.is_empty()
                    || name.len() > 64
                    || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            })
        {
            return Err("c5plus_config");
        }
        Ok(())
    }
}
