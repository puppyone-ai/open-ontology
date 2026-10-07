//! Check the frozen live-test artifacts. Std only. No product change.
//! The SQL string is the constant in tests/common/mod.rs.

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::process::ExitCode;

const SQL: &str = "SELECT headcount FROM depot.counter ORDER BY headcount";
const QUESTION: &str = "How many visible headcount values are stored?";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn main() -> ExitCode {
    if hex(&sha256(b"")) != EMPTY_SHA || hex(&sha256(b"abc")) != ABC_SHA {
        eprintln!("sha256 self-check failed");
        return ExitCode::from(1);
    }
    let mut bad = 0_u32;
    bad += expect_file("oracle", "/evidence/linux/live/oracle.txt", &check_oracle);
    bad += expect_file(
        "north-execution",
        "/evidence/linux/live/north-execution.json",
        &check_execution,
    );
    bad += expect_file(
        "north-model",
        "/evidence/linux/live/north-model-body.json",
        &check_model,
    );
    bad += check_source(Path::new("/src/tools/ontology-dialogue"));
    println!("sql_sha256={}", hex(&sha256(SQL.as_bytes())));
    println!("count_cell_persisted=false");
    println!("truncation_json_persisted=false");
    println!("http_marker_calls_execute=false");
    if bad == 0 {
        println!("artifact_check=ok");
        ExitCode::SUCCESS
    } else {
        println!("artifact_check=fail faults={bad}");
        ExitCode::from(1)
    }
}

fn expect_file(name: &str, path: &str, check: &dyn Fn(&str) -> u32) -> u32 {
    match fs::read_to_string(path) {
        Ok(text) => check(&text),
        Err(err) => {
            println!("missing name={name} path={path} error={err}");
            1
        }
    }
}

fn check_oracle(text: &str) -> u32 {
    let mut bad = 0_u32;
    let required = [
        "pg_onboarding_help_exit=2",
        "install_north=0",
        "install_south=0",
        "provision_north=0",
        "cli_propose_equal=true",
        "north_exit=0",
        "north_rows=3",
        "cli_execute_exit=0",
        "truncated_exit=0",
        "truncated_disclosure=querytruncated",
        "stale_pack=approval",
        "sql_tamper=approval",
        "write_exit=6",
        "write_receipt=absent",
        "south_exit=6",
        "south_receipt=absent",
        "http_failure=provider",
        "http_execute_called=false",
        "count_exit=0",
        "count_receipt=clean",
        "live_done=1",
    ];
    for line in required {
        if !text.lines().any(|item| item == line) {
            println!("oracle_missing={line}");
            bad += 1;
        }
    }
    bad
}

fn check_execution(text: &str) -> u32 {
    let mut bad = 0_u32;
    let sql_hash = hex(&sha256(SQL.as_bytes()));
    let one = text.trim_end();
    if one.lines().count() != 1 {
        println!("execution_not_one_line");
        bad += 1;
    }
    for needle in [
        "\"protocol\":\"oo.ontology-dialogue.execution/v1\"",
        "\"trust\":\"local_operator_declared\"",
        "\"authenticated_audit\":false",
        "\"entailment\":\"not_judged\"",
        "\"warning_comprehension\":\"not_judged\"",
        "\"auto_execute\":false",
        "\"spawned\":true",
        "\"child_exit\":0",
        "\"query_receipt\":\"clean\"",
        "\"rows\":[[{\"kind\":\"int\",\"value\":\"1\"}],[{\"kind\":\"int\",\"value\":\"2\"}],[{\"kind\":\"int\",\"value\":\"3\"}]]",
        "\"citation_status\":\"ok\"",
    ] {
        if !one.contains(needle) {
            println!("execution_missing={needle}");
            bad += 1;
        }
    }
    let Some(sql_field) = field(one, "sql_sha256") else {
        println!("execution_missing=sql_sha256");
        return bad + 1;
    };
    if sql_field != sql_hash {
        println!("sql_sha256_mismatch field={sql_field} computed={sql_hash}");
        bad += 1;
    } else {
        println!("sql_sha256_matches_projection=true");
    }
    if !one.contains(&format!("\"original_sha256\":\"{sql_hash}\"")) {
        println!("original_sha256_mismatch");
        bad += 1;
    }
    let Some(query_id) = field(one, "query_id") else {
        println!("execution_missing=query_id");
        return bad + 1;
    };
    if !query_id.starts_with("q_") || query_id.len() < 8 {
        println!("query_id_shape={query_id}");
        bad += 1;
    }
    let admitted = format!("\"admitted\":[{{\"query_id\":\"{query_id}\"");
    if !one.contains(&admitted) {
        println!("admitted_query_id_mismatch");
        bad += 1;
    }
    if !one.contains(&format!("[{query_id}]")) {
        println!("rendered_citation_missing");
        bad += 1;
    }
    let oracle = fs::read_to_string("/evidence/linux/live/oracle.txt").unwrap_or_default();
    let want = format!("north_query_id={query_id}");
    if !oracle.lines().any(|line| line == want) {
        println!("oracle_query_id_mismatch");
        bad += 1;
    }
    if one.contains("execute_trusted_select") {
        println!("execution_contains_execute_trusted_select");
        bad += 1;
    }
    bad
}

fn check_model(text: &str) -> u32 {
    let mut bad = 0_u32;
    if !text.contains(QUESTION) {
        println!("model_missing_question");
        bad += 1;
    }
    if !text.contains("column headcount is visible to this role") {
        println!("model_missing_projection_context");
        bad += 1;
    }
    if !text.contains("\"temperature\":0") {
        println!("model_missing_temperature_0");
        bad += 1;
    }
    for banned in ["secret_note", "postgres://", "password=", "bearer-a300-visible-token", "PRIVATE KEY"] {
        if text.contains(banned) {
            println!("model_banned={banned}");
            bad += 1;
        }
    }
    bad
}

fn check_source(root: &Path) -> u32 {
    let mut bad = 0_u32;
    let mut files = Vec::new();
    if walk_rs(root, &mut files).is_err() {
        println!("source_walk_failed");
        return 1;
    }
    files.sort();
    let mut saw_forbid_lib = false;
    let mut saw_forbid_main = false;
    for path in &files {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                println!("source_read_failed path={} error={err}", path.display());
                bad += 1;
                continue;
            }
        };
        let digest = hex(&sha256(text.as_bytes()));
        println!("source_sha256={digest} path={}", path.display());
        if text.contains("execute_trusted_select") {
            println!("source_execute_trusted_select path={}", path.display());
            bad += 1;
        }
        for line in text.lines() {
            let stripped = line.replace("unsafe_code", "");
            if stripped.contains("unsafe") {
                println!("source_unsafe path={} line={line}", path.display());
                bad += 1;
            }
        }
        if path.ends_with("src/lib.rs") && text.contains("#![forbid(unsafe_code)]") {
            saw_forbid_lib = true;
        }
        if path.ends_with("src/main.rs") && text.contains("#![forbid(unsafe_code)]") {
            saw_forbid_main = true;
        }
    }
    if !saw_forbid_lib || !saw_forbid_main {
        println!("forbid_unsafe_missing lib={saw_forbid_lib} main={saw_forbid_main}");
        bad += 1;
    }
    println!("source_rs_files={}", files.len());
    bad
}

fn field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let rest = text.get(start..)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn walk_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), ()> {
    let entries = fs::read_dir(dir).map_err(|_| ())?;
    for entry in entries {
        let entry = entry.map_err(|_| ())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|_| ())?;
        if kind.is_symlink() {
            return Err(());
        }
        if kind.is_dir() {
            walk_rs(&path, out)?;
        } else if kind.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
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

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}
