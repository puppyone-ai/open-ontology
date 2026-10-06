//! Copy the hand-written suite files and insert measured bounds and digests.
//! Does not read query output and does not print cells or secrets.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let hand = PathBuf::from(args.next().expect("hand"));
    let built = PathBuf::from(args.next().expect("built"));
    let query_path = args.next().expect("query path");
    let query_sha = args.next().expect("query sha");
    let adapter_path = args.next().expect("adapter path");
    let adapter_sha = args.next().expect("adapter sha");
    let comparator_path = args.next().expect("comparator path");
    let comparator_sha = args.next().expect("comparator sha");
    let trunc = args.next().expect("trunc");
    let secret_path = args.next().expect("secret");
    if !hex64(&query_sha) || !hex64(&adapter_sha) || !hex64(&comparator_sha) {
        eprintln!("fill: digest");
        return ExitCode::from(2);
    }
    if !trunc_ok(&trunc) {
        eprintln!("fill: truncate bound");
        return ExitCode::from(2);
    }
    if fs::create_dir_all(&built).is_err() {
        eprintln!("fill: dir");
        return ExitCode::from(2);
    }
    let names = [
        "request-desk.json",
        "request-trunc.json",
        "request-seal.json",
        "request-hold.json",
        "request-peek.json",
        "request-peek-hide.json",
        "header-desk.json",
        "header-trunc.json",
        "header-peek.json",
        "gold-exact.json",
        "gold-changed.json",
        "gold-prefix.json",
        "gold-peek.json",
    ];
    for name in names {
        let raw = fs::read(hand.join(name)).expect("hand file");
        let text = String::from_utf8(raw).expect("utf8");
        let replaced = text.replace("__TRUNC__", &trunc);
        if replaced.contains("__TRUNC__") {
            eprintln!("fill: token left");
            return ExitCode::from(2);
        }
        if fs::write(built.join(name), replaced).is_err() {
            eprintln!("fill: write");
            return ExitCode::from(2);
        }
    }
    let mut secret = fs::read(&secret_path).expect("secret");
    if secret.ends_with(b"\n") {
        secret.pop();
    }
    if secret.ends_with(b"\r") {
        secret.pop();
    }
    if secret.is_empty() {
        eprintln!("fill: empty secret");
        return ExitCode::from(2);
    }
    for name in names {
        if !name.starts_with("request-") {
            continue;
        }
        let bytes = fs::read(built.join(name)).expect("request");
        if contains(&bytes, &secret) || contains(&bytes, b"/evidence/built/gold") {
            eprintln!("fill: request exposes secret or gold path");
            return ExitCode::from(1);
        }
    }
    let b = built.to_str().expect("utf8 built");
    let desk = format!("{b}/request-desk.json");
    let trunc_request = format!("{b}/request-trunc.json");
    let seal = format!("{b}/request-seal.json");
    let hold = format!("{b}/request-hold.json");
    let peek = format!("{b}/request-peek.json");
    let peek_hide = format!("{b}/request-peek-hide.json");
    let header = format!("{b}/header-desk.json");
    let header_trunc = format!("{b}/header-trunc.json");
    let header_peek = format!("{b}/header-peek.json");
    let gold_exact = format!("{b}/gold-exact.json");
    let gold_changed = format!("{b}/gold-changed.json");
    let gold_prefix = format!("{b}/gold-prefix.json");
    let gold_peek = format!("{b}/gold-peek.json");
    let exe_json = exes(&query_path, &query_sha, &adapter_path, &adapter_sha, &comparator_path, &comparator_sha);
    write_suite(
        &built.join("suite-pass.json"),
        "frozen_eval",
        "desk",
        "a244-pass",
        "desk",
        &exe_json,
        &[case("exact", "desk", 2, 60000, &desk, &header, &gold_exact)],
    );
    write_suite(
        &built.join("suite-fail.json"),
        "synthetic",
        "desk",
        "a244-fail",
        "desk",
        &exe_json,
        &[
            case("changed", "desk", 1, 60000, &desk, &header, &gold_changed),
            case("seal", "desk", 1, 60000, &seal, &header, &gold_exact),
            case("hold", "desk", 1, 60000, &hold, &header, &gold_exact),
            case("trunc", "desk", 1, 60000, &trunc_request, &header_trunc, &gold_prefix),
        ],
    );
    write_suite(
        &built.join("suite-peek.json"),
        "synthetic",
        "peek",
        "a244-peek",
        "peek",
        &exe_json,
        &[
            case("notes", "peek", 1, 60000, &peek, &header_peek, &gold_peek),
            case("hidden", "peek", 1, 60000, &peek_hide, &header_peek, &gold_peek),
        ],
    );
    write_suite(
        &built.join("suite-timeout.json"),
        "synthetic",
        "desk",
        "a244-timeout",
        "desk",
        &exe_json,
        &[case("slow", "desk", 1, 2000, &desk, &header, &gold_exact)],
    );
    write_suite(
        &built.join("suite-noend.json"),
        "synthetic",
        "desk",
        "a244-noend",
        "desk",
        &exe_json,
        &[case("noend", "desk", 1, 60000, &desk, &header, &gold_exact)],
    );
    let flipped = flip(&query_sha);
    let bad_exe_json = exes(&query_path, &flipped, &adapter_path, &adapter_sha, &comparator_path, &comparator_sha);
    write_suite(
        &built.join("suite-bad-hash.json"),
        "synthetic",
        "desk",
        "a244-bad-hash",
        "desk",
        &bad_exe_json,
        &[case("exact", "desk", 1, 60000, &desk, &header, &gold_exact)],
    );
    let desk_bytes = fs::read(built.join("request-desk.json")).expect("desk");
    let desk_text = String::from_utf8(desk_bytes).expect("utf8");
    let needle = "SELECT lane, rank, stamp, note, open FROM dispatch.ticket ORDER BY lane";
    let poison_sql = format!("SELECT 1 /* {gold_exact} */");
    if !desk_text.contains(needle) {
        eprintln!("fill: desk sql missing");
        return ExitCode::from(2);
    }
    let poisoned = desk_text.replace(needle, &poison_sql);
    if !poisoned.contains(&gold_exact) {
        eprintln!("fill: poison missed the gold path");
        return ExitCode::from(2);
    }
    if fs::write(built.join("request-poison.json"), poisoned).is_err() {
        eprintln!("fill: poison write");
        return ExitCode::from(2);
    }
    let poison = format!("{b}/request-poison.json");
    write_suite(
        &built.join("suite-gold-poison.json"),
        "synthetic",
        "desk",
        "a244-gold-poison",
        "desk",
        &exe_json,
        &[case("exact", "desk", 1, 60000, &poison, &header, &gold_exact)],
    );
    println!("fill_ok trunc={trunc}");
    ExitCode::from(0)
}

fn write_suite(path: &Path, mode: &str, role: &str, name: &str, dataset: &str, exes: &str, cases: &[String]) {
    let joined = cases.join(",");
    let body = format!(
        "{{\"cases\":[{joined}],\"executables\":{exes},\"metadata\":\"declared\",\"mode\":\"{mode}\",\"tuple\":{{\"as_of\":\"2026-10-06\",\"budget\":\"k1\",\"dataset\":\"{dataset}\",\"harness\":\"sql-runner\",\"model\":\"none\",\"project\":\"dispatch\",\"role\":\"{role}\",\"scorer_version\":\"oo.benchmark.sql-runner/v1\",\"snapshot\":\"fixture-a244\",\"suite\":\"{name}\"}},\"version\":\"oo.benchmark.sql-runner/v1\"}}\n"
    );
    fs::write(path, body).expect("suite");
}

fn case(id: &str, role: &str, k: u64, wall: u64, request: &str, binding: &str, gold: &str) -> String {
    format!(
        "{{\"annotation\":\"score\",\"binding_file\":\"{binding}\",\"gold_file\":\"{gold}\",\"id\":\"{id}\",\"k\":{k},\"request_file\":\"{request}\",\"role\":\"{role}\",\"trial_wall_ms\":{wall}}}"
    )
}

fn exes(query_path: &str, query_sha: &str, adapter_path: &str, adapter_sha: &str, comparator_path: &str, comparator_sha: &str) -> String {
    format!(
        "{{\"adapter\":{{\"path\":\"{adapter_path}\",\"sha256\":\"{adapter_sha}\"}},\"comparator\":{{\"path\":\"{comparator_path}\",\"sha256\":\"{comparator_sha}\"}},\"query_pg\":{{\"path\":\"{query_path}\",\"sha256\":\"{query_sha}\"}}}}"
    )
}

fn flip(sha: &str) -> String {
    let mut chars: Vec<u8> = sha.as_bytes().to_vec();
    let last = chars.len() - 1;
    chars[last] = if chars[last] == b'0' { b'1' } else { b'0' };
    String::from_utf8(chars).expect("hex")
}

fn hex64(text: &str) -> bool {
    text.len() == 64
        && text.bytes().all(|byte| {
            byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        })
}

fn trunc_ok(text: &str) -> bool {
    let Ok(value) = text.parse::<u64>() else {
        return false;
    };
    !text.is_empty()
        && !text.starts_with('0')
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (1..=262_144).contains(&value)
        && value != 4096
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
