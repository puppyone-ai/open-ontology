#!/bin/bash
# Shared run identity, secret checks, and cleanup for the SP00 fixture.
# Source this file. Do not execute it.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  echo "source testing/infra/lib/common.sh" >&2
  exit 2
fi

oo_session_id() {
  # No baked-in evidence directory. Unset, stale, or unverified input stays
  # unspecified. A bound id is accepted only from a readonly owned seal whose
  # bytes name the same canonical session id and this uid.
  local claimed=${OO_EVIDENCE_SESSION:-}
  local seal=${OO_EVIDENCE_SEAL:-}
  if ! printf '%s' "$claimed" | grep -Eq '^eef831aa-dba4-4af2-a78b-541bc9e0a0[0-9a-f]{2}$'; then
    printf '%s\n' 'unspecified'
    return 0
  fi
  if ! perl -MFcntl - "$seal" "$claimed" "$(id -u)" << 'PERL'
use strict;
use warnings;
use Fcntl;
my ($seal, $claimed, $uid) = @ARGV;
exit 1 if !defined $seal || $seal !~ m{\A/} || $seal =~ /[[:cntrl:]]/;
exit 1 if !defined $claimed || $claimed !~ /\Aeef831aa-dba4-4af2-a78b-541bc9e0a0[0-9a-f]{2}\z/;
exit 1 if !defined $uid || $uid !~ /\A[0-9]+\z/ || $uid != $>;
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
exit 1 unless ancestors_ok($seal);
exit 1 if -l $seal || !-f $seal;
my @st = lstat($seal);
exit 1 if !@st || -l _ || !-f _;
exit 1 if $st[4] != $> || $st[3] != 1;
exit 1 unless sprintf("%o", $st[2] & 07777) eq "400";
my $chflags = "";
for my $cand (qw(/usr/bin/chflags /sbin/chflags /bin/chflags)) {
  if (-x $cand) { $chflags = $cand; last; }
}
if ($chflags ne "") {
  my $stat = -x "/usr/bin/stat" ? "/usr/bin/stat" : "/bin/stat";
  open(my $sf, "-|", $stat, "-f", "%f", $seal) or exit 1;
  my $flags = <$sf>;
  close $sf or exit 1;
  exit 1 unless defined $flags && $flags =~ /\A[0-9]+\s*\z/;
  exit 1 unless (int($flags) & 2) != 0;
}
sysopen(my $fh, $seal, O_RDONLY | O_NOFOLLOW) or exit 1;
binmode $fh;
local $/;
my $raw = <$fh>;
close $fh;
my @lines = split(/\n/, $raw // "");
exit 1 unless @lines == 3;
exit 1 unless $lines[0] eq "oo-evidence-session-v1";
exit 1 unless $lines[1] eq "session $claimed";
exit 1 unless $lines[2] eq "uid $uid";
exit 0;
PERL
  then
    printf '%s\n' 'unspecified'
    return 0
  fi
  printf '%s\n' "$claimed"
}

oo_validate_uuid() {
  printf '%s' "${1:-}" | grep -Eq '^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
}

oo_require_run_id() {
  if [ -z "${OO_RUN_ID:-}" ]; then
    echo "OO_RUN_ID is required. Pass a fresh lowercase UUID. Evidence session: $(oo_session_id). A session id is not a Docker run id." >&2
    return 2
  fi
  if ! oo_validate_uuid "$OO_RUN_ID"; then
    echo "OO_RUN_ID is not a canonical lowercase UUID" >&2
    return 2
  fi
  return 0
}

oo_run_root() {
  printf '/tmp/oo-env-%s' "$OO_RUN_ID"
}

oo_secret_dir() {
  printf '/tmp/oo-env-%s/secrets' "$OO_RUN_ID"
}

oo_resume_requested() {
  [ "${OO_RESUME:-}" = "1" ]
}

oo_reject_secret_override() {
  local canonical
  canonical=$(oo_secret_dir)
  if [ -n "${OO_SECRET_DIR:-}" ] && [ "$OO_SECRET_DIR" != "$canonical" ]; then
    echo "OO_SECRET_DIR override rejected" >&2
    return 2
  fi
  OO_SECRET_DIR=$canonical
  export OO_SECRET_DIR
  return 0
}

oo_secret_names() {
  printf '%s\n' \
    pg_control_password \
    pg_agent_password \
    replication_password \
    agent_ro_password \
    mysql_root_password \
    mysql_reader_password \
    valkey_buffer_password \
    valkey_state_password
}

oo_stat_field() {
  local kind=$1 file=$2
  if stat --version >/dev/null 2>&1; then
    case "$kind" in
      uid) stat -c '%u' "$file" ;;
      mode) stat -c '%a' "$file" ;;
      nlink) stat -c '%h' "$file" ;;
      *) return 2 ;;
    esac
  else
    case "$kind" in
      uid) stat -f '%u' "$file" ;;
      mode) stat -f '%OLp' "$file" ;;
      nlink) stat -f '%l' "$file" ;;
      *) return 2 ;;
    esac
  fi
}

oo_validate_secret_file() {
  local file=$1
  local uid mode nlink bytes lines
  if [ -L "$file" ] || [ ! -f "$file" ]; then
    echo "secret is not a regular file: $(basename "$file")" >&2
    return 1
  fi
  uid=$(oo_stat_field uid "$file") || return 1
  mode=$(oo_stat_field mode "$file") || return 1
  nlink=$(oo_stat_field nlink "$file") || return 1
  if [ "$uid" != "$(id -u)" ]; then
    echo "secret owner is not the current user: $(basename "$file")" >&2
    return 1
  fi
  if [ "$mode" != "600" ]; then
    echo "secret mode is not 0600: $(basename "$file")" >&2
    return 1
  fi
  if [ "$nlink" != "1" ]; then
    echo "secret has extra links: $(basename "$file")" >&2
    return 1
  fi
  # openssl rand -hex 24 writes 48 lowercase hex digits and one newline.
  # Command substitution would strip that newline, so the check reads the file.
  bytes=$(wc -c < "$file" | tr -d ' ')
  lines=$(wc -l < "$file" | tr -d ' ')
  if [ "$bytes" != "49" ] || [ "$lines" != "1" ]; then
    echo "secret is not 48 lowercase hex: $(basename "$file")" >&2
    return 1
  fi
  if ! grep -Eq '^[0-9a-f]{48}$' "$file"; then
    echo "secret is not 48 lowercase hex: $(basename "$file")" >&2
    return 1
  fi
  return 0
}

oo_hex_secret() {
  local file=$1 value
  oo_validate_secret_file "$file" || return 1
  value=$(tr -d '\n' < "$file")
  if ! printf '%s' "$value" | grep -Eq '^[0-9a-f]{48}$'; then
    return 1
  fi
  printf '%s' "$value"
}

oo_classify_mysql_write_denial() {
  local text=$1
  if printf '%s\n' "$text" | grep -Eq 'ERROR 1045|ERROR 2002|ERROR 2003|Can'\''t connect|Lost connection|Access denied for user'; then
    printf '%s\n' 'class=connection_or_auth'
    return 1
  fi
  if printf '%s\n' "$text" | grep -Eq 'ERROR 1142 \(42000\)' \
    && printf '%s\n' "$text" | grep -q 'INSERT command denied' \
    && printf '%s\n' "$text" | grep -q 'shipment_leg'; then
    printf '%s\n' 'class=mysql_1142_shipment_leg'
    return 0
  fi
  printf '%s\n' 'class=other'
  return 1
}

oo_classify_pg_write_denial() {
  local text=$1
  if printf '%s\n' "$text" | grep -Eq 'could not connect|Connection refused|server closed the connection|password authentication failed|28P01|No such file|connection to server'; then
    printf '%s\n' 'class=connection_or_auth'
    return 1
  fi
  if printf '%s\n' "$text" | grep -q 'permission denied for schema'; then
    printf '%s\n' 'class=schema_denial'
    return 1
  fi
  if printf '%s\n' "$text" | grep -q '42501' \
    && printf '%s\n' "$text" | grep -q 'permission denied for table replay_probe'; then
    printf '%s\n' 'class=pg_42501_replay_probe'
    return 0
  fi
  printf '%s\n' 'class=other'
  return 1
}

oo_prepare_run_root() {
  local root physical got
  oo_require_run_id || return 2
  root=$(oo_run_root)
  if [ -L "$root" ]; then
    echo "run root is a symlink" >&2
    return 2
  fi
  if [ -e "$root" ] && [ ! -d "$root" ]; then
    echo "run root exists and is not a directory" >&2
    return 2
  fi
  if [ ! -d "$root" ]; then
    mkdir -m 700 "$root"
    printf '%s\n' "$OO_RUN_ID" > "$root/run-id"
    chmod 600 "$root/run-id"
    chmod 700 "$root"
  else
    if [ -L "$root/run-id" ] || [ ! -f "$root/run-id" ]; then
      echo "run root marker missing" >&2
      return 2
    fi
    got=$(tr -d '\n' < "$root/run-id")
    if [ "$got" != "$OO_RUN_ID" ]; then
      echo "run root marker does not match OO_RUN_ID" >&2
      return 2
    fi
  fi
  physical=$(realpath "$root")
  case "$physical" in
    "/tmp/oo-env-$OO_RUN_ID"|"/private/tmp/oo-env-$OO_RUN_ID") ;;
    *)
      echo "run root resolved outside the canonical temp path" >&2
      return 2
      ;;
  esac
  return 0
}

oo_known_root_entry() {
  case "$1" in
    run-id|secret-manifest|secrets|snapshot|snapshot.sha256|snapshot.tree|snapshot.owner|infra|infra.seal|logs) return 0 ;;
    *) return 1 ;;
  esac
}

oo_require_regular() {
  local path=$1
  if [ -L "$path" ] || [ ! -e "$path" ]; then
    return 1
  fi
  return 0
}

oo_filename_safe() {
  perl -e 'my $n = $ARGV[0]; exit 1 if $n eq "" || $n eq "." || $n eq ".." || $n =~ /[[:cntrl:]]/ || $n !~ /\A[A-Za-z0-9._-]+\z/; exit 0' -- "$1"
}

oo_mode_has_write() {
  local mode=$1
  [ $((8#$mode & 0222)) -ne 0 ]
}

oo_make_immutable() {
  local file=$1
  if command -v chflags >/dev/null 2>&1; then
    chflags uchg "$file"
  fi
}

oo_clear_immutable() {
  local file=$1
  if command -v chflags >/dev/null 2>&1; then
    chflags nouchg "$file"
  fi
}

oo_is_immutable() {
  local file=$1 flags
  if ! command -v chflags >/dev/null 2>&1; then
    return 0
  fi
  flags=$(stat -f '%f' "$file") || return 1
  [ $((flags & 2)) -ne 0 ]
}

oo_child_count() {
  perl -e '
    opendir(my $dh, $ARGV[0]) or exit 2;
    my $n = 0;
    while (defined(my $e = readdir($dh))) {
      next if $e eq "." || $e eq "..";
      $n++;
    }
    print $n;
  ' -- "$1"
}

oo_assert_run_root() {
  local root physical parent uid mode marker_uid marker_mode target
  oo_require_run_id || return 2
  root=$(oo_run_root)
  if [ -L "$root" ]; then
    echo "run root is a symlink" >&2
    return 2
  fi
  if [ ! -d "$root" ]; then
    echo "run root is missing" >&2
    return 2
  fi
  if [ -L /tmp ]; then
    target=$(readlink /tmp) || return 2
    case "$target" in
      private/tmp|/private/tmp) ;;
      *)
        echo "refusing unexpected /tmp symlink" >&2
        return 2
        ;;
    esac
  fi
  physical=$(realpath "$root") || return 2
  case "$physical" in
    "/tmp/oo-env-$OO_RUN_ID"|"/private/tmp/oo-env-$OO_RUN_ID") ;;
    *)
      echo "run root resolved outside the canonical temp path" >&2
      return 2
      ;;
  esac
  parent=$(dirname "$physical")
  if [ -L "$parent" ]; then
    echo "run root parent is a symlink" >&2
    return 2
  fi
  uid=$(oo_stat_field uid "$root") || return 2
  mode=$(oo_stat_field mode "$root") || return 2
  if [ "$uid" != "$(id -u)" ] || [ "$mode" != "700" ]; then
    echo "run root owner or mode is not 0700 for the current user" >&2
    return 2
  fi
  if [ -L "$root/run-id" ] || [ ! -f "$root/run-id" ]; then
    echo "run root marker missing" >&2
    return 2
  fi
  if [ "$(tr -d '\n' < "$root/run-id")" != "$OO_RUN_ID" ]; then
    echo "run root marker does not match" >&2
    return 2
  fi
  marker_uid=$(oo_stat_field uid "$root/run-id") || return 2
  marker_mode=$(oo_stat_field mode "$root/run-id") || return 2
  if [ "$marker_uid" != "$(id -u)" ] || [ "$marker_mode" != "600" ]; then
    echo "run root marker owner or mode is wrong" >&2
    return 2
  fi
  return 0
}

oo_reject_symlink_under_root() {
  local root=$1 path=$2 cursor
  case "$path" in
    "$root"|"$root"/*) ;;
    *)
      echo "path is outside the run root" >&2
      return 2
      ;;
  esac
  if [ -L "$root" ]; then
    echo "run root is a symlink" >&2
    return 2
  fi
  cursor=$path
  while [ "$cursor" != "$root" ]; do
    if [ -L "$cursor" ]; then
      echo "symlink component refused" >&2
      return 2
    fi
    cursor=$(dirname "$cursor")
  done
  return 0
}

oo_hash_nofollow_file() {
  local path=$1
  perl -MDigest::SHA=sha256_hex -MFcntl -e '
    my $path = $ARGV[0];
    sysopen(my $fh, $path, O_RDONLY | O_NOFOLLOW) or die "open\n";
    binmode $fh;
    local $/;
    my $raw = <$fh>;
    $raw = "" unless defined $raw;
    close $fh;
    my @st = lstat($path) or die "lstat\n";
    die "symlink\n" if -l _;
    die "type\n" unless -f _;
    print sha256_hex($raw);
  ' -- "$path"
}

oo_stage_nofollow() {
  local src=$1 expect=$2 dest=$3
  if ! printf '%s' "$expect" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "refusing stage without a hash" >&2
    return 2
  fi
  case "$dest" in
    /tmp/oo-trusted-"$OO_RUN_ID".*/common.sh|/tmp/oo-trusted-"$OO_RUN_ID".*/entry.sh) ;;
    *)
      echo "refusing stage destination" >&2
      return 2
      ;;
  esac
  perl -MDigest::SHA=sha256_hex -MFcntl - "$src" "$expect" "$dest" << 'PERL'
use strict;
use warnings;
use Digest::SHA qw(sha256_hex);
use Fcntl;
my ($src, $expect, $dest) = @ARGV;
sysopen(my $in, $src, O_RDONLY | O_NOFOLLOW) or die "open\n";
binmode $in;
local $/;
my $raw = <$in>;
$raw = "" unless defined $raw;
close $in;
my $hex = sha256_hex($raw);
die "hash\n" if $hex ne $expect;
sysopen(my $out, $dest, O_WRONLY | O_CREAT | O_EXCL, 0400) or die "create\n";
binmode $out;
my $wrote = syswrite($out, $raw);
die "write\n" if !defined $wrote || $wrote != length($raw);
close $out;
sysopen(my $back, $dest, O_RDONLY | O_NOFOLLOW) or die "reread\n";
binmode $back;
local $/;
my $copy = <$back>;
$copy = "" unless defined $copy;
close $back;
die "copy\n" if sha256_hex($copy) ne $expect;
PERL
}

oo_freeze_perl() {
  perl -MDigest::SHA=sha256_hex -MFcntl - "$@" << 'PERL'
use strict;
use warnings;
use Digest::SHA qw(sha256_hex);
use Fcntl;
my ($mode, $root, $run_id, $uid, $rel, $dest) = @ARGV;
die "args\n" unless defined $mode && defined $root && defined $run_id && defined $uid;
die "run\n" unless $run_id =~ /\A[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\z/;
die "root\n" unless $root eq "/tmp/oo-env-$run_id";
die "uid\n" unless $uid =~ /\A[0-9]+\z/ && $uid == $>;
my $infra = "$root/infra";
my $seal = "$root/infra.seal";
my @required = qw(
  .run-id
  lib/common.sh
  compose/cleanup.sh
  compose/up.sh
  compose/smoke.sh
  compose/dns-check.sh
  compose/redact-log.sh
  compose/fault.sh
  compose/configure-proxies.sh
  compose/compose.yaml
  images.pins
  images.lock.json
);

sub slurp_nofollow {
  my ($path) = @_;
  my @st = lstat($path) or die "lstat\n";
  die "symlink\n" if -l _;
  die "type\n" unless -f _;
  die "owner\n" unless $st[4] == $>;
  die "writable\n" if $st[2] & 0222;
  die "link\n" unless $st[3] == 1;
  sysopen(my $fh, $path, O_RDONLY | O_NOFOLLOW) or die "open\n";
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  $raw = "" unless defined $raw;
  close $fh;
  my $mode_s = sprintf("%o", $st[2] & 07777);
  return ($mode_s, sha256_hex($raw));
}

sub walk_tree {
  my ($dir, $prefix, $out) = @_;
  opendir(my $dh, $dir) or die "opendir\n";
  my @names = readdir($dh);
  closedir $dh;
  foreach my $name (@names) {
    next if $name eq "." || $name eq "..";
    die "unsafe\n" if $name eq "" || $name =~ /[[:cntrl:]]/ || $name !~ /\A[A-Za-z0-9._-]+\z/;
    my $path = "$dir/$name";
    my $child = $prefix eq "" ? $name : "$prefix/$name";
    my @st = lstat($path) or die "lstat\n";
    die "symlink\n" if -l _;
    die "owner\n" unless $st[4] == $>;
    die "writable\n" if $st[2] & 0222;
    my $mode_s = sprintf("%o", $st[2] & 07777);
    if (-d _) {
      $out->{$child} = join(" ", "d", $child, $mode_s, $st[4]);
      walk_tree($path, $child, $out);
    } elsif (-f _) {
      die "link\n" unless $st[3] == 1;
      sysopen(my $fh, $path, O_RDONLY | O_NOFOLLOW) or die "open\n";
      binmode $fh;
      local $/;
      my $raw = <$fh>;
      $raw = "" unless defined $raw;
      close $fh;
      $out->{$child} = join(" ", "f", $child, $mode_s, $st[4], $st[3], sha256_hex($raw));
    } else {
      die "type\n";
    }
  }
}

sub read_seal {
  my @st = lstat($seal) or die "seal-missing\n";
  die "symlink\n" if -l _;
  die "type\n" unless -f _;
  die "owner\n" unless $st[4] == $>;
  die "writable\n" if $st[2] & 0222;
  die "link\n" unless $st[3] == 1;
  die "mode\n" unless sprintf("%o", $st[2] & 07777) eq "400";
  sysopen(my $fh, $seal, O_RDONLY | O_NOFOLLOW) or die "open\n";
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  close $fh;
  my @lines = split(/\n/, $raw // "");
  die "seal\n" unless @lines >= 4;
  die "seal\n" unless $lines[0] eq "oo-infra-seal-v1";
  die "seal\n" unless $lines[1] eq "run_id $run_id";
  die "seal\n" unless $lines[2] eq "uid $uid";
  die "seal\n" unless $lines[3] =~ /\Aentries ([0-9]+)\z/;
  my $count = $1;
  my %want;
  for my $line (@lines[4 .. $#lines]) {
    die "seal\n" if $line !~ /\A(?:d|f) /;
    my @f = split(/ /, $line);
    my $path;
    if ($f[0] eq "d") {
      die "seal\n" unless @f == 4;
      $path = $f[1];
    } else {
      die "seal\n" unless @f == 6 && $f[5] =~ /\A[0-9a-f]{64}\z/;
      $path = $f[1];
    }
    die "seal\n" if $path !~ /\A[A-Za-z0-9._-]+(?:\/[A-Za-z0-9._-]+)*\z/;
    die "seal\n" if exists $want{$path};
    $want{$path} = $line;
  }
  die "seal\n" unless scalar(keys %want) == $count;
  return \%want;
}

sub compare_tree {
  my ($want) = @_;
  my %got;
  walk_tree($infra, "", \%got);
  foreach my $path (sort keys %$want) {
    die "missing $path\n" unless exists $got{$path};
    die "mismatch $path\n" unless $want->{$path} eq $got{$path};
  }
  foreach my $path (sort keys %got) {
    die "unexpected $path\n" unless exists $want->{$path};
  }
  foreach my $path (@required) {
    die "missing $path\n" unless exists $want->{$path} && $want->{$path} =~ /\Af /;
  }
  my $marker = "$infra/.run-id";
  sysopen(my $fh, $marker, O_RDONLY | O_NOFOLLOW) or die "marker\n";
  binmode $fh;
  local $/;
  my $text = <$fh>;
  close $fh;
  $text =~ s/\n\z//;
  die "marker\n" unless $text eq $run_id;
}

if ($mode eq "seal") {
  die "exists\n" if -e $seal || -l $seal;
  my @st = lstat($infra) or die "infra\n";
  die "symlink\n" if -l _;
  die "type\n" unless -d _;
  my %got;
  walk_tree($infra, "", \%got);
  foreach my $path (@required) {
    die "missing $path\n" unless exists $got{$path} && $got{$path} =~ /\Af /;
  }
  my $tmp = "$root/.seal-writing";
  die "exists\n" if -e $tmp || -l $tmp;
  sysopen(my $out, $tmp, O_WRONLY | O_CREAT | O_EXCL, 0400) or die "create\n";
  binmode $out;
  my $body = "oo-infra-seal-v1\nrun_id $run_id\nuid $uid\nentries " . scalar(keys %got) . "\n";
  foreach my $path (sort keys %got) {
    $body .= $got{$path} . "\n";
  }
  my $wrote = syswrite($out, $body);
  die "write\n" if !defined $wrote || $wrote != length($body);
  close $out;
  rename($tmp, $seal) or die "rename\n";
  exit 0;
}

if ($mode eq "verify") {
  my $want = read_seal();
  compare_tree($want);
  exit 0;
}

if ($mode eq "stage") {
  die "rel\n" unless defined $rel && (
    $rel eq "lib/common.sh" || $rel =~ /\Acompose\/(?:cleanup|up|smoke)\.sh\z/
  );
  die "dest\n" unless defined $dest && $dest =~ /\A\/tmp\/oo-trusted-\Q$run_id\E\.[A-Za-z0-9]+\/(?:common|entry)\.sh\z/;
  my $want = read_seal();
  compare_tree($want);
  die "missing $rel\n" unless exists $want->{$rel} && $want->{$rel} =~ /\Af /;
  my (undef, undef, undef, undef, undef, $expect) = split(/ /, $want->{$rel});
  my ($mode_s, $hex) = slurp_nofollow("$infra/$rel");
  die "mismatch $rel\n" unless $hex eq $expect;
  die "mode\n" unless $want->{$rel} =~ /\Af \Q$rel\E \Q$mode_s\E /;
  sysopen(my $out, $dest, O_WRONLY | O_CREAT | O_EXCL, 0400) or die "create\n";
  sysopen(my $in, "$infra/$rel", O_RDONLY | O_NOFOLLOW) or die "open\n";
  binmode $in;
  binmode $out;
  local $/;
  my $raw = <$in>;
  $raw = "" unless defined $raw;
  close $in;
  die "hash\n" unless sha256_hex($raw) eq $expect;
  my $wrote = syswrite($out, $raw);
  die "write\n" if !defined $wrote || $wrote != length($raw);
  close $out;
  exit 0;
}

die "mode\n";
PERL
}

oo_verify_frozen_infra() {
  local root
  oo_assert_run_root || return 2
  root=$(oo_run_root)
  oo_reject_symlink_under_root "$root" "$root/infra" || return 2
  oo_reject_symlink_under_root "$root" "$root/infra.seal" || return 2
  if [ ! -f "$root/infra.seal" ] || [ -L "$root/infra.seal" ]; then
    echo "infra seal is missing" >&2
    return 2
  fi
  if ! oo_is_immutable "$root/infra.seal"; then
    echo "infra seal is not immutable" >&2
    return 2
  fi
  if ! oo_freeze_perl verify "$root" "$OO_RUN_ID" "$(id -u)"; then
    echo "infra freeze does not match the seal" >&2
    return 2
  fi
  return 0
}

oo_seal_infra_freeze() {
  local root
  oo_assert_run_root || return 2
  root=$(oo_run_root)
  if [ -e "$root/infra.seal" ] || [ -L "$root/infra.seal" ]; then
    echo "infra seal already exists" >&2
    return 2
  fi
  if [ -L "$root/infra" ] || [ ! -d "$root/infra" ]; then
    echo "infra freeze is not a real directory" >&2
    return 2
  fi
  if [ -L "$root/infra/.run-id" ] || [ ! -f "$root/infra/.run-id" ]; then
    echo "infra freeze marker missing" >&2
    return 2
  fi
  chmod -R a-w "$root/infra" || return 2
  if ! oo_freeze_perl seal "$root" "$OO_RUN_ID" "$(id -u)"; then
    oo_clear_immutable "$root/infra.seal" 2>/dev/null || true
    rm -f "$root/infra.seal" "$root/.seal-writing"
    echo "infra seal failed" >&2
    return 2
  fi
  chmod 400 "$root/infra.seal" || return 2
  if ! oo_make_immutable "$root/infra.seal"; then
    echo "infra seal could not be made immutable" >&2
    return 2
  fi
  oo_verify_frozen_infra || return 2
  return 0
}

oo_drop_trusted_common() {
  local dir
  [ -n "${OO_TRUSTED_COMMON:-}" ] || return 0
  dir=$(dirname "$OO_TRUSTED_COMMON")
  case "$dir" in
    /tmp/oo-trusted-"$OO_RUN_ID".*)
      rm -f "$dir/common.sh" "$dir/entry.sh"
      rmdir "$dir" 2>/dev/null || true
      ;;
  esac
  return 0
}

oo_exec_frozen_script() {
  local root=$1 rel=$2 priv
  shift 2
  case "$rel" in
    compose/cleanup.sh|compose/up.sh|compose/smoke.sh) ;;
    *)
      echo "refusing frozen entry" >&2
      return 2
      ;;
  esac
  oo_verify_frozen_infra || return 2
  root=$(oo_run_root)
  priv=$(mktemp -d "/tmp/oo-trusted-${OO_RUN_ID}.XXXXXX") || return 2
  chmod 700 "$priv" || return 2
  if ! oo_freeze_perl stage "$root" "$OO_RUN_ID" "$(id -u)" "lib/common.sh" "$priv/common.sh"; then
    rm -f "$priv/common.sh" "$priv/entry.sh"
    rmdir "$priv" 2>/dev/null || true
    echo "refusing to stage frozen common.sh" >&2
    return 2
  fi
  if ! oo_freeze_perl stage "$root" "$OO_RUN_ID" "$(id -u)" "$rel" "$priv/entry.sh"; then
    rm -f "$priv/common.sh" "$priv/entry.sh"
    rmdir "$priv" 2>/dev/null || true
    echo "refusing to stage frozen script" >&2
    return 2
  fi
  chmod 400 "$priv/common.sh" "$priv/entry.sh" || {
    rm -f "$priv/common.sh" "$priv/entry.sh"
    rmdir "$priv" 2>/dev/null || true
    return 2
  }
  export OO_TRUSTED_COMMON=$priv/common.sh
  export OO_FROZEN_INFRA=$root/infra
  export OO_FIXTURE_FROZEN=1
  export OO_TRUSTED_COMMON_OWNER=1
  exec bash "$priv/entry.sh" "$@"
}

oo_bash_frozen_sibling() {
  local name=$1
  shift
  case "$name" in
    dns-check.sh|fault.sh|redact-log.sh|configure-proxies.sh|debezium/pending.sh) ;;
    *)
      echo "refusing frozen sibling" >&2
      return 2
      ;;
  esac
  if [ "${OO_FIXTURE_FROZEN:-}" = 1 ]; then
    oo_verify_frozen_infra || return 2
    if [ -L "$here/$name" ] || [ ! -f "$here/$name" ]; then
      echo "frozen sibling is not a regular file" >&2
      return 2
    fi
  fi
  bash "$here/$name" "$@"
}

oo_verify_snapshot_members() {
  local snapshot=$1 root manifest rel expect got path mode
  oo_assert_run_root || return 2
  root=$(oo_run_root)
  case "$snapshot" in
    "$root"/snapshot) ;;
    *)
      echo "snapshot must be the run's frozen snapshot" >&2
      return 2
      ;;
  esac
  oo_reject_symlink_under_root "$root" "$snapshot" || return 2
  manifest=$root/snapshot.sha256
  oo_reject_symlink_under_root "$root" "$manifest" || return 2
  oo_reject_symlink_under_root "$root" "$root/snapshot.tree" || return 2
  oo_reject_symlink_under_root "$root" "$root/snapshot.owner" || return 2
  if [ -L "$root/snapshot.owner" ] || [ ! -f "$root/snapshot.owner" ]; then
    echo "snapshot owner marker missing" >&2
    return 2
  fi
  if [ "$(tr -d '\n' < "$root/snapshot.owner")" != "$OO_RUN_ID" ]; then
    echo "snapshot owner marker does not match" >&2
    return 2
  fi
  for rel in \
    testing/infra/builders/cargo-locked.sh \
    testing/infra/builders/container-cargo.sh \
    testing/infra/lib/common.sh \
    testing/infra/images.pins \
    testing/infra/images.lock.json
  do
    path=$snapshot/$rel
    oo_reject_symlink_under_root "$root" "$path" || return 2
    if [ -L "$path" ] || [ ! -f "$path" ]; then
      echo "snapshot member missing: $rel" >&2
      return 2
    fi
    expect=$(awk -v p="$rel" '$2 == p { print $1; c++ } END { if (c != 1) exit 1 }' "$manifest") || {
      echo "snapshot manifest missing $rel" >&2
      return 2
    }
    got=$(oo_hash_nofollow_file "$path") || {
      echo "snapshot member could not be hashed: $rel" >&2
      return 2
    }
    if [ "$got" != "$expect" ]; then
      echo "snapshot member hash mismatch: $rel" >&2
      return 2
    fi
    mode=$(oo_stat_field mode "$path") || return 2
    if oo_mode_has_write "$mode"; then
      echo "snapshot member is writable: $rel" >&2
      return 2
    fi
  done
  mode=$(oo_stat_field mode "$manifest") || return 2
  if oo_mode_has_write "$mode"; then
    echo "snapshot manifest is writable" >&2
    return 2
  fi
  mode=$(oo_stat_field mode "$root/snapshot.tree") || return 2
  if oo_mode_has_write "$mode"; then
    echo "snapshot tree hash is writable" >&2
    return 2
  fi
  return 0
}

oo_exec_snapshot_wrapper() {
  local snapshot=$3 root priv common_hash script_hash
  if [ $# -ne 4 ]; then
    echo "usage: OO_RUN_ID=<uuid> cargo-locked.sh <1.98.1|1.94.0> <linux/arm64|linux/amd64> <snapshot-dir> <log-dir>" >&2
    return 2
  fi
  oo_verify_snapshot_members "$snapshot" || return 2
  root=$(oo_run_root)
  priv=$(mktemp -d "/tmp/oo-trusted-${OO_RUN_ID}.XXXXXX") || return 2
  chmod 700 "$priv" || return 2
  common_hash=$(awk -v p="testing/infra/lib/common.sh" '$2 == p { print $1 }' "$root/snapshot.sha256")
  script_hash=$(awk -v p="testing/infra/builders/cargo-locked.sh" '$2 == p { print $1 }' "$root/snapshot.sha256")
  if ! oo_stage_nofollow "$snapshot/testing/infra/lib/common.sh" "$common_hash" "$priv/common.sh"; then
    rm -f "$priv/common.sh" "$priv/entry.sh"
    rmdir "$priv" 2>/dev/null || true
    echo "refusing to stage snapshot common.sh" >&2
    return 2
  fi
  if ! oo_stage_nofollow "$snapshot/testing/infra/builders/cargo-locked.sh" "$script_hash" "$priv/entry.sh"; then
    rm -f "$priv/common.sh" "$priv/entry.sh"
    rmdir "$priv" 2>/dev/null || true
    echo "refusing to stage snapshot cargo-locked.sh" >&2
    return 2
  fi
  chmod 400 "$priv/common.sh" "$priv/entry.sh" || {
    rm -f "$priv/common.sh" "$priv/entry.sh"
    rmdir "$priv" 2>/dev/null || true
    return 2
  }
  export OO_TRUSTED_COMMON=$priv/common.sh
  export OO_WRAPPER_FROM_SNAPSHOT=1
  export OO_TRUSTED_COMMON_OWNER=1
  exec bash "$priv/entry.sh" "$@"
}

oo_secret_is_required() {
  local name=$1 required
  while IFS= read -r required; do
    if [ "$name" = "$required" ]; then
      return 0
    fi
  done < <(oo_secret_names)
  return 1
}

oo_audit_secret_dir() {
  local dir=$1 listed counted file name
  if [ -L "$dir" ] || [ ! -d "$dir" ]; then
    echo "secrets path is not a real directory" >&2
    return 3
  fi
  listed=0
  counted=$(oo_child_count "$dir") || return 3
  while IFS= read -r -d '' file; do
    listed=$((listed + 1))
    name=${file##*/}
    if ! oo_filename_safe "$name"; then
      echo "unsafe secret filename rejected" >&2
      return 3
    fi
    if ! oo_secret_is_required "$name"; then
      echo "unexpected secret entry: $name" >&2
      return 3
    fi
    if [ -L "$file" ] || [ ! -f "$file" ]; then
      echo "secret is not a regular file: $name" >&2
      return 3
    fi
    oo_validate_secret_file "$file" || return 3
  done < <(find -P "$dir" -mindepth 1 -maxdepth 1 -print0)
  if [ "$listed" -ne "$counted" ]; then
    echo "secret enumeration is incomplete" >&2
    return 3
  fi
  while IFS= read -r name; do
    if [ -L "$dir/$name" ] || [ ! -f "$dir/$name" ]; then
      echo "required secret missing: $name" >&2
      return 3
    fi
  done < <(oo_secret_names)
  if [ "$listed" -ne 8 ]; then
    echo "secret set is not exactly the required eight" >&2
    return 3
  fi
  return 0
}

oo_audit_log_tree() {
  local logs=$1
  if [ -L "$logs" ] || [ ! -d "$logs" ]; then
    echo "logs path is not a real directory" >&2
    return 3
  fi
  perl -e '
    use strict;
    use warnings;
    my $logs = $ARGV[0];
    die "logs path is not a real directory\n" unless defined $logs && -d $logs && !-l $logs;
    sub walk {
      my ($dir) = @_;
      opendir(my $dh, $dir) or die "unsafe log entry rejected\n";
      my @names = readdir($dh);
      closedir $dh;
      foreach my $name (@names) {
        next if $name eq "." || $name eq "..";
        if ($name eq "" || $name =~ /[[:cntrl:]]/ || $name !~ /\A[A-Za-z0-9._-]+\z/) {
          die "unsafe log entry rejected\n";
        }
        my $path = "$dir/$name";
        my @st = lstat($path) or die "unsafe log entry rejected\n";
        if (-l _) {
          die "refusing symlink under logs\n";
        }
        if (-d _) {
          walk($path);
        } elsif (-f _) {
          die "unsafe log entry rejected\n" unless $st[3] == 1;
        } else {
          die "unsafe log entry rejected\n";
        }
      }
    }
    walk($logs);
  ' -- "$logs" || return 3
  return 0
}

oo_remove_owned_logs() {
  local root=$1 logs logical phys file dir parent parent_phys base
  logs=$root/logs
  if [ -L "$logs" ] || [ ! -d "$logs" ]; then
    echo "logs path is not a real directory" >&2
    return 3
  fi
  oo_audit_log_tree "$logs" || return 3
  logical=$logs
  phys=$(realpath "$logs") || return 3
  case "$phys" in
    "/tmp/oo-env-$OO_RUN_ID/logs"|"/private/tmp/oo-env-$OO_RUN_ID/logs") ;;
    *)
      echo "unsafe log entry rejected" >&2
      return 3
      ;;
  esac
  while IFS= read -r -d '' file; do
    base=${file##*/}
    if ! oo_filename_safe "$base"; then
      echo "unsafe log entry rejected" >&2
      return 3
    fi
    case "$file" in
      "$logical"/*|"$phys"/*) ;;
      *)
        echo "unsafe log entry rejected" >&2
        return 3
        ;;
    esac
    if [ -L "$file" ] || [ ! -f "$file" ]; then
      echo "unsafe log entry rejected" >&2
      return 3
    fi
    parent=${file%/*}
    parent_phys=$(realpath -- "$parent") || return 3
    case "$parent_phys" in
      "$phys"|"$phys"/*) ;;
      *)
        echo "unsafe log entry rejected" >&2
        return 3
        ;;
    esac
    rm -f -- "$file" || return 3
  done < <(find -P "$logical" -type f -print0)
  while IFS= read -r -d '' dir; do
    case "$dir" in
      "$logical"|"$phys") ;;
      "$logical"/*|"$phys"/*)
        base=${dir##*/}
        if ! oo_filename_safe "$base"; then
          echo "unsafe log entry rejected" >&2
          return 3
        fi
        ;;
      *)
        echo "unsafe log entry rejected" >&2
        return 3
        ;;
    esac
    if [ -L "$dir" ] || [ ! -d "$dir" ]; then
      echo "unsafe log entry rejected" >&2
      return 3
    fi
    parent_phys=$(realpath -- "$dir") || return 3
    case "$parent_phys" in
      "$phys"|"$phys"/*) ;;
      *)
        echo "unsafe log entry rejected" >&2
        return 3
        ;;
    esac
    rmdir -- "$dir" || return 3
  done < <(find -P "$logical" -depth -type d -print0)
  return 0
}

oo_audit_run_root() {
  local root=$1 secret listed counted file name
  secret=$(oo_secret_dir)
  listed=0
  counted=$(oo_child_count "$root") || return 3
  while IFS= read -r -d '' file; do
    listed=$((listed + 1))
    name=${file##*/}
    if ! oo_filename_safe "$name"; then
      echo "unsafe filename rejected" >&2
      return 3
    fi
    if ! oo_known_root_entry "$name"; then
      echo "unexpected run root entry: $name" >&2
      return 3
    fi
    if [ -L "$file" ]; then
      echo "unexpected symlink: $name" >&2
      return 3
    fi
  done < <(find -P "$root" -mindepth 1 -maxdepth 1 -print0)
  if [ "$listed" -ne "$counted" ]; then
    echo "run root enumeration is incomplete" >&2
    return 3
  fi
  if [ -d "$secret" ]; then
    if [ -L "$secret" ]; then
      echo "secrets path is a symlink" >&2
      return 3
    fi
    if [ -L "$root/secret-manifest" ] || [ ! -f "$root/secret-manifest" ]; then
      echo "secret manifest missing" >&2
      return 3
    fi
    if ! cmp -s <(oo_secret_names) "$root/secret-manifest"; then
      echo "secret manifest does not match the required set" >&2
      return 3
    fi
    oo_audit_secret_dir "$secret" || return 3
  fi
  if [ -e "$root/logs" ] || [ -L "$root/logs" ]; then
    if [ -L "$root/logs" ] || [ ! -d "$root/logs" ]; then
      echo "logs path is not a real directory" >&2
      return 3
    fi
    oo_audit_log_tree "$root/logs" || return 3
  fi
  if [ -e "$root/snapshot" ] || [ -L "$root/snapshot" ]; then
    if [ -L "$root/snapshot" ] || [ ! -d "$root/snapshot" ]; then
      echo "snapshot path is not a real directory" >&2
      return 3
    fi
    if [ -L "$root/snapshot.owner" ] || [ ! -f "$root/snapshot.owner" ]; then
      echo "snapshot owner marker missing" >&2
      return 3
    fi
    if [ "$(tr -d '\n' < "$root/snapshot.owner")" != "$OO_RUN_ID" ]; then
      echo "snapshot owner marker mismatch" >&2
      return 3
    fi
  fi
  if [ -e "$root/infra" ] || [ -L "$root/infra" ] || [ -e "$root/infra.seal" ] || [ -L "$root/infra.seal" ]; then
    if [ -L "$root/infra" ] || [ ! -d "$root/infra" ]; then
      echo "infra freeze is not a real directory" >&2
      return 3
    fi
    oo_verify_frozen_infra || return 2
  fi
  return 0
}

oo_cleanup_run() {
  local root secret name file physical id kind triple project owner run scope
  local tmp ids
  oo_require_run_id || return 2
  oo_reject_secret_override || return 2
  root=$(oo_run_root)
  secret=$(oo_secret_dir)

  if [ -L "$root" ]; then
    echo "refusing symlink run root" >&2
    return 2
  fi
  if [ -e "$root" ]; then
    if [ ! -d "$root" ]; then
      echo "run root is not a directory" >&2
      return 2
    fi
    physical=$(realpath "$root")
    case "$physical" in
      "/tmp/oo-env-$OO_RUN_ID"|"/private/tmp/oo-env-$OO_RUN_ID") ;;
      *)
        echo "refusing run root that resolves outside canonical temp" >&2
        return 2
        ;;
    esac
    if [ -L "$root/run-id" ] || [ ! -f "$root/run-id" ]; then
      echo "refusing run root without a regular marker" >&2
      return 2
    fi
    if [ "$(tr -d '\n' < "$root/run-id")" != "$OO_RUN_ID" ]; then
      echo "refusing run root whose marker does not match" >&2
      return 2
    fi
    oo_audit_run_root "$root" || return
  fi

  tmp=$(mktemp -d "${TMPDIR:-/tmp}/oo-cleanup.XXXXXX")
  # shellcheck disable=SC2064
  trap "rm -rf '$tmp'" RETURN
  for kind in container network volume; do
    : > "$tmp/$kind.ids"
  done
  docker ps -aq --filter "label=oo.run_id=$OO_RUN_ID" > "$tmp/container.ids"
  docker network ls -q --filter "label=oo.run_id=$OO_RUN_ID" > "$tmp/network.ids"
  docker volume ls -q --filter "label=oo.run_id=$OO_RUN_ID" > "$tmp/volume.ids"
  for kind in container network volume; do
    while IFS= read -r id; do
      [ -n "$id" ] || continue
      if [ "$kind" = "container" ]; then
        triple=$(docker container inspect --format '{{index .Config.Labels "oo.project"}} {{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.run_id"}} {{index .Config.Labels "oo.scope"}}' "$id")
      elif [ "$kind" = "network" ]; then
        triple=$(docker network inspect --format '{{index .Labels "oo.project"}} {{index .Labels "oo.owner"}} {{index .Labels "oo.run_id"}} {{index .Labels "oo.scope"}}' "$id")
      else
        triple=$(docker volume inspect --format '{{index .Labels "oo.project"}} {{index .Labels "oo.owner"}} {{index .Labels "oo.run_id"}} {{index .Labels "oo.scope"}}' "$id")
      fi
      project=${triple%% *}
      owner=$(printf '%s' "$triple" | awk '{print $2}')
      run=$(printf '%s' "$triple" | awk '{print $3}')
      scope=$(printf '%s' "$triple" | awk '{print $4}')
      if [ "$project" != "open-ontology" ] || [ "$owner" != "env" ] || [ "$run" != "$OO_RUN_ID" ]; then
        echo "refusing $kind $id: label verification failed" >&2
        return 4
      fi
      case "$scope" in
        sp00-infra-fixture|sp00-infra-builder) ;;
        *)
          echo "refusing $kind $id: unexpected scope" >&2
          return 4
          ;;
      esac
      printf '%s\n' "$id" >> "$tmp/$kind.verified"
    done < "$tmp/$kind.ids"
  done

  if [ -s "$tmp/container.verified" ]; then
    while IFS= read -r id; do
      [ -n "$id" ] || continue
      docker rm -f -v "$id" >/dev/null
    done < "$tmp/container.verified"
  else
    echo "containers=none"
  fi
  if [ -s "$tmp/network.verified" ]; then
    while IFS= read -r id; do
      [ -n "$id" ] || continue
      docker network rm "$id" >/dev/null
    done < "$tmp/network.verified"
  else
    echo "networks=none"
  fi
  if [ -s "$tmp/volume.verified" ]; then
    while IFS= read -r id; do
      [ -n "$id" ] || continue
      docker volume rm "$id" >/dev/null
    done < "$tmp/volume.verified"
  else
    echo "volumes=none"
  fi

  if [ -e "$root" ]; then
    oo_audit_run_root "$root" || return
  fi
  if [ -d "$secret" ]; then
    oo_audit_secret_dir "$secret" || return 3
    for name in $(oo_secret_names); do
      file="$secret/$name"
      oo_validate_secret_file "$file" || return 3
      dd if=/dev/zero of="$file" bs=128 count=1 conv=notrunc >/dev/null 2>&1
      rm -f "$file"
    done
    rmdir "$secret"
    rm -f "$root/secret-manifest"
    echo "secrets_removed=yes"
  else
    echo "secrets_removed=absent"
  fi
  if [ -d "$root/logs" ]; then
    oo_remove_owned_logs "$root" || return
  fi
  if [ -d "$root/snapshot" ]; then
    chmod -R u+w "$root/snapshot"
    rm -rf "$root/snapshot"
    rm -f "$root/snapshot.sha256" "$root/snapshot.tree" "$root/snapshot.owner"
    echo "snapshot_removed=yes"
  fi
  if [ -d "$root/infra" ] && [ ! -L "$root/infra" ]; then
    if command -v chflags >/dev/null 2>&1; then
      chflags -R nouchg "$root/infra" || return 3
    fi
    chmod -R u+w "$root/infra"
    rm -rf "$root/infra"
    echo "infra_freeze_removed=yes"
  fi
  if [ -e "$root/infra.seal" ] || [ -L "$root/infra.seal" ]; then
    if [ -L "$root/infra.seal" ] || [ ! -f "$root/infra.seal" ]; then
      echo "infra seal is not a regular file" >&2
      return 3
    fi
    oo_clear_immutable "$root/infra.seal" || return 3
    rm -f "$root/infra.seal"
    echo "infra_seal_removed=yes"
  fi
  if [ -d "$root" ]; then
    rm -f "$root/run-id"
    if ! rmdir "$root"; then
      echo "run root not empty after owned cleanup" >&2
      return 3
    fi
    echo "run_root_removed=yes"
  else
    echo "run_root_removed=absent"
  fi
  echo "remaining_run_containers=$(docker ps -aq --filter "label=oo.run_id=$OO_RUN_ID" | wc -l | tr -d ' ')"
  echo "remaining_run_networks=$(docker network ls -q --filter "label=oo.run_id=$OO_RUN_ID" | wc -l | tr -d ' ')"
  echo "remaining_run_volumes=$(docker volume ls -q --filter "label=oo.run_id=$OO_RUN_ID" | wc -l | tr -d ' ')"
  return 0
}
