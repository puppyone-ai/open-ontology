#!/bin/bash
# Build query-pg from the fixed archive, prepare the private database with the
# reviewed live test, then run one operator-approved dialogue execute.
# The model SQL is not rewritten. A guard refusal is recorded and is not success.
# No upstream token and no client token are mounted.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
mkdir -p /evidence/linux/execute-2/phases /evidence/linux/execute-2/live /tmp /work
cd /src
rustc -vV | tee /evidence/linux/execute-2/rustc-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
test -f tools/ontology-dialogue/src/lib.rs
test -f crates/oo-gateway/src/bin/query-pg.rs
test -d crates/oo-pipeline/migrations
if [ -f rust-toolchain.toml ]; then
  echo "toolchain file left in place; archive mount is readonly" > /evidence/linux/execute-2/toolchain-note.txt
fi

phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/linux/execute-2/${name}.stdout" 2> "/evidence/linux/execute-2/${name}.stderr"
  local code=$?
  printf '%s\n' "$code" > "/evidence/linux/execute-2/phases/${name}.exit"
  echo "phase_exit=${code} phase=${name}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

phase libclang bash /evidence/linux/execute-2/libclang-extract.sh
rawdir=$(cat /evidence/linux/execute-2/libclang-dir.txt)
if [ ! -d /cargo-home/a334-clang-root/usr/lib ]; then
  cp -a /tmp/clang-root /cargo-home/a334-clang-root
fi
libdir="/cargo-home/a334-clang-root${rawdir#/tmp/clang-root}"
printf '%s\n' "$libdir" > /evidence/linux/execute-2/libclang-dir.txt
multiarch=/cargo-home/a334-clang-root/usr/lib/aarch64-linux-gnu
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a334-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a334-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a334-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  ls -l "$libdir"/libclang.so* || true
} > /evidence/linux/execute-2/libclang.txt

phase fetch-workspace cargo fetch --manifest-path Cargo.toml --locked
phase build-query cargo build --manifest-path Cargo.toml --locked --offline \
  --bin query-pg --bin install-column-acl --bin provision-mapping-reader
phase fetch-onboarding cargo fetch --manifest-path tools/pg-onboarding/Cargo.toml --locked
phase build-onboarding cargo build --manifest-path tools/pg-onboarding/Cargo.toml --locked --offline
phase fetch-citations cargo fetch --manifest-path tools/answer-citations/Cargo.toml --locked
phase build-citations cargo build --manifest-path tools/answer-citations/Cargo.toml --locked --offline
phase fetch-dialogue cargo fetch --manifest-path tools/ontology-dialogue/Cargo.toml --locked

set +e
OO_A300_LIVE=1 \
OO_A300_PG_HOST=oo-a334-pg \
OO_A300_PG_CA=/run/fixture/ca.crt \
OO_A300_ADMIN_PASSWORD_FILE=/run/fixture/admin.pass \
OO_A300_LOGIN_PASSWORD_FILE=/run/fixture/login.pass \
OO_A300_QUERY_PG=/target/debug/query-pg \
OO_A300_INSTALL_COLUMN_ACL=/target/debug/install-column-acl \
OO_A300_PROVISION_MAPPING_READER=/target/debug/provision-mapping-reader \
OO_A300_PG_ONBOARDING=/target/debug/pg-onboarding \
OO_A300_ANSWER_CITATIONS=/target/debug/answer-citations \
OO_A300_ORACLE=/evidence/linux/execute-2/live/oracle.txt \
  cargo test --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline --test live -- --nocapture \
  > /evidence/linux/execute-2/test-live.stdout 2> /evidence/linux/execute-2/test-live.stderr
live_code=$?
set -e
printf '%s\n' "$live_code" > /evidence/linux/execute-2/phases/test-live.exit
echo "phase_exit=${live_code} phase=test-live"
if [ "$live_code" -ne 0 ]; then
  exit "$live_code"
fi
if [ ! -f /evidence/linux/execute-2/live/oracle.txt ] || ! grep -q '^live_done=1$' /evidence/linux/execute-2/live/oracle.txt; then
  echo "live oracle missing" >&2
  printf '%s\n' 1 > /evidence/linux/execute-2/phases/live-oracle.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/linux/execute-2/phases/live-oracle.exit

cat > /tmp/emit-exec.rs << 'RS'
use std::env;
use std::fs;
use std::process::Command;

fn esc(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn json_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let mut out = String::new();
    let mut escaped = false;
    for ch in text[start..].chars() {
        if escaped {
            out.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            return Some(out);
        }
        out.push(ch);
    }
    None
}

fn sha256_file(path: &str) -> String {
    let output = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn main() {
    let mut args = env::args();
    let _ = args.next();
    let query_pg = args.next().expect("query-pg path");
    let proposal = fs::read_to_string("/evidence/linux/chain-3/proposal.json").unwrap();
    let sql = json_string(&proposal, "sql").expect("sql");
    let proposal_sha = json_string(&proposal, "proposal_sha256").expect("proposal");
    let sql_sha = json_string(&proposal, "sql_sha256").expect("sql sha");
    let tuple_sha = json_string(&proposal, "tuple_sha256").expect("tuple");
    let pack_digest = json_string(&proposal, "pack_digest").expect("pack");
    let trimmed = sql.trim();
    let supported = trimmed == "SELECT count(*) FROM depot.counter"
        || trimmed == "SELECT headcount FROM depot.counter"
        || trimmed == "SELECT headcount FROM depot.counter ORDER BY headcount";
    if !supported {
        fs::write(
            "/evidence/linux/execute-2/withhold.txt",
            format!("sql_bytes={}\nreason=statement_not_in_supported_catalog_shapes\nrewritten=false\nexecuted=false\n", sql.len()),
        )
        .unwrap();
        std::process::exit(6);
    }
    let policy = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let approval = format!(
        "{{\"protocol\":\"oo.ontology-dialogue.approval/v1\",\"trust\":\"local_operator_declared\",\"authenticated\":false,\"proposal_sha256\":{proposal},\"sql_sha256\":{sql_sha},\"tuple_sha256\":{tuple},\"pack_digest\":{digest}}}",
        proposal = esc(&proposal_sha),
        sql_sha = esc(&sql_sha),
        tuple = esc(&tuple_sha),
        digest = esc(&pack_digest)
    );
    fs::write("/work/approval.json", &approval).unwrap();
    let request = format!(
        "{{\"version\":\"oo.gateway.query-pg/v1\",\"endpoint\":{{\"host\":\"oo-a334-pg\",\"port\":5432,\"database\":\"oo_a300\",\"user\":\"oo_a300_login\"}},\"password_file\":\"/run/fixture/login.pass\",\"ca_file\":\"/run/fixture/ca.crt\",\"audit_file\":\"/work/audit.jsonl\",\"identity\":{{\"project\":\"depot\",\"role\":\"oo_r_north\",\"policy_id\":{policy}}},\"policies\":[{{\"project\":\"depot\",\"config_digest\":{policy},\"generation\":3,\"role\":\"north\",\"login\":\"oo_a300_login\",\"table\":{{\"namespace\":\"depot\",\"name\":\"counter\",\"columns\":[{{\"name\":\"headcount\",\"status\":\"confirmed\",\"semantic_type\":\"value\",\"pii\":{{\"class\":\"none\"}}}}]}}}}],\"catalog\":{{\"max_tables\":4,\"max_columns\":64,\"max_functions\":1,\"max_operators\":64,\"large_table_row_threshold\":1000000,\"unknown_rows\":\"refuse\"}},\"statement\":{{\"source\":\"request\",\"sql\":{sql},\"max_rows\":20,\"max_result_bytes\":4096,\"wall_clock_ms\":30000}}}}",
        policy = esc(policy),
        sql = esc(&sql)
    );
    fs::write("/work/request.json", &request).unwrap();
    let binary_sha = sha256_file(&query_pg);
    let config = format!(
        "{{\"protocol\":\"oo.ontology-dialogue.execute-config/v1\",\"proposal_file\":\"/evidence/linux/chain-3/proposal.json\",\"approval_file\":\"/work/approval.json\",\"pack_file\":\"/evidence/linux/chain-3/pack.json\",\"request_file\":\"/work/request.json\",\"query_pg\":{{\"path\":{path},\"sha256\":{sha}}},\"binding\":{{\"session_id\":\"sess-a334b\",\"turn_id\":\"turn-a334b\",\"declares_data\":true,\"render_rows\":true}},\"budget\":{{\"deadline_ms\":30000,\"max_stdout_bytes\":1048576}}}}",
        path = esc(&query_pg),
        sha = esc(&binary_sha)
    );
    fs::write("/work/execute.json", config).unwrap();
    fs::write("/work/query-pg.sha256", &binary_sha).unwrap();
}
RS
rustc --edition 2021 -O -o /target/emit-exec /tmp/emit-exec.rs > /evidence/linux/execute-2/emit.stdout 2> /evidence/linux/execute-2/emit.stderr
emit_code=$?
printf '%s\n' "$emit_code" > /evidence/linux/execute-2/phases/emit.exit
echo "phase_exit=${emit_code} phase=emit"
if [ "$emit_code" -ne 0 ]; then
  exit "$emit_code"
fi
/target/emit-exec /target/debug/query-pg > /evidence/linux/execute-2/emit-run.stdout 2> /evidence/linux/execute-2/emit-run.stderr
run_code=$?
printf '%s\n' "$run_code" > /evidence/linux/execute-2/phases/emit-run.exit
echo "phase_exit=${run_code} phase=emit-run"
if [ "$run_code" -ne 0 ]; then
  exit "$run_code"
fi

copy_clean() {
  local src="$1"
  local dest="$2"
  if [ ! -f "$src" ]; then
    echo "missing $dest" > "$dest.missing"
    return
  fi
  if grep -F -q -f /run/fixture/admin.pass "$src" || grep -F -q -f /run/fixture/login.pass "$src"; then
    echo "contained a database password; not copied" > "$dest.redacted"
    return
  fi
  cp "$src" "$dest"
}

copy_clean /work/approval.json /evidence/linux/execute-2/approval.json
copy_clean /work/request.json /evidence/linux/execute-2/request.json
copy_clean /work/execute.json /evidence/linux/execute-2/execute-config.json
copy_clean /work/query-pg.sha256 /evidence/linux/execute-2/query-pg.sha256

set +e
/target/debug/ontology-dialogue execute --config /work/execute.json > /work/execute.stdout 2> /work/execute.stderr
exec_code=$?
set -e
printf '%s\n' "$exec_code" > /evidence/linux/execute-2/phases/execute.exit
echo "phase_exit=${exec_code} phase=execute"
copy_clean /work/execute.stdout /evidence/linux/execute-2/execute.stdout
copy_clean /work/execute.stderr /evidence/linux/execute-2/execute.stderr
if [ -f /work/audit.jsonl ]; then
  copy_clean /work/audit.jsonl /evidence/linux/execute-2/audit.jsonl
else
  echo "audit_absent" > /evidence/linux/execute-2/audit-absent.txt
fi
echo "execute_recorded exit=${exec_code}"
