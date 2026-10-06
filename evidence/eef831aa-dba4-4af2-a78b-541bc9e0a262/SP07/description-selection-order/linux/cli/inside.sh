#!/bin/bash
# Compile the unchanged description-candidate CLI from commit 827e768b and run
# it on the documents the repaired library just produced. No PostgreSQL. No model.
set -u
echo "oo_task=sp07-description-selection-order-cli"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a262"
echo "oo_fixed=fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a"
echo "oo_cli=827e768b9f94af6134a5734bd16e066ab0d47528"
echo "oo_platform=linux/arm64"
echo "phase=cli"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/phases /evidence/fixtures /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -a /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/toolchain-aside.txt
cp /patch/description_candidate.rs crates/oo-builder/src/description_candidate.rs
cp /overlay/description_selection_order.rs crates/oo-builder/tests/description_selection_order.rs
cp /cli/description-candidate.rs crates/oo-builder/src/bin/description-candidate.rs

record() {
  mkdir -p /evidence/phases
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

abort() {
  record "$1" "$2"
  printf '%s\n' "$2" > /evidence/cli.exit
  exit "$2"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; abort toolchain 2 ;;
esac

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
if [ "$(cat /evidence/phases/sha256-tool.exit)" -ne 0 ]; then
  printf '%s\n' 2 > /evidence/cli.exit
  exit 2
fi
export PATH="/cargo-home/bin:${PATH}"

if cmp -s crates/oo-builder/src/description_candidate.rs /blobs/description_candidate.rs; then
  echo "cli proof source is still the unfixed blob" >&2
  abort source 2
fi
if ! cmp -s crates/oo-builder/src/bin/description-candidate.rs /cli/description-candidate.rs; then
  echo "cli source copy drifted" >&2
  abort source 2
fi
if [ -e /src/crates/oo-builder/src/bin/description-candidate.rs ]; then
  echo "fixed archive already contains the CLI" >&2
  abort source 2
fi
printf '%s\n' "patch_test_and_cli" > /evidence/overlay-copied.txt
record source 0
/cargo-home/bin/sha256 file \
  crates/oo-builder/src/description_candidate.rs \
  crates/oo-builder/src/bin/description-candidate.rs \
  crates/oo-builder/tests/description_selection_order.rs \
  > /evidence/source-sha256.txt
record owned-hash $?
diff -rq /src /cargo-home/work/src | LC_ALL=C sort > /evidence/tree-diff.txt || true
cat > /evidence/tree-diff-expected.txt <<'EOF'
Files /src/crates/oo-builder/src/description_candidate.rs and /cargo-home/work/src/crates/oo-builder/src/description_candidate.rs differ
Only in /cargo-home/work/src/crates/oo-builder/src/bin: description-candidate.rs
Only in /cargo-home/work/src/crates/oo-builder/tests: description_selection_order.rs
Only in /cargo-home/work/src: rust-toolchain.toml.aside
Only in /src: rust-toolchain.toml
EOF
if ! cmp -s /evidence/tree-diff.txt /evidence/tree-diff-expected.txt; then
  echo "cli tree diff is not the patch, the test, the CLI, and toolchain-aside" >&2
  record tree 2
else
  record tree 0
fi
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.log 2>&1
offline_fetch=$?
set -e
printf '%s\n' "$offline_fetch" > /evidence/phases/fetch-offline.exit
if [ "$offline_fetch" -ne 0 ]; then
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.log 2>&1
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.log /evidence/fetch.log
  fetch_exit=0
fi
record fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' "$fetch_exit" > /evidence/cli.exit
  exit "$fetch_exit"
fi

echo "=== dump repaired library documents ==="
set +e
OO_A262_DUMP=/evidence/fixtures \
  cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_selection_order -- --test-threads=1 \
  > /evidence/test-order.log 2>&1
order_exit=$?
set -e
record test-order "$order_exit"
if [ "$order_exit" -ne 0 ]; then
  printf '%s\n' "$order_exit" > /evidence/cli.exit
  exit "$order_exit"
fi

echo "=== cargo build description-candidate ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin description-candidate \
  > /evidence/build-cli.log 2>&1
build_exit=$?
set -e
record build-cli "$build_exit"
if [ "$build_exit" -ne 0 ]; then
  printf '%s\n' "$build_exit" > /evidence/cli.exit
  exit "$build_exit"
fi

echo "=== cli stdout matches the repaired library ==="
cli_exit=0
: > /evidence/cli-compare.txt
shopt -s nullglob
inputs=(/evidence/fixtures/*.in)
if [ "${#inputs[@]}" -eq 0 ]; then
  echo "no dumped command documents" >> /evidence/cli-compare.txt
  cli_exit=1
fi
for input in "${inputs[@]}"; do
  base=${input%.in}
  name=$(basename "$base")
  phase=$(cat "$base.phase")
  set +e
  /target/debug/description-candidate "$phase" < "$input" > "$base.cli" 2> "$base.cli.err"
  one=$?
  set -e
  printf '%s %s %s\n' "$name" "$phase" "$one" >> /evidence/cli-compare.txt
  if [ "$one" -ne 0 ] || ! cmp -s "$base.out" "$base.cli"; then
    echo "cli mismatch $name phase=$phase exit=$one" >> /evidence/cli-compare.txt
    cli_exit=1
  fi
done
record cli-compare "$cli_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/lock-after.sha256
before=$(/cargo-home/bin/sha256 file /blobs/Cargo.lock | awk '{print $1}')
after=$(cat /evidence/lock-after.sha256)
printf '%s\n' "$before" > /evidence/lock-archive.sha256
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/lock-hash.txt
  record lock-hash 1
else
  echo "unchanged $after" > /evidence/lock-hash.txt
  record lock-hash 0
fi

final=0
if [ "$cli_exit" -ne 0 ]; then
  final=$cli_exit
elif [ "$(cat /evidence/phases/tree.exit)" -ne 0 ]; then
  final=2
elif [ "$(cat /evidence/phases/lock-hash.exit)" -ne 0 ]; then
  final=1
fi
printf '%s\n' "$final" > /evidence/cli.exit
if [ "$final" -eq 0 ]; then
  echo "cli=ok"
else
  echo "cli=failed exit=$final"
fi
exit "$final"
