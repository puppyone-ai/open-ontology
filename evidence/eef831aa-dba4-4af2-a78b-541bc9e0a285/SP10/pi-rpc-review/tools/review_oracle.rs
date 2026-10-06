//! Review oracle for the H-D01 Pi RPC spike. Std only. Not a product crate.
//! Hash checks use the FIPS 180-4 function in this file. Nothing shells out.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const NODE_TARBALL_SHA: &str = "5ced2d48d1d7198739b7f86804de0171aefb6823b684b12341d3321afc3cb0b2";
const NODE_SUMS_SHA: &str = "4fe99a2ba9d552a6f51c13ed68fb11104cfa5df601aec616be689253a8139e7a";
const NPM_TARBALL_SHA: &str = "04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5";
const GITHUB_TARBALL_SHA: &str = "a95c2ded6a2d8b56247f4205c99e3bef6241b5b51044086d4185d75c2822c521";
const PI_LICENSE_SHA: &str = "0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48";
const NODE_LICENSE_SHA: &str = "c738ae413cf561f174e34f6961f8ca458aae2369a73640dda6234c629b98bcc4";
const LOCK_SHA: &str = "35fd666029cb44d3cceb5a372cbacd2aecc478c69daa14898f006af291d4d869";
const TOKEN: &str = "public-dummy-fixture-token";

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("review_oracle: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), String> {
    check_sha256()?;
    let mut args = env::args().skip(1);
    let cmd = args.next().ok_or("usage")?;
    match cmd.as_str() {
        "spike" => {
            let work = take_path(&mut args)?;
            spike(&work)?;
        }
        "historical" => {
            let clippy_exit = take_path(&mut args)?;
            let compare = take_path(&mut args)?;
            historical(&clippy_exit, &compare)?;
        }
        "same" => {
            let left = take_path(&mut args)?;
            let right = take_path(&mut args)?;
            same(&left, &right)?;
        }
        "boundary" => {
            let stdout = take_path(&mut args)?;
            boundary(&stdout)?;
        }
        "pin" => pin(&mut args)?,
        "live" => {
            let proof = take_path(&mut args)?;
            let spike_dir = take_path(&mut args)?;
            live(&proof, &spike_dir)?;
        }
        _ => return Err("usage: spike|historical|same|boundary|pin|live".to_owned()),
    }
    if args.next().is_some() {
        return Err("unexpected extra argument".to_owned());
    }
    Ok(())
}

fn take_path(args: &mut impl Iterator<Item = String>) -> Result<PathBuf, String> {
    args.next().map(PathBuf::from).ok_or_else(|| "missing path".to_owned())
}

fn spike(work: &Path) -> Result<(), String> {
    let drive = read_string(&work.join("src/drive.rs"))?;
    if !drive.contains("if let Ok(mut guard) = slot.lock()") || !drive.contains("&& guard.len() < 262_144") {
        return Err("drive.rs is missing the b0cc1d24 let chain".to_owned());
    }
    if !drive.contains("262_144 - guard.len()") {
        return Err("drive.rs room calculation changed".to_owned());
    }
    println!("drive_let_chain=present");
    let map = read_string(&work.join("src/map.rs"))?;
    if map.matches("TurnFinished").count() != 2 {
        return Err("map.rs TurnFinished count is not the enum plus the stop arm".to_owned());
    }
    if !map.contains("stop == \"aborted\"") || !map.contains("if stop == \"stop\"") {
        return Err("map.rs abort or stop arm missing".to_owned());
    }
    let stop_at = map.find("if stop == \"stop\"").ok_or("stop arm")?;
    let finished_at = map.rfind("TurnFinished").ok_or("TurnFinished")?;
    if finished_at < stop_at {
        return Err("TurnFinished is not in the stop arm".to_owned());
    }
    println!("abort_is_not_turn_finished_source=ok");
    let lock = read_string(&work.join("Cargo.lock"))?;
    let names = lock.lines().filter(|line| line.starts_with("name = ")).count();
    if names != 1 || !lock.contains("name = \"oo-p09-pi-rpc\"") {
        return Err(format!("Cargo.lock package count {names}"));
    }
    println!("cargo_lock_packages=1");
    let manifest = read_string(&work.join("Cargo.toml"))?;
    if !manifest.contains("rust-version = \"1.94.0\"") {
        return Err("Cargo.toml rust-version is not 1.94.0".to_owned());
    }
    println!("rust_version=1.94.0");
    unsafe_inventory(work)?;
    expect_hash(&work.join("package-lock.json"), LOCK_SHA, "spike-package-lock")?;
    expect_hash(&work.join("PI-LICENSE"), PI_LICENSE_SHA, "spike-pi-license")?;
    expect_hash(&work.join("NODE-LICENSE"), NODE_LICENSE_SHA, "spike-node-license")?;
    println!("spike_ok");
    Ok(())
}

fn unsafe_inventory(work: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    walk_rs(work, &mut files)?;
    files.sort();
    let mut hits = 0usize;
    for path in files {
        let text = read_string(&path)?;
        for (index, line) in text.lines().enumerate() {
            if !line.contains("unsafe") {
                continue;
            }
            hits += 1;
            if !line.contains("forbid(unsafe_code)") {
                return Err(format!("unsafe outside forbid: {}:{}", path.display(), index + 1));
            }
        }
    }
    if hits == 0 {
        return Err("no forbid(unsafe_code) lines found".to_owned());
    }
    println!("unsafe_forbid_lines={hits}");
    Ok(())
}

fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|err| format!("dirent: {err}"))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| format!("type {}: {err}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("symlink refused {}", path.display()));
        }
        if kind.is_dir() {
            walk_rs(&path, out)?;
        } else if kind.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn historical(clippy_exit: &Path, compare: &Path) -> Result<(), String> {
    let exit_bytes = read_bytes(clippy_exit)?;
    let exit_text = std::str::from_utf8(&exit_bytes).map_err(|_| "clippy exit utf8")?.trim();
    if exit_text != "127" {
        return Err(format!("a279 clippy exit is {exit_text}, expected preserved 127"));
    }
    println!("a279_clippy_exit=127 preserved");
    let compare_text = read_string(compare)?;
    for needle in ["expected=473", "fresh=474", "bad=0"] {
        if !compare_text.contains(needle) {
            return Err(format!("a282 compare missing {needle}"));
        }
    }
    println!("a282_a279_compare=bad=0 preserved");
    Ok(())
}

fn same(left: &Path, right: &Path) -> Result<(), String> {
    let left_hex = hex(&sha256(&read_bytes(left)?));
    let right_hex = hex(&sha256(&read_bytes(right)?));
    println!("left={left_hex}");
    println!("right={right_hex}");
    if left_hex != right_hex {
        return Err(format!("byte mismatch {} {}", left.display(), right.display()));
    }
    println!("same_ok");
    Ok(())
}

fn boundary(path: &Path) -> Result<(), String> {
    let text = read_string(path)?;
    if text.contains("FAILED") {
        return Err("boundary stdout contains FAILED".to_owned());
    }
    let required = [
        "pairs_response_id_and_keeps_the_event",
        "pairs_out_of_order_ids",
        "reports_clean_eof",
        "reports_a_partial_line_at_eof",
        "deadline_elapses_before_a_late_process",
        "observed_fixtures_map_without_inventing_tool_events",
    ];
    for name in required {
        let line = format!("test {name} ... ok");
        if !text.contains(&line) {
            return Err(format!("missing passing test {name}"));
        }
        println!("boundary {name}=ok");
    }
    if !text.contains("12 passed") || !text.contains("0 failed") || !text.contains("0 ignored") {
        return Err("boundary summary is not 12 passed, 0 failed, 0 ignored".to_owned());
    }
    println!("boundary_ok");
    Ok(())
}

fn pin(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
    let pairs = [
        ("node-tarball", NODE_TARBALL_SHA),
        ("node-sums", NODE_SUMS_SHA),
        ("npm-tarball", NPM_TARBALL_SHA),
        ("github-tarball", GITHUB_TARBALL_SHA),
        ("pi-license", PI_LICENSE_SHA),
        ("node-license", NODE_LICENSE_SHA),
        ("package-lock", LOCK_SHA),
    ];
    for (label, expected) in pairs {
        let path = take_path(args)?;
        expect_hash(&path, expected, label)?;
    }
    println!("pin_ok");
    Ok(())
}

fn live(proof: &Path, spike: &Path) -> Result<(), String> {
    let dir = proof.join("live");
    eq_trim(&proof.join("live.stdout"), "live_ok", "live.stdout")?;
    eq_trim(&proof.join("live.exit"), "0", "live.exit")?;
    let pre = read_string(&dir.join("pi-version-pre.exit"))?;
    let pre_text = pre.trim();
    if pre_text.parse::<i32>().is_err() {
        return Err("pi-version-pre.exit is not an integer".to_owned());
    }
    println!("pi_version_pre={pre_text} note=not_the_driver");
    eq_trim(&dir.join("version.stdout"), "1.0.4", "version.stdout")?;
    eq_trim(&dir.join("version.exit"), "0", "version.exit")?;
    eq_trim(&dir.join("help.exit"), "0", "help.exit")?;
    has(&dir.join("help.stdout"), "--mode", "help --mode")?;
    has(&dir.join("cold-get_state.json"), "\"success\":true", "cold get_state")?;
    has(&dir.join("cold-get_state.json"), "\"id\":\"unknown\"", "cold model unknown")?;
    has_any(&dir.join("cold-get_messages.json"), &["\"messages\":[]", "\"messages\": []"], "cold empty messages")?;
    has(&dir.join("cold-abort.json"), "\"success\":true", "cold abort")?;
    eq_line(&dir.join("cold-export.json"), &spike.join("fixtures/cold_export.json"), "cold-export")?;
    eq_line(&dir.join("cold-restore.json"), &spike.join("fixtures/cold_restore.json"), "cold-restore")?;
    has(&dir.join("cold-export.json"), "Unknown command: export", "export unsupported")?;
    has(&dir.join("cold-restore.json"), "Unknown command: restore", "restore unsupported")?;
    has(&dir.join("abort-prompt.json"), "\"command\":\"prompt\"", "prompt command")?;
    has(&dir.join("abort-prompt.json"), "\"success\":true", "prompt success")?;
    has(&dir.join("abort-events.bin"), "\"type\":\"text_delta\"", "text_delta")?;
    has(&dir.join("abort-events.bin"), "fixture-reply", "delta text")?;
    lacks(&dir.join("abort-events.bin"), "turn_finished", "abort events product name")?;
    lacks(&dir.join("abort-events.bin"), "\"stopReason\":\"stop\"", "abort events clean stop")?;
    eq_line(&dir.join("abort-response.json"), &spike.join("fixtures/abort_response.json"), "abort-response")?;
    has(&dir.join("abort-messages.json"), "\"success\":true", "abort get_messages")?;
    has(&dir.join("abort-messages.json"), "fixture-reply", "abort transcript text")?;
    has(&dir.join("abort-messages.json"), "\"stopReason\":\"aborted\"", "abort stopReason")?;
    has(&dir.join("abort-messages.json"), "\"errorMessage\":\"Request was aborted\"", "abort errorMessage")?;
    lacks(&dir.join("abort-messages.json"), "\"stopReason\":\"stop\"", "abort messages clean stop")?;
    lacks(&dir.join("abort-messages.json"), "turn_finished", "abort messages product name")?;
    lacks(&dir.join("abort-messages.json"), TOKEN, "abort messages token")?;
    has(&dir.join("session.jsonl"), "fixture-reply", "session text")?;
    has(&dir.join("session.jsonl"), "\"stopReason\":\"stop\"", "session stop")?;
    lacks(&dir.join("session.jsonl"), TOKEN, "session token")?;
    eq_line(&dir.join("switch.json"), &spike.join("fixtures/restore_switch_session.json"), "switch_session")?;
    has(&dir.join("switch.json"), "\"cancelled\":false", "cancelled false")?;
    has(&dir.join("restore-messages.json"), "\"success\":true", "restored get_messages")?;
    has(&dir.join("restore-messages.json"), "fixture-reply", "restored text")?;
    has(&dir.join("restore-messages.json"), "\"stopReason\":\"stop\"", "restored stop")?;
    lacks(&dir.join("restore-messages.json"), "\"stopReason\":\"aborted\"", "restored aborted")?;
    lacks(&dir.join("restore-messages.json"), TOKEN, "restored token")?;
    for name in ["abort-models.json", "restore-models.json"] {
        has(&dir.join(name), "127.0.0.1", name)?;
        has(&dir.join(name), "fixture-echo", name)?;
        has(&dir.join(name), TOKEN, name)?;
    }
    for name in ["abort-mock.log", "restore-mock.log"] {
        has(&dir.join(name), "auth_ok=true", name)?;
        has(&dir.join(name), "local_host=true", name)?;
        has(&dir.join(name), "sent-first", name)?;
        has(&dir.join(name), "/chat/completions", name)?;
    }
    println!("live_oracle_ok");
    Ok(())
}

fn eq_trim(path: &Path, expected: &str, label: &str) -> Result<(), String> {
    let text = read_string(path)?;
    if text.trim() != expected {
        return Err(format!("{label} is not {expected}"));
    }
    println!("{label}=ok");
    Ok(())
}

fn has(path: &Path, needle: &str, label: &str) -> Result<(), String> {
    let bytes = read_bytes(path)?;
    if !contains(&bytes, needle) {
        return Err(format!("missing {label}"));
    }
    println!("{label}=present");
    Ok(())
}

fn has_any(path: &Path, needles: &[&str], label: &str) -> Result<(), String> {
    let bytes = read_bytes(path)?;
    if !needles.iter().any(|needle| contains(&bytes, needle)) {
        return Err(format!("missing {label}"));
    }
    println!("{label}=present");
    Ok(())
}

fn lacks(path: &Path, needle: &str, label: &str) -> Result<(), String> {
    let bytes = read_bytes(path)?;
    if contains(&bytes, needle) {
        return Err(format!("forbidden {label}"));
    }
    println!("{label}=absent");
    Ok(())
}

fn contains(hay: &[u8], needle: &str) -> bool {
    let needle = needle.as_bytes();
    hay.windows(needle.len()).any(|window| window == needle)
}

fn eq_line(live_path: &Path, fixture: &Path, label: &str) -> Result<(), String> {
    let live_bytes = strip_lf(read_bytes(live_path)?);
    let fixture_bytes = strip_lf(read_bytes(fixture)?);
    if live_bytes != fixture_bytes {
        return Err(format!("{label} bytes differ live={} fixture={}", live_bytes.len(), fixture_bytes.len()));
    }
    println!("{label}=byte_match");
    Ok(())
}

fn strip_lf(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    bytes
}

fn expect_hash(path: &Path, expected: &str, label: &str) -> Result<(), String> {
    let got = hex(&sha256(&read_bytes(path)?));
    if got != expected {
        return Err(format!("{label} sha256 {got} expected {expected}"));
    }
    println!("{label}={got}");
    Ok(())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))
}

fn read_string(path: &Path) -> Result<String, String> {
    let bytes = read_bytes(path)?;
    String::from_utf8(bytes).map_err(|_| format!("utf8 {}", path.display()))
}

fn check_sha256() -> Result<(), String> {
    if hex(&sha256(b"")) != EMPTY_SHA || hex(&sha256(b"abc")) != ABC_SHA {
        return Err("sha256 self-check failed".to_owned());
    }
    Ok(())
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
    let bit_len = u64::try_from(input.len()).expect("len").checked_mul(8).expect("bits");
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
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().expect("word"));
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(s0)
                .wrapping_add(word[index - 7])
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
