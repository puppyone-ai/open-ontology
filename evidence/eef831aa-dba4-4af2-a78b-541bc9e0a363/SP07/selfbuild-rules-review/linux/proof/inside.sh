#!/bin/bash
# Focused E49 review proof. Author tests, Clippy, lock subset, sealed CLI, hand oracle.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules/linux/proof/inside.sh
# Change: owner a363, fixed e5427c1a, independent oracle, no PostgreSQL, no cargo update.
set -u
echo "oo_task=sp07-selfbuild-rules-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a363"
echo "oo_fixed=e5427c1a18e2ddbb480791d214c76888eb7ca998"
echo "oo_platform=linux/arm64"
echo "phase=proof"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
mkdir -p /evidence/linux/proof/phases /cargo-home /target
cd /work

record() {
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/proof/proof.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/proof/rustc-version.txt
cargo -V | tee /evidence/linux/proof/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

printf '%s\n' no > /evidence/linux/proof/cargo-update-invoked.txt

if rustup component list --installed | grep -q '^clippy-'; then
  printf '%s\n' present > /evidence/linux/proof/clippy-component.txt
  record clippy-component 0
else
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  status=$?
  printf '%s\n' "$status" > /evidence/linux/proof/phases/clippy-component.exit
  echo "phase_exit=$status phase=clippy-component"
  if [ "$status" -ne 0 ]; then
    printf '%s\n' "$status" > /evidence/linux/proof/proof.exit
    exit "$status"
  fi
  rustup component list --installed | grep '^clippy-' > /evidence/linux/proof/clippy-component.txt
fi

cargo fetch --locked --manifest-path /work/Cargo.toml > /evidence/linux/proof/fetch.log 2>&1
record fetch $?

export SEALED_CATALOG=/sealed/catalog.json
export SEALED_PROFILE=/sealed/profile.json
export SEALED_CARDS=/sealed/cards.json
cargo test --locked --offline --manifest-path /work/Cargo.toml > /evidence/linux/proof/test.log 2>&1
record test $?

cargo clippy --locked --offline --all-targets --manifest-path /work/Cargo.toml -- -D warnings > /evidence/linux/proof/clippy.log 2>&1
record clippy $?

rustc --edition 2024 -C debuginfo=0 -o /target/lock_subset /evidence/tools/lock_subset.rs > /evidence/linux/proof/lock-subset-build.log 2>&1
record lock-subset-build $?
/target/lock_subset /work/Cargo.lock /locks/Cargo.lock > /evidence/linux/proof/lock-subset.log 2>&1
record lock-subset $?

/target/debug/selfbuild-rules produce \
  --catalog /sealed/catalog.json \
  --profile /sealed/profile.json \
  --cards /sealed/cards.json \
  --auto-accept rule_high_confidence \
  > /evidence/linux/proof/shop-review.json 2> /evidence/linux/proof/shop-review.stderr
record cli $?

sha256sum /sealed/catalog.json /sealed/profile.json /sealed/cards.json /work/tests/fixtures/d-truth.json /evidence/linux/proof/shop-review.json > /evidence/linux/proof/sealed-sha256.txt
fail=0
: > /evidence/linux/proof/forbidden-counts.txt
while IFS= read -r needle; do
  count=$(grep -o -F -- "$needle" /evidence/linux/proof/shop-review.json | wc -l | tr -d ' ')
  printf '%s\t%s\n' "$count" "$needle" >> /evidence/linux/proof/forbidden-counts.txt
  if [ "$count" != "0" ]; then
    fail=1
  fi
done <<'EOF'
甲注评名录
e901f90576b625d9f2252ed2f8659f90d792002742b9d37348549a16c867c991
Ignore previous
non-PII
current_privileges
EOF
record forbidden "$fail"

lib=$(ls /target/debug/deps/libselfbuild_rules-*.rlib | head -n 1)
sj=$(ls /target/debug/deps/libserde_json-*.rlib | head -n 1)
rustc --edition 2024 -C debuginfo=0 -L dependency=/target/debug/deps \
  --extern selfbuild_rules="$lib" \
  --extern serde_json="$sj" \
  -o /target/a363-oracle /evidence/oracle/main.rs > /evidence/linux/proof/oracle-build.log 2>&1
record oracle-build $?

/target/a363-oracle > /evidence/linux/proof/oracle.log 2> /evidence/linux/proof/oracle.stderr
record oracle $?

if ! cmp -s <(printf '%s\n' "$(cat /evidence/linux/proof/oracle-shop.json)") /evidence/linux/proof/shop-review.json; then
  record cli-oracle-bytes 1
fi
record cli-oracle-bytes 0

printf '%s\n' 0 > /evidence/linux/proof/proof.exit
echo proof_done
