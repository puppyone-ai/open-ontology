//! Write the live suite documents. Passwords stay in the mounted files.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const PROTOCOL: &str = "oo.benchmark.sql-runner/v1";
const CONFIG: &str = "a198a198a198a198a198a198a198a198a198a198a198a198a198a198a198a198";

struct Tools {
    query: String,
    query_sha: String,
    adapter: String,
    adapter_sha: String,
    comparator: String,
    comparator_sha: String,
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let out = PathBuf::from(args.next().expect("out"));
    let tools = Tools {
        query: args.next().expect("query"),
        query_sha: args.next().expect("query sha"),
        adapter: args.next().expect("adapter"),
        adapter_sha: args.next().expect("adapter sha"),
        comparator: args.next().expect("comparator"),
        comparator_sha: args.next().expect("comparator sha"),
    };
    let header = args.next().expect("header");
    let trunc_header = args.next().expect("trunc header");
    let gold_exact = args.next().expect("exact");
    let gold_changed = args.next().expect("changed");
    let gold_prefix = args.next().expect("prefix");
    let gold_set = args.next().expect("set");
    let trunc_bytes: u64 = args.next().expect("trunc").parse().expect("trunc number");
    if !(1..262_144).contains(&trunc_bytes) || trunc_bytes == 4096 {
        eprintln!("prepare: truncate bound is not a smaller positive limit");
        return ExitCode::from(2);
    }
    fs::create_dir_all(&out).expect("out");
    let original = fs::read_to_string(&header).expect("header");
    if header_with_limit(&header, 4096) != original {
        panic!("full header rewrite changed bytes");
    }
    let trunc_header_body = header_with_limit(&header, trunc_bytes);
    fs::write(&trunc_header, &trunc_header_body).expect("trunc header");
    let exact_sql = "SELECT lane, rank, stamp, note, open FROM dispatch.ticket ORDER BY lane";
    write_request(&out, "exact", exact_sql, 4096);
    write_request(&out, "changed", exact_sql, 4096);
    write_request(&out, "seal", "SELECT seal FROM dispatch.ticket", 4096);
    write_request(&out, "hold", "SELECT id FROM dispatch.hold", 4096);
    write_request(&out, "trunc", exact_sql, trunc_bytes);
    write_request(&out, "cmpset", exact_sql, 4096);
    write_request(&out, "slow", exact_sql, 4096);
    write_request(&out, "noend", exact_sql, 4096);
    let req = |name: &str| out.join(format!("request-{name}.json"));
    write_suite(
        &out.join("suite-exact.json"),
        &tools,
        "frozen_eval",
        "live-exact",
        &[case(
            "exact",
            60_000,
            &req("exact"),
            &header,
            &gold_exact,
        )],
    );
    write_suite(
        &out.join("suite-fail.json"),
        &tools,
        "synthetic",
        "live-fail",
        &[
            case("exact", 60_000, &req("exact"), &header, &gold_exact),
            case(
                "changed",
                60_000,
                &req("changed"),
                &header,
                &gold_changed,
            ),
            case("seal", 60_000, &req("seal"), &header, &gold_exact),
            case("hold", 60_000, &req("hold"), &header, &gold_exact),
            case(
                "trunc",
                60_000,
                &req("trunc"),
                &trunc_header,
                &gold_prefix,
            ),
            case("cmpset", 60_000, &req("cmpset"), &header, &gold_set),
        ],
    );
    write_suite(
        &out.join("suite-timeout.json"),
        &tools,
        "synthetic",
        "live-timeout",
        &[case("slow", 2_000, &req("slow"), &header, &gold_exact)],
    );
    write_suite(
        &out.join("suite-noend.json"),
        &tools,
        "synthetic",
        "live-noend",
        &[case("noend", 60_000, &req("noend"), &header, &gold_exact)],
    );
    ExitCode::from(0)
}

fn header_with_limit(path: &str, limit: u64) -> String {
    let text = fs::read_to_string(path).expect("header");
    let from = "\"max_result_bytes\":4096";
    let to = format!("\"max_result_bytes\":{limit}");
    if !text.contains(from) {
        panic!("header limit token missing");
    }
    text.replace(from, &to)
}

fn write_request(out: &Path, name: &str, sql: &str, max_bytes: u64) {
    let body = format!(
        concat!(
            "{{\"version\":\"oo.gateway.query-pg/v1\",",
            "\"endpoint\":{{\"host\":\"oo-a198-pg\",\"port\":5432,\"database\":\"oo_a198\",\"user\":\"oo_a198_login\"}},",
            "\"password_file\":\"/run/oo-a198/fixture/login.pass\",",
            "\"ca_file\":\"/run/oo-a198/fixture/ca.crt\",",
            "\"audit_file\":\"/run/oo-a198/fixture/not-used-audit.jsonl\",",
            "\"identity\":{{\"project\":\"tickets\",\"role\":\"oo_r_desk\",\"policy_id\":\"{config}\"}},",
            "\"policies\":[{{\"project\":\"tickets\",\"config_digest\":\"{config}\",\"generation\":7,",
            "\"role\":\"desk\",\"login\":\"oo_a198_login\",",
            "\"table\":{{\"namespace\":\"dispatch\",\"name\":\"ticket\",\"columns\":[",
            "{col}",
            "]}}}}],",
            "\"catalog\":{{\"max_tables\":4,\"max_columns\":64,\"max_functions\":1,\"max_operators\":64,",
            "\"large_table_row_threshold\":1000000,\"unknown_rows\":\"refuse\"}},",
            "\"statement\":{{\"source\":\"request\",\"sql\":\"{sql}\",\"max_rows\":20,",
            "\"max_result_bytes\":{max_bytes},\"wall_clock_ms\":30000}}}}\n"
        ),
        config = CONFIG,
        col = columns(),
        sql = sql,
        max_bytes = max_bytes,
    );
    fs::write(out.join(format!("request-{name}.json")), body).expect("request");
}

fn columns() -> String {
    ["lane", "rank", "stamp", "note", "open"]
        .iter()
        .map(|name| {
            format!(
                "{{\"name\":\"{name}\",\"status\":\"confirmed\",\"semantic_type\":\"value\",\"pii\":{{\"class\":\"none\"}}}}"
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn case(id: &str, wall: u64, request: &Path, header: &str, gold: &str) -> String {
    format!(
        concat!(
            "{{\"id\":\"{id}\",\"role\":\"desk\",\"k\":1,\"annotation\":\"score\",",
            "\"trial_wall_ms\":{wall},\"request_file\":\"{request}\",",
            "\"binding_file\":\"{header}\",\"gold_file\":\"{gold}\"}}"
        ),
        id = id,
        wall = wall,
        request = request.display(),
        header = header,
        gold = gold,
    )
}

fn write_suite(path: &Path, tools: &Tools, mode: &str, suite: &str, cases: &[String]) {
    let body = format!(
        concat!(
            "{{\"version\":\"{protocol}\",\"mode\":\"{mode}\",\"metadata\":\"declared\",",
            "\"tuple\":{{\"project\":\"dispatch\",\"snapshot\":\"fixture-a231\",\"as_of\":\"2026-10-06\",",
            "\"dataset\":\"desk\",\"suite\":\"{suite}\",\"model\":\"none\",\"harness\":\"sql-runner\",",
            "\"role\":\"desk\",\"budget\":\"k1\",\"scorer_version\":\"{protocol}\"}},",
            "\"executables\":{{",
            "\"query_pg\":{{\"path\":\"{query}\",\"sha256\":\"{query_sha}\"}},",
            "\"adapter\":{{\"path\":\"{adapter}\",\"sha256\":\"{adapter_sha}\"}},",
            "\"comparator\":{{\"path\":\"{comparator}\",\"sha256\":\"{comparator_sha}\"}}",
            "}},\"cases\":[{cases}]}}\n"
        ),
        protocol = PROTOCOL,
        mode = mode,
        suite = suite,
        query = tools.query,
        query_sha = tools.query_sha,
        adapter = tools.adapter,
        adapter_sha = tools.adapter_sha,
        comparator = tools.comparator,
        comparator_sha = tools.comparator_sha,
        cases = cases.join(","),
    );
    fs::write(path, body).expect("suite");
}
