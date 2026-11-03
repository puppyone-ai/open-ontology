//! Source-only P2 driver transport. The trusted caller supplies a pinned worker
//! command; credentials travel over stdin, never argv or environment.
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::Duration;

use arrow_array::{Array, StringArray};
use arrow_ipc::reader::StreamReader;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::engine::OracleConnectConfig;
use super::keyset::Bind;

const PROTOCOL: &str = "oo.oracle.driver/v1";
const MAX_FRAME: usize = 32 * 1024 * 1024;

#[derive(Debug)]
pub enum DriverError {
    Thin(oracledb::Error),
    Worker { code: usize, timeout: bool },
}

impl From<oracledb::Error> for DriverError {
    fn from(value: oracledb::Error) -> Self {
        Self::Thin(value)
    }
}

fn invalid() -> DriverError {
    DriverError::Worker {
        code: 0,
        timeout: false,
    }
}

type Frame = (u8, Vec<u8>);

fn read_frame(reader: &mut impl Read) -> std::io::Result<Frame> {
    let mut header = [0; 5];
    reader.read_exact(&mut header)?;
    let size = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
    if !matches!(header[0], b'J' | b'A') || size > MAX_FRAME {
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
    pub fn connect(command: &mut Command, cfg: &OracleConnectConfig) -> Result<Self, DriverError> {
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
                "dsn": format!("{}:{}/{}", cfg.host, cfg.port, cfg.service),
            }))
            .map_err(|_| invalid())?,
        );
        worker.send(&request)?;
        let (kind, data) = worker.frame()?;
        let reply: Value = serde_json::from_slice(&data).map_err(|_| invalid())?;
        if kind != b'J' || reply["ok"] != true || reply["thin"] != false {
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
                "protocol": PROTOCOL, "sql": sql, "binds": binds,
                "timeout_ms": self.timeout.get().as_millis(), "max_rows": max_rows,
            }))
            .map_err(|_| invalid())?,
        )?;
        Ok(WorkerRows {
            worker: self,
            buffered: VecDeque::new(),
            done: false,
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
                self.done = true;
                let value: Value = serde_json::from_slice(&data).map_err(|_| invalid())?;
                return if value["ok"] == true {
                    Ok(())
                } else {
                    Err(DriverError::Worker {
                        code: value["code"].as_u64().unwrap_or(0) as usize,
                        timeout: value["timeout"].as_bool().unwrap_or(false),
                    })
                };
            }
            let stream =
                StreamReader::try_new(std::io::Cursor::new(data), None).map_err(|_| invalid())?;
            for batch in stream {
                let batch = batch.map_err(|_| invalid())?;
                if batch.num_rows() > 512 || batch.num_columns() == 0 {
                    return Err(invalid());
                }
                for i in 0..batch.num_rows() {
                    let row = batch
                        .columns()
                        .iter()
                        .map(|column| {
                            let column = column
                                .as_any()
                                .downcast_ref::<StringArray>()
                                .ok_or_else(invalid)?;
                            Ok((!column.is_null(i)).then(|| column.value(i).to_owned()))
                        })
                        .collect::<Result<Vec<_>, DriverError>>()?;
                    self.buffered.push_back(row);
                }
            }
            if self.buffered.is_empty() {
                return Err(invalid());
            }
            Ok(())
        })();
        if let Err(error) = result {
            if !self.done {
                self.worker.stop();
            }
            self.done = true;
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

    fn shell_frame(data: &[u8]) -> String {
        let mut frame = vec![b'J'];
        frame.extend_from_slice(&(data.len() as u32).to_be_bytes());
        frame.extend_from_slice(data);
        frame.iter().map(|byte| format!("\\{byte:03o}")).collect()
    }

    #[test]
    fn preserve_database_error_and_reject_missing_completion() {
        for reply in [
            Some(br#"{"ok":false,"code":1555,"timeout":false}"#.as_slice()),
            None,
        ] {
            let mut command = Command::new("sh");
            let script = format!(
                "read request; printf '{}'; read request; {}",
                shell_frame(br#"{"ok":true,"thin":false}"#),
                reply
                    .map(|r| format!("printf '{}'", shell_frame(r)))
                    .unwrap_or("exit 0".into()),
            );
            command.args(["-c", &script]);
            let cfg = OracleConnectConfig::new(
                "localhost",
                1521,
                "XE",
                "FIXTURE",
                Zeroizing::new(String::new()),
            );
            let worker = Worker::connect(&mut command, &cfg).unwrap();
            let mut rows = worker.query("SELECT 1 FROM dual", &[], 1).unwrap();
            let error = rows.next().unwrap().unwrap_err();
            if reply.is_some() {
                assert!(matches!(
                    error,
                    DriverError::Worker {
                        code: 1555,
                        timeout: false
                    }
                ));
            } else {
                assert!(matches!(error, DriverError::Worker { code: 0, .. }));
            }
            assert!(rows.next().is_none());
        }
    }

    #[test]
    fn reject_oversized_unknown_and_truncated_frames() {
        for frame in [
            vec![b'A', 2, 0, 0, 1],
            vec![b'X', 0, 0, 0, 0],
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
