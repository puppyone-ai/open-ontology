#!/bin/bash
# Exercise one fresh fixture run. Observations are specific; weaker errors fail.
set -euo pipefail

# Classify the invocation and the library before either one is read.
case "$0" in
  *[[:cntrl:]]*)
    echo "refusing control character in invocation path" >&2
    exit 2
    ;;
esac
OO_BOOT_REL='compose/smoke.sh'
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

oo_require_run_id || exit 2
oo_reject_secret_override || exit 2
root=$(oo_run_root)
secret=$(oo_secret_dir)
if [ "${OO_FIXTURE_FROZEN:-}" != 1 ]; then
  if [ -e "$root/infra" ] || [ -L "$root/infra" ]; then
    oo_exec_frozen_script "$root" compose/smoke.sh "$@"
    exit 2
  fi
fi
if [ "${OO_FIXTURE_FROZEN:-}" = 1 ]; then
  oo_verify_frozen_infra || exit 2
fi

echo "evidence_session=$(oo_session_id)"
echo "run_id=$OO_RUN_ID"
mkdir -p "$root/logs"
chmod 700 "$root/logs"
prefix="oo-${OO_RUN_ID}"

psql_at() {
  local container=$1 secret_name=$2 host=$3 port=$4 db=$5 sql=$6
  docker exec "$prefix-$container" bash -c \
    'export PGPASSWORD=$(tr -d "\n" < /run/secrets/'"$secret_name"'); psql -h '"$host"' -p '"$port"' -U oo_fixture_admin -d '"$db"' -v ON_ERROR_STOP=1 -tAc "$1"' \
    _ "$sql"
}

section() { echo "=== $1 ==="; }

token=replay-$(openssl rand -hex 16)
denied=denied-$(openssl rand -hex 16)
ref=$(openssl rand -hex 8)
leg=$((300000 + 16#$(openssl rand -hex 2)))
rid=$((100000 + 16#$(openssl rand -hex 3)))
if ! printf '%s' "$token" | grep -Eq '^replay-[0-9a-f]{32}$'; then
  echo "replay token failed validation" >&2
  exit 1
fi
if ! printf '%s' "$denied" | grep -Eq '^denied-[0-9a-f]{32}$'; then
  echo "denied token failed validation" >&2
  exit 1
fi
if ! printf '%s' "$ref" | grep -Eq '^[0-9a-f]{16}$'; then
  echo "mysql ref failed validation" >&2
  exit 1
fi
echo "replay_token=$token"
echo "denied_token=$denied"
echo "mysql_ref=$ref"
echo "mysql_leg=$leg"
echo "replay_id=$rid"

section control_marker
marker=$(psql_at pg-control pg_control_password toxiproxy-control 15432 oo_control "SELECT fixture || ' ' || note FROM oo_fixture.ownership WHERE id = 1")
echo "control_marker=$marker"
[ "$marker" = "pg-control synthetic local control fixture" ]

section agent_replay
before=$(psql_at pg-agent-primary pg_agent_password toxiproxy-agent 15433 oo_agent "SELECT count(*) FROM oo_fixture.replay_probe WHERE id = $rid OR note = '$token'")
echo "replay_before=$before"
[ "$before" = "0" ]
psql_at pg-agent-primary pg_agent_password toxiproxy-agent 15433 oo_agent "INSERT INTO oo_fixture.replay_probe (id, note) VALUES ($rid, '$token')" >/dev/null
primary_lsn=$(psql_at pg-agent-primary pg_agent_password toxiproxy-agent 15433 oo_agent "SELECT pg_current_wal_lsn()")
if ! printf '%s' "$primary_lsn" | grep -Eq '^[0-9A-F]+/[0-9A-F]+$'; then
  echo "primary lsn is not a pg_lsn" >&2
  exit 1
fi
echo "primary_lsn=$primary_lsn"
ok=0
for i in $(seq 1 60); do
  row=$(docker exec "$prefix-pg-agent-replica" bash -c \
    'export PGPASSWORD=$(tr -d "\n" < /run/secrets/pg_agent_password); psql -h toxiproxy-agent -p 15434 -U oo_fixture_admin -d oo_agent -tAc "$1"' \
    _ "SELECT pg_is_in_recovery()::text || ' ' || COALESCE(status,'') || ' ' || COALESCE(pg_last_wal_replay_lsn()::text,'') || ' ' || (pg_last_wal_replay_lsn() >= '$primary_lsn'::pg_lsn)::text || ' ' || COALESCE((SELECT note FROM oo_fixture.replay_probe WHERE id = $rid),'') FROM pg_stat_wal_receiver")
  echo "poll=$i $row"
  set -- $row
  # boolean::text is "true"; an uncast psql boolean is "t".
  if { [ "${1:-}" = "true" ] || [ "${1:-}" = "t" ]; } \
    && [ "${2:-}" = "streaming" ] \
    && { [ "${4:-}" = "true" ] || [ "${4:-}" = "t" ]; } \
    && [ "${5:-}" = "$token" ]; then
    ok=1
    break
  fi
  sleep 1
done
[ "$ok" = 1 ]
echo "replay_caught_up=yes"

section agent_reader_denial
reader_count=$(docker exec "$prefix-pg-agent-primary" bash -c \
  'export PGPASSWORD=$(tr -d "\n" < /run/secrets/agent_ro_password); psql -h toxiproxy-agent -p 15433 -U oo_agent_reader -d oo_agent -v ON_ERROR_STOP=1 -tAc "SELECT count(*) FROM oo_fixture.replay_probe"')
echo "reader_count=$reader_count"
[ "$reader_count" -ge 1 ]
set +e
denial=$(docker exec "$prefix-pg-agent-primary" bash -c \
  'export PGPASSWORD=$(tr -d "\n" < /run/secrets/agent_ro_password); psql -h toxiproxy-agent -p 15433 -U oo_agent_reader -d oo_agent -v VERBOSITY=verbose -c "$1"' \
  _ "INSERT INTO oo_fixture.replay_probe (id, note) VALUES (9, '$denied')" 2>&1)
denial_status=$?
set -e
printf '%s\n' "$denial" > "$root/logs/pg-denial.txt"
echo "pg_denial_status=$denial_status"
oo_classify_pg_write_denial "$denial"
[ "$denial_status" -ne 0 ]
left=$(psql_at pg-agent-primary pg_agent_password toxiproxy-agent 15433 oo_agent "SELECT count(*) FROM oo_fixture.replay_probe WHERE note = '$denied'")
echo "denied_rows_written=$left"
[ "$left" = "0" ]
cols=$(psql_at pg-agent-primary pg_agent_password toxiproxy-agent 15433 oo_agent "SELECT string_agg(column_name, ',' ORDER BY column_name) FROM information_schema.columns WHERE table_schema='oo_fixture' AND table_name='replay_probe'")
echo "replay_columns=$cols"
[ "$cols" = "id,note" ]
attrs=$(psql_at pg-agent-primary pg_agent_password toxiproxy-agent 15433 oo_agent "SELECT rolsuper::int || ' ' || rolcreaterole::int || ' ' || rolcreatedb::int || ' ' || rolcanlogin::int || ' ' || rolreplication::int || ' ' || rolbypassrls::int FROM pg_roles WHERE rolname='oo_agent_reader'")
echo "reader_attrs=$attrs"
[ "$attrs" = "0 0 0 1 0 0" ]
privs=$(psql_at pg-agent-primary pg_agent_password toxiproxy-agent 15433 oo_agent "SELECT has_table_privilege('oo_agent_reader','oo_fixture.replay_probe','SELECT')::int || ' ' || has_table_privilege('oo_agent_reader','oo_fixture.replay_probe','INSERT')::int || ' ' || has_table_privilege('oo_agent_reader','oo_fixture.replay_probe','UPDATE')::int || ' ' || has_table_privilege('oo_agent_reader','oo_fixture.replay_probe','DELETE')::int || ' ' || has_table_privilege('oo_agent_reader','oo_fixture.replay_probe','TRUNCATE')::int")
echo "reader_table_privs=$privs"
[ "$privs" = "1 0 0 0 0" ]
echo "denial_attempts=1"

section mysql_reader_audit
reader_out=$(docker exec "$prefix-mysql-source" bash -c \
  'umask 077; cnf=/tmp/oo-reader.cnf; printf "[client]\nuser=oo_src_reader\npassword=%s\nprotocol=tcp\n" "$(tr -d "\n" < /run/secrets/mysql_reader_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" --connect-timeout=8 -h toxiproxy-source -P 13306 -N -e "SELECT COUNT(*) FROM oo_local_test.shipment_leg"; rc=$?; rm -f "$cnf"; exit $rc')
echo "reader_count=$reader_out"
[ "$reader_out" = "2" ]
set +e
mysql_denial=$(docker exec "$prefix-mysql-source" bash -c \
  'umask 077; cnf=/tmp/oo-reader.cnf; printf "[client]\nuser=oo_src_reader\npassword=%s\nprotocol=tcp\n" "$(tr -d "\n" < /run/secrets/mysql_reader_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" --connect-timeout=8 -h toxiproxy-source -P 13306 -e "$1"; rc=$?; rm -f "$cnf"; exit $rc' \
  _ "INSERT INTO oo_local_test.shipment_leg (leg_id, synthetic_ref, qty) VALUES ($leg, '$ref', 1)" 2>&1)
mysql_status=$?
set -e
printf '%s\n' "$mysql_denial" > "$root/logs/mysql-denial.txt"
echo "mysql_denial_status=$mysql_status"
oo_classify_mysql_write_denial "$mysql_denial"
[ "$mysql_status" -ne 0 ]
present=$(docker exec "$prefix-mysql-source" bash -c \
  'umask 077; cnf=/tmp/oo-root.cnf; printf "[client]\nuser=root\npassword=%s\nprotocol=socket\n" "$(tr -d "\n" < /run/secrets/mysql_root_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" -N -e "$1"; rc=$?; rm -f "$cnf"; exit $rc' \
  _ "SELECT COUNT(*) FROM oo_local_test.shipment_leg WHERE synthetic_ref='$ref'")
echo "mysql_denied_rows_written=$present"
[ "$present" = "0" ]
audit=$(docker exec "$prefix-mysql-source" bash -c \
  'umask 077; cnf=/tmp/oo-root.cnf; printf "[client]\nuser=root\npassword=%s\nprotocol=socket\n" "$(tr -d "\n" < /run/secrets/mysql_root_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" -N -e "$1"; rc=$?; rm -f "$cnf"; exit $rc' \
  _ "SELECT CONCAT_WS(' ', (SELECT COUNT(*) FROM mysql.general_log WHERE argument LIKE '%$ref%' AND argument LIKE 'INSERT INTO oo_local_test.shipment_leg%'), (SELECT COUNT(*) FROM mysql.general_log WHERE (argument LIKE 'CREATE%' OR argument LIKE 'ALTER USER%' OR argument LIKE 'SET PASSWORD%' OR argument LIKE 'GRANT %') AND argument LIKE CONCAT('%IDENTIFIED',' BY%')), (SELECT @@global.general_log), (SELECT @@global.log_output))")
echo "mysql_audit=$audit"
set -- $audit
[ "${1:-0}" -ge 1 ]
[ "${2:-1}" -eq 0 ]
[ "${3:-0}" = "1" ]
[ "${4:-}" = "TABLE" ]
schema_privs=$(docker exec "$prefix-mysql-source" bash -c \
  'umask 077; cnf=/tmp/oo-root.cnf; printf "[client]\nuser=root\npassword=%s\nprotocol=socket\n" "$(tr -d "\n" < /run/secrets/mysql_root_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" -N -B -e "$1"; rc=$?; rm -f "$cnf"; exit $rc' \
  _ "SELECT GRANTEE, PRIVILEGE_TYPE, TABLE_SCHEMA FROM information_schema.SCHEMA_PRIVILEGES WHERE GRANTEE IN ('''oo_src_reader''@''%''', '''oo_src_reader''@''localhost''') ORDER BY 1, 2, 3")
printf '%s\n' "$schema_privs" > "$root/logs/mysql-schema-privs.txt"
echo "mysql_schema_privs<<EOF"
printf '%s\n' "$schema_privs"
echo "EOF"
user_privs=$(docker exec "$prefix-mysql-source" bash -c \
  'umask 077; cnf=/tmp/oo-root.cnf; printf "[client]\nuser=root\npassword=%s\nprotocol=socket\n" "$(tr -d "\n" < /run/secrets/mysql_root_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" -N -B -e "$1"; rc=$?; rm -f "$cnf"; exit $rc' \
  _ "SELECT GRANTEE, PRIVILEGE_TYPE FROM information_schema.USER_PRIVILEGES WHERE GRANTEE IN ('''oo_src_reader''@''%''', '''oo_src_reader''@''localhost''') ORDER BY 1, 2")
printf '%s\n' "$user_privs" > "$root/logs/mysql-user-privs.txt"
echo "mysql_user_privs<<EOF"
printf '%s\n' "$user_privs"
echo "EOF"
extra=$(docker exec "$prefix-mysql-source" bash -c \
  'umask 077; cnf=/tmp/oo-root.cnf; printf "[client]\nuser=root\npassword=%s\nprotocol=socket\n" "$(tr -d "\n" < /run/secrets/mysql_root_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" -N -e "$1"; rc=$?; rm -f "$cnf"; exit $rc' \
  _ "SELECT CONCAT_WS(' ', (SELECT COUNT(*) FROM information_schema.SCHEMA_PRIVILEGES WHERE GRANTEE IN ('''oo_src_reader''@''%''', '''oo_src_reader''@''localhost''') AND PRIVILEGE_TYPE <> 'SELECT'), (SELECT COUNT(*) FROM information_schema.SCHEMA_PRIVILEGES WHERE GRANTEE IN ('''oo_src_reader''@''%''', '''oo_src_reader''@''localhost''') AND TABLE_SCHEMA <> 'oo_local_test'), (SELECT COUNT(*) FROM mysql.role_edges WHERE TO_USER='oo_src_reader' OR FROM_USER='oo_src_reader'), (SELECT COUNT(*) FROM mysql.global_grants WHERE USER='oo_src_reader'))")
echo "mysql_extra_privs=$extra"
[ "$extra" = "0 0 0 0" ]
[ -s "$root/logs/mysql-schema-privs.txt" ]
grep -q "SELECT" "$root/logs/mysql-schema-privs.txt"
if grep -E -q $'\t(INSERT|UPDATE|DELETE|DROP|CREATE|ALTER|INDEX|GRANT)\t' "$root/logs/mysql-schema-privs.txt"; then
  echo "mysql reader has a non-select schema privilege" >&2
  exit 1
fi
usage_lines=$(grep -c $'\tUSAGE$' "$root/logs/mysql-user-privs.txt" || true)
echo "mysql_usage_lines=$usage_lines"
[ "$usage_lines" -ge 2 ]
if grep -E -vq $'\tUSAGE$' "$root/logs/mysql-user-privs.txt"; then
  echo "mysql reader has a global privilege other than USAGE" >&2
  exit 1
fi

section valkey
for role in buffer state; do
  expect=always
  [ "$role" = "state" ] && expect=everysec
  docker exec "$prefix-valkey-$role" sh -c 'REDISCLI_AUTH=$(tr -d "\n" < /run/secrets/valkey_password) valkey-cli -h toxiproxy-'"$role"' -p '"$([ "$role" = buffer ] && echo 16379 || echo 16380)"' --no-auth-warning SET oo-aof-probe '"$token"' >/dev/null'
  fsync=$(docker exec "$prefix-valkey-$role" sh -c 'REDISCLI_AUTH=$(tr -d "\n" < /run/secrets/valkey_password) valkey-cli --raw --no-auth-warning CONFIG GET appendfsync' | awk 'NF { value=$0 } END { print value }')
  info=$(docker exec "$prefix-valkey-$role" sh -c 'REDISCLI_AUTH=$(tr -d "\n" < /run/secrets/valkey_password) valkey-cli --no-auth-warning INFO persistence' | grep -E '^(aof_enabled|aof_last_write_status):')
  uid=$(docker exec "$prefix-valkey-$role" sh -c 'for comm in /proc/[0-9]*/comm; do name=$(tr -d "\n" < "$comm"); if [ "$name" = "valkey-server" ]; then pid=${comm%/comm}; pid=${pid##*/}; awk -v f=2 "/^Uid:/ { print \$"f"; exit }" "/proc/$pid/status"; exit 0; fi; done; exit 1')
  echo "valkey_$role fsync=$fsync uid=$uid"
  printf '%s\n' "$info"
  [ "$fsync" = "$expect" ]
  printf '%s\n' "$info" | grep -q '^aof_enabled:1'
  printf '%s\n' "$info" | grep -q '^aof_last_write_status:ok'
  [ "$uid" != "0" ]
done

section dns
oo_bash_frozen_sibling dns-check.sh

section ports
count=0
while IFS= read -r id; do
  [ -n "$id" ] || continue
  name=$(docker inspect --format '{{.Name}}' "$id")
  privileged=$(docker inspect --format '{{.HostConfig.Privileged}}' "$id")
  mode=$(docker inspect --format '{{.HostConfig.NetworkMode}}' "$id")
  bindings=$(docker inspect --format '{{json .HostConfig.PortBindings}}' "$id")
  binds=$(docker inspect --format '{{json .HostConfig.Binds}}' "$id")
  project=$(docker inspect --format '{{index .Config.Labels "oo.project"}}' "$id")
  owner=$(docker inspect --format '{{index .Config.Labels "oo.owner"}}' "$id")
  run=$(docker inspect --format '{{index .Config.Labels "oo.run_id"}}' "$id")
  echo "resource name=$name privileged=$privileged network=$mode port_bindings=$bindings project=$project owner=$owner run=$run"
  [ "$privileged" = "false" ]
  [ "$mode" != "host" ]
  [ "$project" = "open-ontology" ]
  [ "$owner" = "env" ]
  [ "$run" = "$OO_RUN_ID" ]
  case "$bindings" in
    null|'{}') ;;
    *) echo "published ports on $name" >&2; exit 1 ;;
  esac
  printf '%s\n' "$binds" | grep -q 'docker.sock' && { echo "docker.sock on $name" >&2; exit 1; }
  count=$((count + 1))
done < <(docker ps -aq --filter "label=oo.run_id=$OO_RUN_ID" --filter "label=oo.scope=sp00-infra-fixture")
echo "fixture_containers=$count"
[ "$count" -ge 11 ]

section fault
oo_bash_frozen_sibling fault.sh prove
set +e
oo_bash_frozen_sibling fault.sh inject not-a-listener > "$root/logs/fault-rejected.txt" 2>&1
rejected=$?
set -e
echo "fault_rejected_exit=$rejected"
grep -q 'status=rejected_target' "$root/logs/fault-rejected.txt"
[ "$rejected" -eq 2 ]
if grep -q 'status=restored' "$root/logs/fault-rejected.txt"; then
  echo "rejected target echoed success" >&2
  exit 1
fi

section secret_scan
if [ "${OO_FIXTURE_FROZEN:-}" = 1 ]; then
  oo_verify_frozen_infra || exit 2
fi
docker exec "$prefix-mysql-source" bash -c \
  'umask 077; cnf=/tmp/oo-root.cnf; printf "[client]\nuser=root\npassword=%s\nprotocol=socket\n" "$(tr -d "\n" < /run/secrets/mysql_root_password)" > "$cnf"; chmod 600 "$cnf"; mysql --defaults-extra-file="$cnf" -N -e "SELECT COALESCE(argument,'"'"''"'"') FROM mysql.general_log" > /tmp/oo-gl.txt; rm -f "$cnf"'
docker cp "$prefix-mysql-source:/tmp/oo-gl.txt" "$root/logs/mysql-general-arguments.txt"
docker exec "$prefix-mysql-source" rm -f /tmp/oo-gl.txt
chmod 600 "$root/logs/mysql-general-arguments.txt"
while IFS= read -r id; do
  [ -n "$id" ] || continue
  name=$(docker inspect --format '{{.Name}}' "$id")
  name=${name#/}
  docker logs "$id" > "$root/logs/${name}.log" 2>&1
  chmod 600 "$root/logs/${name}.log"
done < <(docker ps -aq --filter "label=oo.run_id=$OO_RUN_ID")
scan_fail=0
for log in "$root/logs"/*.log "$root/logs"/*.txt; do
  [ -f "$log" ] || continue
  case "$log" in
    *.sha256|*/compose-config.txt) ;;
  esac
  set +e
  oo_bash_frozen_sibling redact-log.sh "$log" "$secret"
  status=$?
  set -e
  echo "redact file=$(basename "$log") exit=$status"
  if [ "$status" -ne 0 ]; then
    scan_fail=1
  fi
done
if [ "$scan_fail" -ne 0 ]; then
  echo "secret scan failed; raw values were redacted and the failure stands" >&2
  exit 1
fi

section debezium_pending
set +e
oo_bash_frozen_sibling debezium/pending.sh
debezium_status=$?
set -e
echo "debezium_pending_exit=$debezium_status"
[ "$debezium_status" -eq 2 ]

section runsc
if command -v runsc >/dev/null 2>&1; then
  echo "runsc_present=yes"
else
  echo "runsc_present=no"
fi
docker info --format 'runtimes={{range $k, $v := .Runtimes}}{{$k}} {{end}}default={{.DefaultRuntime}}' || true
echo "gvisor_acceptance=not_claimed"
echo "fixture_admin_is_test_setup=yes"
echo "production_zero_source_write=not_claimed"
echo "smoke_ok"
