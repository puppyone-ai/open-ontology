#!/bin/bash
# Run the four locked recipes, two at a time. Each recipe keeps its own exit.
set -euo pipefail

case "$0" in
  *[[:cntrl:]]*)
    echo "refusing control character in invocation path" >&2
    exit 2
    ;;
esac
oo_boot_phys=$(perl -MCwd=abs_path -e 'my $p = abs_path($ARGV[0]); exit 2 if !defined $p || $p =~ /[[:cntrl:]]/; print $p' -- "$0") || {
  echo "refusing invocation path" >&2
  exit 2
}
case "$oo_boot_phys" in
  /tmp/oo-env-*|/private/tmp/oo-env-*)
    echo "refusing direct execution of an unverified frozen script" >&2
    exit 2
    ;;
esac
case "$oo_boot_phys" in
  */builders/run-matrix.sh) ;;
  *)
    echo "refusing invocation path" >&2
    exit 2
    ;;
esac
oo_boot_base=${oo_boot_phys%/builders/run-matrix.sh}
# Shipped fingerprint lives in the heredoc. A sibling hash is not the anchor.
perl -MDigest::SHA=sha256_hex -MFcntl -MCwd=abs_path - "$oo_boot_phys" "$oo_boot_base/lib/common.sh" << 'PERL' || exit 2
use strict;
use warnings;
use Digest::SHA qw(sha256_hex);
use Fcntl;
use Cwd qw(abs_path);
my $SHIPPED_COMMON_SHA256 = "faee8f0564d5105b3fbfc3fbe27d2f5e61917d0e2b0fdebd1d593fd6bd667520";
sub die_msg {
  my ($msg) = @_;
  print STDERR "$msg\n";
  exit 2;
}
sub ancestors_ok {
  my ($path) = @_;
  my @parts = split(/\//, $path);
  my $cur = "";
  for my $part (@parts) {
    next if $part eq "";
    $cur .= "/$part";
    next if $cur eq $path;
    if (-l $cur) {
      my $target = readlink($cur);
      return 0 unless defined $target;
      next if $cur eq "/tmp" && ($target eq "private/tmp" || $target eq "/private/tmp");
      return 0;
    }
  }
  return 1;
}
sub slurp_nlink1 {
  my ($path) = @_;
  my @st = lstat($path);
  die_msg("trusted common is not a regular file") if !@st || -l _ || !-f _;
  die_msg("refusing hardlinked invocation") if $st[3] != 1;
  sysopen(my $fh, $path, O_RDONLY | O_NOFOLLOW) or die_msg("trusted common is not a regular file");
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  close $fh;
  return defined $raw ? $raw : "";
}
my ($script, $common) = @ARGV;
die_msg("refusing invocation path") if !defined $script || !defined $common;
die_msg("refusing symlink path") if -l $script || -l $common;
my $lib = $common;
$lib =~ s{/common\.sh\z}{};
die_msg("refusing symlink path") if -l $lib;
my $script_phys = abs_path($script);
my $common_phys = abs_path($common);
die_msg("refusing invocation path") if !defined $script_phys || !defined $common_phys;
die_msg("refusing symlink path") unless ancestors_ok($script_phys) && ancestors_ok($common_phys);
slurp_nlink1($script_phys);
my $raw = slurp_nlink1($common_phys);
die_msg("refusing substituted library") unless sha256_hex($raw) eq $SHIPPED_COMMON_SHA256;
exit 0;
PERL
here=$oo_boot_base/builders
# shellcheck source=../lib/common.sh
source "$oo_boot_base/lib/common.sh"
unset oo_boot_phys oo_boot_base

if [ $# -ne 2 ]; then
  echo "usage: OO_RUN_ID=<uuid> run-matrix.sh <snapshot-dir> <log-root>" >&2
  exit 2
fi
snapshot=$1
log_root=$2
oo_require_run_id || exit 2
mkdir -p "$log_root"

run_one() {
  local tc=$1 plat=$2 slug dir
  case "$plat" in
    linux/arm64) slug=arm64 ;;
    linux/amd64) slug=amd64 ;;
    *) return 2 ;;
  esac
  dir="$log_root/${tc}-${slug}"
  mkdir -p "$dir"
  set +e
  bash "$here/cargo-locked.sh" "$tc" "$plat" "$snapshot" "$dir" > "$dir/wrapper.log" 2>&1
  local status=$?
  set -e
  printf '%s\n' "$status" > "$dir/wait.exit"
  echo "recipe ${tc} ${plat} wait_exit=$status"
  return 0
}

run_pair() {
  local tc1=$1 plat1=$2 tc2=$3 plat2=$4
  run_one "$tc1" "$plat1" &
  local pid1=$!
  run_one "$tc2" "$plat2" &
  local pid2=$!
  local status1=0 status2=0
  wait "$pid1" || status1=$?
  wait "$pid2" || status2=$?
  if [ "$status1" -ne 0 ] || [ "$status2" -ne 0 ]; then
    return 1
  fi
  return 0
}

pair_status=0
run_pair 1.98.1 linux/arm64 1.98.1 linux/amd64 || pair_status=1
run_pair 1.94.0 linux/arm64 1.94.0 linux/amd64 || pair_status=1

worst=0
for dir in "$log_root"/*; do
  [ -f "$dir/wait.exit" ] || continue
  code=$(tr -d '\n' < "$dir/wait.exit")
  echo "recorded $(basename "$dir")=$code"
  if [ "$code" -ne 0 ]; then
    worst=$code
  fi
done
if [ "$worst" -ne 0 ]; then
  exit "$worst"
fi
exit "$pair_status"
