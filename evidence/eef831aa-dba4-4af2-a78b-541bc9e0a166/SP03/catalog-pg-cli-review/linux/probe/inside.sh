#!/bin/bash
# Review probe. Archive copy plus one safe Rust test. One private PostgreSQL.
set -u
echo "oo_task=catalog-pg-cli-review-probe"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=34498e75_archive_plus_a166_overlay"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a166-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a166-gitsystem
unset RUST_LOG || true
unset CARGO_REGISTRIES_CRATES_IO_PROTOCOL || true
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a166-probe
git config --file "$GIT_CONFIG_GLOBAL" user.email a166-probe@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line"
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit
rustc -Vv > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt

cd /src
fail=0
note() {
  name=$1
  code=$2
  printf '%s\n' "$code" > "/evidence/${name}.exit"
  echo "phase_exit=$code phase=$name"
  if [ "$code" -ne 0 ] && [ "$fail" -eq 0 ]; then
    fail=$code
  fi
}

sha256sum \
  Cargo.lock \
  Cargo.toml \
  crates/oo-pipeline/Cargo.toml \
  crates/oo-builder/Cargo.toml \
  crates/oo-connectors/Cargo.toml \
  contracts/pipeline/catalog-pg-cli-v1.md \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/src/catalog_cli.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/full_import.rs \
  crates/oo-pipeline/tests/catalog_pg_cli_live.rs \
  crates/oo-pipeline/tests/import_pg_cli_live.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  > /evidence/archive-sha256.txt
if ! cmp -s /evidence/archive-sha256.txt /evidence/expect-sha256.txt; then
  echo "product archive sha mismatch" >&2
  diff -u /evidence/expect-sha256.txt /evidence/archive-sha256.txt >&2 || true
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ ! -f crates/oo-pipeline/tests/catalog_pg_a166_review.rs ]; then
  echo "review overlay is absent" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
overlay_got=$(sha256sum crates/oo-pipeline/tests/catalog_pg_a166_review.rs | awk '{print $1}')
overlay_exp=$(tr -d '[:space:]' < /evidence/overlay.sha256)
printf '%s\n' "$overlay_got" > /evidence/overlay-got.sha256
if [ "$overlay_got" != "$overlay_exp" ]; then
  echo "overlay sha mismatch" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if grep -n 'unsafe' crates/oo-pipeline/tests/catalog_pg_a166_review.rs | grep -v 'forbid(unsafe_code)'; then
  echo "review overlay contains an unsafe token" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
echo "archive_and_overlay_match=1"
printf '%s\n' 0 > /evidence/snapshot.exit

set +e
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy-'; then
  echo "clippy_component=present" | tee /evidence/rustup-clippy.stdout
  printf '%s\n' 0 > /evidence/rustup-clippy.exit
else
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
  printf '%s\n' $? > /evidence/rustup-clippy.exit
fi
note rustup-clippy "$(cat /evidence/rustup-clippy.exit)"
if [ "$(cat /evidence/rustup-clippy.exit)" -ne 0 ]; then
  printf '%s\n' "$(cat /evidence/rustup-clippy.exit)" > /evidence/inside.exit
  exit "$(cat /evidence/rustup-clippy.exit)"
fi

cargo build --manifest-path Cargo.toml --locked --offline -p oo-pipeline -p oo-builder --bins > /evidence/build-bins.stdout 2> /evidence/build-bins.stderr
note build-bins $?
if [ "$(cat /evidence/build-bins.exit)" -ne 0 ]; then
  printf '%s\n' "$(cat /evidence/build-bins.exit)" > /evidence/inside.exit
  exit "$(cat /evidence/build-bins.exit)"
fi

cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test import_pg_cli_live > /evidence/import-existing.stdout 2> /evidence/import-existing.stderr
note import-existing $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test catalog_pg_cli_live > /evidence/catalog-existing.stdout 2> /evidence/catalog-existing.stderr
note catalog-existing $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test catalog_pg_a166_review -- --exact narrow_invalid_strict_and_noargs_exits > /evidence/narrow.stdout 2> /evidence/narrow.stderr
note narrow $?

cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings > /evidence/pipeline-clippy.stdout 2> /evidence/pipeline-clippy.stderr
note pipeline-clippy $?

mkdir -p /evidence/out
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test catalog_pg_a166_review -- --ignored --exact catalog_stdout_matches_postgres_and_the_fixed_builder > /evidence/review.stdout 2> /evidence/review.stderr
note review $?

if [ -d /evidence/out ]; then
  if grep -R -n -e 'a166-keel-comment' -e 'PRIVATE KEY' /evidence/out; then
    echo "raw comment or private key in probe output" >&2
    note leak 1
  else
    echo "probe_output_scan=clean" | tee /evidence/leak.stdout
    note leak 0
  fi
fi
set -e
echo "probe_fail=$fail"
printf '%s\n' "$fail" > /evidence/inside.exit
exit "$fail"
