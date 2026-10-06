//! Independent boundary checks for cdc-spool. Expected bytes and digests are
//! computed in this file. This file does not call the author test helpers or
//! the product hash as the expected digest.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::fs::symlink;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use cdc_spool::{
    recover, verify_replay, Cell, FaultCode, Limits, Op, OutcomeKind, Recovery, Spool,
    INDEX_ALLOWANCE, PROGRESS_ALLOWANCE,
};

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TS: &str = "2026-04-15T08:30:00.123456Z";
const TS_Z: &str = "2026-04-15T08:30:01Z";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const SAMPLE_PREIMAGE: &str = "{\"data_collection_sequence\":2,\"global_txn_sequence\":3,\"key\":[{\"ordinal\":0,\"type\":\"int\",\"value\":\"1\"}],\"op\":\"upsert\",\"payload\":[{\"ordinal\":0,\"type\":\"text\",\"value\":\"a\"},{\"ordinal\":1,\"type\":\"null\"},{\"ordinal\":2,\"type\":\"unavailable\"}],\"table\":\"public.lines\",\"txn_id\":\"inv17\",\"unavailable_ordinals\":[2]}";
const SAMPLE_HASH: &str = "4864cde4bd37438f0fa3c69808c0adb5ddd244d2450987ad69299330b2b8e8ee";
const TOTAL: u64 = 500_000;
const CHUNK_EVENTS: u64 = 8_192;
const CHUNK_BYTES: u64 = 4 * 1024 * 1024;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_root() -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!("oo-a299-{}-{n}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[allow(clippy::cognitive_complexity)]
fn sha256(input: &[u8]) -> [u8; 32] {
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
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(input.len()).unwrap().checked_mul(8).unwrap();
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let small = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let large = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(small)
                .wrapping_add(word[index - 7])
                .wrapping_add(large);
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
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn check_literals() {
    assert_eq!(hex(&sha256(b"")), EMPTY_SHA);
    assert_eq!(hex(&sha256(b"abc")), ABC_SHA);
    assert_eq!(hex(&sha256(SAMPLE_PREIMAGE.as_bytes())), SAMPLE_HASH);
}

fn epoch1_position() -> String {
    let lsn = (0x16u64 << 32) + 0xB6_E40;
    let position = (1u128 << 64) + u128::from(lsn);
    assert_eq!(lsn, 94_490_029_632);
    let text = position.to_string();
    assert_eq!(text, "18446744168199581248");
    text
}

fn place(seq: u64) -> (&'static str, u64) {
    if seq.is_multiple_of(2) {
        ("public.lines", seq / 2)
    } else {
        ("public.orders", seq.div_ceil(2))
    }
}

#[allow(clippy::too_many_arguments)]
fn preimage(
    txn: &str,
    global: u64,
    collection: u64,
    table: &str,
    key_value: &str,
    op: &str,
    payload: &str,
    unavailable: &str,
) -> String {
    format!(
        "{{\"data_collection_sequence\":{collection},\"global_txn_sequence\":{global},\"key\":[{{\"ordinal\":0,\"type\":\"int\",\"value\":\"{key_value}\"}}],\"op\":\"{op}\",\"payload\":{payload},\"table\":\"{table}\",\"txn_id\":\"{txn}\",\"unavailable_ordinals\":{unavailable}}}"
    )
}

fn bulk_preimage(seq: u64) -> String {
    let (table, collection) = place(seq);
    let payload = format!("[{{\"ordinal\":0,\"type\":\"int\",\"value\":\"{seq}\"}}]");
    preimage("rev500", seq, collection, table, &seq.to_string(), "upsert", &payload, "[]")
}

fn bind_two() -> String {
    format!(
        "{{\"kind\":\"bind\",\"source\":\"billing\",\"lineage\":\"{HASH}\",\"epoch\":1,\"selected_tables\":[\"public.lines\",\"public.orders\"],\"schema_hash\":\"{HASH}\",\"config_hash\":\"{HASH}\",\"type_hash\":\"{HASH}\",\"hook_hashes\":[],\"commit_position_provenance\":\"caller_declared\"}}"
    )
}

fn bind_orders() -> String {
    format!(
        "{{\"kind\":\"bind\",\"source\":\"billing\",\"lineage\":\"{HASH}\",\"epoch\":1,\"selected_tables\":[\"public.orders\"],\"schema_hash\":\"{HASH}\",\"config_hash\":\"{HASH}\",\"type_hash\":\"{HASH}\",\"hook_hashes\":[],\"commit_position_provenance\":\"caller_declared\"}}"
    )
}

fn begin(txn: &str) -> String {
    format!("{{\"kind\":\"begin\",\"txn_id\":\"{txn}\"}}")
}

#[allow(clippy::too_many_arguments)]
fn data_line(
    txn: &str,
    global: u64,
    collection: u64,
    table: &str,
    key_value: &str,
    op: &str,
    payload: &str,
    unavailable: &str,
) -> String {
    format!(
        "{{\"kind\":\"data\",\"txn_id\":\"{txn}\",\"global_txn_sequence\":{global},\"data_collection_sequence\":{collection},\"table\":\"{table}\",\"key\":[{{\"type\":\"int\",\"ordinal\":0,\"value\":\"{key_value}\"}}],\"op\":\"{op}\",\"payload\":{payload},\"unavailable_ordinals\":{unavailable}}}"
    )
}

fn end_line(txn: &str, count: u64, lines: u64, orders: u64, lsn: &str, timestamp: &str) -> String {
    format!(
        "{{\"kind\":\"end\",\"txn_id\":\"{txn}\",\"count\":{count},\"per_collection_counts\":{{\"public.lines\":{lines},\"public.orders\":{orders}}},\"commit_lsn\":\"{lsn}\",\"source_commit_timestamp\":\"{timestamp}\"}}"
    )
}

fn end_orders(txn: &str, count: u64, orders: u64, lsn: &str, timestamp: &str) -> String {
    format!(
        "{{\"kind\":\"end\",\"txn_id\":\"{txn}\",\"count\":{count},\"per_collection_counts\":{{\"public.orders\":{orders}}},\"commit_lsn\":\"{lsn}\",\"source_commit_timestamp\":\"{timestamp}\"}}"
    )
}

fn feed_bytes(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

fn ingest(dir: &Path, limits: Limits, bytes: &[u8]) -> Result<cdc_spool::Outcome, cdc_spool::Fault> {
    let mut spool = Spool::create_new(dir, limits)?;
    spool.ingest_reader(std::io::Cursor::new(bytes))
}

fn refused_create(dir: &Path) -> cdc_spool::Fault {
    match Spool::create_new(dir, Limits::product()) {
        Ok(_) => panic!("create_new accepted {}", dir.display()),
        Err(err) => err,
    }
}

fn sample_lines() -> Vec<String> {
    vec![
        bind_two(),
        begin("inv17"),
        data_line(
            "inv17",
            1,
            1,
            "public.orders",
            "17",
            "upsert",
            "[{\"type\":\"text\",\"ordinal\":0,\"value\":\"open\"}]",
            "[]",
        ),
        data_line("inv17", 2, 1, "public.lines", "1", "delete", "[]", "[]"),
        data_line(
            "inv17",
            3,
            2,
            "public.lines",
            "1",
            "upsert",
            "[{\"type\":\"text\",\"ordinal\":0,\"value\":\"a\"},{\"type\":\"null\",\"ordinal\":1},{\"type\":\"unavailable\",\"ordinal\":2}]",
            "[2]",
        ),
        end_line("inv17", 3, 2, 1, "16/B6E40", TS),
    ]
}

fn seal_sample(root: &Path) -> (PathBuf, Vec<u8>) {
    let dir = root.join("txn");
    let bytes = feed_bytes(&sample_lines());
    let outcome = ingest(&dir, Limits::product(), &bytes).unwrap();
    assert!(matches!(outcome.kind, OutcomeKind::Sealed(_)));
    (dir, bytes)
}

fn vm_hwm() -> Option<u64> {
    let text = fs::read_to_string("/proc/self/status").ok()?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("VmHWM:") else {
            continue;
        };
        let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        return kb.checked_mul(1024);
    }
    None
}

fn digest_tree(dir: &Path) -> Vec<(String, String)> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let meta = fs::symlink_metadata(entry.path()).unwrap();
        assert!(!meta.file_type().is_symlink(), "{}", entry.path().display());
        assert!(meta.is_file(), "{}", entry.path().display());
        names.push(entry.file_name().into_string().unwrap());
    }
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let bytes = fs::read(dir.join(&name)).unwrap();
            (name, hex(&sha256(&bytes)))
        })
        .collect()
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir(dst).unwrap();
    for (name, _) in digest_tree(src) {
        fs::copy(src.join(&name), dst.join(&name)).unwrap();
    }
}

fn assert_no_ack_files(dir: &Path) {
    assert!(!dir.join("SEAL").exists());
    assert!(!dir.join("ROLLED_BACK").exists());
}

fn int_cell(ordinal: u16, value: &str) -> Cell {
    Cell::Int {
        ordinal,
        value: value.to_owned(),
    }
}

#[test]
fn known_literals_match_the_independent_oracle() {
    check_literals();
    let position = epoch1_position();
    assert_eq!(position, "18446744168199581248");
}

fn write_bulk(path: &Path) {
    let mut out = BufWriter::new(File::create(path).unwrap());
    writeln!(out, "{}", bind_two()).unwrap();
    writeln!(out, "{}", begin("rev500")).unwrap();
    for seq in 1..=TOTAL {
        let (table, collection) = place(seq);
        let payload = format!("[{{\"ordinal\":0,\"type\":\"int\",\"value\":\"{seq}\"}}]");
        writeln!(
            out,
            "{}",
            data_line("rev500", seq, collection, table, &seq.to_string(), "upsert", &payload, "[]")
        )
        .unwrap();
    }
    writeln!(out, "{}", end_line("rev500", TOTAL, 250_000, 250_000, "16/B6E40", TS)).unwrap();
    out.flush().unwrap();
}

fn check_bulk_chunks(dir: &Path, manifest: &cdc_spool::Manifest) {
    let full = manifest.count / CHUNK_EVENTS;
    let rem = manifest.count % CHUNK_EVENTS;
    assert_eq!(u64::try_from(manifest.chunks.len()).unwrap(), full + u64::from(rem > 0));
    let mut set = String::new();
    let mut events = 0u64;
    for (index, chunk) in manifest.chunks.iter().enumerate() {
        let expect_events = if index + 1 == manifest.chunks.len() {
            rem
        } else {
            CHUNK_EVENTS
        };
        assert_eq!(chunk.events, expect_events, "{}", chunk.name);
        assert!(chunk.bytes <= CHUNK_BYTES && chunk.bytes > 0, "{}", chunk.name);
        assert_eq!(chunk.name, format!("chunk-{:06}", index + 1));
        let bytes = fs::read(dir.join(&chunk.name)).unwrap();
        assert_eq!(u64::try_from(bytes.len()).unwrap(), chunk.bytes);
        assert_eq!(hex(&sha256(&bytes)), chunk.sha256);
        set.push_str(&chunk.sha256);
        set.push('\n');
        events += chunk.events;
    }
    assert_eq!(events, TOTAL);
    assert_eq!(manifest.chunk_set_sha256, hex(&sha256(set.as_bytes())));
    assert!(manifest.chunks.len() < 96);
}

fn check_bulk_rows(committed: &cdc_spool::Committed) {
    let mut iter = committed.iter().unwrap();
    let mut orders = 0u64;
    let mut lines = 0u64;
    for seq in 1..=TOTAL {
        let event = iter.next().unwrap().unwrap();
        let expected = bulk_preimage(seq);
        let (table, collection) = place(seq);
        assert_eq!(event.preimage, expected, "{seq}");
        assert_eq!(event.event_hash, hex(&sha256(expected.as_bytes())), "{seq}");
        assert_eq!(event.global_txn_sequence, seq);
        assert_eq!(event.data_collection_sequence, collection);
        assert_eq!(event.table, table);
        assert_eq!(event.op, Op::Upsert);
        assert_eq!(event.key, vec![int_cell(0, &seq.to_string())]);
        assert_eq!(event.payload, vec![int_cell(0, &seq.to_string())]);
        assert!(event.unavailable_ordinals.is_empty());
        if table == "public.orders" {
            orders += 1;
        } else {
            lines += 1;
        }
    }
    assert!(iter.next().is_none());
    assert_eq!(orders, 250_000);
    assert_eq!(lines, 250_000);
}

fn record_hwm(name: &str, body: &str) {
    if let Ok(hwm_dir) = env::var("REVIEW_HWM_DIR") {
        fs::write(Path::new(&hwm_dir).join(name), body).unwrap();
    }
}

#[test]
fn five_hundred_thousand_events_match_hand_formulas() {
    check_literals();
    let position = epoch1_position();
    let root = temp_root();
    let feed = root.join("feed.jsonl");
    write_bulk(&feed);
    let dir = root.join("txn");
    let mut spool = Spool::create_new(&dir, Limits::product()).unwrap();
    let outcome = spool
        .ingest_reader(BufReader::new(File::open(&feed).unwrap()))
        .expect("seal");
    let OutcomeKind::Sealed(summary) = outcome.kind else {
        panic!("not sealed");
    };
    // VmHWM on the seal summary is read inside finish, before this replay.
    let seal_hwm = summary.resident_hwm_bytes.expect("seal VmHWM");
    assert!(seal_hwm > 0 && seal_hwm < 256 * 1024 * 1024, "{seal_hwm}");
    record_hwm("seal-hwm.txt", &format!("{seal_hwm}\n"));
    assert_eq!(summary.count, TOTAL);
    assert_eq!(summary.commit_position, position);
    assert!(!summary.ack_allowed);
    assert!(!summary.target_applied);
    assert!(!summary.cross_table_consistent);
    assert_eq!(outcome.progress.durable_events, TOTAL);
    assert!(outcome.progress.applied.is_none());

    let Recovery::Committed(committed) = recover(&dir).unwrap() else {
        panic!("not committed");
    };
    assert!(!committed.ack_allowed());
    let manifest = &committed.manifest;
    assert_eq!(manifest.contract, "cdc-spool-v1");
    assert_eq!(manifest.source, "billing");
    assert_eq!(manifest.lineage, HASH);
    assert_eq!(manifest.epoch, 1);
    assert_eq!(manifest.schema_hash, HASH);
    assert_eq!(manifest.config_hash, HASH);
    assert_eq!(manifest.type_hash, HASH);
    assert!(manifest.hook_hashes.is_empty());
    assert_eq!(manifest.selected_tables, vec!["public.lines".to_owned(), "public.orders".to_owned()]);
    assert_eq!(manifest.commit_position_provenance, "caller_declared");
    assert_eq!(manifest.commit_lsn, "16/B6E40");
    assert_eq!(manifest.commit_position, position);
    assert_eq!(manifest.count, TOTAL);
    assert_eq!(manifest.first_sequence, Some(1));
    assert_eq!(manifest.last_sequence, Some(TOTAL));
    assert_eq!(manifest.per_collection_counts.get("public.lines").copied(), Some(250_000));
    assert_eq!(manifest.per_collection_counts.get("public.orders").copied(), Some(250_000));
    assert!(!manifest.ack_allowed);
    assert!(!manifest.target_applied);
    assert!(!manifest.cross_table_consistent);
    assert!(manifest.applied.is_null());
    assert!(!manifest.upstream_commit_lsn_verified);
    assert!(!manifest.debezium_normalizer);
    assert!(!manifest.p2_verified);
    assert!(!manifest.p11_verified);
    assert_eq!(manifest.limits.max_events, TOTAL);
    assert_eq!(manifest.limits.max_chunk_events, CHUNK_EVENTS);
    assert_eq!(manifest.limits.max_chunk_bytes, CHUNK_BYTES);
    check_bulk_chunks(&dir, manifest);
    check_bulk_rows(&committed);

    let before = digest_tree(&dir);
    verify_replay(&dir, BufReader::new(File::open(&feed).unwrap())).expect("exact replay");
    assert_eq!(digest_tree(&dir), before);
    let replay_hwm = vm_hwm().expect("replay VmHWM");
    assert!(replay_hwm >= seal_hwm, "seal={seal_hwm} replay={replay_hwm}");
    assert!(replay_hwm < 256 * 1024 * 1024, "{replay_hwm}");
    record_hwm(
        "replay-hwm.txt",
        &format!("seal_before_replay={seal_hwm}\nafter_replay={replay_hwm}\nprior_author_seal_not_this_run=27754496\n"),
    );
    eprintln!("seal_hwm={seal_hwm} replay_hwm={replay_hwm}");
}

#[test]
fn ordinal_32767_seals_and_32768_and_65535_are_not_stored() {
    let root = temp_root();
    let dir = root.join("ok");
    let payload = "[{\"type\":\"int\",\"ordinal\":32767,\"value\":\"9\"}]";
    let bytes = feed_bytes(&[
        bind_orders(),
        begin("ord1"),
        data_line("ord1", 1, 1, "public.orders", "1", "upsert", payload, "[]"),
        end_orders("ord1", 1, 1, "0/1", TS_Z),
    ]);
    ingest(&dir, Limits::product(), &bytes).unwrap();
    let Recovery::Committed(committed) = recover(&dir).unwrap() else {
        panic!("not committed");
    };
    let event = committed.iter().unwrap().next().unwrap().unwrap();
    let expected = preimage(
        "ord1",
        1,
        1,
        "public.orders",
        "1",
        "upsert",
        "[{\"ordinal\":32767,\"type\":\"int\",\"value\":\"9\"}]",
        "[]",
    );
    assert_eq!(event.preimage, expected);
    assert_eq!(event.event_hash, hex(&sha256(expected.as_bytes())));
    assert_eq!(event.payload, vec![int_cell(32_767, "9")]);
    assert!(!committed.ack_allowed());

    for bad in ["32768", "65535"] {
        let root = temp_root();
        let dir = root.join("bad");
        let payload = format!("[{{\"type\":\"int\",\"ordinal\":{bad},\"value\":\"9\"}}]");
        let bytes = feed_bytes(&[
            bind_orders(),
            begin("ord1"),
            data_line("ord1", 1, 1, "public.orders", "1", "upsert", &payload, "[]"),
            end_orders("ord1", 1, 1, "0/1", TS_Z),
        ]);
        let err = ingest(&dir, Limits::product(), &bytes).expect_err(bad);
        assert_eq!(err.code, FaultCode::OrdinalRange, "{bad}");
        assert!(!dir.join("chunk-000001").exists(), "{bad}");
        assert!(!dir.join("chunk-000001.partial").exists(), "{bad}");
        assert!(!dir.join("SEAL").exists(), "{bad}");
        assert!(!dir.join("manifest.json").exists(), "{bad}");
    }
}

fn write_kill_feed(path: &Path) {
    let mut out = BufWriter::new(File::create(path).unwrap());
    writeln!(out, "{}", bind_orders()).unwrap();
    writeln!(out, "{}", begin("kill1")).unwrap();
    for seq in 1..=CHUNK_EVENTS {
        writeln!(
            out,
            "{}",
            data_line(
                "kill1",
                seq,
                seq,
                "public.orders",
                &seq.to_string(),
                "upsert",
                &format!("[{{\"ordinal\":0,\"type\":\"int\",\"value\":\"{seq}\"}}]"),
                "[]",
            )
        )
        .unwrap();
    }
    writeln!(out, "{}", end_orders("kill1", CHUNK_EVENTS, CHUNK_EVENTS, "0/2", TS_Z)).unwrap();
    out.flush().unwrap();
}

fn kill_at(checkpoint: &str) -> PathBuf {
    let root = temp_root();
    let dir = root.join("txn");
    let mark = root.join("mark");
    let feed = root.join("feed.jsonl");
    write_kill_feed(&feed);
    let mut child = Command::new(env!("CARGO_BIN_EXE_cdc-spool"))
        .arg("spool")
        .arg(&dir)
        .env("CDC_SPOOL_CHECKPOINT", checkpoint)
        .env("CDC_SPOOL_MARK", &mark)
        .stdin(Stdio::from(File::open(&feed).unwrap()))
        .stdout(Stdio::from(File::create(root.join("child.out")).unwrap()))
        .stderr(Stdio::from(File::create(root.join("child.err")).unwrap()))
        .spawn()
        .unwrap();
    for _ in 0..600 {
        if mark.is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !mark.is_file() {
        let err = fs::read_to_string(root.join("child.err")).unwrap_or_default();
        panic!("mark missing for {checkpoint}: {err}");
    }
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(9), "{checkpoint} {status:?}");
    dir
}

fn assert_incomplete(dir: &Path) -> cdc_spool::Pending {
    assert_no_ack_files(dir);
    match recover(dir).unwrap() {
        Recovery::Incomplete(pending) => {
            assert!(!pending.ack_allowed);
            assert!(!pending.target_applied);
            assert!(!pending.cross_table_consistent);
            pending
        }
        other => panic!("not incomplete: {other:?}"),
    }
}

#[test]
fn sigkill_before_chunk_sync_is_incomplete_without_ack() {
    let dir = kill_at("before_chunk_sync");
    assert!(dir.join("chunk-000001.partial").is_file());
    assert!(!dir.join("chunk-000001").exists());
    assert!(!dir.join("manifest.json").exists());
    assert!(!fs::read(dir.join("chunk-000001.partial")).unwrap().is_empty());
    let pending = assert_incomplete(&dir);
    assert_eq!(pending.durable_events, 0);
}

#[test]
fn sigkill_before_end_is_incomplete_without_ack() {
    let dir = kill_at("before_end");
    assert!(dir.join("chunk-000001").is_file());
    assert!(dir.join("chunks.idx").is_file());
    assert!(!dir.join("manifest.json").exists());
    let pending = assert_incomplete(&dir);
    assert_eq!(pending.durable_events, CHUNK_EVENTS);
    assert!(!pending.ack_allowed);
}

#[test]
fn sigkill_before_final_seal_is_incomplete_without_ack() {
    let dir = kill_at("before_final_seal");
    assert!(dir.join("manifest.json").is_file());
    assert!(dir.join("chunk-000001").is_file());
    let text = fs::read_to_string(dir.join("manifest.json")).unwrap();
    assert!(text.contains("\"ack_allowed\":false"));
    assert!(text.contains("\"debezium_normalizer\":false"));
    assert!(text.contains("\"applied\":null"));
    let pending = assert_incomplete(&dir);
    assert!(!pending.ack_allowed);
}

#[test]
fn torn_seal_is_unknown_without_ack_and_keeps_files() {
    let root = temp_root();
    let (dir, _) = seal_sample(&root);
    let before = digest_tree(&dir);
    fs::write(dir.join("SEAL"), b"nope").unwrap();
    match recover(&dir).unwrap() {
        Recovery::Unknown { reason } => assert_eq!(reason, "seal_torn"),
        other => panic!("not unknown: {other:?}"),
    }
    assert!(!dir.join("ROLLED_BACK").exists());
    assert!(dir.join("chunk-000001").is_file());
    assert!(dir.join("manifest.json").is_file());
    let mut after = digest_tree(&dir);
    after.retain(|(name, _)| name != "SEAL");
    let mut kept = before;
    kept.retain(|(name, _)| name != "SEAL");
    assert_eq!(after, kept);
}

#[test]
fn exact_replay_matches_and_payload_conflict_writes_nothing() {
    check_literals();
    let root = temp_root();
    let (dir, bytes) = seal_sample(&root);
    let Recovery::Committed(committed) = recover(&dir).unwrap() else {
        panic!("committed");
    };
    let mut seen = Vec::new();
    for item in committed.iter().unwrap() {
        seen.push(item.unwrap());
    }
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[2].preimage, SAMPLE_PREIMAGE);
    assert_eq!(seen[2].event_hash, SAMPLE_HASH);
    assert_eq!(seen[2].event_hash, hex(&sha256(SAMPLE_PREIMAGE.as_bytes())));
    assert_eq!(
        seen[0].preimage,
        preimage(
            "inv17",
            1,
            1,
            "public.orders",
            "17",
            "upsert",
            "[{\"ordinal\":0,\"type\":\"text\",\"value\":\"open\"}]",
            "[]",
        )
    );
    assert_eq!(
        seen[1].preimage,
        preimage("inv17", 2, 1, "public.lines", "1", "delete", "[]", "[]")
    );
    assert!(!committed.manifest.ack_allowed);
    assert!(!committed.manifest.debezium_normalizer);
    assert!(committed.manifest.applied.is_null());
    let before = digest_tree(&dir);
    verify_replay(&dir, std::io::Cursor::new(bytes.clone())).unwrap();
    assert_eq!(digest_tree(&dir), before);
    let mut conflict = bytes;
    let needle = b"\"value\":\"a\"";
    let start = conflict.windows(needle.len()).position(|window| window == needle).unwrap();
    conflict[start + needle.len() - 2] = b'b';
    let err = verify_replay(&dir, std::io::Cursor::new(conflict)).unwrap_err();
    assert_eq!(err.code, FaultCode::PayloadConflict);
    assert_eq!(digest_tree(&dir), before);
}

#[test]
fn missing_truncated_and_checksum_chunks_are_refused() {
    let root = temp_root();
    let (dir, _) = seal_sample(&root);
    let missing = root.join("missing");
    copy_tree(&dir, &missing);
    fs::remove_file(missing.join("chunk-000001")).unwrap();
    let err = recover(&missing).unwrap_err();
    assert_eq!(err.code, FaultCode::MissingChunk);
    assert!(missing.join("SEAL").is_file());
    assert!(missing.join("manifest.json").is_file());

    let short = root.join("short");
    copy_tree(&dir, &short);
    let mut bytes = fs::read(short.join("chunk-000001")).unwrap();
    bytes.pop();
    fs::write(short.join("chunk-000001"), &bytes).unwrap();
    let err = recover(&short).unwrap_err();
    assert_eq!(err.code, FaultCode::Truncated);
    assert!(short.join("SEAL").is_file());

    let flipped = root.join("flipped");
    copy_tree(&dir, &flipped);
    let mut bytes = fs::read(flipped.join("chunk-000001")).unwrap();
    bytes[10] ^= 0x01;
    fs::write(flipped.join("chunk-000001"), &bytes).unwrap();
    let err = recover(&flipped).unwrap_err();
    assert_eq!(err.code, FaultCode::ChecksumFault);
    assert!(flipped.join("manifest.json").is_file());
    assert!(!flipped.join("ROLLED_BACK").exists());
}

#[test]
fn manifest_and_seal_checksums_are_refused() {
    let root = temp_root();
    let (dir, _) = seal_sample(&root);
    let manifest_dir = root.join("manifest");
    copy_tree(&dir, &manifest_dir);
    let mut body = fs::read(manifest_dir.join("manifest.json")).unwrap();
    let last = body.len() - 1;
    body[last] ^= 0x01;
    fs::write(manifest_dir.join("manifest.json"), &body).unwrap();
    let err = recover(&manifest_dir).unwrap_err();
    assert_eq!(err.code, FaultCode::ChecksumFault);
    assert!(manifest_dir.join("chunk-000001").is_file());

    let seal_dir = root.join("seal");
    copy_tree(&dir, &seal_dir);
    fs::write(seal_dir.join("SEAL"), format!("{}\n", "0".repeat(64))).unwrap();
    let err = recover(&seal_dir).unwrap_err();
    assert_eq!(err.code, FaultCode::ChecksumFault);
    assert!(seal_dir.join("manifest.json").is_file());
    assert!(seal_dir.join("chunk-000001").is_file());
    assert!(!seal_dir.join("ROLLED_BACK").exists());
}

#[test]
fn foreign_existing_paths_are_not_overwritten() {
    let root = temp_root();
    let file = root.join("file");
    fs::write(&file, b"keep-file").unwrap();
    let err = refused_create(&file);
    assert_eq!(err.code, FaultCode::PathExists);
    assert_eq!(fs::read(&file).unwrap(), b"keep-file");

    let pending = root.join("pending");
    fs::create_dir(&pending).unwrap();
    fs::write(pending.join("note"), b"foreign").unwrap();
    let err = refused_create(&pending);
    assert_eq!(err.code, FaultCode::PathExists);
    assert_eq!(fs::read(pending.join("note")).unwrap(), b"foreign");

    let (sealed, _) = seal_sample(&root);
    let before = fs::read(sealed.join("SEAL")).unwrap();
    let err = refused_create(&sealed);
    assert_eq!(err.code, FaultCode::PathExists);
    assert_eq!(fs::read(sealed.join("SEAL")).unwrap(), before);

    let real = root.join("real");
    fs::create_dir(&real).unwrap();
    let link = root.join("link");
    symlink(&real, &link).unwrap();
    let err = refused_create(&link.join("txn"));
    assert_eq!(err.code, FaultCode::SymlinkRefused);
    assert!(!real.join("txn").exists());
}

#[test]
fn disk_quota_is_an_internal_counter() {
    assert_eq!(INDEX_ALLOWANCE, 128);
    assert_eq!(PROGRESS_ALLOWANCE, 512);
    let stored = preimage("q1", 1, 1, "public.orders", "1", "upsert", "[]", "[]");
    let line_len = u64::try_from(stored.len()).unwrap() + 1;
    let mut limits = Limits::product();
    limits.max_chunk_events = 1;
    limits.max_disk_bytes = line_len + INDEX_ALLOWANCE + PROGRESS_ALLOWANCE;
    assert!(limits.max_disk_bytes < 256 * 1024 * 1024);
    let root = temp_root();
    let dir = root.join("txn");
    let bytes = feed_bytes(&[
        bind_orders(),
        begin("q1"),
        data_line("q1", 1, 1, "public.orders", "1", "upsert", "[]", "[]"),
        data_line("q1", 2, 2, "public.orders", "2", "upsert", "[]", "[]"),
        end_orders("q1", 2, 2, "0/2", TS_Z),
    ]);
    let err = ingest(&dir, limits, &bytes).unwrap_err();
    assert_eq!(err.code, FaultCode::DiskQuota);
    assert!(dir.join("chunk-000001").is_file());
    assert!(!dir.join("chunk-000002").exists());
    assert!(!dir.join("SEAL").exists());
    assert!(!dir.join("ROLLED_BACK").exists());
    fs::write(dir.join("still-writable"), b"ok").unwrap();
    assert_eq!(fs::read(dir.join("still-writable")).unwrap(), b"ok");
}

#[test]
fn end_too_early_differs_from_collection_unknown() {
    let root = temp_root();
    let unknown = root.join("unknown");
    let bytes = feed_bytes(&[
        bind_orders(),
        begin("e0"),
        end_orders("e0", 1, 1, "0/1", TS_Z),
    ]);
    let err = ingest(&unknown, Limits::product(), &bytes).unwrap_err();
    assert_eq!(err.code, FaultCode::CollectionUnknown);
    assert_no_ack_files(&unknown);
    assert!(!unknown.join("manifest.json").exists());

    let dir = root.join("early");
    let mut spool = Spool::create_new(&dir, Limits::product()).unwrap();
    let early = feed_bytes(&[
        bind_orders(),
        begin("e1"),
        data_line("e1", 1, 1, "public.orders", "4", "upsert", "[]", "[]"),
        end_orders("e1", 2, 2, "0/1", TS_Z),
    ]);
    let err = spool.ingest_reader(std::io::Cursor::new(early)).unwrap_err();
    assert_eq!(err.code, FaultCode::EndTooEarly);
    assert_no_ack_files(&dir);
    let rest = feed_bytes(&[
        data_line("e1", 2, 2, "public.orders", "5", "upsert", "[]", "[]"),
        end_orders("e1", 2, 2, "0/1", TS_Z),
    ]);
    let outcome = spool.ingest_reader(std::io::Cursor::new(rest)).unwrap();
    assert!(matches!(outcome.kind, OutcomeKind::Sealed(_)));
    assert!(dir.join("SEAL").is_file());
    let Recovery::Committed(committed) = recover(&dir).unwrap() else {
        panic!("sealed");
    };
    assert_eq!(committed.manifest.count, 2);
    assert!(!committed.ack_allowed());
    assert!(!committed.manifest.debezium_normalizer);
    assert!(committed.manifest.applied.is_null());
}
