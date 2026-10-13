//! Hash, font-cmap, and Pi tool-event checks for the runtime kit.
//!
//! Development install evidence only. This crate does not accept SP10-W04,
//! SP10-W05, the parent phase, or the short-term goal.

#![forbid(unsafe_code)]

pub mod cmap;
pub mod events;
pub mod handoff;
pub mod json;
pub mod manifest;
pub mod probe;
pub mod provider;
pub mod scan;
pub mod sha256;

pub const PINS_ENV: &str = include_str!("../pins.env");
pub const PUBLIC_MARKER: &str = "public-proxy-marker";
pub const PACK_MARKER: &str = "pack-marker-a406-public";
pub const PI_VERSION: &str = "1.0.4";

pub fn pin_value(key: &str) -> Result<&'static str, String> {
    for line in PINS_ENV.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!("pin line has no equals: {line}"));
        };
        if name == key {
            if value.is_empty() || value.chars().any(char::is_whitespace) {
                return Err(format!("pin {key} is empty or padded"));
            }
            return Ok(value);
        }
    }
    Err(format!("pin {key} is absent"))
}
