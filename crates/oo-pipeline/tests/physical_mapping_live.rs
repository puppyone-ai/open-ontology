//! Live PostgreSQL checks for physical-mapping/v1.
//!
//! The ignored test speaks the PostgreSQL wire protocol to one private
//! server. Register and lookup use the extended protocol. Migration files are
//! static text. This test does not copy user rows and it is not the Loader.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Barrier;
use std::thread;
use std::time::Duration;

use oo_pipeline::physical_mapping::{
    plan_physical_mapping, quote_physical_name, Level, MappingRequest, PhysicalPlan, LOOKUP_SQL,
    REGISTER_SQL,
};

const BOOTSTRAP: &str = include_str!("../migrations/0000_bootstrap_roles.sql");
const EXPAND: &str = include_str!("../migrations/0001_target_fence_expand.sql");
const MAPPING: &str = include_str!("../migrations/0004_physical_mapping.sql");

const HASH_0000: &str = "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208";
const HASH_0001: &str = "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9";
const HASH_0002: &str = "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512";
const HASH_0003: &str = "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3";
const SEGMENT: &str = include_str!("../migrations/0002_target_segment_null_expand.sql");
const PREFLIGHT: &str = include_str!("../migrations/0003_target_segment_preflight_expand.sql");

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const A200_SHA: &str = "c2a908d98f5df987ade41b5fce213067efbcc21ef2240212a41e54b5e7c28ae5";
const GOLDEN_DIGEST: &str = "d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398";
const LIE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

const OID_BYTEA: i32 = 17;
const OID_INT8: i32 = 20;
const OID_TEXT: i32 = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Apply,
    Skip,
    Mismatch,
}

fn migration_route(stored: Option<&str>, file_hash: &str) -> Route {
    match stored {
        None => Route::Apply,
        Some(value) if value == file_hash => Route::Skip,
        Some(_) => Route::Mismatch,
    }
}

fn sha256_hex(message: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bits = u64::try_from(message.len()).expect("message length") * 8;
    let mut data = message.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bits.to_be_bytes());
    for chunk in data.chunks(64) {
        let mut words = [0u32; 64];
        for (index, block) in chunk.chunks(4).enumerate() {
            words[index] = u32::from_be_bytes(block.try_into().expect("sha word"));
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut hex = String::with_capacity(64);
    for word in state {
        hex.push_str(&format!("{word:08x}"));
    }
    hex
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn q_utf8(original: &str) -> String {
    format!("q_{}", hex_encode(original.as_bytes()))
}

#[test]
fn sha256_and_frozen_migration_bytes() {
    assert_eq!(sha256_hex(b""), EMPTY_SHA);
    assert_eq!(sha256_hex(b"abc"), ABC_SHA);
    assert_eq!(sha256_hex(&[b'a'; 200]), A200_SHA);
    assert_eq!(sha256_hex(BOOTSTRAP.as_bytes()), HASH_0000);
    assert_eq!(sha256_hex(EXPAND.as_bytes()), HASH_0001);
    assert_eq!(sha256_hex(SEGMENT.as_bytes()), HASH_0002);
    assert_eq!(sha256_hex(PREFLIGHT.as_bytes()), HASH_0003);
    assert_eq!(migration_route(None, "abc"), Route::Apply);
    assert_eq!(migration_route(Some("abc"), "abc"), Route::Skip);
    assert_eq!(migration_route(Some("zzz"), "abc"), Route::Mismatch);
}

struct Lab {
    host: String,
    port: u16,
    evidence: PathBuf,
}

impl Lab {
    fn note(&self, line: &str) {
        println!("{line}");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.evidence.join("cases.log"))
            .expect("cases.log");
        writeln!(file, "{line}").expect("cases.log");
    }

    fn write(&self, name: &str, body: &str) {
        fs::write(self.evidence.join(name), body).unwrap_or_else(|error| panic!("write {name}: {error}"));
    }

    fn connect(&self, user: &str, db: &str) -> Conn {
        let mut conn = Conn::connect(&self.host, self.port, user, db);
        let answer = conn.query("SET statement_timeout = '60s'; SET lock_timeout = '15s';");
        assert!(answer.code.is_none(), "limits {}", answer.message.unwrap_or_default());
        conn
    }
}

#[derive(Debug)]
struct Answer {
    rows: Vec<Vec<String>>,
    code: Option<String>,
    message: Option<String>,
    status: u8,
}

struct Param {
    oid: i32,
    format: i16,
    value: Option<Vec<u8>>,
}

fn text_param(value: &str) -> Param {
    Param {
        oid: OID_TEXT,
        format: 0,
        value: Some(value.as_bytes().to_vec()),
    }
}

fn int8_param(value: i64) -> Param {
    Param {
        oid: OID_INT8,
        format: 0,
        value: Some(value.to_string().into_bytes()),
    }
}

fn bytea_param(value: &[u8]) -> Param {
    Param {
        oid: OID_BYTEA,
        format: 1,
        value: Some(value.to_vec()),
    }
}

fn null_bytea() -> Param {
    Param {
        oid: OID_BYTEA,
        format: 1,
        value: None,
    }
}

struct Conn {
    stream: TcpStream,
}

impl Conn {
    fn connect(host: &str, port: u16, user: &str, db: &str) -> Self {
        let stream = TcpStream::connect((host, port))
            .unwrap_or_else(|error| panic!("connect {host}:{port} {db}: {error}"));
        stream.set_nodelay(true).expect("nodelay");
        let timeout = Some(Duration::from_secs(60));
        stream.set_read_timeout(timeout).expect("read timeout");
        stream.set_write_timeout(timeout).expect("write timeout");
        let mut conn = Self { stream };
        conn.startup(user, db);
        conn
    }

    fn startup(&mut self, user: &str, db: &str) {
        let mut payload = Vec::new();
        payload.extend_from_slice(&196_608u32.to_be_bytes());
        for (key, value) in [("user", user), ("database", db), ("client_encoding", "UTF8")] {
            payload.extend_from_slice(key.as_bytes());
            payload.push(0);
            payload.extend_from_slice(value.as_bytes());
            payload.push(0);
        }
        payload.push(0);
        let len = u32::try_from(payload.len() + 4).expect("startup len");
        self.stream.write_all(&len.to_be_bytes()).expect("startup len");
        self.stream.write_all(&payload).expect("startup");
        let mut authed = false;
        loop {
            let (tag, body) = self.read_message();
            match tag {
                b'R' => {
                    let kind = u32::from_be_bytes(body[..4].try_into().expect("auth kind"));
                    assert_eq!(kind, 0, "fixture auth is trust, got auth kind {kind}");
                    authed = true;
                }
                b'Z' => {
                    assert!(authed, "ready before authentication");
                    return;
                }
                b'E' => {
                    let (code, message) = error_fields(&body);
                    panic!("startup {code}: {message}");
                }
                b'S' | b'K' | b'N' => {}
                other => panic!("startup tag {}", other as char),
            }
        }
    }

    fn query(&mut self, sql: &str) -> Answer {
        let mut payload = Vec::with_capacity(sql.len() + 1);
        payload.extend_from_slice(sql.as_bytes());
        payload.push(0);
        self.write_msg(b'Q', &payload);
        self.read_until_ready()
    }

    fn extended(&mut self, sql: &str, params: &[Param]) -> Answer {
        let mut parse = Vec::new();
        parse.push(0);
        parse.extend_from_slice(sql.as_bytes());
        parse.push(0);
        let count = u16::try_from(params.len()).expect("param count");
        parse.extend_from_slice(&count.to_be_bytes());
        for param in params {
            parse.extend_from_slice(&param.oid.to_be_bytes());
        }
        self.write_msg(b'P', &parse);

        let mut bind = Vec::new();
        bind.push(0);
        bind.push(0);
        bind.extend_from_slice(&count.to_be_bytes());
        for param in params {
            bind.extend_from_slice(&param.format.to_be_bytes());
        }
        bind.extend_from_slice(&count.to_be_bytes());
        for param in params {
            match &param.value {
                None => bind.extend_from_slice(&(-1i32).to_be_bytes()),
                Some(bytes) => {
                    let len = i32::try_from(bytes.len()).expect("param len");
                    bind.extend_from_slice(&len.to_be_bytes());
                    bind.extend_from_slice(bytes);
                }
            }
        }
        bind.extend_from_slice(&1u16.to_be_bytes());
        bind.extend_from_slice(&0i16.to_be_bytes());
        self.write_msg(b'B', &bind);

        let mut execute = Vec::new();
        execute.push(0);
        execute.extend_from_slice(&0u32.to_be_bytes());
        self.write_msg(b'E', &execute);
        self.write_msg(b'S', &[]);
        self.read_until_ready()
    }

    fn write_msg(&mut self, tag: u8, body: &[u8]) {
        let len = u32::try_from(body.len() + 4).expect("message len");
        self.stream.write_all(&[tag]).expect("tag");
        self.stream.write_all(&len.to_be_bytes()).expect("len");
        self.stream.write_all(body).expect("body");
        self.stream.flush().expect("flush");
    }

    fn read_until_ready(&mut self) -> Answer {
        let mut rows = Vec::new();
        let mut code = None;
        let mut message = None;
        loop {
            let (tag, body) = self.read_message();
            match tag {
                b'Z' => {
                    return Answer {
                        rows,
                        code,
                        message,
                        status: body.first().copied().unwrap_or(b'?'),
                    };
                }
                b'E' => {
                    let (found, text) = error_fields(&body);
                    code = Some(found);
                    message = Some(text);
                }
                b'D' => rows.push(data_row(&body)),
                b'T' | b'C' | b'N' | b'S' | b'A' | b'I' | b'n' | b'1' | b'2' | b'3' => {}
                other => panic!("response tag {}", other as char),
            }
        }
    }

    fn read_message(&mut self) -> (u8, Vec<u8>) {
        let mut tag = [0u8; 1];
        self.stream.read_exact(&mut tag).expect("message tag");
        let mut len_bytes = [0u8; 4];
        self.stream.read_exact(&mut len_bytes).expect("message length");
        let len = usize::try_from(u32::from_be_bytes(len_bytes)).expect("length");
        assert!((4..=32 * 1024 * 1024).contains(&len), "message length {len}");
        let mut body = vec![0u8; len - 4];
        self.stream.read_exact(&mut body).expect("message body");
        (tag[0], body)
    }
}

fn error_fields(body: &[u8]) -> (String, String) {
    let mut code = String::new();
    let mut message = String::new();
    let mut cursor = 0;
    while cursor < body.len() {
        let kind = body[cursor];
        cursor += 1;
        if kind == 0 {
            break;
        }
        let end = body[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .expect("error field terminator");
        let text = String::from_utf8(body[cursor..cursor + end].to_vec()).expect("error field utf8");
        cursor += end + 1;
        match kind {
            b'C' => code = text,
            b'M' => message = text,
            _ => {}
        }
    }
    (code, message)
}

fn data_row(body: &[u8]) -> Vec<String> {
    assert!(body.len() >= 2, "data row");
    let count = u16::from_be_bytes(body[0..2].try_into().expect("column count"));
    let mut cursor = 2;
    let mut cells = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let len = i32::from_be_bytes(body[cursor..cursor + 4].try_into().expect("cell length"));
        cursor += 4;
        if len < 0 {
            cells.push(String::new());
            continue;
        }
        let take = usize::try_from(len).expect("cell");
        let text = String::from_utf8(body[cursor..cursor + take].to_vec()).expect("cell utf8");
        cursor += take;
        cells.push(text);
    }
    cells
}

fn tx_begin(conn: &mut Conn) {
    let answer = conn.query("BEGIN;");
    assert!(answer.code.is_none(), "begin {}", answer.message.unwrap_or_default());
}

fn tx_commit(conn: &mut Conn) {
    let answer = conn.query("COMMIT;");
    assert!(answer.code.is_none(), "commit {}", answer.message.unwrap_or_default());
}

fn tx_rollback(conn: &mut Conn) {
    let answer = conn.query("ROLLBACK;");
    assert!(answer.code.is_none(), "rollback {}", answer.message.unwrap_or_default());
}

fn sole(answer: &Answer, label: &str) -> String {
    assert!(
        answer.code.is_none(),
        "{label} {}: {}",
        answer.code.as_deref().unwrap_or(""),
        answer.message.as_deref().unwrap_or("")
    );
    assert_eq!(answer.rows.len(), 1, "{label} rows");
    assert_eq!(answer.rows[0].len(), 1, "{label} cols");
    answer.rows[0][0].clone()
}

fn sole_row(answer: &Answer, label: &str) -> Vec<String> {
    assert!(
        answer.code.is_none(),
        "{label} {}: {}",
        answer.code.as_deref().unwrap_or(""),
        answer.message.as_deref().unwrap_or("")
    );
    assert_eq!(answer.rows.len(), 1, "{label} rows");
    answer.rows[0].clone()
}

fn expect_code(conn: &mut Conn, answer: &Answer, code: &str, marker: Option<&str>) {
    assert_eq!(
        answer.code.as_deref(),
        Some(code),
        "{}",
        answer.message.as_deref().unwrap_or("")
    );
    if let Some(marker) = marker {
        let message = answer.message.as_deref().unwrap_or("");
        assert!(!message.contains(marker), "server message echoed the closed input");
    }
    if answer.status == b'E' {
        tx_rollback(conn);
    }
}

fn assert_physical(name: &str) {
    assert_eq!(name.len(), 63);
    assert!(name.starts_with("pn_") || name.starts_with("pt_") || name.starts_with("pc_"));
    assert!(name.as_bytes()[3..].iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)));
    assert_eq!(quote_physical_name(name).expect("quote"), format!("\"{name}\""));
}

#[derive(Clone)]
struct Reg {
    source: String,
    config: String,
    generation: i64,
    level: String,
    namespace: String,
    table: String,
    column: String,
    digest: Vec<u8>,
    level_byte: String,
}

impl Reg {
    fn from_plan(plan: &PhysicalPlan) -> Self {
        let level_byte = match plan.level {
            Level::Namespace => "n",
            Level::Table => "t",
            Level::Column => "c",
        };
        Self {
            source: plan.source.clone(),
            config: plan.config_digest.clone(),
            generation: plan.run_generation,
            level: plan.level.as_str().to_string(),
            namespace: plan.logical_namespace.clone(),
            table: plan.logical_table.clone(),
            column: plan.logical_column.clone(),
            digest: plan.identity_digest.to_vec(),
            level_byte: level_byte.to_string(),
        }
    }

    fn params(&self) -> Vec<Param> {
        vec![
            text_param(&self.source),
            text_param(&self.config),
            int8_param(self.generation),
            text_param(&self.level),
            text_param(&self.namespace),
            text_param(&self.table),
            text_param(&self.column),
            bytea_param(&self.digest),
        ]
    }

    fn lookup_params(&self) -> Vec<Param> {
        self.params().into_iter().take(7).collect()
    }

    fn inspect_params(&self) -> Vec<Param> {
        vec![
            text_param(&self.source),
            text_param(&self.config),
            int8_param(self.generation),
            text_param(&self.level_byte),
            text_param(&self.namespace),
            text_param(&self.table),
            text_param(&self.column),
        ]
    }
}

fn namespace_plan(source: &str, config: &str, generation: i64, namespace: &str) -> PhysicalPlan {
    plan_physical_mapping(&MappingRequest {
        source: source.to_string(),
        config_digest: config.to_string(),
        run_generation: generation,
        level: Level::Namespace,
        logical_namespace: namespace.to_string(),
        logical_table: String::new(),
        logical_column: String::new(),
    })
    .unwrap_or_else(|error| panic!("plan {source}/{namespace}: {error}"))
}

fn config_ab() -> String {
    "ab".repeat(32)
}

fn config_cd() -> String {
    "cd".repeat(32)
}

const INSPECT_SQL: &str = "SELECT octet_length(physical_name)::text, octet_length(logical_namespace)::text, encode(identity_digest, 'hex'), physical_name, registered_at::text, scope_namespace_physical, coalesce(scope_table_physical, '') FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2 AND run_generation = $3 AND level = $4 AND logical_namespace = $5 AND logical_table = $6 AND logical_column = $7";

fn register_commit(conn: &mut Conn, reg: &Reg) -> String {
    tx_begin(conn);
    let answer = conn.extended(REGISTER_SQL, &reg.params());
    let outcome = sole(&answer, "register");
    tx_commit(conn);
    outcome
}

fn lookup_one(conn: &mut Conn, reg: &Reg) -> String {
    let answer = conn.extended(LOOKUP_SQL, &reg.lookup_params());
    sole(&answer, "lookup")
}

fn install(conn: &mut Conn, file_hash: &str) {
    assert_eq!(file_hash.len(), 64);
    assert!(file_hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    let begun = conn.query(
        "BEGIN; SET LOCAL lock_timeout = '15s'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605);",
    );
    assert!(begun.code.is_none(), "migration begin {}", begun.message.unwrap_or_default());
    let bootstrap = conn.query(BOOTSTRAP);
    assert!(bootstrap.code.is_none(), "0000 {}", bootstrap.message.unwrap_or_default());
    let role = conn.query("SET ROLE oo_target_owner;");
    assert!(role.code.is_none(), "set role {}", role.message.unwrap_or_default());
    let expand = conn.query(EXPAND);
    assert!(expand.code.is_none(), "0001 {}", expand.message.unwrap_or_default());
    let locked = conn.query("SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;");
    assert!(locked.code.is_none(), "migration lock {}", locked.message.unwrap_or_default());
    let mapping = conn.query(MAPPING);
    assert!(mapping.code.is_none(), "0004 {}", mapping.message.unwrap_or_default());
    let marker = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('physical-mapping/v1', '{file_hash}', 'expand', '0004_physical_mapping.sql');"
    );
    let booked = conn.query(&marker);
    assert!(booked.code.is_none(), "marker {}", booked.message.unwrap_or_default());
    let done = conn.query("RESET ROLE; COMMIT;");
    assert!(done.code.is_none(), "migration commit {}", done.message.unwrap_or_default());
}

fn marker(conn: &mut Conn) -> (String, String) {
    let answer = conn.query(
        "SELECT checksum, applied_at::text FROM _oo.schema_migrations WHERE version = 'physical-mapping/v1';",
    );
    let row = sole_row(&answer, "marker");
    (row[0].clone(), row[1].clone())
}

#[test]
#[ignore = "requires OO_PHYSICAL_MAPPING_HOST and one private PostgreSQL 17.10"]
fn physical_mapping_live() {
    let host = env::var("OO_PHYSICAL_MAPPING_HOST").expect("OO_PHYSICAL_MAPPING_HOST");
    let port = env::var("OO_PHYSICAL_MAPPING_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5432);
    let evidence = PathBuf::from(env::var("OO_PHYSICAL_MAPPING_EVIDENCE").expect("OO_PHYSICAL_MAPPING_EVIDENCE"));
    fs::create_dir_all(&evidence).expect("evidence dir");
    let lab = Lab { host, port, evidence };
    let rustc_out = Command::new("rustc").arg("--version").output().expect("rustc");
    let rustc = String::from_utf8(rustc_out.stdout).expect("rustc utf8");
    assert!(rustc.starts_with("rustc 1.94.0 "), "{rustc}");
    lab.write("rustc-version.txt", &rustc);
    lab.note(rustc.trim());

    let mut admin = lab.connect("postgres", "postgres");
    let version = sole(&admin.query("SHOW server_version;"), "server_version");
    assert!(version.starts_with("17.10"), "{version}");
    lab.write("server-version.txt", &version);
    lab.note(&format!("server_version={version}"));
    let created = admin.query(
        "CREATE DATABASE oo_pm WITH TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';",
    );
    assert!(created.code.is_none(), "create database {}", created.message.unwrap_or_default());
    drop(admin);

    let mut owner = lab.connect("postgres", "oo_pm");
    let locale = sole(
        &owner.query(
            "SELECT pg_catalog.pg_encoding_to_char(encoding) || '|' || datlocprovider::text || '|' || coalesce(datlocale, '') FROM pg_catalog.pg_database WHERE datname = current_database();",
        ),
        "locale",
    );
    assert_eq!(locale, "UTF8|b|C.UTF-8");
    lab.note(&format!("locale={locale}"));
    let file_hash = sha256_hex(MAPPING.as_bytes());
    lab.write("mapping-file.sha256", &file_hash);
    install(&mut owner, &file_hash);
    let (stored_hash, applied_at) = marker(&mut owner);
    assert_eq!(stored_hash, file_hash);
    lab.note(&format!("marker={stored_hash} applied_at={applied_at}"));

    let empty = sole(&owner.query("SELECT encode(_oo.pm_sha256(decode('', 'hex')), 'hex');"), "sha-empty");
    let abc = sole(&owner.query("SELECT encode(_oo.pm_sha256(convert_to('abc', 'UTF8')), 'hex');"), "sha-abc");
    let many = sole(
        &owner.query("SELECT encode(_oo.pm_sha256(convert_to(repeat('a', 200), 'UTF8')), 'hex');"),
        "sha-a200",
    );
    assert_eq!(empty, EMPTY_SHA);
    assert_eq!(abc, ABC_SHA);
    assert_eq!(many, A200_SHA);
    let golden_payload = sole(
        &owner.query(
            "SELECT encode(_oo.pm_sha256(_oo.pm_payload('src_main', 'abababababababababababababababababababababababababababababababab', 1, 'n', 'customer', '', '')), 'hex');",
        ),
        "sha-golden",
    );
    assert_eq!(golden_payload, GOLDEN_DIGEST);
    lab.write(
        "sha-oracles.txt",
        &format!("empty={empty}\nabc={abc}\na200={many}\ngolden={golden_payload}\n"),
    );
    lab.note("sha_oracles=match");

    let roles = owner.query(
        "CREATE ROLE oo_a140_runtime LOGIN INHERIT; CREATE ROLE oo_a140_agent LOGIN; GRANT oo_pipeline_runtime TO oo_a140_runtime WITH INHERIT TRUE, SET FALSE; GRANT CONNECT ON DATABASE oo_pm TO oo_a140_runtime; GRANT CONNECT ON DATABASE oo_pm TO oo_a140_agent; ALTER ROLE oo_a140_runtime SET search_path = pg_catalog; ALTER ROLE oo_a140_agent SET search_path = pg_catalog;",
    );
    assert!(roles.code.is_none(), "roles {}", roles.message.unwrap_or_default());

    let config = config_ab();
    assert_eq!(config, "abababababababababababababababababababababababababababababababab");
    let golden = Reg::from_plan(&namespace_plan("src_main", &config, 1, "customer"));
    assert_eq!(hex_encode(&golden.digest), GOLDEN_DIGEST);
    let mut runtime = lab.connect("oo_a140_runtime", "oo_pm");
    let path = sole(&runtime.query("SHOW search_path;"), "search_path");
    assert_eq!(path, "pg_catalog");
    tx_begin(&mut runtime);
    let inserted = sole(&runtime.extended(REGISTER_SQL, &golden.params()), "golden insert");
    assert_eq!(inserted, "inserted");
    let gucs = sole_row(
        &runtime.query(
            "SELECT coalesce(current_setting('_oo.physical_mapping_writer', true), ''), coalesce(current_setting('_oo.writer', true), '');",
        ),
        "gucs",
    );
    assert_eq!(gucs, vec!["".to_string(), "".to_string()]);
    tx_commit(&mut runtime);
    let replay = register_commit(&mut runtime, &golden);
    assert_eq!(replay, "replay");
    let first_stamp = sole_row(&owner.extended(INSPECT_SQL, &golden.inspect_params()), "golden inspect");
    let second = register_commit(&mut runtime, &golden);
    assert_eq!(second, "replay");
    let second_stamp = sole_row(&owner.extended(INSPECT_SQL, &golden.inspect_params()), "golden replay inspect");
    assert_eq!(first_stamp[3], physical_from_digest(&golden.digest, "pn_"));
    assert_eq!(first_stamp[0], "63");
    assert_eq!(first_stamp[2], GOLDEN_DIGEST);
    assert_eq!(first_stamp[4], second_stamp[4]);
    assert_physical(&first_stamp[3]);
    lab.write(
        "durable-name.txt",
        &format!("physical_name={}\nidentity_digest={GOLDEN_DIGEST}\n", first_stamp[3]),
    );
    drop(runtime);
    let mut reopened = lab.connect("oo_a140_runtime", "oo_pm");
    let looked = lookup_one(&mut reopened, &golden);
    assert_eq!(looked, first_stamp[3]);
    lab.note(&format!("golden={} replay_stamp={}", first_stamp[3], first_stamp[4]));

    let left_name = format!("q_{}", "ab".repeat(40));
    let right_name = format!("{}cd", &left_name[..left_name.len() - 2]);
    assert_eq!(&left_name.as_bytes()[..63], &right_name.as_bytes()[..63]);
    let left = Reg::from_plan(&namespace_plan("src_long", &config, 1, &left_name));
    let right = Reg::from_plan(&namespace_plan("src_long", &config, 1, &right_name));
    assert_eq!(register_commit(&mut reopened, &left), "inserted");
    assert_eq!(register_commit(&mut reopened, &right), "inserted");
    let left_row = sole_row(&owner.extended(INSPECT_SQL, &left.inspect_params()), "long left");
    let right_row = sole_row(&owner.extended(INSPECT_SQL, &right.inspect_params()), "long right");
    assert_eq!(left_row[0], "63");
    assert_eq!(right_row[0], "63");
    assert!(left_row[1].parse::<i64>().expect("len") > 63);
    assert!(right_row[1].parse::<i64>().expect("len") > 63);
    assert_ne!(left_row[3], right_row[3]);
    assert_eq!(lookup_one(&mut reopened, &left), left_row[3]);
    assert_eq!(lookup_one(&mut reopened, &right), right_row[3]);
    lab.note(&format!("long_pair {} {}", left_row[3], right_row[3]));

    let han = Reg::from_plan(&namespace_plan("src_han", &config, 1, "q_e5aea2e688b7"));
    let long_han_text = q_utf8(&"一".repeat(80));
    assert!(long_han_text.len() > 63);
    let long_han = Reg::from_plan(&namespace_plan("src_han", &config, 1, &long_han_text));
    assert_eq!(register_commit(&mut reopened, &han), "inserted");
    assert_eq!(register_commit(&mut reopened, &long_han), "inserted");
    let han_row = sole_row(&owner.extended(INSPECT_SQL, &long_han.inspect_params()), "long han");
    assert_eq!(han_row[0], "63");
    assert!(han_row[1].parse::<i64>().expect("han len") > 63);
    lab.note(&format!("unicode physical={} logical_bytes={}", han_row[3], han_row[1]));

    let composed = Reg::from_plan(&namespace_plan("src_norm", &config, 1, "q_c3a9"));
    assert_eq!(register_commit(&mut reopened, &composed), "inserted");
    assert_eq!(register_commit(&mut reopened, &composed), "replay");
    let ligature = Reg::from_plan(&namespace_plan("src_norm", &config, 1, "q_efac81"));
    let ascii_fi = Reg::from_plan(&namespace_plan("src_norm", &config, 1, "fi"));
    assert_eq!(register_commit(&mut reopened, &ligature), "inserted");
    assert_eq!(register_commit(&mut reopened, &ascii_fi), "inserted");
    assert_ne!(lookup_one(&mut reopened, &ligature), lookup_one(&mut reopened, &ascii_fi));
    let lower = Reg::from_plan(&namespace_plan("src_case", &config, 1, "customer"));
    let cased = Reg::from_plan(&namespace_plan("src_case", &config, 1, "q_437573746f6d6572"));
    assert_eq!(register_commit(&mut reopened, &lower), "inserted");
    assert_eq!(register_commit(&mut reopened, &cased), "inserted");
    assert_ne!(lookup_one(&mut reopened, &lower), lookup_one(&mut reopened, &cased));
    let reserved = Reg::from_plan(&namespace_plan("src_word", &config, 1, "q_73656c656374"));
    let quoted = Reg::from_plan(&namespace_plan("src_word", &config, 1, "q_612262"));
    assert_eq!(register_commit(&mut reopened, &reserved), "inserted");
    assert_eq!(register_commit(&mut reopened, &quoted), "inserted");
    assert_physical(&lookup_one(&mut reopened, &reserved));
    assert_physical(&lookup_one(&mut reopened, &quoted));
    lab.note("normalization_reserved_quote=distinct");

    let mut raw = composed.clone();
    raw.namespace = "客户".to_string();
    tx_begin(&mut reopened);
    let rejected = reopened.extended(REGISTER_SQL, &raw.params());
    expect_code(&mut reopened, &rejected, "23514", Some("客户"));
    raw.namespace = "select".to_string();
    tx_begin(&mut reopened);
    let rejected = reopened.extended(REGISTER_SQL, &raw.params());
    expect_code(&mut reopened, &rejected, "23514", Some("select"));
    raw.namespace = format!("q_{}", "a".repeat(61));
    tx_begin(&mut reopened);
    let rejected = reopened.extended(REGISTER_SQL, &raw.params());
    expect_code(&mut reopened, &rejected, "23514", None);
    raw.namespace = "zzmarkerzz'; DROP TABLE _oo.physical_mappings; --".to_string();
    tx_begin(&mut reopened);
    let rejected = reopened.extended(REGISTER_SQL, &raw.params());
    expect_code(&mut reopened, &rejected, "23514", Some("zzmarkerzz"));
    let still = sole(
        &owner.query("SELECT (pg_catalog.to_regclass('_oo.physical_mappings') IS NOT NULL)::text;"),
        "table remains",
    );
    assert_eq!(still, "true");
    raw.level = "Namespace".to_string();
    raw.namespace = "customer".to_string();
    tx_begin(&mut reopened);
    let rejected = reopened.extended(REGISTER_SQL, &raw.params());
    expect_code(&mut reopened, &rejected, "23514", None);
    let mut short = golden.clone();
    short.digest = vec![0; 16];
    tx_begin(&mut reopened);
    let rejected = reopened.extended(REGISTER_SQL, &short.params());
    expect_code(&mut reopened, &rejected, "23514", None);
    let mut nulls = golden.params();
    nulls[7] = null_bytea();
    tx_begin(&mut reopened);
    let rejected = reopened.extended(REGISTER_SQL, &nulls);
    expect_code(&mut reopened, &rejected, "23514", None);
    let wide = format!("q_{}", "ab".repeat(2048));
    assert_eq!(wide.len(), 4098);
    let mut too_wide = golden.clone();
    too_wide.namespace = wide;
    too_wide.digest = vec![9; 32];
    tx_begin(&mut reopened);
    let rejected = reopened.extended(REGISTER_SQL, &too_wide.params());
    expect_code(&mut reopened, &rejected, "23514", None);
    let wide_ok_name = format!("q_{}", "ab".repeat(2047));
    assert_eq!(wide_ok_name.len(), 4096);
    let wide_ok = Reg::from_plan(&namespace_plan("src_wide", &config, 1, &wide_ok_name));
    assert_eq!(register_commit(&mut reopened, &wide_ok), "inserted");
    let wide_row = sole_row(&owner.extended(INSPECT_SQL, &wide_ok.inspect_params()), "wide");
    assert_eq!(wide_row[0], "63");
    assert_eq!(wide_row[1], "4096");
    lab.note("closed_payloads=23514 wide=4096");

    let other_source = Reg::from_plan(&namespace_plan("src_other", &config, 1, "customer"));
    let other_config = Reg::from_plan(&namespace_plan("src_main", &config_cd(), 1, "customer"));
    let other_gen = Reg::from_plan(&namespace_plan("src_main", &config, 2, "customer"));
    assert_eq!(register_commit(&mut reopened, &other_source), "inserted");
    assert_eq!(register_commit(&mut reopened, &other_config), "inserted");
    assert_eq!(register_commit(&mut reopened, &other_gen), "inserted");
    let source_name = lookup_one(&mut reopened, &other_source);
    let config_name = lookup_one(&mut reopened, &other_config);
    let gen_name = lookup_one(&mut reopened, &other_gen);
    assert_ne!(source_name, first_stamp[3]);
    assert_ne!(config_name, first_stamp[3]);
    assert_ne!(gen_name, first_stamp[3]);
    let mut absent = other_gen.clone();
    absent.generation = 3;
    tx_begin(&mut reopened);
    let missing_gen = reopened.extended(LOOKUP_SQL, &absent.lookup_params());
    expect_code(&mut reopened, &missing_gen, "OO024", None);
    lab.note("scope_isolation=3 absent_generation=OO024");

    let mut orphan = Reg::from_plan(&plan_table("src_parent", &config, 1, "orphan_ns", "orders"));
    tx_begin(&mut reopened);
    let no_parent = reopened.extended(REGISTER_SQL, &orphan.params());
    expect_code(&mut reopened, &no_parent, "OO024", None);
    let parent_ns = Reg::from_plan(&namespace_plan("src_parent", &config, 1, "sales"));
    assert_eq!(register_commit(&mut reopened, &parent_ns), "inserted");
    orphan = Reg::from_plan(&plan_column("src_parent", &config, 1, "sales", "orders", "amount"));
    tx_begin(&mut reopened);
    let no_table = reopened.extended(REGISTER_SQL, &orphan.params());
    expect_code(&mut reopened, &no_table, "OO024", None);
    let parent_table = Reg::from_plan(&plan_table("src_parent", &config, 1, "sales", "orders"));
    assert_eq!(register_commit(&mut reopened, &parent_table), "inserted");
    assert_eq!(register_commit(&mut reopened, &orphan), "inserted");
    let column_row = sole_row(&owner.extended(INSPECT_SQL, &orphan.inspect_params()), "column");
    assert_eq!(column_row[5], parent_ns_physical(&parent_ns));
    assert_eq!(column_row[6], parent_table_physical(&parent_table));
    assert!(column_row[3].starts_with("pc_"));
    lab.note(&format!("column={}", column_row[3]));

    let mut conflict = Reg::from_plan(&namespace_plan("src_main", &config, 1, "other_ns"));
    conflict.digest = golden.digest.clone();
    tx_begin(&mut reopened);
    let conflicted = reopened.extended(REGISTER_SQL, &conflict.params());
    expect_code(&mut reopened, &conflicted, "OO022", Some("other_ns"));
    assert_eq!(lookup_one(&mut reopened, &golden), first_stamp[3]);
    let conflict_count = sole(
        &owner.extended(
            "SELECT count(*)::text FROM _oo.physical_mappings WHERE source = $1 AND logical_namespace = $2",
            &[text_param("src_main"), text_param("other_ns")],
        ),
        "conflict count",
    );
    assert_eq!(conflict_count, "0");
    lab.note("conflict=OO022 original_kept");

    let rolled = Reg::from_plan(&namespace_plan("src_roll", &config, 1, "customer"));
    tx_begin(&mut reopened);
    let rolled_answer = sole(&reopened.extended(REGISTER_SQL, &rolled.params()), "rollback insert");
    assert_eq!(rolled_answer, "inserted");
    tx_rollback(&mut reopened);
    tx_begin(&mut reopened);
    let gone = reopened.extended(LOOKUP_SQL, &rolled.lookup_params());
    expect_code(&mut reopened, &gone, "OO024", None);
    lab.note("rollback=OO024");

    let race_plan = namespace_plan("src_race", &config_cd(), 1, "race_ns");
    let race = Reg::from_plan(&race_plan);
    let barrier = Barrier::new(2);
    let race_host = lab.host.clone();
    let race_port = lab.port;
    let (left_out, right_out) = thread::scope(|scope| {
        let left = scope.spawn(|| race_once(&race_host, race_port, &race, &barrier));
        let right = scope.spawn(|| race_once(&race_host, race_port, &race, &barrier));
        (left.join().expect("left thread"), right.join().expect("right thread"))
    });
    let mut outcomes = [left_out.expect("left"), right_out.expect("right")];
    outcomes.sort();
    assert_eq!(outcomes, ["inserted".to_string(), "replay".to_string()]);
    let race_count = sole(
        &owner.extended(
            "SELECT count(*)::text FROM _oo.physical_mappings WHERE source = $1",
            &[text_param("src_race")],
        ),
        "race count",
    );
    assert_eq!(race_count, "1");
    assert_eq!(lookup_one(&mut reopened, &race), race_plan.physical_name);
    lab.note("concurrent=inserted+replay rows=1");

    let constraints = owner.query(
        "SELECT c.conname || ' ' || pg_catalog.pg_get_constraintdef(c.oid) FROM pg_catalog.pg_constraint AS c WHERE c.conrelid = '_oo.physical_mappings'::regclass ORDER BY c.conname;",
    );
    assert!(constraints.code.is_none(), "constraints {}", constraints.message.unwrap_or_default());
    let constraint_text = constraints.rows.iter().map(|row| row[0].as_str()).collect::<Vec<_>>().join("\n");
    assert!(constraint_text.contains("physical_mappings_physical_name_unique"));
    assert!(constraint_text.contains("physical_mappings_pkey"));
    assert!(constraint_text.contains("physical_mappings_identity_digest_unique"));
    assert!(constraint_text.contains("UNIQUE NULLS NOT DISTINCT"));
    lab.write("constraints.txt", &constraint_text);
    let indexes = owner.query(
        "SELECT pg_catalog.pg_get_indexdef(index.indexrelid) FROM pg_catalog.pg_index AS index WHERE index.indrelid = '_oo.physical_mappings'::regclass ORDER BY 1;",
    );
    let index_text = indexes.rows.iter().map(|row| row[0].as_str()).collect::<Vec<_>>().join("\n");
    assert!(index_text.contains("UNIQUE"));
    lab.write("indexes.txt", &index_text);
    let relation = sole_row(
        &owner.query(
            "SELECT c.relpersistence::text, COALESCE(c.relacl::text, ''), pg_catalog.pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class AS c JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = 'physical_mappings';",
        ),
        "relation",
    );
    assert_eq!(relation[0], "p");
    assert_eq!(relation[2], "oo_target_owner");
    assert!(!relation[1].contains("oo_a140"));
    assert!(!relation[1].contains(",="));
    let functions = owner.query(
        "SELECT p.proname || '|' || r.rolname || '|' || p.prosecdef::text || '|' || pg_catalog.strpos(p.prosrc, 'pm_sha256')::text FROM pg_catalog.pg_proc AS p JOIN pg_catalog.pg_roles AS r ON r.oid = p.proowner JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace WHERE n.nspname = '_oo' AND p.proname IN ('lookup_physical_mapping', 'pm_sha256', 'register_physical_mapping') ORDER BY p.proname;",
    );
    assert!(
        functions.code.is_none(),
        "functions {}",
        functions.message.as_deref().unwrap_or("")
    );
    let function_text = functions.rows.iter().map(|row| row[0].as_str()).collect::<Vec<_>>().join("\n");
    assert!(
        function_text.contains("lookup_physical_mapping|oo_target_owner|true|0"),
        "{function_text}"
    );
    assert!(
        function_text.contains("pm_sha256|oo_target_owner|false|"),
        "{function_text}"
    );
    assert!(
        function_text.contains("register_physical_mapping|oo_target_owner|true|"),
        "{function_text}"
    );
    lab.write("functions.txt", &function_text);
    let privileges = sole_row(
        &owner.query(
            "SELECT has_table_privilege('oo_a140_agent', '_oo.physical_mappings', 'SELECT')::text, has_table_privilege('oo_a140_runtime', '_oo.physical_mappings', 'SELECT')::text, has_function_privilege('oo_a140_agent', '_oo.lookup_physical_mapping(text,text,bigint,text,text,text,text)', 'EXECUTE')::text, has_function_privilege('oo_a140_runtime', '_oo.lookup_physical_mapping(text,text,bigint,text,text,text,text)', 'EXECUTE')::text, has_function_privilege('oo_a140_runtime', '_oo.pm_sha256(bytea)', 'EXECUTE')::text, has_function_privilege('public', '_oo.lookup_physical_mapping(text,text,bigint,text,text,text,text)', 'EXECUTE')::text;",
        ),
        "privileges",
    );
    assert_eq!(
        privileges,
        vec![
            "false".to_string(),
            "false".to_string(),
            "false".to_string(),
            "true".to_string(),
            "false".to_string(),
            "false".to_string(),
        ]
    );
    lab.write("privileges.txt", &privileges.join("\n"));
    lab.note("catalog=owner_logged_agent_denied");

    let mut agent = lab.connect("oo_a140_agent", "oo_pm");
    let agent_read = agent.query("SELECT count(*) FROM _oo.physical_mappings;");
    expect_code(&mut agent, &agent_read, "42501", None);
    let agent_fn = agent.extended(LOOKUP_SQL, &golden.lookup_params());
    expect_code(&mut agent, &agent_fn, "42501", None);
    let runtime_read = reopened.query("SELECT count(*) FROM _oo.physical_mappings;");
    expect_code(&mut reopened, &runtime_read, "42501", None);
    let runtime_sha = reopened.query("SELECT encode(_oo.pm_sha256(decode('', 'hex')), 'hex');");
    expect_code(&mut reopened, &runtime_sha, "42501", None);
    let set_role = reopened.query("SET ROLE oo_pipeline_runtime;");
    expect_code(&mut reopened, &set_role, "42501", None);
    lab.note("agent_runtime_deny=42501");

    tx_begin(&mut owner);
    let direct = owner.query("SET LOCAL ROLE oo_target_owner; INSERT INTO _oo.physical_mappings DEFAULT VALUES;");
    expect_code(&mut owner, &direct, "OO014", None);
    tx_begin(&mut owner);
    let updated = owner.query(
        "SET LOCAL ROLE oo_target_owner; SELECT pg_catalog.set_config('_oo.physical_mapping_writer', 'register', true); UPDATE _oo.physical_mappings SET algorithm = algorithm;",
    );
    expect_code(&mut owner, &updated, "OO014", None);
    tx_begin(&mut owner);
    let deleted = owner.query(
        "SET LOCAL ROLE oo_target_owner; SELECT pg_catalog.set_config('_oo.physical_mapping_writer', 'register', true); DELETE FROM _oo.physical_mappings;",
    );
    expect_code(&mut owner, &deleted, "OO014", None);
    tx_begin(&mut owner);
    let truncated = owner.query(
        "SET LOCAL ROLE oo_target_owner; SELECT pg_catalog.set_config('_oo.physical_mapping_writer', 'register', true); TRUNCATE _oo.physical_mappings;",
    );
    expect_code(&mut owner, &truncated, "OO014", None);
    assert_eq!(lookup_one(&mut reopened, &golden), first_stamp[3]);
    lab.note("direct_write=OO014");

    tx_begin(&mut reopened);
    let fence = reopened.query(
        "SELECT _oo.commit_apply_receipt('shard_a', '11111111-1111-4111-8111-111111111111'::uuid, 1, 'src_main', 'table_one', 'batch-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 'snapshot-zero/v1', 'lineage_a', 0, 0, false, true, true, NULL, NULL, decode('', 'hex'));",
    );
    expect_code(&mut reopened, &fence, "OO011", None);
    let receipts = sole(&owner.query("SELECT count(*)::text FROM _oo.apply_receipts;"), "receipts");
    assert_eq!(receipts, "0");
    let public_tables = sole(
        &owner.query(
            "SELECT count(*)::text FROM pg_catalog.pg_class AS class JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace WHERE namespace.nspname = 'public' AND class.relkind = 'r';",
        ),
        "public tables",
    );
    assert_eq!(public_tables, "0");
    lab.note("fence=OO011 receipts=0 public_tables=0");

    let again = owner.query("BEGIN; SET LOCAL lock_timeout = '15s'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605);");
    assert!(again.code.is_none(), "second lock {}", again.message.unwrap_or_default());
    let (again_hash, again_at) = marker(&mut owner);
    assert_eq!(migration_route(Some(&again_hash), &file_hash), Route::Skip);
    tx_commit(&mut owner);
    let (after_hash, after_at) = marker(&mut owner);
    assert_eq!(after_hash, file_hash);
    assert_eq!(after_at, applied_at);
    assert_eq!(again_at, applied_at);
    tx_begin(&mut owner);
    let mismatch = owner.query(&format!(
        "DO $pm$ BEGIN IF (SELECT checksum FROM _oo.schema_migrations WHERE version = 'physical-mapping/v1') IS DISTINCT FROM '{LIE}' THEN RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001'; END IF; END $pm$;"
    ));
    expect_code(&mut owner, &mismatch, "OO001", None);
    let (final_hash, _) = marker(&mut owner);
    assert_eq!(final_hash, file_hash);
    lab.note("second_apply=skip mismatch=OO001");
    lab.note("live=ok");
}

fn physical_from_digest(digest: &[u8], prefix: &str) -> String {
    let hex = hex_encode(digest);
    format!("{prefix}{}", &hex[..60])
}

fn parent_ns_physical(reg: &Reg) -> String {
    physical_from_digest(&reg.digest, "pn_")
}

fn parent_table_physical(reg: &Reg) -> String {
    physical_from_digest(&reg.digest, "pt_")
}

fn plan_table(source: &str, config: &str, generation: i64, namespace: &str, table: &str) -> PhysicalPlan {
    plan_physical_mapping(&MappingRequest {
        source: source.to_string(),
        config_digest: config.to_string(),
        run_generation: generation,
        level: Level::Table,
        logical_namespace: namespace.to_string(),
        logical_table: table.to_string(),
        logical_column: String::new(),
    })
    .unwrap_or_else(|error| panic!("plan table {namespace}.{table}: {error}"))
}

fn plan_column(
    source: &str,
    config: &str,
    generation: i64,
    namespace: &str,
    table: &str,
    column: &str,
) -> PhysicalPlan {
    plan_physical_mapping(&MappingRequest {
        source: source.to_string(),
        config_digest: config.to_string(),
        run_generation: generation,
        level: Level::Column,
        logical_namespace: namespace.to_string(),
        logical_table: table.to_string(),
        logical_column: column.to_string(),
    })
    .unwrap_or_else(|error| panic!("plan column {namespace}.{table}.{column}: {error}"))
}

fn race_once(host: &str, port: u16, reg: &Reg, barrier: &Barrier) -> Result<String, String> {
    let mut conn = Conn::connect(host, port, "oo_a140_runtime", "oo_pm");
    let limits = conn.query("SET statement_timeout = '60s'; SET lock_timeout = '15s';");
    if limits.code.is_some() {
        return Err(limits.message.unwrap_or_default());
    }
    let begun = conn.query("BEGIN;");
    if begun.code.is_some() {
        return Err(begun.message.unwrap_or_default());
    }
    barrier.wait();
    let answer = conn.extended(REGISTER_SQL, &reg.params());
    if answer.code.is_some() {
        if answer.status == b'E' {
            let _ = conn.query("ROLLBACK;");
        }
        return Err(format!(
            "{} {}",
            answer.code.unwrap_or_default(),
            answer.message.unwrap_or_default()
        ));
    }
    let outcome = answer.rows.first().and_then(|row| row.first()).cloned().unwrap_or_default();
    let done = conn.query("COMMIT;");
    if done.code.is_some() {
        return Err(done.message.unwrap_or_default());
    }
    Ok(outcome)
}


