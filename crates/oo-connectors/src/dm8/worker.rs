//! Source-only DM8 driver transport. The trusted caller supplies a pinned
//! worker command that runs DM's own DPI client; credentials travel over
//! stdin once, never argv or environment. No Dameng library is linked here.
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::Duration;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Bind, ConnectConfig};

pub(super) const PROTOCOL: &str = "oo.dm8.driver/v1";
const MAX_FRAME: usize = 32 * 1024 * 1024;
const MAX_BATCH: usize = 512;

#[derive(Debug)]
pub enum DriverError {
    /// `code` is DM's numeric error code (negative), 0 when not a database error.
    Worker {
        code: i64,
        timeout: bool,
    },
    Source(crate::SourceError),
}

fn invalid() -> DriverError {
    DriverError::Worker {
        code: 0,
        timeout: false,
    }
}

type Frame = (u8, Vec<u8>);
type Rows = Vec<Vec<Option<String>>>;

fn read_frame(reader: &mut impl Read) -> std::io::Result<Frame> {
    let mut header = [0; 5];
    reader.read_exact(&mut header)?;
    let size = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if !matches!(header[0], b'J' | b'R') || size > MAX_FRAME {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut data = vec![0; size];
    reader.read_exact(&mut data)?;
    Ok((header[0], data))
}

pub struct Worker {
    child: RefCell<Child>,
    input: RefCell<ChildStdin>,
    frames: Receiver<std::io::Result<Frame>>,
    timeout: Cell<Duration>,
}

impl Worker {
    pub fn connect(command: &mut Command, cfg: &ConnectConfig) -> Result<Self, DriverError> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| invalid())?;
        let input = child.stdin.take().ok_or_else(invalid)?;
        let mut output = child.stdout.take().ok_or_else(invalid)?;
        let (sender, frames) = sync_channel(1);
        std::thread::spawn(move || {
            loop {
                let frame = read_frame(&mut output);
                let failed = frame.is_err();
                if sender.send(frame).is_err() || failed {
                    break;
                }
            }
        });
        let worker = Self {
            child: RefCell::new(child),
            input: RefCell::new(input),
            frames,
            timeout: Cell::new(Duration::from_secs(30)),
        };
        let request = Zeroizing::new(
            serde_json::to_vec(&json!({
                "protocol": PROTOCOL, "user": cfg.user, "password": cfg.password.as_str(),
                "host": cfg.host, "port": cfg.port,
            }))
            .map_err(|_| invalid())?,
        );
        worker.send(&request)?;
        let (kind, data) = worker.frame()?;
        let reply: Value = serde_json::from_slice(&data).map_err(|_| invalid())?;
        if kind != b'J'
            || reply["protocol"] != PROTOCOL
            || reply["ok"] != true
            || reply["thin"] != false
        {
            worker.stop();
            return Err(invalid());
        }
        Ok(worker)
    }

    fn send(&self, data: &[u8]) -> Result<(), DriverError> {
        let mut input = self.input.borrow_mut();
        input
            .write_all(data)
            .and_then(|_| input.write_all(b"\n"))
            .and_then(|_| input.flush())
            .map_err(|_| invalid())
    }

    fn frame(&self) -> Result<Frame, DriverError> {
        match self
            .frames
            .recv_timeout(self.timeout.get() + Duration::from_secs(5))
        {
            Ok(Ok(frame)) => Ok(frame),
            _ => {
                self.stop();
                Err(DriverError::Worker {
                    code: 0,
                    timeout: true,
                })
            }
        }
    }

    fn stop(&self) {
        let mut child = self.child.borrow_mut();
        let _ = child.kill();
        let _ = child.wait();
    }

    pub fn set_timeout(&self, timeout: Duration) {
        self.timeout.set(timeout);
    }

    pub fn query(
        &self,
        sql: &str,
        binds: &[Bind],
        max_rows: usize,
        max_bytes: u64,
    ) -> Result<WorkerRows<'_>, DriverError> {
        let binds: Vec<Value> = binds
            .iter()
            .map(|b| match b {
                Bind::Number(n) => json!({"number": n}),
                Bind::Text(s) => json!({"text": s}),
            })
            .collect();
        self.send(
            &serde_json::to_vec(&json!({
                "protocol": PROTOCOL, "operation": "query", "sql": sql, "binds": binds,
                "timeout_ms": self.timeout.get().as_millis(), "max_rows": max_rows,
                "max_bytes": max_bytes,
            }))
            .map_err(|_| invalid())?,
        )?;
        Ok(WorkerRows {
            worker: self,
            buffered: VecDeque::new(),
            done: false,
            sequence: 0,
            rows: 0,
            width: None,
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct WorkerRows<'a> {
    worker: &'a Worker,
    buffered: VecDeque<Vec<Option<String>>>,
    done: bool,
    sequence: u64,
    rows: u64,
    width: Option<usize>,
}

/// One row batch: protocol, consecutive sequence, constant width, text or null.
fn decode_batch(
    data: &[u8],
    sequence: u64,
    width: Option<usize>,
) -> Result<(usize, Rows), DriverError> {
    let value: Value = serde_json::from_slice(data).map_err(|_| invalid())?;
    let batch_width = value["width"].as_u64().ok_or_else(invalid)? as usize;
    let rows = value["rows"].as_array().ok_or_else(invalid)?;
    if value["protocol"] != PROTOCOL
        || value["sequence"].as_u64() != Some(sequence)
        || batch_width == 0
        || width.is_some_and(|w| w != batch_width)
        || rows.is_empty()
        || rows.len() > MAX_BATCH
    {
        return Err(invalid());
    }
    let rows = rows
        .iter()
        .map(|row| {
            let row = row.as_array().filter(|r| r.len() == batch_width);
            row.ok_or_else(invalid)?
                .iter()
                .map(|cell| match cell {
                    Value::Null => Ok(None),
                    Value::String(s) => Ok(Some(s.clone())),
                    _ => Err(invalid()),
                })
                .collect()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((batch_width, rows))
}

impl Iterator for WorkerRows<'_> {
    type Item = Result<Vec<Option<String>>, DriverError>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(row) = self.buffered.pop_front() {
            return Some(Ok(row));
        }
        if self.done {
            return None;
        }
        let result = (|| {
            let (kind, data) = self.worker.frame()?;
            if kind == b'J' {
                let value: Value = serde_json::from_slice(&data).map_err(|_| invalid())?;
                if value["protocol"] != PROTOCOL {
                    return Err(invalid());
                }
                self.done = true;
                if value["ok"] == true {
                    return if value["rows"].as_u64() == Some(self.rows) {
                        Ok(())
                    } else {
                        Err(invalid())
                    };
                }
                if value["fact"] == "value_over_budget" {
                    return Err(DriverError::Source(crate::SourceError::budget(
                        "value_over_budget",
                    )));
                }
                return Err(DriverError::Worker {
                    code: value["code"].as_i64().unwrap_or(0),
                    timeout: value["timeout"].as_bool().unwrap_or(false),
                });
            }
            let (width, rows) = decode_batch(&data, self.sequence, self.width)?;
            self.width = Some(width);
            self.rows += rows.len() as u64;
            self.sequence += 1;
            self.buffered.extend(rows);
            Ok(())
        })();
        if let Err(error) = result {
            if !self.done {
                self.worker.stop();
            }
            self.done = true;
            self.buffered.clear();
            return Some(Err(error));
        }
        self.buffered.pop_front().map(Ok)
    }
}

impl Drop for WorkerRows<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.worker.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell_frame(kind: u8, data: &[u8]) -> String {
        let mut frame = vec![kind];
        frame.extend_from_slice(&(data.len() as u32).to_be_bytes());
        frame.extend_from_slice(data);
        frame.iter().map(|byte| format!("\\{byte:03o}")).collect()
    }

    fn worker(replies: &[(u8, &[u8])]) -> Worker {
        let mut script = format!(
            "read request; printf '{}'; read request; ",
            shell_frame(
                b'J',
                br#"{"protocol":"oo.dm8.driver/v1","ok":true,"thin":false}"#
            )
        );
        for (kind, data) in replies {
            script.push_str(&format!("printf '{}'; ", shell_frame(*kind, data)));
        }
        script.push_str("read request");
        let mut command = Command::new("sh");
        command.args(["-c", &script]);
        let cfg = ConnectConfig::new("127.0.0.1", 5236, "FIXTURE", Zeroizing::new(String::new()));
        Worker::connect(&mut command, &cfg).unwrap()
    }

    #[test]
    fn rows_need_a_matching_completion() {
        let rows = br#"{"protocol":"oo.dm8.driver/v1","sequence":0,"width":2,"rows":[["1",null]]}"#;
        let ok = br#"{"protocol":"oo.dm8.driver/v1","ok":true,"rows":1}"#;
        let w = worker(&[(b'R', rows), (b'J', ok)]);
        let got: Vec<_> = w.query("SELECT 1", &[], 2, 100).unwrap().collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].as_ref().unwrap(), &vec![Some("1".into()), None]);

        let short = br#"{"protocol":"oo.dm8.driver/v1","ok":true,"rows":2}"#;
        let w = worker(&[(b'R', rows), (b'J', short)]);
        let got: Vec<_> = w.query("SELECT 1", &[], 2, 100).unwrap().collect();
        assert!(got[1].is_err());
    }

    #[test]
    fn database_errors_and_budget_overflow_are_distinct() {
        let failed = br#"{"protocol":"oo.dm8.driver/v1","ok":false,"code":0,"timeout":false}"#;
        let w = worker(&[(b'J', failed)]);
        let mut rows = w.query("SELECT 1", &[], 1, 100).unwrap();
        assert!(matches!(
            rows.next(),
            Some(Err(DriverError::Worker {
                code: 0,
                timeout: false
            }))
        ));
        assert!(rows.next().is_none());
        let over = br#"{"protocol":"oo.dm8.driver/v1","ok":false,"fact":"value_over_budget"}"#;
        let w = worker(&[(b'J', over)]);
        let mut rows = w.query("SELECT 1", &[], 1, 100).unwrap();
        assert!(matches!(rows.next(), Some(Err(DriverError::Source(_)))));
    }

    #[test]
    fn batches_require_sequence_width_and_text_cells() {
        let ok = |s: &str| decode_batch(s.as_bytes(), 0, None);
        assert!(
            ok(r#"{"protocol":"oo.dm8.driver/v1","sequence":0,"width":1,"rows":[["a"]]}"#).is_ok()
        );
        for bad in [
            r#"{"protocol":"oo.dm8.driver/v1","sequence":1,"width":1,"rows":[["a"]]}"#,
            r#"{"protocol":"oo.dm8.driver/v1","sequence":0,"width":2,"rows":[["a"]]}"#,
            r#"{"protocol":"oo.dm8.driver/v1","sequence":0,"width":1,"rows":[[1]]}"#,
            r#"{"protocol":"oo.dm8.driver/v1","sequence":0,"width":1,"rows":[]}"#,
            r#"{"protocol":"other","sequence":0,"width":1,"rows":[["a"]]}"#,
        ] {
            assert!(ok(bad).is_err(), "{bad}");
        }
        let one = r#"{"protocol":"oo.dm8.driver/v1","sequence":0,"width":1,"rows":[["a"]]}"#;
        assert!(decode_batch(one.as_bytes(), 0, Some(2)).is_err());
    }

    #[test]
    fn reject_oversized_unknown_and_truncated_frames() {
        for frame in [
            vec![b'R', 2, 0, 0, 1],
            vec![b'A', 0, 0, 0, 0],
            vec![b'J', 0, 0, 0, 2, 1],
        ] {
            assert!(read_frame(&mut &frame[..]).is_err());
        }
        assert_eq!(
            read_frame(&mut &b"J\0\0\0\x02{}"[..]).unwrap(),
            (b'J', b"{}".to_vec())
        );
    }
}
