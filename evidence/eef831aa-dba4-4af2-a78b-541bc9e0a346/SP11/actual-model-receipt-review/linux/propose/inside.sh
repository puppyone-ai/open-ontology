#!/bin/bash
# Linux clippy of a346-observe, then one dialogue propose through the observer.
# No approval file. No query-pg. No second chat call.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
export TMPDIR=/tmp
mkdir -p /evidence/linux/propose/phases /work /tmp
printf '%s\n' 'execute_started=false' > /evidence/linux/propose/execute-started.txt
printf '%s\n' 'approval_written_by_container=false' > /evidence/linux/propose/approval-written.txt
cd /src
rustc -vV | tee /evidence/linux/propose/rustc-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
test -f tools/ontology-dialogue/Cargo.toml
test -f tools/ontology-dialogue/src/lib.rs
test -f /relay/Cargo.toml
test -f /relay/src/bin/observe.rs
if [ -f rust-toolchain.toml ]; then
  echo "toolchain file left in place; archive mount is readonly" > /evidence/linux/propose/toolchain-note.txt
  exit 2
fi
printf '%s\n' "toolchain omitted from the private archive copy" > /evidence/linux/propose/toolchain-note.txt

phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/linux/propose/${name}.stdout" 2> "/evidence/linux/propose/${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/linux/propose/phases/${name}.exit"
  echo "phase_exit=${code} phase=${name}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

phase clippy-fetch cargo fetch --manifest-path /relay/Cargo.toml --locked
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  phase clippy-component rustup component add clippy --toolchain 1.94.0
else
  echo "already installed" > /evidence/linux/propose/clippy-component.stdout
  printf '%s\n' 0 > /evidence/linux/propose/phases/clippy-component.exit
fi
phase clippy cargo clippy --manifest-path /relay/Cargo.toml --locked --offline --bin a346-observe -- -D warnings

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
        "{\"protocol\":\"oo.ontology-dialogue.question/v1\",\"text\":\"How many stored rows are in depot.counter?\"}",
    )
    .unwrap();
    let config = format!(
        "{{\"protocol\":\"oo.ontology-dialogue.propose-config/v1\",\"question_file\":\"/work/question.json\",\"pack_file\":\"/work/pack.json\",\"binding\":{{\"role\":\"oo_r_north\",\"project\":\"depot\",\"policy_id\":{policy},\"policy_version\":\"v1\",\"knowledge_id\":\"desk\",\"knowledge_version\":\"v1\",\"as_of\":\"2026-10-06T00:00:00Z\"}},\"endpoint\":{{\"origin\":\"https://host.docker.internal:18447\",\"path\":\"/v1/chat/completions\",\"model_id\":\"grok-4.7\"}},\"trust\":{{\"ca_file\":\"/run/relay-ca.pem\",\"residency_approved\":true}},\"secret\":{{\"bearer_file\":\"/run/client-token\"}},\"budget\":{{\"max_question_bytes\":8192,\"max_context_bytes\":65536,\"max_request_bytes\":65536,\"max_response_bytes\":16384,\"max_response_chunks\":32,\"deadline_ms\":30000,\"max_prompt_tokens\":8000,\"max_completion_tokens\":1024,\"max_total_tokens\":16000,\"max_sql_bytes\":4096}}}}",
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
set +e
rustc --edition 2021 -O -o /target/emit-pack /tmp/emit-pack.rs > /evidence/linux/propose/emit.stdout 2> /evidence/linux/propose/emit.stderr
emit_code=$?
set -e
printf '%s\n' "$emit_code" > /evidence/linux/propose/phases/emit.exit
echo "phase_exit=${emit_code} phase=emit"
if [ "$emit_code" -ne 0 ]; then
  exit "$emit_code"
fi
set +e
/target/emit-pack > /evidence/linux/propose/emit-run.stdout 2> /evidence/linux/propose/emit-run.stderr
run_code=$?
set -e
printf '%s\n' "$run_code" > /evidence/linux/propose/phases/emit-run.exit
echo "phase_exit=${run_code} phase=emit-run"
if [ "$run_code" -ne 0 ]; then
  exit "$run_code"
fi
test -s /work/propose.json
test -s /work/pack.json
test -s /work/question.json

secret_hit() {
  local file="$1"
  if grep -F -q -f /run/client-token "$file"; then
    return 0
  fi
  if grep -F -q -f /run/upstream-token "$file"; then
    return 0
  fi
  if grep -F -q "PRIVATE KEY" "$file"; then
    return 0
  fi
  return 1
}

copy_clean() {
  local src="$1"
  local dest="$2"
  local label="$3"
  if [ ! -f "$src" ]; then
    printf 'absent label=%s\n' "$label" > "${dest}.absent"
    return 1
  fi
  if secret_hit "$src"; then
    printf 'secret_scan_hit label=%s not_copied\n' "$label" > "${dest}.redacted"
    return 1
  fi
  cp "$src" "$dest"
  return 0
}

copy_clean /work/question.json /evidence/linux/propose/question.json question || exit 1
copy_clean /work/pack.json /evidence/linux/propose/pack.json pack || exit 1
copy_clean /work/propose.json /evidence/linux/propose/propose-config.json config || exit 1

set +e
/target/debug/ontology-dialogue propose --config /work/propose.json > /work/proposal.json 2> /work/propose.stderr
propose_code=$?
set -e
printf '%s\n' "$propose_code" > /evidence/linux/propose/phases/propose.exit
echo "phase_exit=${propose_code} phase=propose"
if [ -s /work/propose.stderr ]; then
  copy_clean /work/propose.stderr /evidence/linux/propose/propose.stderr propose-stderr || exit 1
else
  : > /evidence/linux/propose/propose.stderr
fi
if [ -s /work/proposal.json ]; then
  copy_clean /work/proposal.json /evidence/linux/propose/proposal.json proposal || exit 1
  set +e
  /target/emit-pack meta > /evidence/linux/propose/meta.stdout 2> /evidence/linux/propose/meta.stderr
  meta_code=$?
  set -e
  printf '%s\n' "$meta_code" > /evidence/linux/propose/phases/meta.exit
  if [ "$meta_code" -eq 0 ] && [ -s /work/proposal-meta.json ]; then
    copy_clean /work/proposal-meta.json /evidence/linux/propose/proposal-meta.json meta || exit 1
  else
    printf '%s\n' '{"meta":"unparsed"}' > /evidence/linux/propose/proposal-meta.json
  fi
else
  echo "proposal_absent" > /evidence/linux/propose/proposal-absent.txt
fi
for name in request-body.json completion.json exchange.txt; do
  if [ -f "/observe-out/${name}" ]; then
    copy_clean "/observe-out/${name}" "/evidence/linux/propose/${name}" "$name" || exit 1
  else
    printf 'absent label=%s\n' "$name" > "/evidence/linux/propose/${name}.absent"
  fi
done
if [ -f /evidence/linux/propose/approval.json ]; then
  echo "propose container must not write approval" >&2
  exit 5
fi
printf '%s\n' 'execute_started=false' > /evidence/linux/propose/execute-started.txt
if [ "$propose_code" -ne 0 ]; then
  exit "$propose_code"
fi
test -s /evidence/linux/propose/request-body.json
test -s /evidence/linux/propose/completion.json
test -s /evidence/linux/propose/proposal.json
echo "propose_ok"
