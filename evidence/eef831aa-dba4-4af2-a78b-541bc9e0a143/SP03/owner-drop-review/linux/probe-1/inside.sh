#!/bin/bash
# Fixed 6b95d6f2 archive. One drop, then the affected cancel and keys tests once.
set -eu
echo "oo_task=owner-drop-review-probe-1"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=6b95d6f2_archive"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a143-probe-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a143-probe-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a143-probe-1
git config --file "$GIT_CONFIG_GLOBAL" user.email a143-probe-1@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
rustc -Vv
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

cd /src
sha256sum \
  crates/oo-connectors/src/snapshot.rs \
  crates/oo-connectors/src/session.rs \
  crates/oo-connectors/src/stream.rs \
  crates/oo-connectors/tests/pg_reader_drop_live.rs \
  contracts/source/pgsql/reader-lifecycle-v1.md \
  > /evidence/archive-sha256.txt
check_hash() {
  rel=$1
  expect=$2
  got=$(sha256sum "$rel" | awk '{print $1}')
  test "$got" = "$(awk '{print $1}' "$expect")"
}
check_hash crates/oo-connectors/src/snapshot.rs /evidence/expect-snapshot.sha256
check_hash crates/oo-connectors/src/session.rs /evidence/expect-session.sha256
check_hash crates/oo-connectors/src/stream.rs /evidence/expect-stream.sha256
check_hash crates/oo-connectors/tests/pg_reader_drop_live.rs /evidence/expect-drop-test.sha256
check_hash contracts/source/pgsql/reader-lifecycle-v1.md /evidence/expect-lifecycle.sha256
echo "archive_hashes_match=1"
stat -c %s /pglog/postgresql.log > /evidence/log-mark-before.txt

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

set +e
cargo test --offline --locked -p oo-connectors --test pg_reader_drop_live -- --nocapture --test-threads=1
note pg_reader_drop_live $?
set -e
set +e
cargo test --offline --locked -p oo-connectors --test pg_reader_cancel_live -- --nocapture --test-threads=1
note pg_reader_cancel_live $?
set -e
set +e
cargo test --offline --locked -p oo-connectors --test pg_reader_keys_live -- --nocapture --test-threads=1
note pg_reader_keys_live $?
set -e

set +e
perl -e '
  use strict;
  use warnings;
  my $mark = 0;
  if (open my $m, "<", "/evidence/log-mark-before.txt") {
    $mark = 0 + <$m>;
  }
  open my $log, "<:raw", "/pglog/postgresql.log" or die $!;
  my $size = -s $log;
  $mark = 0 if $mark < 0 || $mark > $size;
  seek $log, $mark, 0 or die $!;
  local $/;
  my $text = <$log> // "";
  close $log;
  for my $name ("reader.pass", "writer.pass", "super.pass") {
    next unless open my $p, "<", "/secrets/$name";
    my $secret = <$p> // "";
    close $p;
    $secret =~ s/\s+\z//;
    next if $secret eq "";
    my $quoted = quotemeta($secret);
    $text =~ s/$quoted/[redacted]/g;
  }
  open my $out, ">:raw", "/evidence/postgres-window.log" or die $!;
  print $out $text;
  close $out;
'
slice_exit=$?
printf '%s\n' "$slice_exit" > /evidence/slice.exit
echo "slice_exit=$slice_exit"
echo "container_fail=$fail"
exit "$fail"
