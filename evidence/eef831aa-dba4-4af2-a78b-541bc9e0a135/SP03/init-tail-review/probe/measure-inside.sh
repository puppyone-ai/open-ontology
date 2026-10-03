#!/bin/bash
# Numeric replay of the unit plateau and the TLS read.
# Does not rerun pg_stream_live or pg_stream_bounds.
set -u
echo "oo_task=init-tail-review-measure"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
cd /src
mkdir -p /evidence/probe/measured

got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
printf '%s\n' "$got" > /evidence/probe/measured/rustc.txt

buf=$(sha256sum crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs | awk '{print $1}')
stream=$(sha256sum crates/oo-connectors/src/stream.rs | awk '{print $1}')
printf 'buffered=%s\nstream=%s\n' "$buf" "$stream" > /evidence/probe/measured/blobs-before.txt

run_phase() {
  name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/probe/measured/${name}.stdout" 2> "/evidence/probe/measured/${name}.stderr"
  phase_exit=$?
  set +e
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/probe/measured/${name}.exit"
  cat "/evidence/probe/measured/${name}.stdout"
  cat "/evidence/probe/measured/${name}.stderr" >&2
}

run_phase unit cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a135_tail_probe -- --test-threads=1 --nocapture --skip tls_verify_full_moderate_rows
unit_exit=$(cat /evidence/probe/measured/unit.exit)
if [ "$unit_exit" -ne 0 ]; then
  printf '%s\n' "$unit_exit" > /evidence/probe/measured/worst.exit
  exit "$unit_exit"
fi

run_phase tls env OO_A135_REQUIRE_TLS=1 cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a135_tail_probe -- --test-threads=1 --nocapture tls_verify_full_moderate_rows --exact
tls_exit=$(cat /evidence/probe/measured/tls.exit)

buf_after=$(sha256sum crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs | awk '{print $1}')
stream_after=$(sha256sum crates/oo-connectors/src/stream.rs | awk '{print $1}')
printf 'buffered=%s\nstream=%s\n' "$buf_after" "$stream_after" > /evidence/probe/measured/blobs-after.txt
blob_exit=0
if [ "$buf_after" != "$buf" ] || [ "$stream_after" != "$stream" ]; then
  blob_exit=4
fi
printf '%s\n' "$blob_exit" > /evidence/probe/measured/blob-after.exit

worst=0
for phase_exit in "$tls_exit" "$blob_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /evidence/probe/measured/worst.exit
exit "$worst"
