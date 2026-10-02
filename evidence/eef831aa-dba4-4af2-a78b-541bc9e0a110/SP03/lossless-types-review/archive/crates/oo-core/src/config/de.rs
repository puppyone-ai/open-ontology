//! Numeric deserializers whose accepted range matches the JSON Schema keyword.

use serde::{Deserialize, Deserializer};

pub fn port<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    let value = u16::deserialize(deserializer)?;
    if (1..=65535).contains(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("port must be in 1..=65535"))
    }
}

macro_rules! de_u32 {
    ($name:ident, $min:literal, $max:literal) => {
        pub fn $name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
            let value = u32::deserialize(deserializer)?;
            if ($min..=$max).contains(&value) {
                Ok(value)
            } else {
                Err(serde::de::Error::custom(concat!(
                    "integer must be in ",
                    stringify!($min),
                    "..=",
                    stringify!($max)
                )))
            }
        }
    };
}

de_u32!(connections, 1, 128);
de_u32!(rows_per_sec, 1, 1_000_000_000);
de_u32!(txn_events, 1, 10_000_000);
de_u32!(retention_days, 1, 36500);
de_u32!(max_rows, 1, 100_000);
de_u32!(max_rows_cap, 1, 100_000);
de_u32!(export_rows, 1, 100_000_000);
de_u32!(concurrency, 1, 256);
de_u32!(tokens, 1, 100_000_000);
de_u32!(tool_calls, 1, 10_000);
de_u32!(turn_seconds, 1, 86_400);
de_u32!(cpu, 1, 64);
de_u32!(pids, 1, 65_535);
de_u32!(pool_size, 0, 10_000);
de_u32!(header_row, 0, 1_000_000);
de_u32!(runs_per_item, 1, 100);
de_u32!(deprecation_releases, 0, 100);
de_u32!(token_budget, 1, 1_000_000);
de_u32!(sample_cap, 0, 100);
de_u32!(reject_count, 0, 1_000_000_000);
de_u32!(page_limit, 1, 200);
de_u32!(query_rows, 1, 2000);
de_u32!(timeout_ms, 1, 120_000);
