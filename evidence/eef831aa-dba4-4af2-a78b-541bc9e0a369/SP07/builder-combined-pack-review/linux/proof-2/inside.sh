#!/bin/bash
# Independent review of be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba.
# The archive is mounted read-only. The oracle is copied in only after the
# focused combined tests and Clippy. PostgreSQL is not started. No model call.
# cargo update is not invoked. Locks are not regenerated.
set -u
set -e
set -o pipefail
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a369-review
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/cargo-home/tmp
export A362_RECEIPTS=/receipts
export A369_RECEIPTS=/receipts
export A369_SUMMARY=/evidence/oracle-summary.json
mkdir -p /cargo-home/tmp /evidence/phases /work
printf '%s\n' no > /evidence/cargo-update-invoked.txt
printf '%s\n' not_started > /evidence/postgres.txt

record() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

run_phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/${name}.log" 2>&1
  local status=$?
  set -e
  record "$name" "$status"
  if [ "$status" -ne 0 ]; then
    exit "$status"
  fi
}

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
uname -a | tee /evidence/uname.txt
id
rustc_exit=0
grep -q 'release: 1.94' /evidence/rustc-version.txt || rustc_exit=1
grep -q 'aarch64-unknown-linux-gnu' /evidence/rustc-version.txt || rustc_exit=1
record toolchain "$rustc_exit"
if [ "$rustc_exit" -ne 0 ]; then
  exit "$rustc_exit"
fi
if [ ! -d /receipts/live/ready/pass1-calls ]; then
  echo "stored receipts are not mounted" >&2
  record receipts 2
  exit 2
fi
want_oracle=b29bbd5e85ed7006c02dab912bb65b7c6a092f71cfc412a16d0aa51cbb1512af
got_oracle=$(sha256sum /a357/a357_consumer.rs | awk '{print $1}')
printf '%s\n' "$got_oracle" > /evidence/a357-oracle.sha256
if [ "$got_oracle" != "$want_oracle" ]; then
  echo "a357 oracle hash drifted" >&2
  record receipts 2
  exit 2
fi
record receipts 0

echo "=== copy the git archive ==="
rm -rf /work
mkdir -p /work
cp -a /src/. /work/
rm -f /work/rust-toolchain.toml
printf '%s\n' moved-aside > /evidence/toolchain-aside.txt
cd /work
if [ -f tools/pg-builder/tests/a369_review.rs ]; then
  echo "oracle was inside the archive" >&2
  record pre-oracle 2
  exit 2
fi
for lock in Cargo.lock tools/pg-builder/Cargo.lock tools/description-pack/Cargo.lock; do
  if ! cmp -s "/src/$lock" "/work/$lock"; then
    echo "lock differs from the archive: $lock" >&2
    record archive-locks 2
    exit 2
  fi
done
record archive-locks 0
sha256sum Cargo.lock > /evidence/root-lock-before.sha256
sha256sum tools/pg-builder/Cargo.lock tools/description-pack/Cargo.lock > /evidence/child-locks-before.sha256
{
  sha256sum \
    /receipts/live/ready/pass1-calls/shop.party-batch0.json \
    /receipts/live/ready/pass1-calls/shop.account-batch0.json \
    /receipts/live/ready/pass1-calls/shop.entry-batch0.json \
    /receipts/live/pass1/curated.json \
    /receipts/live/pass1/cards.json \
    /receipts/live/pass1/catalog.json \
    /receipts/live/pass1/profile.json \
    /receipts/live/pass1/seal.json \
    /receipts/live/ready/timeout.json \
    /receipts/live/ready/unavailable.json \
    /receipts/live/ready/pass2.json \
    /a357/a357_consumer.rs
} > /evidence/provenance-before.sha256

echo "=== fetch the committed locks ==="
run_phase fetch-builder cargo fetch --manifest-path tools/pg-builder/Cargo.toml --locked
run_phase fetch-pack cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked
run_phase fetch-builder-offline cargo fetch --manifest-path tools/pg-builder/Cargo.toml --locked --offline
run_phase fetch-pack-offline cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked --offline

echo "=== pre-oracle focused tests ==="
record pre-oracle 0
run_phase test-builder-combined cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --offline --test combined -- --test-threads=1
run_phase test-pack-combined cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test combined -- --test-threads=1

echo "=== clippy of the repaired crates ==="
set +e
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  component_exit=0
  printf '%s\n' already_installed > /evidence/clippy-component.log
else
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
  component_exit=$?
fi
set -e
record clippy-component "$component_exit"
if [ "$component_exit" -ne 0 ]; then
  exit "$component_exit"
fi
run_phase clippy-builder cargo clippy --manifest-path tools/pg-builder/Cargo.toml --locked --offline --all-targets -- -D warnings
run_phase clippy-pack cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings

echo "=== independent oracle ==="
cp /oracle/a369_review.rs /work/tools/pg-builder/tests/a369_review.rs
run_phase test-oracle cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --offline --test a369_review -- --test-threads=1 --nocapture
run_phase clippy-oracle cargo clippy --manifest-path tools/pg-builder/Cargo.toml --locked --offline --test a369_review -- -D warnings
rm -f /work/tools/pg-builder/tests/a369_review.rs
if [ -f /work/tools/pg-builder/tests/a369_review.rs ]; then
  echo "oracle file remained in the work tree" >&2
  record product-bytes 2
  exit 2
fi

echo "=== product bytes match the archive ==="
set +e
python_status=0
while IFS= read -r -d '' file; do
  rel=${file#/src/}
  if [ "$rel" = "rust-toolchain.toml" ]; then
    continue
  fi
  if ! cmp -s "$file" "/work/$rel"; then
    echo "product byte mismatch: $rel" >&2
    python_status=1
    break
  fi
done < <(find /src/tools/pg-builder /src/tools/description-pack /src/tools/description-review /src/crates /src/Cargo.toml /src/Cargo.lock /src/.cargo -type f ! -name '._*' ! -name '.DS_Store' -print0)
set -e
record product-bytes "$python_status"
if [ "$python_status" -ne 0 ]; then
  exit "$python_status"
fi

sha256sum Cargo.lock > /evidence/root-lock-after.sha256
sha256sum tools/pg-builder/Cargo.lock tools/description-pack/Cargo.lock > /evidence/child-locks-after.sha256
if ! cmp -s /evidence/root-lock-before.sha256 /evidence/root-lock-after.sha256; then
  echo "root lock changed" >&2
  record locks-unchanged 2
  exit 2
fi
if ! cmp -s /evidence/child-locks-before.sha256 /evidence/child-locks-after.sha256; then
  echo "child lock changed" >&2
  record locks-unchanged 2
  exit 2
fi
record locks-unchanged 0
{
  sha256sum \
    /receipts/live/ready/pass1-calls/shop.party-batch0.json \
    /receipts/live/ready/pass1-calls/shop.account-batch0.json \
    /receipts/live/ready/pass1-calls/shop.entry-batch0.json \
    /receipts/live/pass1/curated.json \
    /receipts/live/pass1/cards.json \
    /receipts/live/pass1/catalog.json \
    /receipts/live/pass1/profile.json \
    /receipts/live/pass1/seal.json \
    /receipts/live/ready/timeout.json \
    /receipts/live/ready/unavailable.json \
    /receipts/live/ready/pass2.json \
    /a357/a357_consumer.rs
} > /evidence/provenance-after.sha256
if ! cmp -s /evidence/provenance-before.sha256 /evidence/provenance-after.sha256; then
  echo "receipt bytes changed" >&2
  record provenance 2
  exit 2
fi
record provenance 0
test -s /evidence/oracle-summary.json
echo PROOF_OK
