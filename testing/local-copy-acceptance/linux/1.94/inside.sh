#!/bin/bash
# Linux arm64 Rust 1.94 measurement of the acceptance runner only.
# Does not compile tools/local-pg-mirror, does not open a database, does not start PostgreSQL.
set -u
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/target/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
EV=/evidence
mkdir -p "$EV/phases" "$TMPDIR"
cd /src/testing/local-copy-acceptance

echo "=== identity ==="
rustc -vV | tee "$EV/rustc-version.txt"
echo "rustc_vv_exit=$?"
uname -a | tee "$EV/uname.txt"
id | tee "$EV/id.txt"

if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > "$EV/version.exit"
  printf '%s\n' 2 > "$EV/smoke.exit"
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > "$EV/version.exit"
  printf '%s\n' 2 > "$EV/smoke.exit"
  exit 2
fi
printf '%s\n' 0 > "$EV/version.exit"
echo "PHASE identity=0"

echo "=== producer archive observation ==="
sha256sum /src/tools/local-pg-mirror/src/copy.rs | tee "$EV/producer-copy-rs.sha256"
grep -n 'verified_reader_guard' /src/tools/local-pg-mirror/src/copy.rs > "$EV/producer-guard-lines.txt" || true
printf '%s\n' "postgres_started=0" > "$EV/postgres-started.txt"
printf '%s\n' "enterprise_stop=false" > "$EV/enterprise-stop.txt"
printf '%s\n' "mirror_compiled=0" > "$EV/mirror-compiled.txt"

echo "=== smoke build ==="
cargo build --locked --offline --bin local-copy-acceptance > "$EV/phases/smoke-build.stdout" 2> "$EV/phases/smoke-build.stderr"
smoke_exit=$?
printf '%s\n' "$smoke_exit" > "$EV/smoke.exit"
echo "PHASE smoke=$smoke_exit"
if [ "$smoke_exit" -ne 0 ]; then
  exit "$smoke_exit"
fi

echo "=== focused test ==="
cargo test --locked --offline --all-targets > "$EV/phases/test.stdout" 2> "$EV/phases/test.stderr"
test_exit=$?
printf '%s\n' "$test_exit" > "$EV/phases/test.exit"
echo "PHASE test=$test_exit"

echo "=== clippy ==="
if ! cargo clippy --version > "$EV/clippy-version.txt" 2>"$EV/phases/clippy-version.stderr"; then
  rustup component add clippy --toolchain 1.94.0 > "$EV/phases/clippy-component.stdout" 2>"$EV/phases/clippy-component.stderr"
  component_exit=$?
  printf '%s\n' "$component_exit" > "$EV/phases/clippy-component.exit"
  echo "PHASE clippy-component=$component_exit"
  cargo clippy --version >> "$EV/clippy-version.txt" 2>>"$EV/phases/clippy-version.stderr" || true
fi
cargo clippy --locked --offline --all-targets --no-deps -- -D warnings > "$EV/phases/clippy.stdout" 2>"$EV/phases/clippy.stderr"
clippy_exit=$?
printf '%s\n' "$clippy_exit" > "$EV/phases/clippy.exit"
echo "PHASE clippy=$clippy_exit"

echo "=== judge archive ==="
BIN=/target/debug/local-copy-acceptance
"$BIN" judge /src/testing/local-copy-acceptance/fixtures/archive-unfixed.json "$EV/verdict.json" > "$EV/phases/judge.stdout" 2>"$EV/phases/judge.stderr"
judge_exit=$?
printf '%s\n' "$judge_exit" > "$EV/phases/judge.exit"
echo "PHASE judge=$judge_exit"

cat > "$EV/must-not-run" << 'EOF'
#!/bin/sh
echo executed > /evidence/executed-marker
EOF
chmod 755 "$EV/must-not-run"
rm -f "$EV/executed-marker"
"$BIN" run /evidence/must-not-run /src/testing/local-copy-acceptance/fixtures/archive-unfixed.json "$EV/run.json" > "$EV/phases/run.stdout" 2>"$EV/phases/run.stderr"
run_exit=$?
printf '%s\n' "$run_exit" > "$EV/phases/run.exit"
echo "PHASE run=$run_exit"
if [ -f "$EV/executed-marker" ]; then
  echo "PHASE producer_spawned=1"
  printf '%s\n' 1 > "$EV/producer-spawned.exit"
else
  echo "PHASE producer_spawned=0"
  printf '%s\n' 0 > "$EV/producer-spawned.exit"
fi

if [ "$test_exit" -ne 0 ] || [ "$clippy_exit" -ne 0 ] || [ "$judge_exit" -ne 0 ] || [ "$run_exit" -ne 4 ]; then
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
if [ -f "$EV/executed-marker" ]; then
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
if ! grep -q '"mp01_signed": false' "$EV/verdict.json"; then
  echo "judge signed or verdict missing" >&2
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
printf '%s\n' 0 > "$EV/focused.exit"
exit 0
