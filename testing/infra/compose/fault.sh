#!/bin/bash
# Inject and clear latency or a disconnect on one owned listener.
# A wrong target or a toxic that does not change the query is a failure.
set -euo pipefail

# Classify the invocation and the library before either one is read.
case "$0" in
  *[[:cntrl:]]*)
    echo "refusing control character in invocation path" >&2
    exit 2
    ;;
esac
OO_BOOT_REL='compose/fault.sh'
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
  trap oo_drop_trusted_common EXIT
  unset OO_TRUSTED_COMMON_OWNER
fi

usage() {
  echo "usage: OO_RUN_ID=<uuid> fault.sh prove | fault.sh inject <listener> | fault.sh clear <listener>" >&2
  exit 2
}

oo_require_run_id || exit 2
cmd=${1:-}
name=${2:-}
table=$here/proxies.tsv
[ -f "$table" ] || usage

owned() {
  awk -v n="$1" '$1==n { found=1 } END { exit found ? 0 : 1 }' "$table"
}

case "$cmd" in
  prove) ;;
  inject|clear)
    if ! owned "$name"; then
      echo "status=rejected_target"
      exit 2
    fi
    ;;
  *) usage ;;
esac

if [ "${OO_FIXTURE_FROZEN:-}" = 1 ]; then
  oo_verify_frozen_infra || exit 2
fi

pins=$here/../images.pins
digest=$(awk '$1=="rust-1.98.1" && $2=="linux/arm64" { print $3 }' "$pins")
image="docker.io/library/rust@${digest}"

fields() {
  awk -v n="$1" '$1==n { print $2, $3, $4, $5, $6 }' "$table"
}

api() {
  local network=$1
  shift
  docker run --rm --pull never \
    --network "oo-${OO_RUN_ID}-net-${network}" \
    --label oo.project=open-ontology \
    --label oo.owner=env \
    --label "oo.run_id=${OO_RUN_ID}" \
    --label oo.scope=sp00-infra-fixture \
    --memory 256m \
    --cpus 0.5 \
    --pids-limit 128 \
    --security-opt no-new-privileges \
    --entrypoint curl \
    "$image" \
    "$@"
}

add_toxic() {
  local listener=$1 kind=$2 network service body
  network=$(fields "$listener" | awk '{print $4}')
  service=$(fields "$listener" | awk '{print $5}')
  case "$kind" in
    slow)
      body='{"name":"slow","type":"latency","stream":"downstream","toxicity":1.0,"attributes":{"latency":2000,"jitter":0}}'
      ;;
    cut)
      body='{"name":"cut","type":"timeout","stream":"downstream","toxicity":1.0,"attributes":{"timeout":200}}'
      ;;
    *)
      echo "status=rejected_target"
      exit 2
      ;;
  esac
  if ! api "$network" -fsS -X POST "http://${service}:8474/proxies/${listener}/toxics" -H 'Content-Type: application/json' -d "$body" >/dev/null; then
    echo "status=api_failed listener=$listener"
    exit 1
  fi
  listed=$(api "$network" -fsS "http://${service}:8474/proxies/${listener}/toxics")
  if ! printf '%s\n' "$listed" | grep -Eq "\"name\"[[:space:]]*:[[:space:]]*\"$kind\""; then
    echo "status=noop listener=$listener toxic=$kind"
    exit 1
  fi
  echo "toxic_added listener=$listener kind=$kind"
}

clear_toxic() {
  local listener=$1 kind=$2 network service listed
  network=$(fields "$listener" | awk '{print $4}')
  service=$(fields "$listener" | awk '{print $5}')
  if ! api "$network" -fsS -X DELETE "http://${service}:8474/proxies/${listener}/toxics/${kind}" >/dev/null; then
    echo "status=api_failed listener=$listener"
    exit 1
  fi
  listed=$(api "$network" -fsS "http://${service}:8474/proxies/${listener}/toxics")
  if printf '%s\n' "$listed" | grep -Eq "\"name\"[[:space:]]*:[[:space:]]*\"$kind\""; then
    echo "status=noop listener=$listener toxic=$kind still_present=yes"
    exit 1
  fi
  echo "toxic_cleared listener=$listener kind=$kind"
}

query_listener() {
  local listener=$1 budget=$2 port
  port=$(fields "$listener" | awk '{print $1}')
  case "$port" in
    ''|*[!0-9]*) echo "bad listener port" >&2; exit 1 ;;
  esac
  case "$listener" in
    pg-control)
      docker exec -e "PGCONNECT_TIMEOUT=$budget" "oo-${OO_RUN_ID}-pg-control" \
        bash -c 'export PGPASSWORD=$(tr -d "\n" < /run/secrets/pg_control_password); psql -h toxiproxy-control -p '"$port"' -U oo_fixture_admin -d oo_control -tAc "SELECT 1"'
      ;;
    pg-agent-primary)
      docker exec -e "PGCONNECT_TIMEOUT=$budget" "oo-${OO_RUN_ID}-pg-agent-primary" \
        bash -c 'export PGPASSWORD=$(tr -d "\n" < /run/secrets/pg_agent_password); psql -h toxiproxy-agent -p '"$port"' -U oo_fixture_admin -d oo_agent -tAc "SELECT 1"'
      ;;
    pg-agent-replica)
      docker exec -e "PGCONNECT_TIMEOUT=$budget" "oo-${OO_RUN_ID}-pg-agent-replica" \
        bash -c 'export PGPASSWORD=$(tr -d "\n" < /run/secrets/pg_agent_password); psql -h toxiproxy-agent -p '"$port"' -U oo_fixture_admin -d oo_agent -tAc "SELECT 1"'
      ;;
    mysql-source)
      docker exec "oo-${OO_RUN_ID}-mysql-source" \
        bash -c 'umask 077; cnf=/tmp/oo-fault.cnf; printf "[client]\nuser=oo_src_reader\npassword=%s\nprotocol=tcp\n" "$(tr -d "\n" < /run/secrets/mysql_reader_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" --connect-timeout='"$budget"' -h toxiproxy-source -P '"$port"' -N -e "SELECT 1"; rc=$?; rm -f "$cnf"; exit $rc'
      ;;
    valkey-buffer)
      docker exec "oo-${OO_RUN_ID}-valkey-buffer" \
        sh -c 'command -v timeout >/dev/null || exit 127; REDISCLI_AUTH=$(tr -d "\n" < /run/secrets/valkey_password) timeout '"$budget"' valkey-cli -h toxiproxy-buffer -p '"$port"' --no-auth-warning PING'
      ;;
    valkey-state)
      docker exec "oo-${OO_RUN_ID}-valkey-state" \
        sh -c 'command -v timeout >/dev/null || exit 127; REDISCLI_AUTH=$(tr -d "\n" < /run/secrets/valkey_password) timeout '"$budget"' valkey-cli -h toxiproxy-state -p '"$port"' --no-auth-warning PING'
      ;;
    *)
      echo "status=rejected_target"
      exit 2
      ;;
  esac
}

valkey_latency() {
  local port status elapsed
  port=$(fields valkey-buffer | awk '{print $1}')
  set +e
  elapsed=$(docker exec "oo-${OO_RUN_ID}-valkey-buffer" \
    sh -c 'command -v timeout >/dev/null || exit 127; start=$(date +%s%N); REDISCLI_AUTH=$(tr -d "\n" < /run/secrets/valkey_password); timeout 8 valkey-cli -h toxiproxy-buffer -p '"$port"' --no-auth-warning PING >/dev/null; status=$?; end=$(date +%s%N); echo $((end - start)); exit $status')
  status=$?
  set -e
  echo "latency_ns=$elapsed listener=valkey-buffer status=$status"
  if [ "$status" -ne 0 ] || [ "${elapsed:-0}" -lt 1500000000 ]; then
    echo "status=noop listener=valkey-buffer phase=latency"
    exit 1
  fi
}

prove_one() {
  local listener=$1 status
  echo "=== prove $listener ==="
  query_listener "$listener" 8 >/dev/null
  echo "legal_path=$listener"
  if [ "$listener" = "valkey-buffer" ]; then
    add_toxic "$listener" slow
    valkey_latency
    clear_toxic "$listener" slow
    query_listener "$listener" 8 >/dev/null
    echo "latency_restored listener=$listener"
  fi
  add_toxic "$listener" cut
  set +e
  query_listener "$listener" 2 >/dev/null
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    echo "status=noop listener=$listener phase=disconnect"
    exit 1
  fi
  echo "disconnect_observed listener=$listener status=$status"
  clear_toxic "$listener" cut
  query_listener "$listener" 8 >/dev/null
  echo "restored listener=$listener"
}

case "$cmd" in
  inject)
    add_toxic "$name" "${3:-cut}"
    ;;
  clear)
    clear_toxic "$name" "${3:-cut}"
    ;;
  prove)
    while read -r listener _; do
      [ -n "$listener" ] || continue
      prove_one "$listener"
    done < <(grep -v '^#' "$table")
    echo "status=restored"
    ;;
esac
