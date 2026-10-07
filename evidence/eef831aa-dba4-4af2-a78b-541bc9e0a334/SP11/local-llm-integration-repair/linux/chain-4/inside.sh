#!/bin/bash
# One dialogue propose through the host relay.
# chain/ failed because /tmp is noexec and rust-toolchain.toml is on a readonly archive.
# This attempt compiles the pack helper onto /target and does not move the toolchain file.
# No retry of the chat call. No query-pg. The client token is mounted and is not printed.
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
rustc -vV | tee /evidence/linux/chain/rustc-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
test -f tools/ontology-dialogue/Cargo.toml
test -f tools/ontology-dialogue/src/lib.rs
test -f crates/oo-gateway/src/bin/query-pg.rs
if [ -f rust-toolchain.toml ]; then
  echo "toolchain file left in place; archive mount is readonly" > /evidence/linux/chain/toolchain-note.txt
fi

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

cat > /tmp/emit-pack.rs << 'RS'
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

fn sha256(bytes: &[u8]) -> String {
    let path = "/tmp/sha-input";
    fs::write(path, bytes).unwrap();
    let output = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
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

fn operator_precheck(sql: &str) -> bool {
    let trimmed = sql.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return false;
    }
    let semis = trimmed.matches(';').count();
    if !(semis == 0 || (semis == 1 && trimmed.ends_with(';'))) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("select") {
        return false;
    }
    let banned = [
        "insert", "update", "delete", "drop", "alter", "grant", "copy", "truncate", "comment",
        "vacuum", "secret_note",
    ];
    let mut word = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
            continue;
        }
        if banned.contains(&word.as_str()) {
            return false;
        }
        word.clear();
    }
    !banned.contains(&word.as_str())
}

fn write_meta() {
    let raw = fs::read_to_string("/work/proposal.json").unwrap_or_default();
    let protocol = json_string(&raw, "protocol").unwrap_or_default();
    let trust = json_string(&raw, "trust").unwrap_or_default();
    let model_sql = json_string(&raw, "model_sql").unwrap_or_default();
    let role = json_string(&raw, "role").unwrap_or_default();
    let project = json_string(&raw, "project").unwrap_or_default();
    let sql = json_string(&raw, "sql").unwrap_or_default();
    let proposal_sha256 = json_string(&raw, "proposal_sha256").unwrap_or_default();
    let sql_sha256 = json_string(&raw, "sql_sha256").unwrap_or_default();
    let tuple_sha256 = json_string(&raw, "tuple_sha256").unwrap_or_default();
    let pack_digest = json_string(&raw, "pack_digest").unwrap_or_default();
    let auto_execute = raw.contains("\"auto_execute\":false");
    let authenticated = raw.contains("\"authenticated\":false");
    let precheck = operator_precheck(&sql);
    let meta = format!(
        "{{\"protocol\":{protocol},\"trust\":{trust},\"authenticated\":{authenticated},\"model_sql\":{model_sql},\"auto_execute_false\":{auto_execute},\"role\":{role},\"project\":{project},\"sql_bytes\":{sql_bytes},\"operator_precheck_single_select\":{precheck},\"proposal_sha256\":{proposal},\"sql_sha256\":{sql_sha},\"tuple_sha256\":{tuple},\"pack_digest\":{digest}}}",
        protocol = esc(&protocol),
        trust = esc(&trust),
        authenticated = if authenticated { "false" } else { "true" },
        model_sql = esc(&model_sql),
        auto_execute = if auto_execute { "true" } else { "false" },
        role = esc(&role),
        project = esc(&project),
        sql_bytes = sql.len(),
        precheck = if precheck { "true" } else { "false" },
        proposal = esc(&proposal_sha256),
        sql_sha = esc(&sql_sha256),
        tuple = esc(&tuple_sha256),
        digest = esc(&pack_digest)
    );
    fs::write("/work/proposal-meta.json", meta).unwrap();
}

fn write_inputs() {
    let policy = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let manifest = format!(
        "{{\"knowledge\":{{\"id\":\"desk\",\"version\":\"v1\"}},\"policy\":{{\"id\":\"{policy}\",\"version\":\"v1\"}},\"project\":\"depot\",\"protocol\":\"oo.builder.role-pack-manifest/v1\",\"role\":\"oo_r_north\"}}"
    );
    let table = "column headcount is visible to this role\nAllowed statements for this role:\nSELECT count(*) FROM depot.counter\nSELECT headcount FROM depot.counter ORDER BY headcount\n";
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
        "{\"protocol\":\"oo.ontology-dialogue.question/v1\",\"text\":\"List each visible headcount in depot.counter from smallest to largest.\"}",
    )
    .unwrap();
    let config = format!(
        "{{\"protocol\":\"oo.ontology-dialogue.propose-config/v1\",\"question_file\":\"/work/question.json\",\"pack_file\":\"/work/pack.json\",\"binding\":{{\"role\":\"oo_r_north\",\"project\":\"depot\",\"policy_id\":{policy},\"policy_version\":\"v1\",\"knowledge_id\":\"desk\",\"knowledge_version\":\"v1\",\"as_of\":\"2026-10-06T00:00:00Z\"}},\"endpoint\":{{\"origin\":\"https://host.docker.internal:18317\",\"path\":\"/v1/chat/completions\",\"model_id\":\"grok-4.7\"}},\"trust\":{{\"ca_file\":\"/run/relay-ca.pem\",\"residency_approved\":true}},\"secret\":{{\"bearer_file\":\"/run/client-token\"}},\"budget\":{{\"max_question_bytes\":8192,\"max_context_bytes\":65536,\"max_request_bytes\":65536,\"max_response_bytes\":16384,\"max_response_chunks\":32,\"deadline_ms\":30000,\"max_prompt_tokens\":8000,\"max_completion_tokens\":1024,\"max_total_tokens\":16000,\"max_sql_bytes\":4096}}}}",
        policy = esc(policy)
    );
    fs::write("/work/propose.json", config).unwrap();
}

fn main() {
    let mut args = env::args();
    let _ = args.next();
    if args.next().as_deref() == Some("meta") {
        write_meta();
    } else {
        write_inputs();
    }
}
RS
rustc --edition 2021 -O -o /target/emit-pack /tmp/emit-pack.rs > /evidence/linux/chain/emit.stdout 2> /evidence/linux/chain/emit.stderr
emit_code=$?
printf '%s\n' "$emit_code" > /evidence/linux/chain/phases/emit.exit
echo "phase_exit=${emit_code} phase=emit"
if [ "$emit_code" -ne 0 ]; then
  exit "$emit_code"
fi
/target/emit-pack > /evidence/linux/chain/emit-run.stdout 2> /evidence/linux/chain/emit-run.stderr
run_code=$?
printf '%s\n' "$run_code" > /evidence/linux/chain/phases/emit-run.exit
echo "phase_exit=${run_code} phase=emit-run"
if [ "$run_code" -ne 0 ]; then
  exit "$run_code"
fi
test -s /work/propose.json
test -s /work/pack.json
set +e
/target/debug/ontology-dialogue propose --config /work/propose.json > /work/proposal.json 2> /work/propose.stderr
propose_code=$?
set -e
printf '%s\n' "$propose_code" > /evidence/linux/chain/phases/propose.exit
echo "phase_exit=${propose_code} phase=propose"
if [ -s /work/propose.stderr ] && grep -F -q -f /run/client-token /work/propose.stderr; then
  echo "propose stderr contained the client token; not copied" > /evidence/linux/chain/proposal-scan.txt
  exit 1
fi
cp /work/propose.stderr /evidence/linux/chain/propose.stderr
if [ -s /work/proposal.json ]; then
  if grep -F -q -f /run/client-token /work/proposal.json /work/pack.json /work/propose.json; then
    echo "proposal contained the client token; not copied" > /evidence/linux/chain/proposal-scan.txt
    exit 1
  fi
  cp /work/proposal.json /evidence/linux/chain/proposal.json
  cp /work/pack.json /evidence/linux/chain/pack.json
  cp /work/propose.json /evidence/linux/chain/propose-config.json
  set +e
  /target/emit-pack meta > /evidence/linux/chain/meta.stdout 2> /evidence/linux/chain/meta.stderr
  meta_code=$?
  set -e
  printf '%s\n' "$meta_code" > /evidence/linux/chain/phases/meta.exit
  if [ "$meta_code" -eq 0 ] && [ -s /work/proposal-meta.json ]; then
    if grep -F -q -f /run/client-token /work/proposal-meta.json; then
      echo "meta contained the client token; not copied" > /evidence/linux/chain/proposal-scan.txt
      exit 1
    fi
    cp /work/proposal-meta.json /evidence/linux/chain/proposal-meta.json
  else
    printf '%s\n' '{"meta":"unparsed"}' > /evidence/linux/chain/proposal-meta.json
  fi
else
  echo "proposal_absent" > /evidence/linux/chain/proposal-absent.txt
fi
if [ "$propose_code" -ne 0 ]; then
  exit "$propose_code"
fi
echo "propose_ok"
