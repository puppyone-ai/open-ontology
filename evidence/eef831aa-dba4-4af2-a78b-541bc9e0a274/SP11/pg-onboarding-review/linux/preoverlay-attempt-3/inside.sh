#!/bin/bash
# MSRV focused smoke before any review overlay. live_chain stays ignored.
# Linux arm64 Rust 1.94. No cargo update. No product crate. No PostgreSQL.
set -u
echo "oo_task=sp11-pg-onboarding-review-preoverlay-2"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a274"
echo "oo_platform=linux/arm64"
echo "phase=preoverlay"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/onboarding
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/preoverlay-attempt-3/phases /target/onboarding
cd /onboarding

record() {
  mkdir -p /evidence/linux/preoverlay-attempt-3/phases
  printf '%s\n' "$2" > "/evidence/linux/preoverlay-attempt-3/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/preoverlay-attempt-3/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/preoverlay-attempt-3/rustc-version.txt
rustc --version --verbose > /evidence/linux/preoverlay-attempt-3/rustc-verbose.txt
cargo --version > /evidence/linux/preoverlay-attempt-3/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
printf '%s\n' "no" > /evidence/linux/preoverlay-attempt-3/cargo-update-invoked.txt

if ! grep -q 'changes_may_have_been_applied' src/outcome.rs; then
  echo "repaired failure state is absent" >&2
  record source-identity 2
fi
if ! grep -q 'unchanged_by_this_tool' src/outcome.rs; then
  echo "pre-spawn failure string is absent" >&2
  record source-identity 2
fi
if ! grep -q 'fn parse_json_slice' src/spawn.rs; then
  echo "shared parser entry is absent" >&2
  record source-identity 2
fi
if ! grep -q 'crate::json_doc::parse' src/spawn.rs; then
  echo "child JSON does not use the shared visitor" >&2
  record source-identity 2
fi
if [ -f tests/pg_onboarding_a274_review.rs ]; then
  echo "review overlay is present" >&2
  record source-identity 2
fi
record source-identity 0

echo "=== rustc sha256_manifest ==="
set +e
rustc --edition 2024 /tools/sha256_manifest.rs -o /target/sha256_manifest \
  > /evidence/linux/preoverlay-attempt-3/sha256-tool.log 2>&1
tool_exit=$?
set -e
record sha256-tool "$tool_exit"
/target/sha256_manifest /onboarding /evidence/linux/preoverlay-attempt-3/tool-sha256.txt \
  > /evidence/linux/preoverlay-attempt-3/sha256-tool-run.txt
record sha256-walk 0

if [ ! -f Cargo.lock ]; then
  echo "lock missing" >&2
  record lock 2
fi
printf '%s\n' "lock=present" > /evidence/linux/preoverlay-attempt-3/lock-before.txt
record lock 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/preoverlay-attempt-3/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test --locked --offline live ignored ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/linux/preoverlay-attempt-3/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

sum_field() {
  grep '^test result:' /evidence/linux/preoverlay-attempt-3/test.log \
    | sed -n "s/.* \\([0-9][0-9]*\\) $1.*/\\1/p" \
    | awk '{s+=$1} END{print s+0}'
}
printf 'passed=%s failed=%s ignored=%s\n' "$(sum_field passed)" "$(sum_field failed)" "$(sum_field ignored)" \
  > /evidence/linux/preoverlay-attempt-3/test-count.txt
read_count=$(cat /evidence/linux/preoverlay-attempt-3/test-count.txt)
if [ "$read_count" != "passed=24 failed=0 ignored=1" ]; then
  echo "unexpected test count: $read_count" >&2
  record test-count 1
fi
record test-count 0

need_ok() {
  if ! grep -F "test $1 ... ok" /evidence/linux/preoverlay-attempt-3/test.log >/dev/null; then
    echo "missing ok result for $1" >&2
    record named-tests 1
  fi
}
need_ok json_doc::tests::duplicate_root_nested_and_escaped_keys_refuse
need_ok duplicate_root_key_refuses
need_ok duplicate_nested_key_refuses
need_ok duplicate_generation_key_refuses
need_ok duplicate_nested_source_cut_refuses
need_ok duplicate_role_slug_refuses
need_ok cli_duplicate_config_stderr_is_static
need_ok cli_failed_child_keeps_marker_and_static_stderr
need_ok init_deadline_failure_does_not_claim_unchanged
need_ok catalog_failure_after_import_does_not_claim_unchanged
need_ok duplicate_manifest_refuses_before_spawn
need_ok duplicate_decisions_refuse_before_review
need_ok preflight_deadline_refuses_before_run_dir
if ! grep -F 'test live_chain ... ignored' /evidence/linux/preoverlay-attempt-3/test.log >/dev/null; then
  echo "live_chain was not ignored" >&2
  record named-tests 1
fi
record named-tests 0

if grep -q 'cargo update' /evidence/linux/preoverlay-attempt-3/fetch.log /evidence/linux/preoverlay-attempt-3/test.log; then
  echo "cargo update text appeared in a log" >&2
  record cargo-update-text 1
fi
record cargo-update-text 0

printf '%s\n' 0 > /evidence/linux/preoverlay-attempt-3/smoke.exit
echo "preoverlay=ok"
exit 0
