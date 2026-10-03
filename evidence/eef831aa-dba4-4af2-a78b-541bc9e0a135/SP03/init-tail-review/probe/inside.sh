#!/bin/bash
# Probes run only after the smoke container has exited.
# The overlay test is added in the snapshot copy. Product blobs are not edited.
set -u
echo "oo_task=init-tail-review-probe"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
cd /src

got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/probe/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/probe/toolchain.exit

if [ ! -f crates/oo-connectors/tests/a135_tail_probe.rs ]; then
  echo "overlay test missing" >&2
  printf '%s\n' 4 > /evidence/probe/snapshot.exit
  exit 4
fi
buf=$(sha256sum crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs | awk '{print $1}')
want=$(awk '{print $1}' /evidence/smoke/expect-buffered.sha256)
stream=$(sha256sum crates/oo-connectors/src/stream.rs | awk '{print $1}')
want_stream=$(awk '{print $1}' /evidence/smoke/stream.sha256)
printf 'buffered=%s\nstream=%s\n' "$buf" "$stream" > /evidence/probe/blobs-before.txt
if [ "$buf" != "$want" ] || [ "$stream" != "$want_stream" ]; then
  echo "product blob changed before probes" >&2
  printf '%s\n' 4 > /evidence/probe/snapshot.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/probe/snapshot.exit

run_phase() {
  name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/probe/${name}.stdout" 2> "/evidence/probe/${name}.stderr"
  phase_exit=$?
  set +e
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/probe/${name}.exit"
  cat "/evidence/probe/${name}.stdout"
  cat "/evidence/probe/${name}.stderr" >&2
}

run_phase probe cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a135_tail_probe -- --test-threads=1 --skip tls_verify_full_moderate_rows
probe_exit=$(cat /evidence/probe/probe.exit)
if [ "$probe_exit" -ne 0 ]; then
  printf '%s\n' "$probe_exit" > /evidence/probe/worst.exit
  exit "$probe_exit"
fi

run_phase author-tail cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_wire_tls_tail -- --test-threads=1 --skip tls_pg_sixty_thousand_small_rows
author_exit=$(cat /evidence/probe/author-tail.exit)

run_phase tls env OO_A135_REQUIRE_TLS=1 cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a135_tail_probe -- --test-threads=1 tls_verify_full_moderate_rows --exact
tls_exit=$(cat /evidence/probe/tls.exit)

run_phase stream-live env OO_PG_STREAM_OUTCOME=/evidence/probe/stream-live-outcome.txt cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_stream_live -- --test-threads=1 --nocapture
live_exit=$(cat /evidence/probe/stream-live.exit)

run_phase stream-bounds env OO_PG_STREAM_OUTCOME=/evidence/probe/stream-bounds-outcome.txt cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_stream_bounds -- --test-threads=1 --nocapture
bounds_exit=$(cat /evidence/probe/stream-bounds.exit)

buf_after=$(sha256sum crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs | awk '{print $1}')
stream_after=$(sha256sum crates/oo-connectors/src/stream.rs | awk '{print $1}')
printf 'buffered=%s\nstream=%s\n' "$buf_after" "$stream_after" > /evidence/probe/blobs-after.txt
if [ "$buf_after" != "$want" ] || [ "$stream_after" != "$want_stream" ]; then
  echo "product blob changed during probes" >&2
  printf '%s\n' 4 > /evidence/probe/blob-after.exit
else
  printf '%s\n' 0 > /evidence/probe/blob-after.exit
fi

worst=0
for phase_exit in "$author_exit" "$tls_exit" "$live_exit" "$bounds_exit" "$(cat /evidence/probe/blob-after.exit)"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
echo "elapsed_seconds=$SECONDS"
printf '%s\n' "$worst" > /evidence/probe/worst.exit
exit "$worst"
