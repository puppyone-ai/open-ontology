#!/bin/bash
# The package test stopped after the lib harness because two pre-existing
# license tests failed. This follow-up runs only the xtask CLI harness.
set -u
OUT=/out
exec > >(tee -a "$OUT/console.log") 2>&1
printf 'start %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
rustc -vV >"$OUT/rustc-version.txt" 2>&1 || true
cargo test --manifest-path /src/Cargo.toml --locked --offline -p xtask --test cli -- --test-threads=8 >"$OUT/xtask-cli.log" 2>&1
code=$?
printf '%s\n' "$code" >"$OUT/xtask-cli.exit"
printf 'cli exit %s\n' "$code"
awk '/^test result:/{print}' "$OUT/xtask-cli.log" >"$OUT/test-results.txt" || true
awk '/^test [^ ]+ \.\.\. FAILED/{print}' "$OUT/xtask-cli.log" >"$OUT/test-failures.txt" || true
if [ "$code" -eq 0 ]; then
  exit 0
fi
exit 1
