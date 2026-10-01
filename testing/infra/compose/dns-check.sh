#!/bin/bash
# Positive DNS on the service's own network, then a cross-network denial.
# A missing resolver is a failure on both sides.
# The probe container is removed only by the id recorded from this run.
set -euo pipefail

# Classify the invocation and the library before either one is read.
case "$0" in
  *[[:cntrl:]]*)
    echo "refusing control character in invocation path" >&2
    exit 2
    ;;
esac
OO_BOOT_REL='compose/dns-check.sh'
OO_BOOT_ARGV0=$0
export OO_BOOT_REL OO_BOOT_ARGV0
oo_boot_line=$(perl -MCwd=abs_path -MDigest::SHA=sha256_hex -MFcntl - << 'PERL'
use strict;
use warnings;
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use Fcntl;

my $UUID = qr/[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}/;

# Shipped fingerprint of testing/infra/lib/common.sh.
# Coordinated constant. Do not hash the sibling to invent this value.
my $SHIPPED_COMMON_SHA256 = "faee8f0564d5105b3fbfc3fbe27d2f5e61917d0e2b0fdebd1d593fd6bd667520";

sub fail {
  my ($msg) = @_;
  print STDERR "$msg\n";
  exit 2;
}

sub bad_text {
  my ($s) = @_;
  return 1 if !defined $s || $s eq "" || $s =~ /[[:cntrl:]]/;
  return 0;
}

my $rel = $ENV{OO_BOOT_REL} // "";
my $argv0 = $ENV{OO_BOOT_ARGV0} // "";
my %ok_rel = map { $_ => 1 } qw(
  compose/cleanup.sh
  compose/up.sh
  compose/smoke.sh
  compose/dns-check.sh
  compose/redact-log.sh
  compose/fault.sh
  compose/configure-proxies.sh
);
fail("refusing invocation path") if bad_text($argv0) || !$ok_rel{$rel};
my $phys = abs_path($argv0);
fail("refusing invocation path") if !defined $phys || $phys =~ /[[:cntrl:]]/;

sub flag_on {
  return ($ENV{OO_FIXTURE_FROZEN} // "") eq "1";
}

sub in_run {
  my ($p) = @_;
  return $p =~ m{\A/(?:private/)?tmp/oo-env-[^/]+/};
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
      if ($cur eq "/tmp" && ($target eq "private/tmp" || $target eq "/private/tmp")) {
        next;
      }
      return 0;
    }
  }
  return 1;
}

sub hash_regular {
  my ($path) = @_;
  fail("refusing symlink path") unless ancestors_ok($path);
  my @st = lstat($path);
  fail("trusted common is not a regular file") if !@st || -l _ || !-f _;
  fail("infra freeze does not match the seal") if $st[4] != $> || ($st[2] & 0222) || $st[3] != 1;
  sysopen(my $fh, $path, O_RDONLY | O_NOFOLLOW) or fail("trusted common is not a regular file");
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  close $fh;
  $raw = "" unless defined $raw;
  return sha256_hex($raw);
}

sub read_nlink1_bytes {
  my ($path) = @_;
  fail("refusing symlink path") unless ancestors_ok($path);
  my @st = lstat($path);
  fail("trusted common is not a regular file") if !@st || -l _ || !-f _;
  fail("refusing hardlinked invocation") if $st[3] != 1;
  sysopen(my $fh, $path, O_RDONLY | O_NOFOLLOW) or fail("trusted common is not a regular file");
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  close $fh;
  return defined $raw ? $raw : "";
}

sub immutable_ok {
  my ($path) = @_;
  my $chflags = "";
  for my $cand (qw(/usr/bin/chflags /sbin/chflags /bin/chflags)) {
    if (-x $cand) { $chflags = $cand; last; }
  }
  return 1 if $chflags eq "";
  my $stat = -x "/usr/bin/stat" ? "/usr/bin/stat" : "/bin/stat";
  open(my $fh, "-|", $stat, "-f", "%f", $path) or return 0;
  my $flags = <$fh>;
  close $fh or return 0;
  return 0 unless defined $flags && $flags =~ /\A[0-9]+\s*\z/;
  return (int($flags) & 2) != 0;
}

sub seal_hash {
  my ($run_id) = @_;
  my $seal = "/tmp/oo-env-$run_id/infra.seal";
  fail("refusing symlink path") unless ancestors_ok($seal);
  my @st = lstat($seal);
  fail("infra freeze does not match the seal") if !@st || -l _ || !-f _;
  fail("infra freeze does not match the seal") if $st[4] != $> || $st[3] != 1;
  fail("infra freeze does not match the seal") unless sprintf("%o", $st[2] & 07777) eq "400";
  fail("infra seal is not immutable") unless immutable_ok($seal);
  sysopen(my $fh, $seal, O_RDONLY | O_NOFOLLOW) or fail("infra freeze does not match the seal");
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  close $fh;
  my @lines = split(/\n/, $raw // "");
  fail("infra freeze does not match the seal") unless @lines >= 4;
  fail("infra freeze does not match the seal") unless $lines[0] eq "oo-infra-seal-v1";
  fail("infra freeze does not match the seal") unless $lines[1] eq "run_id $run_id";
  fail("infra freeze does not match the seal") unless $lines[2] eq "uid $>";
  fail("infra freeze does not match the seal") unless $lines[3] =~ /\Aentries ([0-9]+)\z/;
  my $count = $1;
  my %hash;
  my $seen = 0;
  for my $line (@lines[4 .. $#lines]) {
    next if $line eq "";
    fail("infra freeze does not match the seal") unless $line =~ /\A(?:d|f) /;
    my @f = split(/ /, $line);
    my $path;
    if ($f[0] eq "d") {
      fail("infra freeze does not match the seal") unless @f == 4;
      $path = $f[1];
    } else {
      fail("infra freeze does not match the seal") unless @f == 6 && $f[5] =~ /\A[0-9a-f]{64}\z/;
      $path = $f[1];
      $hash{$path} = $f[5];
    }
    fail("infra freeze does not match the seal") unless $path =~ /\A[A-Za-z0-9._-]+(?:\/[A-Za-z0-9._-]+)*\z/;
    $seen++;
  }
  fail("infra freeze does not match the seal") unless $seen == $count;
  return \%hash;
}

sub require_bytes {
  my ($run_id, $script_path, $common_path) = @_;
  my $want = seal_hash($run_id);
  fail("infra freeze does not match the seal") unless $want->{$rel} && $want->{"lib/common.sh"};
  my $script_hex = hash_regular($script_path);
  my $common_hex = hash_regular($common_path);
  fail("infra freeze does not match the seal") unless $script_hex eq $want->{$rel};
  fail("mismatch lib/common.sh") unless $common_hex eq $want->{"lib/common.sh"};
  fail("refusing substituted library") unless $common_hex eq $SHIPPED_COMMON_SHA256;
  my $tree_script = "/tmp/oo-env-$run_id/infra/$rel";
  my $tree_common = "/tmp/oo-env-$run_id/infra/lib/common.sh";
  fail("infra freeze does not match the seal") if -l $tree_script || -l $tree_common;
  fail("infra freeze does not match the seal") unless hash_regular($tree_script) eq $want->{$rel};
  fail("mismatch lib/common.sh") unless hash_regular($tree_common) eq $want->{"lib/common.sh"};
}

sub emit {
  my ($common, $here) = @_;
  fail("refusing invocation path") if bad_text($common) || bad_text($here);
  fail("refusing invocation path") if $common =~ /\t/ || $here =~ /\t/;
  print "source\t$common\t$here\n";
  exit 0;
}

if ($phys =~ m{\A/(?:private/)?tmp/oo-trusted-($UUID)\.([A-Za-z0-9]+)/entry\.sh\z}) {
  my $run_id = $1;
  fail("refusing direct execution of an unverified frozen script") unless flag_on();
  my $dir = $phys;
  $dir =~ s{/entry\.sh\z}{};
  my $sibling = "$dir/common.sh";
  my $trusted = $ENV{OO_TRUSTED_COMMON} // "";
  fail("refusing untrusted common path") if bad_text($trusted) || $trusted !~ m{\A/};
  fail("trusted common is not a regular file") if -l $trusted || !-f $trusted;
  my $tphys = abs_path($trusted);
  fail("refusing untrusted common path") if !defined $tphys || $tphys ne $sibling;
  fail("frozen infra path is not this run") if ($ENV{OO_FROZEN_INFRA} // "") ne "/tmp/oo-env-$run_id/infra";
  fail("frozen infra is not a real directory") if -l "/tmp/oo-env-$run_id/infra" || !-d "/tmp/oo-env-$run_id/infra";
  my $run = $ENV{OO_RUN_ID} // "";
  fail("refusing untrusted common path") unless $run eq $run_id && $run =~ /\A$UUID\z/;
  require_bytes($run_id, $phys, $sibling);
  emit($sibling, "/tmp/oo-env-$run_id/infra/compose");
}

if (in_run($phys)) {
  fail("refusing direct execution of an unverified frozen script") unless flag_on();
  if ($phys =~ m{\A/(?:private/)?tmp/oo-env-($UUID)/infra/\Q$rel\E\z}) {
    my $run_id = $1;
    my $trusted = $ENV{OO_TRUSTED_COMMON} // "";
    fail("refusing untrusted common path") if bad_text($trusted) || $trusted !~ m{\A/};
    fail("trusted common is not a regular file") if -l $trusted || !-f $trusted;
    my $tphys = abs_path($trusted);
    fail("refusing untrusted common path") unless defined $tphys && $tphys =~ m{\A/(?:private/)?tmp/oo-trusted-\Q$run_id\E\.[A-Za-z0-9]+/common\.sh\z};
    fail("frozen infra path is not this run") if ($ENV{OO_FROZEN_INFRA} // "") ne "/tmp/oo-env-$run_id/infra";
    fail("frozen infra is not a real directory") if -l "/tmp/oo-env-$run_id/infra" || !-d "/tmp/oo-env-$run_id/infra";
    my $run = $ENV{OO_RUN_ID} // "";
    fail("refusing untrusted common path") unless $run eq $run_id && $run =~ /\A$UUID\z/;
    require_bytes($run_id, $phys, $tphys);
    emit($tphys, "/tmp/oo-env-$run_id/infra/compose");
  }
  fail("refusing direct execution of an unverified frozen script");
}

if (flag_on()
    || (($ENV{OO_TRUSTED_COMMON} // "") ne "")
    || (($ENV{OO_FROZEN_INFRA} // "") ne "")
    || (($ENV{OO_TRUSTED_COMMON_OWNER} // "") ne "")) {
  fail("refusing frozen reentry flags on an unstaged invocation");
}
fail("refusing invocation path") unless $phys =~ m{/\Q$rel\E\z};
my $base = $phys;
$base =~ s{/\Q$rel\E\z}{};
my $lib = "$base/lib";
my $common = "$lib/common.sh";
fail("refusing symlink path") if -l $lib || -l $common;
fail("trusted common is not a regular file") unless -f $common;
fail("refusing symlink path") unless ancestors_ok($phys) && ancestors_ok($common);
fail("refusing direct execution of an unverified frozen script") if in_run($phys) || in_run($common);
my @script_st = lstat($phys);
fail("refusing hardlinked invocation") if !@script_st || -l _ || !-f _;
fail("refusing hardlinked invocation") if $script_st[3] != 1;
my $common_bytes = read_nlink1_bytes($common);
fail("refusing substituted library") unless sha256_hex($common_bytes) eq $SHIPPED_COMMON_SHA256;
my $here = $phys;
$here =~ s{/[^/]+\z}{};
emit($common, $here);
PERL
) || exit 2
unset OO_BOOT_REL OO_BOOT_ARGV0
IFS=$(printf '\t') read -r oo_boot_action OO_ENTRY_COMMON here << EOF || exit 2
${oo_boot_line}
EOF
unset oo_boot_line
case "${oo_boot_action:-}" in
  source) ;;
  *)
    echo "refusing invocation path" >&2
    exit 2
    ;;
esac
unset oo_boot_action
if [ -z "${here:-}" ] || [ -z "${OO_ENTRY_COMMON:-}" ]; then
  echo "refusing invocation path" >&2
  exit 2
fi
case "$OO_ENTRY_COMMON" in
  /tmp/oo-env-*|/private/tmp/oo-env-*)
    echo "refusing untrusted common path" >&2
    exit 2
    ;;
esac
if ! printf '%s\n' "$OO_ENTRY_COMMON" | grep -Eq '^/(private/)?tmp/oo-trusted-[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.[A-Za-z0-9]+/common\.sh$' \
  && ! printf '%s\n' "$OO_ENTRY_COMMON" | grep -Eq '/lib/common\.sh$'; then
  echo "refusing untrusted common path" >&2
  exit 2
fi
if [ -L "$OO_ENTRY_COMMON" ] || [ ! -f "$OO_ENTRY_COMMON" ]; then
  echo "trusted common is not a regular file" >&2
  exit 2
fi
# shellcheck disable=SC1090
source "$OO_ENTRY_COMMON"
unset OO_ENTRY_COMMON
if [ "${OO_FIXTURE_FROZEN:-}" = 1 ] && [ "${OO_TRUSTED_COMMON_OWNER:-}" = 1 ]; then
  OO_TRUSTED_DROP=1
  unset OO_TRUSTED_COMMON_OWNER
fi

oo_require_run_id || exit 2
if [ "${OO_FIXTURE_FROZEN:-}" = 1 ]; then
  oo_verify_frozen_infra || exit 2
fi

pins=$here/../images.pins
digest=$(awk '$1=="postgres-17.10-bookworm" && $2=="linux/arm64" { print $3 }' "$pins")
image="docker.io/library/postgres@${digest}"
probe_name="oo-${OO_RUN_ID}-dns-probe"
probe_id=""

probe_identity_ok() {
  local id=$1 line got project owner run scope name
  line=$(docker container inspect --format '{{.Id}} {{index .Config.Labels "oo.project"}} {{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.run_id"}} {{index .Config.Labels "oo.scope"}} {{.Name}}' "$id") || return 1
  got=$(printf '%s\n' "$line" | awk '{print $1}')
  project=$(printf '%s\n' "$line" | awk '{print $2}')
  owner=$(printf '%s\n' "$line" | awk '{print $3}')
  run=$(printf '%s\n' "$line" | awk '{print $4}')
  scope=$(printf '%s\n' "$line" | awk '{print $5}')
  name=$(printf '%s\n' "$line" | awk '{print $6}')
  [ "$got" = "$id" ] || return 1
  [ "$project" = "open-ontology" ] || return 1
  [ "$owner" = "env" ] || return 1
  [ "$run" = "$OO_RUN_ID" ] || return 1
  [ "$scope" = "sp00-infra-fixture" ] || return 1
  [ "$name" = "/$probe_name" ] || return 1
  return 0
}

remove_recorded_probe() {
  if [ -z "$probe_id" ]; then
    if docker container inspect "$probe_name" >/dev/null 2>&1; then
      echo "refusing to delete an unrecorded dns probe" >&2
      return 1
    fi
    return 0
  fi
  if ! printf '%s' "$probe_id" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "dns probe id is not 64 hex" >&2
    return 1
  fi
  if ! probe_identity_ok "$probe_id"; then
    echo "dns probe identity rejected" >&2
    return 1
  fi
  docker rm -f -v "$probe_id" >/dev/null || return 1
  probe_id=""
  return 0
}

on_exit() {
  local status=$?
  if ! remove_recorded_probe; then
    status=1
  fi
  if [ "${OO_TRUSTED_DROP:-}" = 1 ]; then
    oo_drop_trusted_common || status=1
  fi
  if [ "$status" -ne 0 ]; then
    exit "$status"
  fi
}
trap on_exit EXIT

start_probe() {
  local network=$1 id
  if [ "${OO_FIXTURE_FROZEN:-}" = 1 ]; then
    oo_verify_frozen_infra || exit 2
  fi
  if [ -n "$probe_id" ]; then
    remove_recorded_probe || exit 1
  elif docker container inspect "$probe_name" >/dev/null 2>&1; then
    echo "refusing pre-existing dns probe name" >&2
    exit 1
  fi
  id=$(docker run --pull never -d --name "$probe_name" \
    --network "oo-${OO_RUN_ID}-net-${network}" \
    --label oo.project=open-ontology \
    --label oo.owner=env \
    --label "oo.run_id=${OO_RUN_ID}" \
    --label oo.scope=sp00-infra-fixture \
    --memory 128m \
    --cpus 0.25 \
    --pids-limit 64 \
    --security-opt no-new-privileges \
    --mount type=tmpfs,destination=/var/lib/postgresql/data \
    --entrypoint sleep \
    "$image" \
    60) || {
    echo "dns probe start failed network=$network" >&2
    exit 1
  }
  if ! printf '%s' "$id" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "dns probe id is not 64 hex" >&2
    exit 1
  fi
  probe_id=$id
  if ! probe_identity_ok "$probe_id"; then
    echo "dns probe identity rejected" >&2
    exit 1
  fi
  if ! docker exec "$probe_id" sh -c 'command -v getent >/dev/null'; then
    echo "dns_tool_missing network=$network" >&2
    exit 1
  fi
}

expect_resolve() {
  local network=$1 name=$2 status=0 out
  out=$(docker exec "$probe_id" getent hosts "$name" 2>&1) || status=$?
  if [ "$status" -eq 127 ]; then
    echo "dns_tool_missing network=$network name=$name" >&2
    exit 1
  fi
  if [ "$status" -ne 0 ] || [ -z "$out" ]; then
    echo "dns_positive_failed network=$network name=$name status=$status" >&2
    exit 1
  fi
  echo "resolved network=$network name=$name"
}

expect_deny() {
  local network=$1 name=$2 status=0 out
  out=$(docker exec "$probe_id" getent hosts "$name" 2>&1) || status=$?
  if [ "$status" -eq 127 ]; then
    echo "dns_tool_missing network=$network name=$name" >&2
    exit 1
  fi
  if [ "$status" -eq 0 ]; then
    echo "dns_denial_failed network=$network name=$name" >&2
    exit 1
  fi
  if [ -z "$out" ] && [ "$status" -eq 0 ]; then
    echo "dns_empty_output_not_a_denial network=$network name=$name" >&2
    exit 1
  fi
  echo "denied network=$network name=$name status=$status"
}

start_probe agent
expect_resolve agent pg-agent-primary
expect_resolve agent pg-agent-replica
expect_resolve agent toxiproxy-agent
expect_deny agent valkey-buffer
expect_deny agent mysql-source

start_probe buffer
expect_resolve buffer valkey-buffer
expect_resolve buffer toxiproxy-buffer
expect_deny buffer valkey-state
expect_deny buffer pg-agent-primary

start_probe state
expect_resolve state valkey-state
expect_resolve state toxiproxy-state
expect_deny state valkey-buffer
expect_deny state mysql-source

start_probe source
expect_resolve source mysql-source
expect_resolve source toxiproxy-source
expect_deny source valkey-buffer

start_probe control
expect_resolve control pg-control
expect_resolve control toxiproxy-control
expect_deny control valkey-state

for network in control agent source buffer state; do
  internal=$(docker network inspect "oo-${OO_RUN_ID}-net-${network}" --format '{{.Internal}}')
  echo "network=$network internal=$internal"
  [ "$internal" = "true" ]
done
echo "dns_checks=pass"
