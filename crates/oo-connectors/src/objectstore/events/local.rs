//! The local event source: a spool directory of `*.json` bodies (any format of
//! [`super::parse`]). It stands in for SQS, Queues or MNS in tests and the L7
//! script: `drop` writes a body, `drain` reads the files in name order, parses
//! them and removes the ones it understood (a body it cannot parse stays and is
//! counted, so nothing is lost silently).

use std::path::{Path, PathBuf};

use super::parse::{ParseError, parse_hints};
use crate::objectstore::ChangeHint;

#[derive(Clone, Debug)]
pub struct LocalEvents {
    dir: PathBuf,
    prefix: String,
}

/// One drain of the spool.
#[derive(Debug, Default)]
pub struct Drained {
    pub hints: Vec<ChangeHint>,
    /// Bodies that did not parse (left in place).
    pub rejected: Vec<(String, ParseError)>,
}

impl LocalEvents {
    pub fn new(dir: &Path, prefix: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
            prefix: prefix.to_owned(),
        })
    }

    /// Add one event body; returns the file name.
    pub fn drop(&self, body: &str) -> std::io::Result<String> {
        let mut n = 0u32;
        loop {
            let name = format!("{:020}-{n}.json", now_ns());
            let path = self.dir.join(&name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    std::io::Write::write_all(&mut f, body.as_bytes())?;
                    return Ok(name);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => return Err(e),
            }
        }
    }

    /// Read and consume every parsable body, oldest first.
    pub fn drain(&self, now_ms: i64) -> std::io::Result<Drained> {
        let mut names: Vec<String> = std::fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(".json"))
            .collect();
        names.sort();
        let mut out = Drained::default();
        for name in names {
            let path = self.dir.join(&name);
            let body = std::fs::read_to_string(&path)?;
            match parse_hints(&body, &self.prefix, now_ms) {
                Ok(h) => {
                    out.hints.extend(h);
                    std::fs::remove_file(&path)?;
                }
                Err(e) => out.rejected.push((name, e)),
            }
        }
        Ok(out)
    }
}

fn now_ns() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_reads_in_order_consumes_and_keeps_what_it_cannot_read() {
        let dir =
            std::env::temp_dir().join(format!("oo-events-{}-{}", std::process::id(), now_ns()));
        let ev = LocalEvents::new(&dir, "d/").unwrap();
        ev.drop(r#"{"hints":[{"key":"d/a.md","op":"put","sequencer":"1","at_ms":1}]}"#)
            .unwrap();
        ev.drop("garbage").unwrap();
        ev.drop(r#"{"hints":[{"key":"d/a.md","op":"delete","sequencer":"2","at_ms":2}]}"#)
            .unwrap();
        let d = ev.drain(0).unwrap();
        assert_eq!(d.hints.len(), 2);
        assert_eq!(d.hints[0].sequencer.as_deref(), Some("1"));
        assert_eq!(d.rejected.len(), 1);
        assert_eq!(ev.drain(0).unwrap().hints.len(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
