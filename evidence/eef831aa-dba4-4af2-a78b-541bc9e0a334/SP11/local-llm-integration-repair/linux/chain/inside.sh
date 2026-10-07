#!/bin/bash
# Build the fixed-archive dialogue bin and send one propose to the host relay.
# No retry. No query-pg. The client token is mounted and is not printed.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
export TMPDIR=/tmp
mkdir -p /evidence/linux/chain/phases /work
cd /src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml /tmp/rust-toolchain.toml
fi
rustc -vV | tee /evidence/linux/chain/rustc-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
test -f tools/ontology-dialogue/Cargo.toml
test -f tools/ontology-dialogue/src/lib.rs
test -f crates/oo-gateway/src/bin/query-pg.rs

phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/linux/chain/${name}.stdout" 2> "/evidence/linux/chain/${name}.stderr"
  local code=$?
  printf '%s\n' "$code" > "/evidence/linux/chain/phases/${name}.exit"
  echo "phase_exit=${code} phase=${name}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

phase fetch cargo fetch --manifest-path tools/ontology-dialogue/Cargo.toml --locked
phase build cargo build --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline --bin ontology-dialogue

cat > /tmp/emit-pack.rs <<'RS'
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

fn sha256(bytes: &[u8]) -> String {
    let path = "/tmp/sha-input";
    fs::write(path, bytes).unwrap();
    let output = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().split_whitespace().next().unwrap().to_string()
}

fn main() {
    let policy = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let manifest = format!(
        "{{\"knowledge\":{{\"id\":\"desk\",\"version\":\"v1\"}},\"policy\":{{\"id\":\"{policy}\",\"version\":\"v1\"}},\"project\":\"depot\",\"protocol\":\"oo.builder.role-pack-manifest/v1\",\"role\":\"oo_r_north\"}}"
    );
    let table = "column headcount is visible to this role\n";
    let manifest_sha = sha256(manifest.as_bytes());
    let table_sha = sha256(table.as_bytes());
    let preimage = format!(
        "[{{\"bytes\":{mb},\"path\":\"manifest.json\",\"sha256\":\"{ms}\"}},{{\"bytes\":{tb},\"path\":\"tables/depot.counter.md\",\"sha256\":\"{ts}\"}}]",
        mb = manifest.len(),
        ms = manifest_sha,
        tb = table.len(),
        ts = table_sha
    );
    let digest = sha256(preimage.as_bytes());
    let pack = format!(
        "{{\"protocol\":\"oo.builder.role-pack/v1\",\"compiler\":\"oo-builder/role-pack/v1\",\"pack_digest\":{digest},\"manifest_sha256\":{msha},\"entry_count\":2,\"total_bytes\":{total},\"files\":[{{\"path\":\"manifest.json\",\"sha256\":{msha},\"bytes\":{mb},\"text\":{manifest}}},{{\"path\":\"tables/depot.counter.md\",\"sha256\":{tsha},\"bytes\":{tb},\"text\":{table}}}]}}",
        digest = esc(&digest),
        msha = esc(&manifest_sha),
        total = manifest.len() + table.len(),
        mb = manifest.len(),
        manifest = esc(&manifest),
        tsha = esc(&table_sha),
        tb = table.len(),
        table = esc(table)
    );
    fs::write("/work/pack.json", pack).unwrap();
    fs::write(
        "/work/question.json",
        "{\"protocol\":\"oo.ontology-dialogue.question/v1\",\"text\":\"How many visible headcount values are stored?\"}",
    )
    .unwrap();
    let config = format!(
        "{{\"protocol\":\"oo.ontology-dialogue.propose-config/v1\",\"question_file\":\"/work/question.json\",\"pack_file\":\"/work/pack.json\",\"binding\":{{\"role\":\"oo_r_north\",\"project\":\"depot\",\"policy_id\":{policy},\"policy_version\":\"v1\",\"knowledge_id\":\"desk\",\"knowledge_version\":\"v1\",\"as_of\":\"2026-10-06T00:00:00Z\"}},\"endpoint\":{{\"origin\":\"https://host.docker.internal:18317\",\"path\":\"/v1/chat/completions\",\"model_id\":\"grok-4.7\"}},\"trust\":{{\"ca_file\":\"/run/relay-ca.pem\",\"residency_approved\":true}},\"secret\":{{\"bearer_file\":\"/run/client-token\"}},\"budget\":{{\"max_question_bytes\":8192,\"max_context_bytes\":65536,\"max_request_bytes\":65536,\"max_response_bytes\":16384,\"max_response_chunks\":32,\"deadline_ms\":30000,\"max_prompt_tokens\":8000,\"max_completion_tokens\":1024,\"max_total_tokens\":16000,\"max_sql_bytes\":4096}}}}",
        policy = esc(policy)
    );
    fs::write("/work/propose.json", config).unwrap();
}
RS
rustc --edition 2021 -O -o /tmp/emit-pack /tmp/emit-pack.rs
/tmp/emit-pack
set +e
/target/debug/ontology-dialogue propose --config /work/propose.json > /work/proposal.json 2> /work/propose.stderr
propose_code=$?
set -e
printf '%s\n' "$propose_code" > /evidence/linux/chain/phases/propose.exit
echo "phase_exit=${propose_code} phase=propose"
cp /work/propose.stderr /evidence/linux/chain/propose.stderr
if [ -s /work/proposal.json ]; then
  if grep -F -q -f /run/client-token /work/proposal.json; then
    echo "proposal contained the client token; not copied" > /evidence/linux/chain/proposal-scan.txt
    exit 1
  fi
  cp /work/proposal.json /evidence/linux/chain/proposal.json
  python3 -c 'import json,sys; d=json.load(open("/work/proposal.json")); print(json.dumps({"protocol":d.get("protocol"),"trust":d.get("trust"),"authenticated":d.get("authenticated"),"model_sql":d.get("model_sql"),"auto_execute":d.get("auto_execute"),"role":d.get("role"),"project":d.get("project"),"sql_bytes":len(d.get("sql",""))}))' \
    > /evidence/linux/chain/proposal-meta.json || printf '%s\n' '{"meta":"unparsed"}' > /evidence/linux/chain/proposal-meta.json
fi
if [ "$propose_code" -ne 0 ]; then
  exit "$propose_code"
fi
echo "propose_ok"
