#!/bin/bash
# Deciding capture of the independent fixture. Earlier probes keep their logs.
set -u
echo "oo_task=import-pg-cli-review-probe-4"
echo "composition=3fba40dd_archive_plus_a164_overlay"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a164-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a164-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a164-probe
git config --file "$GIT_CONFIG_GLOBAL" user.email a164-probe@invalid.example
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit
rustc -Vv > /evidence/rustc-version.txt
cd /src
head -n 1189 crates/oo-connectors/src/session.rs > /tmp/session-prefix.rs
if ! cmp -s /tmp/session-prefix.rs /archive/crates/oo-connectors/src/session.rs; then
  echo "session prefix differs" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if ! cmp -s crates/oo-pipeline/src/full_import.rs /archive/crates/oo-pipeline/src/full_import.rs; then
  echo "full_import differs" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/snapshot.exit
mkdir -p /evidence/out
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test import_pg_a164_review -- --ignored --test-threads=1 > /evidence/review.stdout 2> /evidence/review.stderr
code=$?
set -e
printf '%s\n' "$code" > /evidence/review.exit
printf '%s\n' "$code" > /evidence/inside.exit
echo "phase_exit=$code phase=review"
exit "$code"
