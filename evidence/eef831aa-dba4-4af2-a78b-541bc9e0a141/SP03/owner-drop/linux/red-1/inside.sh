#!/bin/bash
# Unmodified 58afa97 archive plus the owner-Drop regression only.
set -eu
echo "oo_task=owner-drop-red-1"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=58afa97_archive_plus_pg_reader_drop_live_only"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a141-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a141-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a141-red
git config --file "$GIT_CONFIG_GLOBAL" user.email a141-red@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
rustc -Vv
cargo -V
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
if [ ! -f crates/oo-connectors/tests/pg_reader_drop_live.rs ]; then
  echo "drop regression overlay is missing" >&2
  printf '%s\n' 2 > /evidence/test.exit
  exit 2
fi
sha256sum \
  crates/oo-connectors/src/snapshot.rs \
  crates/oo-connectors/src/stream.rs \
  crates/oo-connectors/src/session.rs \
  crates/oo-connectors/tests/pg_reader_drop_live.rs \
  > /evidence/archive-sha256.txt
snap_hash=$(sha256sum crates/oo-connectors/src/snapshot.rs | awk '{print $1}')
stream_hash=$(sha256sum crates/oo-connectors/src/stream.rs | awk '{print $1}')
session_hash=$(sha256sum crates/oo-connectors/src/session.rs | awk '{print $1}')
test "$snap_hash" = "$(awk '{print $1}' /evidence/expect-snapshot.sha256)"
test "$stream_hash" = "$(awk '{print $1}' /evidence/expect-stream.sha256)"
test "$session_hash" = "$(awk '{print $1}' /evidence/expect-session.sha256)"
echo "production_blobs_match_58afa97=1"

stat -c %s /pglog/postgresql.log > /evidence/log-mark-before.txt
echo "log_mark_before=$(cat /evidence/log-mark-before.txt)"

set +e
cargo test --offline --locked -p oo-connectors --test pg_reader_drop_live -- --nocapture --test-threads=1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "phase_exit=$test_exit phase=pg_reader_drop_live"

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
exit "$test_exit"
