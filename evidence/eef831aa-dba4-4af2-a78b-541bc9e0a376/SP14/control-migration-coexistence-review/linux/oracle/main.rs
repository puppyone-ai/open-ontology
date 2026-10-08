//! Independent catalog oracle for the a376 coexistence review.
//! SHA-256 is the enum-store recipe. No product crate is linked.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::exit;

fn main() {
    let mut args = env::args().skip(1);
    let facts_path = args.next().expect("facts");
    let calendar_sql = args.next().expect("calendar sql");
    let enum_sql = args.next().expect("enum sql");
    let calendar_contract = args.next().expect("calendar contract");
    let enum_contract = args.next().expect("enum contract");
    let empty = sha256_hex(b"").expect("empty");
    let abc = sha256_hex(b"abc").expect("abc");
    if empty != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        || abc != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    {
        fail("sha256 vectors");
    }
    let calendar_hash = sha256_hex(&fs::read(&calendar_sql).expect("read calendar sql")).expect("hash");
    let enum_hash = sha256_hex(&fs::read(&enum_sql).expect("read enum sql")).expect("hash");
    expect_eq(
        "calendar migration",
        &calendar_hash,
        "ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520",
    );
    expect_eq(
        "enum migration",
        &enum_hash,
        "b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd",
    );
    for path in [&calendar_contract, &enum_contract] {
        let text = fs::read_to_string(path).expect("contract");
        if !text.contains("Both role pairs may exist before either install.") {
            fail("support sentence missing");
        }
        if !text.contains("ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520")
            || !text.contains("b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd")
        {
            fail("contract digest missing");
        }
    }
    let facts = fs::read_to_string(&facts_path).expect("facts");
    let mut lines = BTreeSet::new();
    for line in facts.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        lines.insert(line.to_owned());
    }
    require(&lines, "role|oo_calendar_admin|t|f|f|f|f");
    require(&lines, "role|oo_calendar_view|f|f|f|f|f");
    require(&lines, "role|oo_enum_admin|t|f|f|f|f");
    require(&lines, "role|oo_enum_view|f|f|f|f|f");
    require(&lines, "role|oo_r_calendar|t|f|f|f|f");
    require(&lines, "role|oo_r_enum|t|f|f|f|f");
    for db in ["oo_a365_cal_first", "oo_a365_enum_first", "oo_a365_hostile"] {
        require(&lines, &format!("before|{db}|0"));
    }
    require(&lines, "runtime_membership|0");
    for db in ["oo_a365_cal_first", "oo_a365_enum_first"] {
        let owner = if db == "oo_a365_cal_first" {
            "oo_calendar_admin"
        } else {
            "oo_enum_admin"
        };
        require(&lines, &format!("owner|{db}|{owner}"));
        for role in ["oo_r_calendar", "oo_r_enum"] {
            require(&lines, &format!("usage|{db}|{role}|t|f"));
        }
        for role in ["oo_r_calendar", "oo_r_enum", "oo_r_other"] {
            for sig in SIGS {
                require(&lines, &format!("exec|{db}|{role}|{sig}|f"));
            }
        }
        require(&lines, &format!("view_create|{db}|oo_calendar_view|f"));
        require(&lines, &format!("view_create|{db}|oo_enum_view|f"));
        require(&lines, &format!("allow|{db}|oo_calendar_control|oo_r_calendar"));
        require(&lines, &format!("allow|{db}|oo_enum_control|oo_r_enum"));
        require(&lines, &format!("allow_count|{db}|oo_calendar_control|1"));
        require(&lines, &format!("allow_count|{db}|oo_enum_control|1"));
    }
    require(&lines, "exec|oo_a365_cal_first|oo_enum_admin|oo_calendar_control.dict_create_open()|t");
    require(&lines, "exec|oo_a365_cal_first|oo_enum_admin|oo_calendar_control.dict_grant_runtime(text)|t");
    require(&lines, "exec|oo_a365_enum_first|oo_calendar_admin|oo_enum_control.dict_create_open()|t");
    require(&lines, "exec|oo_a365_enum_first|oo_calendar_admin|oo_enum_control.dict_grant_runtime(text)|t");
    require(&lines, "noop|oo_a365_cal_first|oo_enum_admin|oo_r_other|f");
    require(&lines, "noop|oo_a365_enum_first|oo_calendar_admin|oo_r_other|f");
    require(&lines, "deny|oo_a365_cal_first|oo_r_enum|open|42501");
    require(&lines, "deny|oo_a365_cal_first|oo_r_enum|grant|42501");
    require(&lines, "deny|oo_a365_cal_first|oo_r_enum|create|42501");
    require(&lines, "deny|oo_a365_enum_first|oo_r_calendar|open|42501");
    require(&lines, "deny|oo_a365_enum_first|oo_r_calendar|grant|42501");
    require(&lines, "deny|oo_a365_enum_first|oo_r_calendar|create|42501");
    require(&lines, "hostile_private|0");
    require(&lines, "drift|oo_a365_cal_first|enums|security_invoker=true");
    require(&lines, "drift|oo_a365_enum_first|calendar|security_invoker=true");
    require(&lines, "drift_schemas|oo_a365_cal_first|2");
    require(&lines, "drift_schemas|oo_a365_enum_first|2");
    require(&lines, "raw_tx|second|42P06");
    require(&lines, "raw_tx|oo_calendar_control|1");
    require(&lines, "raw_tx|oo_enum_control|0");
    require(&lines, "raw_tx|oo_dict|1");
    require(&lines, "raw_auto|dual|1");
    require(&lines, "raw_auto|oo_enum_control|1");
    require(&lines, "raw_auto|calendar_view_owner|oo_calendar_admin");
    require(&lines, "raw_tx|calendar_view_owner|oo_calendar_view");
    require(&lines, "raw_auto|calendar_warning|25P01");
    if !lines.iter().any(|line| line.starts_with("raw_auto|second_exit|") && line != "raw_auto|second_exit|0") {
        fail("raw autocommit second file was not rejected");
    }
    for db in ["oo_a365_cal_first", "oo_a365_enum_first"] {
        for &(schema, name, args, owner, lang, body) in FUNCTIONS {
            let prefix = format!("fn|{db}|{schema}|{name}|{args}|{owner}|t|{lang}|{{search_path=pg_catalog}}|0|");
            let found = lines.iter().any(|line| line.strip_prefix(&prefix) == Some(body));
            if !found {
                eprintln!("missing exact function {db} {schema}.{name}");
                exit(1);
            }
        }
    }
    let path = lines.iter().find(|line| line.starts_with("pathdrift|result|"));
    match path.map(String::as_str) {
        Some("pathdrift|result|accepted") | Some("pathdrift|result|refused") => {}
        Some(other) => {
            eprintln!("unexpected {other}");
            exit(1);
        }
        None => fail("pathdrift result missing"),
    }
    let widened = lines.iter().any(|line| {
        line.starts_with("pathdrift|config|") && line.contains("pg_catalog") && line.contains("public")
    });
    if !widened {
        fail("widened search_path was not recorded");
    }
    println!("oracle_ok calendar={calendar_hash} enum={enum_hash} lines={}", lines.len());
}

const SIGS: [&str; 6] = [
    "oo_calendar_control.dict_create_open()",
    "oo_calendar_control.dict_create_close()",
    "oo_calendar_control.dict_grant_runtime(text)",
    "oo_enum_control.dict_create_open()",
    "oo_enum_control.dict_create_close()",
    "oo_enum_control.dict_grant_runtime(text)",
];

const FUNCTIONS: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "oo_calendar_control",
        "dict_create_open",
        "",
        "oo_calendar_admin",
        "sql",
        "GRANT USAGE, CREATE ON SCHEMA oo_dict TO oo_enum_view",
    ),
    (
        "oo_calendar_control",
        "dict_create_close",
        "",
        "oo_calendar_admin",
        "sql",
        "REVOKE CREATE ON SCHEMA oo_dict FROM oo_enum_view",
    ),
    (
        "oo_calendar_control",
        "dict_grant_runtime",
        "runtime_role text",
        "oo_calendar_admin",
        "plpgsql",
        "BEGIN IF runtime_role IS NULL OR runtime_role !~ '^[a-z_][a-z0-9_]{0,62}$' THEN RAISE EXCEPTION 'runtime role' USING ERRCODE = '42501'; END IF; PERFORM 1 FROM oo_enum_control.runtime_allow WHERE role_name = runtime_role; IF NOT FOUND THEN RAISE EXCEPTION 'runtime role' USING ERRCODE = '42501'; END IF; EXECUTE format('GRANT USAGE ON SCHEMA oo_dict TO %I', runtime_role); END",
    ),
    (
        "oo_enum_control",
        "dict_create_open",
        "",
        "oo_enum_admin",
        "sql",
        "GRANT USAGE, CREATE ON SCHEMA oo_dict TO oo_calendar_view",
    ),
    (
        "oo_enum_control",
        "dict_create_close",
        "",
        "oo_enum_admin",
        "sql",
        "REVOKE CREATE ON SCHEMA oo_dict FROM oo_calendar_view",
    ),
    (
        "oo_enum_control",
        "dict_grant_runtime",
        "runtime_role text",
        "oo_enum_admin",
        "plpgsql",
        "BEGIN IF runtime_role IS NULL OR runtime_role !~ '^[a-z_][a-z0-9_]{0,62}$' THEN RAISE EXCEPTION 'runtime role' USING ERRCODE = '42501'; END IF; PERFORM 1 FROM oo_calendar_control.runtime_allow WHERE role_name = runtime_role; IF NOT FOUND THEN RAISE EXCEPTION 'runtime role' USING ERRCODE = '42501'; END IF; EXECUTE format('GRANT USAGE ON SCHEMA oo_dict TO %I', runtime_role); END",
    ),
];

fn require(lines: &BTreeSet<String>, line: &str) {
    if !lines.contains(line) {
        eprintln!("missing {line}");
        exit(1);
    }
}

fn expect_eq(label: &str, found: &str, expected: &str) {
    if found != expected {
        eprintln!("{label} {found}");
        exit(1);
    }
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    exit(1);
}

fn sha256_hex(input: &[u8]) -> Option<String> {
    sha256(input).map(|bytes| hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

fn sha256(input: &[u8]) -> Option<[u8; 32]> {
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
    let bit_len = u64::try_from(input.len()).ok()?.checked_mul(8)?;
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
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
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().take(16).enumerate() {
            let start = index * 4;
            *slot = u32::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
            ]);
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
        let mixed = [a, b, c, d, e, f, g, h];
        for (slot, value) in state.iter_mut().zip(mixed) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        let bytes = value.to_be_bytes();
        let start = index * 4;
        out[start] = bytes[0];
        out[start + 1] = bytes[1];
        out[start + 2] = bytes[2];
        out[start + 3] = bytes[3];
    }
    Some(out)
}
