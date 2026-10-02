#!/bin/bash
# Phases on the frozen snapshot. A failing phase is recorded and the script continues.
set -u
cd /src
mkdir -p /phases
: > /phases/exits.txt

phase() {
  name=$1
  shift
  set +e
  "$@" > "/phases/${name}.out" 2> "/phases/${name}.err"
  ec=$?
  printf '%s %s\n' "$name" "$ec" >> /phases/exits.txt
  printf '%s\n' "$ec" > "/phases/${name}.exit"
  return 0
}

rm -f /phases/gold.done /phases/reader.log
phase rustc rustc -vV
phase clippy-component rustup component add clippy --toolchain 1.94.0
phase fetch cargo fetch --locked

sha256sum crates/oo-connectors/src/types/mod.rs crates/oo-connectors/Cargo.toml Cargo.lock ci/deny.toml > /phases/before-probe.sha256
cp /probe/review_wire.rs crates/oo-connectors/src/types/review_wire.rs
cp /probe/independent_gold.rs crates/oo-connectors/tests/independent_gold.rs
cp /probe/log_audit.rs crates/oo-connectors/tests/log_audit.rs
if ! grep -q '^mod review_wire;$' crates/oo-connectors/src/types/mod.rs; then
  printf '\n#[cfg(test)]\nmod review_wire;\n' >> crates/oo-connectors/src/types/mod.rs
fi
sha256sum crates/oo-connectors/src/types/mod.rs crates/oo-connectors/src/types/review_wire.rs crates/oo-connectors/tests/independent_gold.rs crates/oo-connectors/tests/log_audit.rs crates/oo-connectors/Cargo.toml Cargo.lock ci/deny.toml > /phases/after-probe.sha256

phase test-lib cargo test -p oo-connectors --lib --locked --offline
phase test-package env -u OO_PG_FIXTURE -u OO_PG_TYPES_FIXTURE cargo test -p oo-connectors --locked --offline
if grep -q 'OO_PG_FIXTURE is required' /phases/test-package.err /phases/test-package.out; then
  echo yes > /phases/package-panic-is-missing-fixture.txt
else
  echo no > /phases/package-panic-is-missing-fixture.txt
fi
phase clippy cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings
: > /phases/independent-gold.txt
rm -f /phases/reader.log
phase test-independent-gold env OO_A110_POINTER=/fixture/pointer.json OO_A110_REPORT=/phases/independent-gold.txt cargo test -p oo-connectors --test independent_gold --locked --offline -- --ignored --nocapture
printf 'gold-finished\n' > /phases/gold.done

waited=0
while [ "$waited" -lt 180 ]; do
  if [ -f /phases/reader.log ]; then
    break
  fi
  sleep 2
  waited=$((waited + 2))
done
printf '%s\n' "$waited" > /phases/log-wait-seconds.txt
if [ -f /phases/reader.log ]; then
  phase test-log-audit env OO_A110_LOG=/phases/reader.log OO_A110_PASSWORD_FILE=/fixture/reader.pass OO_A110_AUDIT_REPORT=/phases/log-audit.txt cargo test -p oo-connectors --test log_audit --locked --offline -- --ignored --nocapture
else
  echo 'test-log-audit skipped' >> /phases/exits.txt
fi

phase install-audit cargo install cargo-audit --version 0.22.2 --locked --root /cargo/tools
phase install-deny cargo install cargo-deny --version 0.20.2 --locked --root /cargo/tools
export PATH="/cargo/tools/bin:${PATH}"
phase audit-version cargo audit --version
phase deny-version cargo deny --version
phase deny-help cargo deny --help
phase audit cargo audit --json
# 0.20.2 takes --config as a global flag before the subcommand. The attempt-1
# form `cargo deny check --config` is preserved and is not a license result.
phase deny cargo deny --config ci/deny.toml --locked check
phase tree-tiny cargo tree -p oo-connectors --locked --offline -e all -i tiny-keccak@2.0.2
phase tree-tiny-normal cargo tree -p oo-connectors --locked --offline --edges normal -i tiny-keccak@2.0.2
phase tree-bigint4 cargo tree -p oo-connectors --locked --offline --edges normal -i num-bigint@0.4.8
phase tree-bigint5 cargo tree -p oo-connectors --locked --offline --edges normal -i num-bigint@0.5.1
phase tree-workspace-tiny cargo tree --locked --offline -e all -i tiny-keccak@2.0.2
phase tree-tiny-all cargo tree -p oo-connectors --locked --offline --target all -e all -i tiny-keccak@2.0.2
phase tree-normal cargo tree -p oo-connectors --locked --offline --edges normal
echo inside_done >> /phases/exits.txt
