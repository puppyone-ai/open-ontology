#!/bin/bash
# Pre-overlay smoke inside the one arm64 builder.
# Same onboarding commands as deploy/trusted-demo/linux/inside.sh, then stop.
# No workspace binaries, no PostgreSQL, no apply-reviewed, no cargo update.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/phases /cargo-home/bin /target/onboarding
cd /src

record() {
  mkdir -p /evidence/phases
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/phase.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" > /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/fixed-base.txt
if [ "$sha" != "25dfc9022780c3deff12f37b7ad7ea4ed1fb6480" ]; then
  record source 2
fi
if ! grep -q 'changes_may_have_been_applied' /src/tools/pg-onboarding/src/outcome.rs; then
  echo "archive tool is not the fixed outcome vocabulary" >&2
  record source 2
fi
record source 0
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

echo "=== hash frozen product and recipes ==="
rustup run 1.94.0 rustc --edition 2021 /deploy/tools/sha256_manifest.rs -o /cargo-home/bin/sha256-manifest \
  > /evidence/phases/manifest-build.log 2>&1
record manifest-build $?
/cargo-home/bin/sha256-manifest /deploy /evidence/deploy-sha256.txt > /evidence/deploy-sha256.console 2>&1
record hash-deploy $?
/cargo-home/bin/sha256-manifest /src/tools/pg-onboarding /evidence/onboarding-sha256.txt > /evidence/onboarding-sha256.console 2>&1
record hash-onboarding $?
/cargo-home/bin/sha256-manifest /recipes /evidence/recipes-sha256.txt > /evidence/recipes-sha256.console 2>&1
record hash-recipes $?

echo "=== cargo fetch workspace --locked ==="
cargo fetch --manifest-path /src/Cargo.toml --locked > /evidence/fetch-workspace.log 2>&1
record fetch-workspace $?
echo "=== cargo fetch pg-onboarding --locked ==="
cargo fetch --manifest-path /src/tools/pg-onboarding/Cargo.toml --locked > /evidence/fetch-onboarding.log 2>&1
record fetch-onboarding $?

echo "=== rustup component add clippy ==="
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
record clippy-component $?

echo "=== cargo test pg-onboarding --locked --offline ==="
CARGO_TARGET_DIR=/target/onboarding cargo test --manifest-path /src/tools/pg-onboarding/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/test.log 2>&1
record test $?

sum_field() {
  grep '^test result:' /evidence/test.log \
    | sed -n "s/.* \\([0-9][0-9]*\\) $1.*/\\1/p" \
    | awk '{s+=$1} END{print s+0}'
}
printf 'passed=%s failed=%s ignored=%s\n' "$(sum_field passed)" "$(sum_field failed)" "$(sum_field ignored)" \
  > /evidence/test-count.txt
failed=$(sed -n 's/.*failed=\([0-9][0-9]*\).*/\1/p' /evidence/test-count.txt)
passed=$(sed -n 's/passed=\([0-9][0-9]*\).*/\1/p' /evidence/test-count.txt)
ignored=$(sed -n 's/.*ignored=\([0-9][0-9]*\).*/\1/p' /evidence/test-count.txt)
if [ "$failed" != "0" ]; then
  echo "onboarding tests failed" >&2
  record test-count 1
fi
if ! grep -F 'test live_chain ... ignored' /evidence/test.log >/dev/null; then
  echo "live_chain was not ignored" >&2
  record test-count 1
fi
if ! grep -F 'test duplicate_decisions_refuse_before_review ... ok' /evidence/test.log >/dev/null; then
  echo "duplicate decision test did not pass" >&2
  record test-count 1
fi
if ! grep -F 'test cli_failed_child_keeps_marker_and_static_stderr ... ok' /evidence/test.log >/dev/null; then
  echo "CLI child-failure test did not pass" >&2
  record test-count 1
fi
if [ "$passed" != "24" ] || [ "$ignored" != "1" ]; then
  echo "onboarding count was not 24 passed and 1 ignored" >&2
  record test-count 1
fi
record test-count 0

echo "=== cargo clippy pg-onboarding ==="
CARGO_TARGET_DIR=/target/onboarding cargo clippy --manifest-path /src/tools/pg-onboarding/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/clippy.log 2>&1
record clippy $?

echo "=== drive tests ==="
rustup run 1.94.0 rustc --edition 2024 --test -D warnings /deploy/src/drive.rs -o /cargo-home/bin/drive-test \
  > /evidence/drive-test-build.log 2>&1
record drive-test-build $?
/cargo-home/bin/drive-test --test-threads=1 > /evidence/drive-test.log 2>&1
record drive-test $?

echo "=== drive clippy ==="
rustup run 1.94.0 clippy-driver --edition 2024 -D warnings /deploy/src/drive.rs -o /target/drive-clippy \
  > /evidence/drive-clippy.log 2>&1
record drive-clippy $?

printf '%s\n' 0 > /evidence/phase.exit
printf '%s\n' 'onboarding_passed=24 onboarding_failed=0 onboarding_ignored=1 live_chain=ignored chain=not_run' > /evidence/smoke-ok.txt
echo "smoke=ok"
exit 0
