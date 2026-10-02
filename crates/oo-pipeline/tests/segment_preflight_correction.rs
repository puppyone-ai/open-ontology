//! Reference applicator for target-fence-receipts/3.
//!
//! Version 3 supersedes `0002_target_segment_null_expand.sql` when the
//! database is still on v1, and upgrades a database that already stored the
//! frozen v2 checksum. It does not rewrite 0000, 0001, 0002, or a stored
//! checksum. The ignored test speaks the PostgreSQL simple-query protocol to
//! one owned PostgreSQL 17 fixture. It is not a Loader.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const V3_JSON: &str = include_str!(
    "../../../contracts/pipeline/target-fence-receipts/target-fence-receipts-v3.json"
);
const BOOTSTRAP: &str = include_str!("../migrations/0000_bootstrap_roles.sql");
const EXPAND: &str = include_str!("../migrations/0001_target_fence_expand.sql");
const SEGMENT: &str = include_str!("../migrations/0002_target_segment_null_expand.sql");
const PREFLIGHT: &str = include_str!("../migrations/0003_target_segment_preflight_expand.sql");

const OWNER: &str = "11111111-1111-4111-8111-111111111111";
const V1_BOOT: &str = "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208";
const V1_EXPAND: &str = "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9";
const V2_FILE: &str = "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512";
const V3_FILE: &str = "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3";
const LIE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const ABSENT_MESSAGE: &str = "target-fence-receipts/1 segment relation is absent";
const HISTORY_MESSAGE: &str = "refusing target segment correction: preexisting apply receipts violate the closed segment rule; this migration does not delete, rewrite, or mutate receipts";
const MISMATCH_MESSAGE: &str = "migration checksum mismatch";
const FILE_V3: &str = "0003_target_segment_preflight_expand.sql";
const FILE_V2: &str = "0002_target_segment_null_expand.sql";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    AbsentPreflight,
    Skip,
    MismatchV3,
    MismatchV2,
    UpgradeFromV2,
    SupersedeFromV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Observed {
    relation: bool,
    constraint: bool,
    v2: Option<String>,
    v3: Option<String>,
}

fn route(observed: &Observed, file_hash: &str) -> Route {
    if !observed.relation || !observed.constraint {
        return Route::AbsentPreflight;
    }
    if let Some(stored) = &observed.v3 {
        if stored == file_hash {
            return Route::Skip;
        }
        return Route::MismatchV3;
    }
    if let Some(stored) = &observed.v2 {
        if stored == V2_FILE {
            return Route::UpgradeFromV2;
        }
        return Route::MismatchV2;
    }
    Route::SupersedeFromV1
}

fn reference_files(decision: Route) -> &'static [&'static str] {
    match decision {
        Route::AbsentPreflight | Route::UpgradeFromV2 | Route::SupersedeFromV1 => &[FILE_V3],
        Route::Skip | Route::MismatchV3 | Route::MismatchV2 => &[],
    }
}

fn reference_sql(decision: Route) -> Option<&'static str> {
    match decision {
        Route::AbsentPreflight | Route::UpgradeFromV2 | Route::SupersedeFromV1 => Some(PREFLIGHT),
        Route::Skip | Route::MismatchV3 | Route::MismatchV2 => None,
    }
}

fn dead_path_files() -> &'static [&'static str] {
    &[FILE_V2, FILE_V3]
}

fn v3_driver_authorized(computed_file_hash: &str, ready_for_that_hash: bool) -> bool {
    computed_file_hash == V3_FILE && ready_for_that_hash
}

fn segment_legal(index: Option<i32>, count: Option<i32>, bitmap_len: Option<usize>) -> bool {
    match (index, count, bitmap_len) {
        (None, None, Some(0)) => true,
        (Some(index), Some(count), Some(len))
            if count >= 1 && index >= 0 && index < count && (1..=8192).contains(&len) =>
        {
            true
        }
        _ => false,
    }
}

fn cut<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let at = text.find(start).unwrap_or_else(|| panic!("missing {start}"));
    let from = at;
    let rel = text[from + start.len()..]
        .find(end)
        .unwrap_or_else(|| panic!("missing {end}"));
    &text[from..from + start.len() + rel]
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

fn sha_after_filename<'a>(contract: &'a str, filename: &str) -> &'a str {
    let marker = format!("\"filename\": \"{filename}\"");
    let slice = contract
        .get(contract.find(&marker).unwrap_or_else(|| panic!("missing {filename}"))..)
        .expect("slice");
    let key = "\"sha256\": \"";
    let start = slice.find(key).expect("sha256") + key.len();
    let rest = &slice[start..];
    &rest[..rest.find('"').expect("sha end")]
}

fn bootstrap_sql(roles_exist: bool) -> String {
    if !roles_exist {
        return BOOTSTRAP.to_string();
    }
    let mut out = String::new();
    let mut skipping = false;
    for line in BOOTSTRAP.lines() {
        if line.starts_with("CREATE ROLE ") {
            skipping = true;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
        if skipping && line.trim_end().ends_with(';') {
            skipping = false;
        }
    }
    out
}

fn book_sql(rows: &[(&str, &str, &str)]) -> String {
    let values = rows
        .iter()
        .map(|(version, checksum, filename)| {
            format!("  ('{version}', '{checksum}', 'expand', '{filename}')")
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values};\n"
    )
}

fn receipt(shard: &str, generation: i64, batch: &str, payload: char, index: &str, count: &str, bitmap: &str) -> String {
    format!(
        "SELECT _oo.commit_apply_receipt('{shard}', '{OWNER}'::uuid, {generation}, 'warehouse', 'orders_g1', '{batch}', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('{payload}', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, {index}, {count}, {bitmap});"
    )
}

fn adopt(shard: &str) -> String {
    format!("SELECT _oo.adopt_target_generation('{shard}', '{OWNER}'::uuid);")
}

#[test]
fn published_bytes_and_reference_route_are_closed() {
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(sha256_hex(BOOTSTRAP.as_bytes()), V1_BOOT);
    assert_eq!(sha256_hex(EXPAND.as_bytes()), V1_EXPAND);
    assert_eq!(sha256_hex(SEGMENT.as_bytes()), V2_FILE);
    assert_eq!(sha256_hex(PREFLIGHT.as_bytes()), V3_FILE);
    assert_eq!(sha_after_filename(V3_JSON, "0000_bootstrap_roles.sql"), V1_BOOT);
    assert_eq!(sha_after_filename(V3_JSON, "0001_target_fence_expand.sql"), V1_EXPAND);
    assert_eq!(sha_after_filename(V3_JSON, FILE_V2), V2_FILE);
    assert_eq!(sha_after_filename(V3_JSON, FILE_V3), V3_FILE);
    assert!(V3_JSON.contains("\"executes_0002_on_reference_path\": false"));
    assert!(V3_JSON.contains("\"v1_schema_ready_authorizes_this_contract\": false"));
    assert!(V3_JSON.contains("\"v2_segment_contract_ready_authorizes_this_contract\": false"));
    assert!(!PREFLIGHT.contains("::regclass"));
    assert!(SEGMENT.contains("::pg_catalog.regclass"));
    assert_eq!(PREFLIGHT.matches(ABSENT_MESSAGE).count(), 3);
    assert!(!PREFLIGHT.contains("DELETE FROM _oo.apply_receipts"));
    assert!(!PREFLIGHT.contains("UPDATE _oo.apply_receipts"));
    assert!(!PREFLIGHT.contains("GRANT oo_target_owner TO oo_pipeline_runtime"));
    assert!(!PREFLIGHT.contains("FUNCTION _oo.segment_contract_ready"));
    assert!(!PREFLIGHT.contains("VIEW _oo.segment_service_readiness"));
    let fn_start = "CREATE OR REPLACE FUNCTION _oo.commit_apply_receipt(";
    let fn_end = "COMMENT ON FUNCTION _oo.commit_apply_receipt(";
    assert_eq!(cut(SEGMENT, fn_start, fn_end), cut(PREFLIGHT, fn_start, fn_end));
    assert_eq!(
        cut(SEGMENT, "DO $segment_history$", "$segment_history$;"),
        cut(PREFLIGHT, "DO $segment_history$", "$segment_history$;")
    );
    assert_eq!(
        cut(SEGMENT, "ADD CONSTRAINT apply_receipts_segment_check CHECK (", "COMMENT ON CONSTRAINT apply_receipts_segment_check"),
        cut(PREFLIGHT, "ADD CONSTRAINT apply_receipts_segment_check CHECK (", "COMMENT ON CONSTRAINT apply_receipts_segment_check")
    );
    assert!(cut(PREFLIGHT, fn_start, fn_end).contains(") IS NOT TRUE THEN"));
    assert!(cut(PREFLIGHT, "ADD CONSTRAINT apply_receipts_segment_check CHECK (", "COMMENT ON CONSTRAINT").contains(") IS TRUE"));
    let segment_at = PREFLIGHT.find("closed segment metadata").expect("segment rejection");
    let fence_at = PREFLIGHT.find("PERFORM _oo._load_held_generation").expect("fence");
    assert!(segment_at < fence_at);

    let indexes = [None, Some(0), Some(-1), Some(1), Some(8191), Some(8192)];
    let counts = [None, Some(0), Some(1), Some(-1), Some(8192), Some(8193)];
    let bitmaps = [None, Some(0), Some(1), Some(8192), Some(8193)];
    let mut legal = 0;
    let mut illegal = 0;
    for index in indexes {
        for count in counts {
            for bitmap in bitmaps {
                if segment_legal(index, count, bitmap) {
                    legal += 1;
                } else {
                    illegal += 1;
                }
            }
        }
    }
    assert_eq!(legal + illegal, 180);
    assert_eq!(legal, 17);
    assert_eq!(illegal, 163);
    assert!(!segment_legal(None, Some(1), Some(1)));
    assert!(!segment_legal(Some(0), None, Some(1)));
    assert!(!segment_legal(None, None, Some(1)));

    let present = Observed {
        relation: true,
        constraint: true,
        v2: None,
        v3: None,
    };
    assert_eq!(route(&present, V3_FILE), Route::SupersedeFromV1);
    assert_eq!(reference_files(Route::SupersedeFromV1), &[FILE_V3]);
    let v2 = Observed {
        v2: Some(V2_FILE.to_string()),
        ..present.clone()
    };
    assert_eq!(route(&v2, V3_FILE), Route::UpgradeFromV2);
    let bad_v2 = Observed {
        v2: Some(LIE.to_string()),
        ..present.clone()
    };
    assert_eq!(route(&bad_v2, V3_FILE), Route::MismatchV2);
    assert!(reference_files(Route::MismatchV2).is_empty());
    let same = Observed {
        v3: Some(V3_FILE.to_string()),
        ..present.clone()
    };
    assert_eq!(route(&same, V3_FILE), Route::Skip);
    let wrong = Observed {
        v3: Some(LIE.to_string()),
        ..present.clone()
    };
    assert_eq!(route(&wrong, V3_FILE), Route::MismatchV3);
    let absent = Observed {
        relation: false,
        constraint: false,
        v2: Some(V2_FILE.to_string()),
        v3: Some(V3_FILE.to_string()),
    };
    assert_eq!(route(&absent, V3_FILE), Route::AbsentPreflight);
    let dropped = Observed {
        relation: true,
        constraint: false,
        v2: None,
        v3: None,
    };
    assert_eq!(route(&dropped, V3_FILE), Route::AbsentPreflight);
    for decision in [
        Route::AbsentPreflight,
        Route::Skip,
        Route::MismatchV3,
        Route::MismatchV2,
        Route::UpgradeFromV2,
        Route::SupersedeFromV1,
    ] {
        assert!(!reference_files(decision).contains(&FILE_V2));
        assert_ne!(reference_files(decision), dead_path_files());
        if let Some(sql) = reference_sql(decision) {
            assert!(!sql.contains("::regclass"));
            assert!(sql.starts_with("-- target-fence-receipts/3"));
        }
    }
    assert_eq!(dead_path_files(), &[FILE_V2, FILE_V3]);
    assert!(SEGMENT.contains("::pg_catalog.regclass"));
    assert!(v3_driver_authorized(V3_FILE, true));
    assert!(!v3_driver_authorized(V3_FILE, false));
    assert!(!v3_driver_authorized(LIE, true));
    assert!(!v3_driver_authorized(V2_FILE, true));
}

#[test]
fn error_and_data_row_parsers_keep_sqlstate_text() {
    let mut body = Vec::new();
    body.push(b'C');
    body.extend_from_slice(b"OO002\0");
    body.push(b'M');
    body.extend_from_slice(ABSENT_MESSAGE.as_bytes());
    body.push(0);
    body.push(0);
    let (code, message) = error_fields(&body);
    assert_eq!(code, "OO002");
    assert_eq!(message, ABSENT_MESSAGE);
    let mut row = Vec::new();
    row.extend_from_slice(&1i16.to_be_bytes());
    row.extend_from_slice(&2i32.to_be_bytes());
    row.extend_from_slice(b"ok");
    assert_eq!(data_row(&row), vec!["ok".to_string()]);
}

#[derive(Debug)]
struct Answer {
    rows: Vec<Vec<String>>,
    code: Option<String>,
    message: Option<String>,
    status: u8,
}

impl Answer {
    fn cells(&self) -> String {
        self.rows
            .iter()
            .map(|row| row.join("|"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

struct Evidence {
    root: PathBuf,
}

impl Evidence {
    fn note(&self, line: &str) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("summary.log"))
            .expect("summary");
        writeln!(file, "{line}").expect("summary write");
    }

    fn case(&self, name: &str, answer: &Answer) {
        let dir = self.root.join("cases");
        fs::create_dir_all(&dir).expect("cases");
        fs::write(dir.join(format!("{name}.stdout")), answer.cells() + "\n").expect("stdout");
        let err = match (&answer.code, &answer.message) {
            (Some(code), Some(message)) => format!("{code}: {message}\n"),
            (Some(code), None) => format!("{code}\n"),
            _ => String::new(),
        };
        fs::write(dir.join(format!("{name}.stderr")), err).expect("stderr");
    }
}

struct Lab {
    host: String,
    port: u16,
    evidence: Evidence,
}

impl Lab {
    fn connect(&self, db: &str) -> Conn {
        self.connect_as("postgres", db)
    }

    fn connect_as(&self, user: &str, db: &str) -> Conn {
        Conn::connect(&self.host, self.port, user, db)
    }

    fn note(&self, line: &str) {
        self.evidence.note(line);
    }

    fn one(&self, db: &str, name: &str, sql: &str) -> String {
        let mut conn = self.connect(db);
        let answer = conn.query(sql);
        self.evidence.case(name, &answer);
        assert!(
            answer.code.is_none(),
            "{name} {}: {}",
            answer.code.unwrap_or_default(),
            answer.message.unwrap_or_default()
        );
        assert_eq!(answer.rows.len(), 1, "{name} rows {}", answer.cells());
        assert_eq!(answer.rows[0].len(), 1, "{name}");
        answer.rows[0][0].clone()
    }

    fn exec(&self, db: &str, name: &str, sql: &str) {
        let mut conn = self.connect(db);
        let answer = conn.query(sql);
        self.evidence.case(name, &answer);
        assert!(
            answer.code.is_none(),
            "{name} {}: {}",
            answer.code.unwrap_or_default(),
            answer.message.unwrap_or_default()
        );
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
        let len = u32::try_from(payload.len() + 4).expect("query len");
        self.stream.write_all(b"Q").expect("query tag");
        self.stream.write_all(&len.to_be_bytes()).expect("query len");
        self.stream.write_all(&payload).expect("query body");
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
                b'T' | b'C' | b'N' | b'S' | b'A' | b'I' | b'n' => {}
                other => panic!("response tag {}", other as char),
            }
        }
    }

    fn rollback(&mut self) {
        let answer = self.query("ROLLBACK");
        assert!(
            answer.code.is_none(),
            "rollback {}: {}",
            answer.code.unwrap_or_default(),
            answer.message.unwrap_or_default()
        );
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

struct ApplyOut {
    route: Route,
    code: Option<String>,
    message: Option<String>,
    executed: &'static [&'static str],
}

fn require_timeout(timeout: &str) {
    assert!(
        timeout.len() > 1
            && timeout.ends_with('s')
            && timeout[..timeout.len() - 1].bytes().all(|byte| byte.is_ascii_digit()),
        "lock timeout {timeout}"
    );
}

fn observe(conn: &mut Conn) -> (bool, bool, Observed) {
    let answer = conn.query(
        "SELECT (pg_catalog.to_regclass('_oo.apply_receipts') IS NOT NULL)::text, (pg_catalog.to_regclass('_oo.schema_migrations') IS NOT NULL)::text, (EXISTS (SELECT 1 FROM pg_catalog.pg_constraint AS constraint_row WHERE constraint_row.conname = 'apply_receipts_segment_check' AND constraint_row.conrelid = pg_catalog.to_regclass('_oo.apply_receipts')))::text, (EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner'))::text, (pg_catalog.to_regclass('_oo.migration_lock') IS NOT NULL)::text;",
    );
    assert!(answer.code.is_none(), "observe {}", answer.message.unwrap_or_default());
    let row = &answer.rows[0];
    let relation = row[0] == "true";
    let migrations = row[1] == "true";
    let constraint = row[2] == "true";
    let role = row[3] == "true";
    let migration_lock = row[4] == "true";
    let (v2, v3) = if migrations {
        let marks = conn.query(
            "SELECT version || '=' || checksum FROM _oo.schema_migrations WHERE version IN ('target-fence-receipts/2', 'target-fence-receipts/3') AND phase = 'expand' ORDER BY version;",
        );
        assert!(marks.code.is_none(), "markers {}", marks.message.unwrap_or_default());
        let mut v2 = None;
        let mut v3 = None;
        for mark in &marks.rows {
            let (version, checksum) = mark[0].split_once('=').expect("marker");
            if version == "target-fence-receipts/2" {
                v2 = Some(checksum.to_string());
            } else if version == "target-fence-receipts/3" {
                v3 = Some(checksum.to_string());
            }
        }
        (v2, v3)
    } else {
        (None, None)
    };
    (
        role,
        migration_lock,
        Observed {
            relation,
            constraint,
            v2,
            v3,
        },
    )
}

fn open_migration(conn: &mut Conn, timeout: &str) -> Result<(bool, bool, Observed), ApplyOut> {
    require_timeout(timeout);
    let begun = conn.query(&format!(
        "BEGIN; SET LOCAL lock_timeout = '{timeout}'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605);"
    ));
    if let Some(code) = begun.code.clone() {
        if begun.status == b'E' {
            conn.rollback();
        }
        return Err(ApplyOut {
            route: Route::AbsentPreflight,
            code: Some(code),
            message: begun.message,
            executed: &[],
        });
    }
    Ok(observe(conn))
}

fn run_preflight(conn: &mut Conn, role: bool, migration_lock: bool) -> Answer {
    if role {
        let set = conn.query("SET ROLE oo_target_owner;");
        assert!(set.code.is_none(), "set role {}", set.message.unwrap_or_default());
    }
    if migration_lock {
        let locked = conn.query("SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;");
        assert!(locked.code.is_none(), "migration lock {}", locked.message.unwrap_or_default());
    }
    assert!(!PREFLIGHT.contains("::regclass"), "reference sql contains a regclass cast");
    conn.query(PREFLIGHT)
}

fn finish_marker(conn: &mut Conn, checksum: &str) -> Answer {
    assert_eq!(checksum.len(), 64);
    assert!(checksum.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    conn.query(&format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/3', '{checksum}', 'expand', '{FILE_V3}'); RESET ROLE; COMMIT;"
    ))
}

/// Reference applicator. The recorded checksum is the SHA-256 of this file's
/// `0003` bytes. A caller cannot pass a different digest through this function.
fn apply_reference(conn: &mut Conn, timeout: &str, fail_before_marker: bool) -> ApplyOut {
    let file_hash = sha256_hex(PREFLIGHT.as_bytes());
    assert_eq!(file_hash, V3_FILE);
    let (role, migration_lock, observed) = match open_migration(conn, timeout) {
        Ok(value) => value,
        Err(failed) => return failed,
    };
    let decision = route(&observed, &file_hash);
    let executed = reference_files(decision);
    match decision {
        Route::Skip => {
            let done = conn.query("COMMIT;");
            assert!(done.code.is_none(), "skip commit {}", done.message.unwrap_or_default());
            ApplyOut {
                route: decision,
                code: None,
                message: None,
                executed,
            }
        }
        Route::MismatchV3 | Route::MismatchV2 => {
            let failed = conn.query(&format!(
                "DO $checksum$ BEGIN RAISE EXCEPTION '{MISMATCH_MESSAGE}' USING ERRCODE = 'OO001'; END $checksum$;"
            ));
            if failed.status == b'E' {
                conn.rollback();
            }
            ApplyOut {
                route: decision,
                code: failed.code,
                message: failed.message,
                executed,
            }
        }
        Route::AbsentPreflight | Route::UpgradeFromV2 | Route::SupersedeFromV1 => {
            let ran = run_preflight(conn, role, migration_lock);
            if ran.code.is_some() {
                if ran.status == b'E' || ran.status == b'T' {
                    conn.rollback();
                }
                return ApplyOut {
                    route: decision,
                    code: ran.code,
                    message: ran.message,
                    executed,
                };
            }
            if fail_before_marker {
                let failed = conn.query(
                    "DO $fail$ BEGIN RAISE EXCEPTION 'forced migration failure' USING ERRCODE = 'OO003'; END $fail$;",
                );
                if failed.status == b'E' || failed.status == b'T' {
                    conn.rollback();
                }
                return ApplyOut {
                    route: decision,
                    code: failed.code,
                    message: failed.message,
                    executed,
                };
            }
            let booked = finish_marker(conn, &file_hash);
            if booked.code.is_some() && (booked.status == b'E' || booked.status == b'T') {
                conn.rollback();
            }
            ApplyOut {
                route: decision,
                code: booked.code,
                message: booked.message,
                executed,
            }
        }
    }
}

/// Test-only recorder. The reference applicator does not call this and does
/// not accept a caller-supplied checksum.
fn book_caller_supplied_checksum(conn: &mut Conn, checksum: &str) -> ApplyOut {
    let file_hash = sha256_hex(PREFLIGHT.as_bytes());
    assert_eq!(file_hash, V3_FILE);
    assert_ne!(checksum, file_hash, "a supplied lie must not be the file hash");
    let (role, migration_lock, observed) = match open_migration(conn, "15s") {
        Ok(value) => value,
        Err(failed) => return failed,
    };
    let decision = route(&observed, &file_hash);
    assert_eq!(decision, Route::SupersedeFromV1);
    let ran = run_preflight(conn, role, migration_lock);
    assert!(ran.code.is_none(), "lie file {}", ran.message.unwrap_or_default());
    let booked = finish_marker(conn, checksum);
    ApplyOut {
        route: decision,
        code: booked.code,
        message: booked.message,
        executed: reference_files(decision),
    }
}

fn install_v1(lab: &Lab, db: &str) {
    let sql = format!(
        "BEGIN; SET LOCAL lock_timeout = '15s'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605);\n{}\nSET ROLE oo_target_owner;\n{EXPAND}\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{}RESET ROLE; COMMIT;",
        bootstrap_sql(true),
        book_sql(&[
            ("target-bootstrap/1", V1_BOOT, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", V1_EXPAND, "0001_target_fence_expand.sql"),
        ])
    );
    lab.exec(db, &format!("install-v1-{db}"), &sql);
}

fn install_historical_v2(lab: &Lab, db: &str) {
    let sql = format!(
        "BEGIN; SET LOCAL lock_timeout = '15s'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605); SET ROLE oo_target_owner; SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{SEGMENT}\n{}RESET ROLE; COMMIT;",
        book_sql(&[("target-fence-receipts/2", V2_FILE, FILE_V2)])
    );
    assert!(sql.contains("::pg_catalog.regclass"));
    lab.exec(db, &format!("historical-v2-{db}"), &sql);
}

fn snapshot(lab: &Lab, db: &str, name: &str) -> String {
    lab.one(
        db,
        name,
        "SELECT coalesce(string_agg(row(receipt.*)::text, E'\\n' ORDER BY source, table_generation, txn_or_batch_id), '') FROM _oo.apply_receipts AS receipt;",
    )
}

fn constraintdef(lab: &Lab, db: &str, name: &str) -> String {
    lab.one(
        db,
        name,
        "SELECT pg_catalog.pg_get_constraintdef(constraint_row.oid) FROM pg_catalog.pg_constraint AS constraint_row WHERE constraint_row.conname = 'apply_receipts_segment_check' AND constraint_row.conrelid = pg_catalog.to_regclass('_oo.apply_receipts');",
    )
}

fn v3_count(lab: &Lab, db: &str, name: &str) -> String {
    lab.one(
        db,
        name,
        "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';",
    )
}

fn marker_rows(lab: &Lab, db: &str, name: &str) -> String {
    lab.one(
        db,
        name,
        "SELECT coalesce(string_agg(version || '|' || checksum || '|' || applied_at::text, E'\\n' ORDER BY version), '') FROM _oo.schema_migrations WHERE version IN ('target-fence-receipts/1', 'target-fence-receipts/2', 'target-fence-receipts/3');",
    )
}

fn seed_lawful(lab: &Lab, db: &str, shard: &str) {
    let mut conn = lab.connect_as("oo_a104_runtime", db);
    let sql = format!(
        "{}\n{}\n{}",
        adopt(shard),
        receipt(shard, 1, &format!("{shard}-empty"), 'e', "NULL", "NULL", "''::bytea"),
        receipt(shard, 1, &format!("{shard}-one"), 'e', "0", "1", "'\\x01'::bytea")
    );
    let answer = conn.query(&sql);
    lab.evidence.case(&format!("seed-{shard}"), &answer);
    assert!(answer.code.is_none(), "seed {shard} {}", answer.message.unwrap_or_default());
    assert!(answer.cells().contains("inserted"), "{}", answer.cells());
}

fn seed_illegal(lab: &Lab, db: &str) {
    lab.exec(
        db,
        "seed-illegal",
        "BEGIN; SET ROLE oo_target_owner; SELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true); INSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'illegal-null', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea, 'bad_shard', 1); COMMIT;",
    );
}

fn expect_apply(lab: &Lab, db: &str, name: &str, fail_before_marker: bool, route_want: Route, code_want: Option<&str>, message_want: Option<&str>) -> ApplyOut {
    let mut conn = lab.connect(db);
    let out = apply_reference(&mut conn, "15s", fail_before_marker);
    lab.evidence.case(
        name,
        &Answer {
            rows: vec![vec![format!("{:?}", out.executed)]],
            code: out.code.clone(),
            message: out.message.clone(),
            status: b'I',
        },
    );
    assert_eq!(out.route, route_want, "{name}");
    assert_eq!(out.code.as_deref(), code_want, "{name} {}", out.message.as_deref().unwrap_or(""));
    if let Some(message) = message_want {
        assert_eq!(out.message.as_deref(), Some(message), "{name}");
    }
    assert_eq!(out.executed, reference_files(route_want), "{name}");
    lab.note(&format!(
        "{name} route={route_want:?} sqlstate={} files={}",
        out.code.as_deref().unwrap_or("ok"),
        out.executed.join(",")
    ));
    out
}

#[test]
#[ignore = "one owned PostgreSQL 17.10; OO_SEGMENT_PREFLIGHT_HOST must be set by the deciding run"]
fn live_segment_preflight() {
    let host = env::var("OO_SEGMENT_PREFLIGHT_HOST").expect("OO_SEGMENT_PREFLIGHT_HOST");
    let port = env::var("OO_SEGMENT_PREFLIGHT_PORT")
        .unwrap_or_else(|_| "5432".to_string())
        .parse::<u16>()
        .expect("port");
    let evidence_root = PathBuf::from(
        env::var("OO_SEGMENT_PREFLIGHT_EVIDENCE").expect("OO_SEGMENT_PREFLIGHT_EVIDENCE"),
    );
    fs::create_dir_all(evidence_root.join("cases")).expect("evidence");
    let rustc = Command::new("rustc").arg("--version").output().expect("rustc");
    let rustc_text = String::from_utf8(rustc.stdout).expect("rustc utf8");
    fs::write(evidence_root.join("rustc-version.txt"), &rustc_text).expect("rustc evidence");
    assert!(
        rustc.status.success() && rustc_text.starts_with("rustc 1.94.0"),
        "deciding compiler {rustc_text}"
    );
    let lab = Lab {
        host,
        port,
        evidence: Evidence { root: evidence_root },
    };
    let mut admin = lab.connect("postgres");
    let version = admin.query("SELECT version();");
    assert!(version.code.is_none(), "{}", version.message.unwrap_or_default());
    let server = version.cells();
    fs::write(lab.evidence.root.join("server-version.txt"), &server).expect("server evidence");
    assert!(server.contains("PostgreSQL 17.10"), "{server}");
    lab.note(&format!("server={server}"));
    lab.note(&format!("rustc={}", rustc_text.trim()));
    exercise(&lab);
    lab.note("live=pass");
}

fn exercise(lab: &Lab) {
    lab.exec("postgres", "create-empty", "CREATE DATABASE oo_pf_empty;");
    phase_empty(lab);
    lab.exec("postgres", "create-schema", "CREATE DATABASE oo_pf_schema;");
    phase_schema(lab);
    lab.exec(
        "postgres",
        "create-logins",
        "CREATE ROLE oo_a104_bootstrap LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION; CREATE ROLE oo_a104_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION; GRANT oo_target_owner TO oo_a104_bootstrap WITH INHERIT FALSE, SET TRUE; GRANT oo_pipeline_runtime TO oo_a104_runtime WITH INHERIT TRUE, SET FALSE; ALTER ROLE oo_a104_bootstrap SET search_path = pg_catalog; ALTER ROLE oo_a104_runtime SET search_path = pg_catalog;",
    );
    for db in [
        "oo_pf_rel",
        "oo_pf_con",
        "oo_pf_fresh",
        "oo_pf_law",
        "oo_pf_v2",
        "oo_pf_bad",
        "oo_pf_redo",
        "oo_pf_mis",
        "oo_pf_roll",
        "oo_pf_lock",
        "oo_pf_lie",
        "oo_pf_own",
    ] {
        lab.exec("postgres", &format!("create-{db}"), &format!("CREATE DATABASE {db};"));
        install_v1(lab, db);
        lab.exec(
            "postgres",
            &format!("grant-{db}"),
            &format!("GRANT CONNECT ON DATABASE {db} TO oo_a104_bootstrap; GRANT CONNECT ON DATABASE {db} TO oo_a104_runtime;"),
        );
    }
    phase_relation(lab);
    phase_constraint(lab);
    phase_fresh(lab);
    phase_lawful(lab);
    phase_v2(lab);
    phase_illegal(lab);
    phase_redo(lab);
    phase_mismatch(lab);
    phase_rollback(lab);
    phase_lock(lab);
    phase_lie(lab);
    phase_owner(lab);
}

fn phase_empty(lab: &Lab) {
    expect_apply(lab, "oo_pf_empty", "empty", false, Route::AbsentPreflight, Some("OO002"), Some(ABSENT_MESSAGE));
    let absent = lab.one(
        "oo_pf_empty",
        "empty-schema",
        "SELECT (pg_catalog.to_regnamespace('_oo') IS NULL)::text;",
    );
    assert_eq!(absent, "true");
    lab.note("empty OO002 no_schema=true");
}

fn phase_schema(lab: &Lab) {
    lab.exec(
        "oo_pf_schema",
        "schema-bootstrap",
        &format!("BEGIN; {} COMMIT;", bootstrap_sql(false)),
    );
    let mut dead = lab.connect("oo_pf_schema");
    let answer = dead.query(&format!(
        "BEGIN; SET ROLE oo_target_owner;\n{SEGMENT}\n{PREFLIGHT}\nCOMMIT;"
    ));
    lab.evidence.case("dead-path", &answer);
    if answer.status == b'E' {
        dead.rollback();
    }
    assert_eq!(answer.code.as_deref(), Some("42P01"), "{}", answer.message.as_deref().unwrap_or(""));
    assert_ne!(answer.message.as_deref(), Some(ABSENT_MESSAGE));
    lab.note("dead_path=42P01 reference_does_not_use_this_order");
    expect_apply(lab, "oo_pf_schema", "schema", false, Route::AbsentPreflight, Some("OO002"), Some(ABSENT_MESSAGE));
    let absent = lab.one(
        "oo_pf_schema",
        "schema-absent",
        "SELECT (pg_catalog.to_regnamespace('_oo') IS NULL)::text;",
    );
    assert_eq!(absent, "true");
    lab.note("schema OO002 no_marker=true");
}

fn phase_relation(lab: &Lab) {
    lab.exec("oo_pf_rel", "drop-relation", "DROP TABLE _oo.apply_receipts CASCADE;");
    expect_apply(lab, "oo_pf_rel", "missing-relation", false, Route::AbsentPreflight, Some("OO002"), Some(ABSENT_MESSAGE));
    assert_eq!(v3_count(lab, "oo_pf_rel", "missing-relation-marker"), "0");
    let v1 = lab.one(
        "oo_pf_rel",
        "missing-relation-v1",
        &format!("SELECT _oo.schema_ready('{V1_EXPAND}')::text;"),
    );
    assert_eq!(v1, "true");
}

fn phase_constraint(lab: &Lab) {
    lab.exec(
        "oo_pf_con",
        "drop-constraint",
        "ALTER TABLE _oo.apply_receipts DROP CONSTRAINT apply_receipts_segment_check;",
    );
    expect_apply(lab, "oo_pf_con", "absent-constraint", false, Route::AbsentPreflight, Some("OO002"), Some(ABSENT_MESSAGE));
    assert_eq!(v3_count(lab, "oo_pf_con", "absent-constraint-marker"), "0");
    let still = lab.one(
        "oo_pf_con",
        "absent-constraint-still",
        "SELECT count(*)::text FROM pg_catalog.pg_constraint WHERE conname = 'apply_receipts_segment_check' AND conrelid = pg_catalog.to_regclass('_oo.apply_receipts');",
    );
    assert_eq!(still, "0");
}

fn phase_fresh(lab: &Lab) {
    let db = "oo_pf_fresh";
    expect_apply(lab, db, "fresh", false, Route::SupersedeFromV1, None, None);
    assert_eq!(v3_count(lab, db, "fresh-marker"), "1");
    let flags = lab.one(
        db,
        "fresh-flags",
        &format!(
            "SELECT _oo.schema_ready('{V1_EXPAND}')::text || '|' || _oo.schema_ready('{V3_FILE}')::text || '|' || _oo.segment_preflight_ready('{V3_FILE}')::text || '|' || _oo.segment_preflight_ready('{LIE}')::text || '|' || (SELECT schema_version_present::text FROM _oo.segment_preflight_readiness) || '|' || (pg_catalog.to_regprocedure('_oo.segment_contract_ready(text)') IS NULL)::text;"
        ),
    );
    assert_eq!(flags, "true|false|true|false|true|true", "{flags}");
    assert!(v3_driver_authorized(V3_FILE, true));
    assert!(!v3_driver_authorized(V3_FILE, false));
    let versions = marker_rows(lab, db, "fresh-versions");
    assert!(versions.contains(&format!("target-fence-receipts/3|{V3_FILE}|")), "{versions}");
    assert!(!versions.contains("target-fence-receipts/2"), "{versions}");
    let def = constraintdef(lab, db, "fresh-check");
    assert!(def.contains("IS TRUE"), "{def}");
    let flags3 = lab.one(
        db,
        "three-flags",
        "SELECT (( (NULL IS NULL AND NULL IS NULL AND octet_length('\\x01'::bytea) = 0) OR (1 >= 1 AND NULL >= 0 AND NULL < 1 AND octet_length('\\x01'::bytea) BETWEEN 1 AND 8192) ) IS NULL)::text || '|' || (( (NULL IS NULL AND NULL IS NULL AND octet_length('\\x01'::bytea) = 0) OR (1 >= 1 AND NULL >= 0 AND NULL < 1 AND octet_length('\\x01'::bytea) BETWEEN 1 AND 8192) ) IS TRUE)::text;",
    );
    assert_eq!(flags3, "true|false", "{flags3}");
    phase_shapes(lab, db);
    phase_direct_unknown(lab, db);
    phase_stale(lab, db);
    phase_replay(lab, db);
    phase_search_path(lab, db);
    phase_acl(lab, db);
    phase_lock_survives(lab, db);
}

fn phase_shapes(lab: &Lab, db: &str) {
    let rows = [
        ("null-index", "NULL", "1", "'\\x01'::bytea", false),
        ("null-count", "0", "NULL", "'\\x01'::bytea", false),
        ("null-both-byte", "NULL", "NULL", "'\\x01'::bytea", false),
        ("absent-valid", "NULL", "NULL", "''::bytea", true),
        ("present-valid", "0", "1", "'\\x01'::bytea", true),
        ("bound-8192", "0", "1", "decode(repeat('aa', 8192), 'hex')", true),
        ("bound-8193", "0", "1", "decode(repeat('aa', 8193), 'hex')", false),
    ];
    for (name, index, count, bitmap, allow) in rows {
        let mut conn = lab.connect_as("oo_a104_runtime", db);
        let sql = format!(
            "{}; {}",
            adopt(&format!("sh_{name}").replace('-', "_")),
            receipt(&format!("sh_{name}").replace('-', "_"), 1, name, 'e', index, count, bitmap)
        );
        let answer = conn.query(&sql);
        lab.evidence.case(&format!("shape-{name}"), &answer);
        if allow {
            assert!(answer.code.is_none(), "{name} {}", answer.message.unwrap_or_default());
            assert!(answer.cells().contains("inserted"), "{name} {}", answer.cells());
            lab.note(&format!("shape {name}=inserted"));
        } else {
            assert_eq!(answer.code.as_deref(), Some("23514"), "{name} {}", answer.message.as_deref().unwrap_or(""));
            assert!(answer.message.as_deref().unwrap_or("").contains("closed segment metadata"), "{name}");
            assert!(!answer.message.as_deref().unwrap_or("").contains("OO013"), "{name}");
            lab.note(&format!("shape {name}=23514"));
        }
    }
}

fn phase_direct_unknown(lab: &Lab, db: &str) {
    let mut conn = lab.connect(db);
    let answer = conn.query(
        "BEGIN; SET ROLE oo_target_owner; SELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true); INSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'direct-unknown', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea, 'owner_direct', 1); COMMIT;",
    );
    lab.evidence.case("direct-unknown", &answer);
    if answer.status == b'E' {
        conn.rollback();
    }
    assert_eq!(answer.code.as_deref(), Some("23514"), "{}", answer.message.as_deref().unwrap_or(""));
    assert!(answer.message.as_deref().unwrap_or("").contains("apply_receipts_segment_check"), "{}", answer.message.as_deref().unwrap_or(""));
    let left = lab.one(
        db,
        "direct-unknown-absent",
        "SELECT count(*)::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'direct-unknown';",
    );
    assert_eq!(left, "0");
    lab.note("direct_unknown=23514");
}

fn phase_stale(lab: &Lab, db: &str) {
    let mut conn = lab.connect_as("oo_a104_runtime", db);
    let sql = format!(
        "{adopt}\nSELECT _oo.fail_target_generation('shard_stale', '{OWNER}'::uuid, 1);\n{adopt}\nDO $stale$ BEGIN BEGIN PERFORM _oo.commit_apply_receipt('shard_stale', '{OWNER}'::uuid, 1, 'warehouse', 'orders_g1', 'stale-bad', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea); RAISE EXCEPTION 'illegal stale segment was stored'; EXCEPTION WHEN SQLSTATE '23514' THEN NULL; WHEN SQLSTATE 'OO013' THEN RAISE EXCEPTION 'OO013 hid the illegal segment'; END; BEGIN PERFORM _oo.commit_apply_receipt('shard_stale', '{OWNER}'::uuid, 1, 'warehouse', 'orders_g1', 'stale-legal', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('f', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, NULL, ''::bytea); RAISE EXCEPTION 'stale legal receipt was stored'; EXCEPTION WHEN SQLSTATE 'OO013' THEN NULL; END; END $stale$;",
        adopt = adopt("shard_stale")
    );
    let answer = conn.query(&sql);
    lab.evidence.case("stale", &answer);
    assert!(answer.code.is_none(), "stale {}", answer.message.unwrap_or_default());
    let counts = lab.one(
        db,
        "stale-counts",
        "SELECT count(*)::text || '|' || (SELECT generation::text FROM _oo.target_generations WHERE shard_id = 'shard_stale') FROM _oo.apply_receipts WHERE txn_or_batch_id IN ('stale-bad', 'stale-legal');",
    );
    assert_eq!(counts, "0|2", "{counts}");
    lab.note("stale_illegal=23514 stale_legal=OO013");
}

fn phase_replay(lab: &Lab, db: &str) {
    lab.exec(
        db,
        "fixture-schema",
        "SET ROLE oo_target_owner; CREATE SCHEMA fixture_synth AUTHORIZATION oo_target_owner; REVOKE ALL ON SCHEMA fixture_synth FROM PUBLIC; GRANT USAGE ON SCHEMA fixture_synth TO oo_pipeline_runtime; CREATE TABLE fixture_synth.applied_row (batch_id text PRIMARY KEY, payload_digest text NOT NULL, applied_pos numeric(38, 0) NOT NULL); REVOKE ALL ON fixture_synth.applied_row FROM PUBLIC; GRANT SELECT, INSERT ON fixture_synth.applied_row TO oo_pipeline_runtime; RESET ROLE;",
    );
    let mut runtime = lab.connect_as("oo_a104_runtime", db);
    let setup = format!(
        "{}; {}",
        adopt("shard_replay"),
        receipt("shard_replay", 1, "batch-replay", 'e', "NULL", "NULL", "''::bytea")
    );
    let inserted = runtime.query(&setup);
    lab.evidence.case("replay-insert", &inserted);
    assert!(inserted.cells().contains("inserted"), "{}", inserted.cells());
    let stamp = lab.one(
        db,
        "replay-stamp",
        "SELECT payload_digest || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';",
    );
    let again = runtime.query(&receipt("shard_replay", 1, "batch-replay", 'e', "NULL", "NULL", "''::bytea"));
    lab.evidence.case("replay-again", &again);
    assert!(again.cells().contains("replay"), "{}", again.cells());
    let stamp2 = lab.one(
        db,
        "replay-stamp-2",
        "SELECT payload_digest || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';",
    );
    assert_eq!(stamp, stamp2, "replay changed the stored receipt");
    let conflict_sql = format!(
        "BEGIN; INSERT INTO fixture_synth.applied_row VALUES ('conflict-side', repeat('f', 64), 1); {} COMMIT;",
        receipt("shard_replay", 1, "batch-replay", 'f', "NULL", "NULL", "''::bytea")
    );
    let conflict = runtime.query(&conflict_sql);
    lab.evidence.case("conflict", &conflict);
    if conflict.status == b'E' {
        runtime.rollback();
    }
    assert_eq!(conflict.code.as_deref(), Some("OO010"), "{}", conflict.message.as_deref().unwrap_or(""));
    let side = lab.one(
        db,
        "conflict-side",
        "SELECT count(*)::text FROM fixture_synth.applied_row WHERE batch_id = 'conflict-side';",
    );
    assert_eq!(side, "0");
    let kept = lab.one(
        db,
        "conflict-kept",
        "SELECT payload_digest || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';",
    );
    assert_eq!(kept, stamp);
    lab.note("replay_immutable=true conflict=OO010 side_rollback=true");
}

fn phase_search_path(lab: &Lab, db: &str) {
    lab.exec(
        db,
        "evil-schema",
        "CREATE SCHEMA a104_evil; CREATE FUNCTION a104_evil.clock_timestamp() RETURNS timestamptz LANGUAGE sql AS $$ SELECT timestamptz '2000-01-01 00:00:00+00' $$; CREATE FUNCTION a104_evil.octet_length(bytea) RETURNS integer LANGUAGE sql AS $$ SELECT 0 $$; GRANT USAGE ON SCHEMA a104_evil TO oo_pipeline_runtime;",
    );
    let mut runtime = lab.connect_as("oo_a104_runtime", db);
    let sql = format!(
        "SET search_path = a104_evil, pg_catalog; {}; {}",
        adopt("shard_path"),
        receipt("shard_path", 1, "path-one", 'e', "0", "1", "'\\x01'::bytea")
    );
    let answer = runtime.query(&sql);
    lab.evidence.case("search-path", &answer);
    assert!(answer.code.is_none(), "search path {}", answer.message.unwrap_or_default());
    assert!(answer.cells().contains("inserted"), "{}", answer.cells());
    let year = lab.one(
        db,
        "search-path-year",
        "SELECT extract(year FROM target_commit_at)::integer::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'path-one';",
    );
    assert_eq!(year, "2026", "{year}");
    let ready = lab.one(
        db,
        "preflight-proconfig",
        "SELECT prosecdef::text || '|' || proconfig::text FROM pg_catalog.pg_proc JOIN pg_catalog.pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname = 'segment_preflight_ready';",
    );
    assert_eq!(ready, "false|{\"search_path=pg_catalog, _oo\"}", "{ready}");
    lab.note("search_path poison_rejected=true year=2026");
}

fn phase_acl(lab: &Lab, db: &str) {
    let mut runtime = lab.connect_as("oo_a104_runtime", db);
    let denied = runtime.query("INSERT INTO _oo.apply_receipts (source) VALUES ('warehouse');");
    lab.evidence.case("runtime-insert", &denied);
    assert_eq!(denied.code.as_deref(), Some("42501"), "{}", denied.message.as_deref().unwrap_or(""));
    let acl = lab.one(
        db,
        "acl",
        "SELECT pg_catalog.pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'member')::text || '|' || pg_catalog.has_function_privilege('oo_pipeline_runtime', receipt.oid, 'execute')::text || '|' || pg_catalog.has_function_privilege('oo_pipeline_runtime', ready.oid, 'execute')::text || '|' || pg_catalog.has_table_privilege('oo_pipeline_runtime', ready_view.oid, 'select')::text || '|' || pg_catalog.has_table_privilege('oo_pipeline_runtime', receipt_table.oid, 'insert')::text || '|' || pg_catalog.pg_get_userbyid(receipt.proowner) || '|' || receipt.prosecdef::text || '|' || receipt.proconfig::text FROM pg_catalog.pg_proc AS receipt JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = receipt.pronamespace JOIN pg_catalog.pg_proc AS ready ON ready.pronamespace = nsp.oid AND ready.proname = 'segment_preflight_ready' JOIN pg_catalog.pg_class AS ready_view ON ready_view.relnamespace = nsp.oid AND ready_view.relname = 'segment_preflight_readiness' JOIN pg_catalog.pg_class AS receipt_table ON receipt_table.relnamespace = nsp.oid AND receipt_table.relname = 'apply_receipts' WHERE nsp.nspname = '_oo' AND receipt.proname = 'commit_apply_receipt';",
    );
    assert_eq!(
        acl,
        "false|true|true|true|false|oo_target_owner|true|{\"search_path=pg_catalog, _oo\"}",
        "{acl}"
    );
    let view = lab.one(
        db,
        "readiness-view",
        "SELECT coalesce(reloptions::text, '') FROM pg_catalog.pg_class JOIN pg_catalog.pg_namespace ON pg_namespace.oid = relnamespace WHERE nspname = '_oo' AND relname = 'segment_preflight_readiness';",
    );
    assert!(view.contains("security_invoker=true"), "{view}");
    lab.note("acl runtime_insert=42501 owner_not_granted=true");
}

fn phase_lock_survives(lab: &Lab, db: &str) {
    let mut holder = lab.connect(db);
    let held = holder.query(
        "BEGIN; SET ROLE oo_target_owner; DO $touch$ BEGIN PERFORM 1 FROM _oo.apply_receipts WHERE false; EXCEPTION WHEN undefined_table THEN RAISE EXCEPTION 'touch saw undefined_table'; END $touch$; LOCK TABLE _oo.apply_receipts IN SHARE ROW EXCLUSIVE MODE; SELECT 'held';",
    );
    lab.evidence.case("lock-held", &held);
    assert!(held.code.is_none(), "holder {}", held.message.unwrap_or_default());
    assert_eq!(held.status, b'T');
    assert!(held.cells().contains("held"), "{}", held.cells());
    let started = Instant::now();
    let mut waiter = lab.connect(db);
    let waited = waiter.query(
        "BEGIN; SET LOCAL lock_timeout = '2s'; SET ROLE oo_target_owner; LOCK TABLE _oo.apply_receipts IN SHARE ROW EXCLUSIVE MODE; COMMIT;",
    );
    let elapsed = started.elapsed();
    lab.evidence.case("lock-waiter", &waited);
    if waited.status == b'E' {
        waiter.rollback();
    }
    holder.rollback();
    assert_eq!(waited.code.as_deref(), Some("55P03"), "{}", waited.message.as_deref().unwrap_or(""));
    assert!(elapsed < Duration::from_secs(8), "lock wait {elapsed:?}");
    lab.note(&format!("share_lock_survives_do=55P03 elapsed_ms={}", elapsed.as_millis()));
}

fn phase_lawful(lab: &Lab) {
    let db = "oo_pf_law";
    seed_lawful(lab, db, "shard_law");
    let before = snapshot(lab, db, "law-before");
    expect_apply(lab, db, "law", false, Route::SupersedeFromV1, None, None);
    let after = snapshot(lab, db, "law-after");
    assert_eq!(before, after, "lawful receipts changed");
    let versions = marker_rows(lab, db, "law-versions");
    assert!(versions.contains("target-fence-receipts/3|"), "{versions}");
    assert!(!versions.contains("target-fence-receipts/2"), "{versions}");
    let ready = lab.one(
        db,
        "law-ready",
        &format!("SELECT _oo.segment_preflight_ready('{V3_FILE}')::text;"),
    );
    assert_eq!(ready, "true");
    assert!(v3_driver_authorized(V3_FILE, ready == "true"));
    lab.note("lawful byte_equal=true v2_marker=absent");
}

fn phase_v2(lab: &Lab) {
    let db = "oo_pf_v2";
    seed_lawful(lab, db, "shard_v2");
    let before_receipts = snapshot(lab, db, "v2-receipts-before");
    install_historical_v2(lab, db);
    let after_v2 = snapshot(lab, db, "v2-receipts-after-0002");
    assert_eq!(before_receipts, after_v2);
    let before_marks = marker_rows(lab, db, "v2-marks-before");
    let before_fn = lab.one(
        db,
        "v2-fn-before",
        "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure('_oo.segment_contract_ready(text)'));",
    );
    expect_apply(lab, db, "v2-upgrade", false, Route::UpgradeFromV2, None, None);
    let after_marks = marker_rows(lab, db, "v2-marks-after");
    let after_fn = lab.one(
        db,
        "v2-fn-after",
        "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure('_oo.segment_contract_ready(text)'));",
    );
    let after_receipts = snapshot(lab, db, "v2-receipts-after");
    assert_eq!(before_receipts, after_receipts);
    assert_eq!(before_fn, after_fn);
    assert!(before_marks.contains(&format!("target-fence-receipts/2|{V2_FILE}|")), "{before_marks}");
    assert!(after_marks.contains("target-fence-receipts/1|"), "{after_marks}");
    for line in before_marks.lines() {
        assert!(after_marks.contains(line), "historical marker changed: {line}\n{after_marks}");
    }
    assert!(after_marks.contains(&format!("target-fence-receipts/3|{V3_FILE}|")), "{after_marks}");
    let ready = lab.one(
        db,
        "v2-ready",
        &format!(
            "SELECT _oo.segment_contract_ready('{V2_FILE}')::text || '|' || _oo.segment_preflight_ready('{V3_FILE}')::text || '|' || _oo.schema_ready('{V1_EXPAND}')::text;"
        ),
    );
    assert_eq!(ready, "true|true|true", "{ready}");
    assert!(!v3_driver_authorized(V2_FILE, true));
    assert!(v3_driver_authorized(V3_FILE, true));
    let owners = lab.one(
        db,
        "v2-owners",
        "SELECT coalesce(string_agg(proname || '=' || pg_catalog.pg_get_userbyid(proowner), ',' ORDER BY proname), '') FROM pg_catalog.pg_proc JOIN pg_catalog.pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname IN ('commit_apply_receipt', 'segment_contract_ready', 'segment_preflight_ready');",
    );
    assert_eq!(
        owners,
        "commit_apply_receipt=oo_target_owner,segment_contract_ready=oo_target_owner,segment_preflight_ready=oo_target_owner",
        "{owners}"
    );
    lab.note("v2_upgrade historical_markers_unchanged=true");
}

fn phase_illegal(lab: &Lab) {
    let db = "oo_pf_bad";
    seed_illegal(lab, db);
    let before = snapshot(lab, db, "bad-before");
    let before_def = constraintdef(lab, db, "bad-check-before");
    expect_apply(lab, db, "bad", false, Route::SupersedeFromV1, Some("OO021"), Some(HISTORY_MESSAGE));
    let after = snapshot(lab, db, "bad-after");
    let after_def = constraintdef(lab, db, "bad-check-after");
    assert_eq!(before, after);
    assert_eq!(before_def, after_def);
    assert!(!after_def.contains("IS TRUE"), "{after_def}");
    assert_eq!(v3_count(lab, db, "bad-marker"), "0");
    lab.note("illegal_history=OO021 receipts_unchanged=true");
}

fn phase_redo(lab: &Lab) {
    let db = "oo_pf_redo";
    expect_apply(lab, db, "redo-first", false, Route::SupersedeFromV1, None, None);
    let stamp = marker_rows(lab, db, "redo-stamp");
    let oid = lab.one(
        db,
        "redo-oid",
        "SELECT constraint_row.oid::text FROM pg_catalog.pg_constraint AS constraint_row WHERE constraint_row.conname = 'apply_receipts_segment_check' AND constraint_row.conrelid = pg_catalog.to_regclass('_oo.apply_receipts');",
    );
    expect_apply(lab, db, "redo-second", false, Route::Skip, None, None);
    let stamp2 = marker_rows(lab, db, "redo-stamp-2");
    let oid2 = lab.one(
        db,
        "redo-oid-2",
        "SELECT constraint_row.oid::text FROM pg_catalog.pg_constraint AS constraint_row WHERE constraint_row.conname = 'apply_receipts_segment_check' AND constraint_row.conrelid = pg_catalog.to_regclass('_oo.apply_receipts');",
    );
    assert_eq!(stamp, stamp2);
    assert_eq!(oid, oid2);
    lab.note("redo skip stamp_stable=true constraint_oid_stable=true");
}

fn phase_mismatch(lab: &Lab) {
    let db = "oo_pf_mis";
    lab.exec(
        db,
        "book-lie-v3",
        &format!(
            "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/3', '{LIE}', 'expand', '{FILE_V3}');"
        ),
    );
    let before = constraintdef(lab, db, "mis-check-before");
    expect_apply(lab, db, "mismatch", false, Route::MismatchV3, Some("OO001"), Some(MISMATCH_MESSAGE));
    let checksum = lab.one(
        db,
        "mismatch-checksum",
        "SELECT checksum FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';",
    );
    assert_eq!(checksum, LIE);
    let after = constraintdef(lab, db, "mis-check-after");
    assert_eq!(before, after);
    assert!(!after.contains("IS TRUE"), "{after}");
    lab.note("mismatch=OO001 row_unchanged=true file_not_executed=true");
}

fn phase_rollback(lab: &Lab) {
    let db = "oo_pf_roll";
    seed_lawful(lab, db, "shard_roll");
    let before = snapshot(lab, db, "roll-before");
    let before_def = constraintdef(lab, db, "roll-check-before");
    expect_apply(lab, db, "rollback", true, Route::SupersedeFromV1, Some("OO003"), Some("forced migration failure"));
    let after = snapshot(lab, db, "roll-after");
    let after_def = constraintdef(lab, db, "roll-check-after");
    assert_eq!(before, after);
    assert_eq!(before_def, after_def);
    assert!(!after_def.contains("IS TRUE"), "{after_def}");
    assert_eq!(v3_count(lab, db, "roll-marker"), "0");
    lab.note("forced_failure=OO003 rollback=true");
}

fn phase_lock(lab: &Lab) {
    let db = "oo_pf_lock";
    let mut holder = lab.connect(db);
    let held = holder.query("BEGIN; SELECT pg_catalog.pg_advisory_xact_lock(6404202605); SELECT 'locked';");
    lab.evidence.case("advisory-held", &held);
    assert!(held.code.is_none(), "holder {}", held.message.unwrap_or_default());
    assert_eq!(held.status, b'T');
    let started = Instant::now();
    let mut waiter = lab.connect(db);
    let out = apply_reference(&mut waiter, "2s", false);
    let elapsed = started.elapsed();
    lab.evidence.case(
        "advisory-waiter",
        &Answer {
            rows: Vec::new(),
            code: out.code.clone(),
            message: out.message.clone(),
            status: b'I',
        },
    );
    holder.rollback();
    assert_eq!(out.code.as_deref(), Some("55P03"), "{}", out.message.as_deref().unwrap_or(""));
    assert!(out.executed.is_empty());
    assert!(elapsed < Duration::from_secs(8), "advisory wait {elapsed:?}");
    assert_eq!(v3_count(lab, db, "lock-marker"), "0");
    lab.note(&format!("concurrent_lock=55P03 elapsed_ms={}", elapsed.as_millis()));
}

fn phase_lie(lab: &Lab) {
    let db = "oo_pf_lie";
    let mut conn = lab.connect(db);
    let out = book_caller_supplied_checksum(&mut conn, LIE);
    lab.evidence.case(
        "stored-lie",
        &Answer {
            rows: Vec::new(),
            code: out.code.clone(),
            message: out.message.clone(),
            status: b'I',
        },
    );
    assert!(out.code.is_none(), "lie book {}", out.message.unwrap_or_default());
    let flags = lab.one(
        db,
        "lie-flags",
        &format!(
            "SELECT _oo.schema_ready('{V1_EXPAND}')::text || '|' || _oo.segment_preflight_ready('{LIE}')::text || '|' || _oo.segment_preflight_ready('{V3_FILE}')::text || '|' || (SELECT checksum FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/1');"
        ),
    );
    assert_eq!(flags, format!("true|true|false|{V1_EXPAND}"), "{flags}");
    let ready_for_file = false;
    let ready_for_lie = true;
    assert!(!v3_driver_authorized(V3_FILE, ready_for_file));
    assert!(!v3_driver_authorized(LIE, ready_for_lie));
    lab.note("stored_lie conjunction=false v1_checksum_unchanged=true");
}

fn phase_owner(lab: &Lab) {
    let db = "oo_pf_own";
    lab.exec(
        db,
        "alter-owner",
        "DO $own$ DECLARE fn oid; BEGIN SELECT proc.oid INTO fn FROM pg_catalog.pg_proc AS proc JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = proc.pronamespace WHERE nsp.nspname = '_oo' AND proc.proname = 'commit_apply_receipt'; EXECUTE format('ALTER FUNCTION %s OWNER TO postgres', fn::regprocedure); END $own$;",
    );
    expect_apply(lab, db, "owner", false, Route::SupersedeFromV1, Some("42501"), None);
    let message = fs::read_to_string(lab.evidence.root.join("cases/owner.stderr")).expect("owner stderr");
    assert!(message.contains("target segment preflight refuses to adopt commit_apply_receipt"), "{message}");
    assert_eq!(v3_count(lab, db, "owner-marker"), "0");
    let owner = lab.one(
        db,
        "owner-still",
        "SELECT pg_catalog.pg_get_userbyid(proowner) FROM pg_catalog.pg_proc JOIN pg_catalog.pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname = 'commit_apply_receipt';",
    );
    assert_eq!(owner, "postgres");
    let def = constraintdef(lab, db, "owner-check");
    assert!(!def.contains("IS TRUE"), "{def}");
    lab.note("foreign_owner=42501 not_adopted=true");
}
