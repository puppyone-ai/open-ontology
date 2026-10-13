//! OCI supervisor for `oo.source.driver-oracle/v2`.
//!
//! This process does not link an Oracle client. It checks the image id the
//! engine reports, then launches one child on the selected internal network.
//! `admits_import` stays false. v1 `kernel_network` stays pending.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::Array;
use arrow_array::cast::AsArray;
use arrow_array::types::Int16Type;
use arrow_ipc::reader::StreamReader;
use arrow_schema::DataType;
use serde_json::{Map, Value};

use crate::p2_contract::{
    accept_completion, accept_schema, approved_image_id, fingerprint_hex, gate_container_inspect,
    map_source_secret, reject_duplicate_keys, P2_PROTOCOL,
};
use crate::p2_docker::{Attach, Engine, Fill};
use crate::protocol::{TAG_COMPLETE, TAG_IPC, TAG_LINEAGE};

const NETWORK: &str = "oo-dl01w1-04-p2-src";
const NAME_PREFIX: &str = "oo-dl01w1-04-p2-";
const SOCKET: &str = "/var/run/docker.sock";
const CHILD_MEMORY: i64 = 512 * 1024 * 1024;
const CHILD_NANOS: i64 = 500_000_000;
const FRAME_MAX: usize = 16 * 1024 * 1024;

const STATEMENTS: &[&str] = &[
    "oracle:batch-asof",
    "oracle:deadline-select",
    "oracle:cancel-select",
    "oracle:fence",
    "oracle:reject-wide",
    "oracle:reject-ts",
    "oracle:reject-tz",
    "oracle:tamper",
    "oracle:secret",
    "oracle:dml",
];

/// Caller pins for one child container. The secret bytes stay on the mapped file.
#[derive(Clone)]
pub struct OracleLaunch {
    pub image_id: String,
    pub network: String,
    pub container_name: String,
    pub docker_socket: PathBuf,
    pub secret_root: PathBuf,
    pub secret_ref: String,
    pub secret_host_path: PathBuf,
    pub audit_host_path: PathBuf,
    pub audit_dir: PathBuf,
    pub statement_ref: String,
    pub generation: u64,
    pub max_rows: u64,
    pub source_host: String,
    pub decoy_host: String,
    pub cancel_file: Option<PathBuf>,
    pub hold_after_first_frame: bool,
    pub crash_after_scn: bool,
    pub lineage_pad: bool,
    pub receipt_path: Option<PathBuf>,
}

pub struct OracleReceipt {
    pub rows: u64,
    pub schema_fingerprint: String,
    pub generation: u64,
    pub scn: String,
    pub image_id: String,
    pub admits_import: bool,
    pub kernel_network: &'static str,
    pub cancel_api: String,
}

pub enum ExecuteResult {
    Receipt(OracleReceipt),
    Observed(&'static str),
}

pub struct Frame {
    pub tag: u32,
    pub payload: Vec<u8>,
}

pub struct P2Supervisor {
    engine: Engine,
    id: String,
    launch: OracleLaunch,
    image_id: String,
    attach: Option<Attach>,
    stdout_buf: Vec<u8>,
    stderr_buf: Vec<u8>,
    frames: Vec<Frame>,
    eof: bool,
    removed: bool,
}

impl OracleLaunch {
    pub fn from_spec(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > crate::protocol::REQUEST_MAX_BYTES {
            return Err("protocol");
        }
        reject_duplicate_keys(bytes)?;
        let value: Value = serde_json::from_slice(bytes).map_err(|_| "protocol")?;
        let map = value.as_object().ok_or("protocol")?;
        for key in map.keys() {
            if !spec_key(key) {
                return Err("protocol");
            }
        }
        if map.get("protocol").and_then(Value::as_str) != Some(P2_PROTOCOL) {
            return Err("protocol");
        }
        let image_id = required_str(map, "image_id")?;
        if !approved_image_id(&image_id) {
            return Err("image_digest");
        }
        let network = required_str(map, "network")?;
        if network != NETWORK {
            return Err("network_scope");
        }
        let container_name = required_str(map, "container_name")?;
        if !container_name.starts_with(NAME_PREFIX) || !host_ok(&container_name) {
            return Err("inspect");
        }
        let socket = required_str(map, "docker_socket")?;
        if socket != SOCKET {
            return Err("launcher_missing");
        }
        let statement_ref = required_str(map, "statement_ref")?;
        if !STATEMENTS.contains(&statement_ref.as_str()) {
            return Err("read_unsupported");
        }
        let generation = required_u64(map, "generation")?;
        let max_rows = required_u64(map, "max_rows")?;
        if !(1..=8).contains(&max_rows) {
            return Err("budget");
        }
        let source_host = required_str(map, "source_host")?;
        if !host_ok(&source_host) {
            return Err("network_scope");
        }
        let decoy_host = optional_str(map, "decoy_host")?;
        if !decoy_host.is_empty() && (!host_ok(&decoy_host) || decoy_host == source_host) {
            return Err("network_scope");
        }
        if statement_ref == "oracle:fence" && decoy_host.is_empty() {
            return Err("network_scope");
        }
        let hold = optional_bool(map, "hold_after_first_frame")?;
        let crash = optional_bool(map, "crash_after_scn")?;
        let lineage_pad = optional_bool(map, "lineage_pad")?;
        if (hold || crash) && statement_ref != "oracle:batch-asof" {
            return Err("protocol");
        }
        if hold && crash {
            return Err("protocol");
        }
        let secret_root = absolute_path(&required_str(map, "secret_root")?)?;
        let secret_host_path = absolute_path(&required_str(map, "secret_host_path")?)?;
        let audit_host_path = absolute_path(&required_str(map, "audit_host_path")?)?;
        let audit_dir = absolute_path(&required_str(map, "audit_dir")?)?;
        let secret_ref = required_str(map, "secret_ref")?;
        map_source_secret(&secret_root, &secret_ref)?;
        let cancel_file = match map.get("cancel_file") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) if text.is_empty() => None,
            Some(Value::String(text)) => Some(absolute_path(text)?),
            _ => return Err("protocol"),
        };
        let receipt_path = match map.get("receipt_path") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) if text.is_empty() => None,
            Some(Value::String(text)) => Some(absolute_path(text)?),
            _ => return Err("protocol"),
        };
        Ok(Self {
            image_id,
            network,
            container_name,
            docker_socket: PathBuf::from(socket),
            secret_root,
            secret_ref,
            secret_host_path,
            audit_host_path,
            audit_dir,
            statement_ref,
            generation,
            max_rows,
            source_host,
            decoy_host,
            cancel_file,
            hold_after_first_frame: hold,
            crash_after_scn: crash,
            lineage_pad,
            receipt_path,
        })
    }
}

impl P2Supervisor {
    pub fn start(launch: &OracleLaunch) -> Result<Self, &'static str> {
        fs::create_dir_all(&launch.audit_dir).map_err(|_| "io")?;
        let engine = Engine::open(&launch.docker_socket)?;
        let image_doc = engine.confirmed_image_id(&launch.image_id)?;
        let image_id = image_doc
            .get("Id")
            .and_then(|value| value.as_str())
            .ok_or("inspect")?
            .to_string();
        fs::write(
            launch.audit_dir.join("image-inspect.json"),
            serde_json::to_vec_pretty(&image_doc).map_err(|_| "io")?,
        )
        .map_err(|_| "io")?;
        let spec = container_spec(launch, &image_id)?;
        let id = engine.create(&launch.container_name, &spec)?;
        let mut sup = Self {
            engine,
            id,
            launch: launch.clone(),
            image_id,
            attach: None,
            stdout_buf: Vec::new(),
            stderr_buf: Vec::new(),
            frames: Vec::new(),
            eof: false,
            removed: false,
        };
        sup.attach = Some(sup.engine.attach(&sup.id)?);
        sup.engine.start(&sup.id)?;
        let doc = sup.engine.inspect_container(&sup.id)?;
        fs::write(
            sup.launch.audit_dir.join("container-inspect.json"),
            serde_json::to_vec_pretty(&doc).map_err(|_| "io")?,
        )
        .map_err(|_| "io")?;
        gate_container_inspect(&doc, &sup.image_id, &sup.launch.network)?;
        let request = child_request(&sup.launch, &sup.image_id)?;
        sup.attach
            .as_mut()
            .ok_or("io")?
            .write_stdin(&request)?;
        Ok(sup)
    }

    pub fn pull(&mut self) -> Result<Option<Frame>, &'static str> {
        loop {
            if let Some(frame) = take_frame(&mut self.stdout_buf)? {
                return Ok(Some(frame));
            }
            if self.eof {
                return Ok(None);
            }
            let fill = self
                .attach
                .as_mut()
                .ok_or("io")?
                .fill(&mut self.stdout_buf, &mut self.stderr_buf)?;
            match fill {
                Fill::Data => continue,
                Fill::Timeout => return Ok(None),
                Fill::Eof => {
                    self.eof = true;
                    return take_frame(&mut self.stdout_buf);
                }
            }
        }
    }

    pub fn cancel(&mut self) -> Result<(), &'static str> {
        self.engine.kill(&self.id)
    }

    pub fn finish(mut self) -> Result<OracleReceipt, &'static str> {
        self.collect_until(Duration::from_secs(120), false)?;
        let receipt = self.batch_receipt()?;
        self.removed = true;
        let _ = self.engine.remove(&self.id);
        Ok(receipt)
    }
}

impl Drop for P2Supervisor {
    fn drop(&mut self) {
        self.persist();
        if self.removed || self.id.is_empty() {
            return;
        }
        self.attach.take();
        let _ = self.engine.kill(&self.id);
        let _ = self.engine.remove(&self.id);
        self.removed = true;
    }
}

pub fn execute(launch: &OracleLaunch) -> Result<ExecuteResult, &'static str> {
    let mut sup = P2Supervisor::start(launch)?;
    match launch.statement_ref.as_str() {
        "oracle:batch-asof" if launch.crash_after_scn => {
            sup.observe_crash()?;
            Ok(ExecuteResult::Observed("crash"))
        }
        "oracle:batch-asof" if launch.hold_after_first_frame => {
            sup.observe_hold()?;
            Ok(ExecuteResult::Observed("backpressure_held"))
        }
        "oracle:batch-asof" => Ok(ExecuteResult::Receipt(sup.finish()?)),
        "oracle:deadline-select" => {
            sup.observe_deadline()?;
            Err("deadline_observed")
        }
        "oracle:cancel-select" => {
            sup.observe_cancel()?;
            Ok(ExecuteResult::Observed("cancel_observed"))
        }
        "oracle:fence" => {
            sup.observe_fence()?;
            Ok(ExecuteResult::Observed("fence"))
        }
        "oracle:tamper" => {
            sup.observe_tamper()?;
            Err("schema_observed")
        }
        "oracle:reject-wide" | "oracle:reject-ts" | "oracle:reject-tz" => {
            sup.observe_exit_only(4, "precision_loss")?;
            Err("precision_observed")
        }
        "oracle:secret" | "oracle:dml" => {
            sup.observe_exit_only(6, "denied")?;
            Err("denied_observed")
        }
        _ => Err("read_unsupported"),
    }
}

impl P2Supervisor {
    fn persist(&self) {
        let _ = fs::write(self.launch.audit_dir.join("child-stderr.txt"), &self.stderr_buf);
    }

    fn collect_until(&mut self, limit: Duration, stop_on_cancel: bool) -> Result<(), &'static str> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if stop_on_cancel && cancel_ready(&self.launch) {
                self.cancel()?;
                return Err("cancel");
            }
            match self.pull()? {
                Some(frame) => self.frames.push(frame),
                None if self.eof => return Ok(()),
                None => {}
            }
            if self.frames.iter().any(|frame| frame.tag == TAG_COMPLETE) && self.eof {
                return Ok(());
            }
        }
        let _ = self.cancel();
        Err("budget")
    }

    fn batch_receipt(&mut self) -> Result<OracleReceipt, &'static str> {
        let code = self.engine.wait_code(&self.id)?;
        if code != 0 {
            return Err("protocol");
        }
        let mut ipc = None;
        let mut completion = None;
        for frame in &self.frames {
            match frame.tag {
                TAG_IPC if ipc.is_none() => ipc = Some(frame.payload.clone()),
                TAG_LINEAGE => {}
                TAG_COMPLETE if completion.is_none() => {
                    completion = Some(std::str::from_utf8(&frame.payload).map_err(|_| "protocol")?.to_string());
                }
                _ => return Err("protocol"),
            }
        }
        let ipc = ipc.ok_or("protocol")?;
        let completion = completion.ok_or("protocol")?;
        let batch = decode_batch(&ipc)?;
        accept_schema(batch.schema().as_ref())?;
        let fingerprint = fingerprint_hex(self.launch.generation, batch.schema().as_ref())?;
        let accepted = accept_completion(
            &completion,
            &fingerprint,
            &self.image_id,
            self.launch.generation,
        )?;
        if accepted.rows != batch.num_rows() as u64 {
            return Err("budget");
        }
        let ids = batch
            .column(0)
            .as_primitive_opt::<Int16Type>()
            .ok_or("schema")?;
        if ids.is_null(0) || ids.value(0) != 41 {
            return Err("schema");
        }
        fs::write(self.launch.audit_dir.join("batch.arrow"), &ipc).map_err(|_| "io")?;
        Ok(OracleReceipt {
            rows: accepted.rows,
            schema_fingerprint: fingerprint,
            generation: self.launch.generation,
            scn: accepted.scn,
            image_id: self.image_id.clone(),
            admits_import: false,
            kernel_network: "internal",
            cancel_api: accepted.cancel_api,
        })
    }

    fn observe_crash(&mut self) -> Result<(), &'static str> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(120) {
            if self.launch.audit_dir.join("scn-ready").is_file() {
                self.cancel()?;
                let code = self.engine.wait_code(&self.id).map_err(|_| "crash")?;
                if self.frames.iter().any(|frame| frame.tag == TAG_COMPLETE) || code == 0 {
                    return Err("crash");
                }
                return Ok(());
            }
            match self.pull()? {
                Some(frame) => {
                    self.frames.push(frame);
                    return Err("crash");
                }
                None if self.eof => return Err("crash"),
                None => {}
            }
        }
        let _ = self.cancel();
        Err("crash")
    }

    fn observe_hold(&mut self) -> Result<(), &'static str> {
        let started = Instant::now();
        let mut first = false;
        while started.elapsed() < Duration::from_secs(90) && !first {
            match self.pull()? {
                Some(frame) if frame.tag == TAG_IPC => {
                    self.frames.push(frame);
                    first = true;
                }
                Some(_) => return Err("backpressure"),
                None if self.eof => return Err("backpressure"),
                None => {}
            }
        }
        if !first {
            return Err("backpressure");
        }
        thread::sleep(Duration::from_secs(3));
        let flag = self.launch.audit_dir.join("lineage-written");
        if flag.exists() || !self.engine.running(&self.id)? {
            return Err("backpressure");
        }
        self.collect_until(Duration::from_secs(60), false)?;
        Ok(())
    }

    fn observe_deadline(&mut self) -> Result<(), &'static str> {
        self.collect_until(Duration::from_secs(60), false)?;
        if !self.frames.is_empty() {
            return Err("deadline");
        }
        let code = self.engine.wait_code(&self.id)?;
        if code != 3 {
            return Err("deadline");
        }
        let path = self.launch.audit_dir.join("deadline.json");
        let text = fs::read_to_string(&path).map_err(|_| "deadline")?;
        let doc: Value = serde_json::from_str(&text).map_err(|_| "deadline")?;
        if doc.get("ready").and_then(Value::as_bool) != Some(false) {
            return Err("deadline");
        }
        if doc.get("client_timeout").and_then(Value::as_bool) != Some(true) {
            return Err("deadline");
        }
        Ok(())
    }

    fn observe_cancel(&mut self) -> Result<(), &'static str> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(100) {
            if cancel_ready(&self.launch) {
                self.cancel()?;
                let _ = self.engine.wait_code(&self.id);
                if self.frames.iter().any(|frame| frame.tag == TAG_COMPLETE) {
                    return Err("cancel");
                }
                return Ok(());
            }
            match self.pull()? {
                Some(frame) => {
                    if frame.tag == TAG_COMPLETE {
                        return Err("cancel");
                    }
                    self.frames.push(frame);
                }
                None if self.eof => return Err("cancel"),
                None => {}
            }
        }
        let _ = self.cancel();
        Err("cancel")
    }

    fn observe_fence(&mut self) -> Result<(), &'static str> {
        self.collect_until(Duration::from_secs(60), false)?;
        if !self.frames.is_empty() {
            return Err("network_scope");
        }
        let code = self.engine.wait_code(&self.id)?;
        if code != 0 {
            return Err("network_scope");
        }
        let text = fs::read_to_string(self.launch.audit_dir.join("fence.json")).map_err(|_| "network_scope")?;
        let doc: Value = serde_json::from_str(&text).map_err(|_| "network_scope")?;
        if doc.get("import_receipt").and_then(Value::as_bool) != Some(false) {
            return Err("network_scope");
        }
        if doc.get("allowed_ok").and_then(Value::as_bool) != Some(true) {
            return Err("network_scope");
        }
        if doc.get("forbidden_ok").and_then(Value::as_bool) != Some(false) {
            return Err("network_scope");
        }
        Ok(())
    }

    fn observe_tamper(&mut self) -> Result<(), &'static str> {
        self.collect_until(Duration::from_secs(120), false)?;
        let ipc = self
            .frames
            .iter()
            .find(|frame| frame.tag == TAG_IPC)
            .ok_or("schema")?;
        let batch = decode_batch(&ipc.payload)?;
        if accept_schema(batch.schema().as_ref()).is_ok() {
            return Err("schema");
        }
        if !batch
            .schema()
            .fields()
            .iter()
            .any(|field| matches!(field.data_type(), DataType::Float64))
        {
            return Err("schema");
        }
        Ok(())
    }

    fn observe_exit_only(&mut self, expect: i64, token: &'static str) -> Result<(), &'static str> {
        self.collect_until(Duration::from_secs(90), false).map_err(|_| token)?;
        if !self.frames.is_empty() {
            return Err(token);
        }
        let code = self.engine.wait_code(&self.id).map_err(|_| token)?;
        if code != expect {
            return Err(token);
        }
        Ok(())
    }
}

fn decode_batch(payload: &[u8]) -> Result<arrow_array::RecordBatch, &'static str> {
    let cursor = io::Cursor::new(payload);
    let mut reader = StreamReader::try_new(cursor, None).map_err(|_| "schema")?;
    let batch = reader.next().ok_or("schema")?.map_err(|_| "schema")?;
    if reader.next().is_some() {
        return Err("schema");
    }
    Ok(batch)
}

fn take_frame(buf: &mut Vec<u8>) -> Result<Option<Frame>, &'static str> {
    if buf.len() < 8 {
        return Ok(None);
    }
    let tag = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    if len == 0 || len > FRAME_MAX {
        return Err("protocol");
    }
    if buf.len() < 8 + len {
        return Ok(None);
    }
    if tag != TAG_IPC && tag != TAG_LINEAGE && tag != TAG_COMPLETE {
        return Err("protocol");
    }
    let payload = buf[8..8 + len].to_vec();
    buf.drain(..8 + len);
    Ok(Some(Frame { tag, payload }))
}

fn cancel_ready(launch: &OracleLaunch) -> bool {
    launch
        .cancel_file
        .as_ref()
        .is_some_and(|path| path.is_file())
}

fn child_request(launch: &OracleLaunch, image_id: &str) -> Result<Vec<u8>, &'static str> {
    let body = serde_json::json!({
        "protocol": P2_PROTOCOL,
        "statement_ref": launch.statement_ref,
        "generation": launch.generation,
        "max_rows": launch.max_rows,
        "source_host": launch.source_host,
        "image_id": image_id,
        "decoy_host": launch.decoy_host,
        "lineage_pad": launch.lineage_pad,
    });
    let mut bytes = serde_json::to_vec(&body).map_err(|_| "protocol")?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn container_spec(launch: &OracleLaunch, image_id: &str) -> Result<Value, &'static str> {
    let secret = path_text(&launch.secret_host_path)?;
    let audit = path_text(&launch.audit_host_path)?;
    Ok(serde_json::json!({
        "Image": image_id,
        "User": "65534:65534",
        "Env": ["HOME=/tmp", "TZ=UTC"],
        "Entrypoint": ["/oo-oracle-p2-child"],
        "OpenStdin": true,
        "StdinOnce": false,
        "AttachStdin": true,
        "AttachStdout": true,
        "AttachStderr": true,
        "Tty": false,
        "Labels": {
            "oo.project": "open-ontology",
            "oo.owner": "dl01w1-04",
            "oo.session": "992aa5d3-6848-4dd9-aff1-d4d31a099983",
            "oo.kind": "oracle-worker",
            "oo.task": "DL01-W1-04"
        },
        "HostConfig": {
            "NetworkMode": launch.network,
            "CapDrop": ["ALL"],
            "Privileged": false,
            "ReadonlyRootfs": true,
            "Memory": CHILD_MEMORY,
            "MemorySwap": CHILD_MEMORY,
            "NanoCpus": CHILD_NANOS,
            "PidsLimit": 64,
            "SecurityOpt": ["no-new-privileges:true"],
            "Mounts": [
                {
                    "Type": "bind",
                    "Source": secret,
                    "Target": "/run/secrets/source-password",
                    "ReadOnly": true
                },
                {
                    "Type": "bind",
                    "Source": audit,
                    "Target": "/audit",
                    "ReadOnly": false
                }
            ],
            "Tmpfs": {"/tmp": "rw,nosuid,nodev,noexec,size=67108864"}
        }
    }))
}

fn path_text(path: &Path) -> Result<String, &'static str> {
    let text = path.to_str().ok_or("inspect")?;
    if text.contains(':') {
        return Err("inspect");
    }
    Ok(text.to_string())
}

fn spec_key(key: &str) -> bool {
    matches!(
        key,
        "protocol"
            | "statement_ref"
            | "image_id"
            | "network"
            | "container_name"
            | "docker_socket"
            | "secret_root"
            | "secret_ref"
            | "secret_host_path"
            | "audit_host_path"
            | "audit_dir"
            | "generation"
            | "max_rows"
            | "source_host"
            | "decoy_host"
            | "cancel_file"
            | "hold_after_first_frame"
            | "crash_after_scn"
            | "lineage_pad"
            | "receipt_path"
    )
}

fn required_str(map: &Map<String, Value>, key: &str) -> Result<String, &'static str> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or("protocol")
}

fn optional_str(map: &Map<String, Value>, key: &str) -> Result<String, &'static str> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err("protocol"),
    }
}

fn required_u64(map: &Map<String, Value>, key: &str) -> Result<u64, &'static str> {
    map.get(key).and_then(Value::as_u64).ok_or("protocol")
}

fn optional_bool(map: &Map<String, Value>, key: &str) -> Result<bool, &'static str> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err("protocol"),
    }
}

fn absolute_path(text: &str) -> Result<PathBuf, &'static str> {
    if !text.starts_with('/') || text.contains(':') || text.contains('\0') {
        return Err("protocol");
    }
    Ok(PathBuf::from(text))
}

fn host_ok(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_lowercase() || ch.is_ascii_digit() => {}
        _ => return false,
    }
    text.len() <= 63
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

pub fn write_batch_receipt(path: &Path, receipt: &OracleReceipt) -> Result<(), &'static str> {
    if receipt.admits_import || receipt.kernel_network != "internal" {
        return Err("protocol");
    }
    if receipt.cancel_api != "absent" && receipt.cancel_api != "oci_marker_interrupt" {
        return Err("protocol");
    }
    if path.exists() {
        return Err("protocol");
    }
    let body = serde_json::json!({
        "protocol": P2_PROTOCOL,
        "rows": receipt.rows,
        "batches": 1,
        "schema_fingerprint": receipt.schema_fingerprint,
        "generation": receipt.generation,
        "scn": receipt.scn,
        "image_id": receipt.image_id,
        "admits_import": false,
        "kernel_network": "internal",
        "published": false,
        "pg_run_complete": false,
        "fallback": false,
        "vendor_library_loaded": false,
        "cancel_api": &receipt.cancel_api,
    });
    let text = serde_json::to_vec_pretty(&body).map_err(|_| "io")?;
    let tmp = path.with_extension("json.partial");
    {
        let mut file = fs::File::create(&tmp).map_err(|_| "io")?;
        file.write_all(&text).map_err(|_| "io")?;
        file.write_all(b"\n").map_err(|_| "io")?;
        file.sync_all().map_err(|_| "io")?;
    }
    fs::rename(&tmp, path).map_err(|_| "io")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::OracleLaunch;

    #[test]
    fn spec_rejects_a_secret_byte_key() {
        let err = OracleLaunch::from_spec(br#"{"protocol":"oo.source.driver-oracle/v2","password":"x"}"#);
        assert!(matches!(err, Err("protocol")));
    }

    #[test]
    fn spec_rejects_duplicate_keys_and_foreign_network() {
        let dup = OracleLaunch::from_spec(
            br#"{"protocol":"oo.source.driver-oracle/v2","protocol":"oo.source.driver-oracle/v2"}"#,
        );
        assert!(matches!(dup, Err("duplicate_key")));
        let image = format!("sha256:{}", "ab".repeat(32));
        let body = format!(
            r#"{{"protocol":"oo.source.driver-oracle/v2","image_id":"{image}","network":"bridge"}}"#
        );
        assert!(matches!(
            OracleLaunch::from_spec(body.as_bytes()),
            Err("network_scope")
        ));
    }
}
