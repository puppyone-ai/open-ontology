#!/bin/bash
# Create the run's 0600 secrets and start the labelled fixture.
# A second start of the same UUID requires OO_RESUME=1.
set -euo pipefail

# Classify the invocation and the library before either one is read.
case "$0" in
  *[[:cntrl:]]*)
    echo "refusing control character in invocation path" >&2
    exit 2
    ;;
esac
OO_BOOT_REL='compose/up.sh'
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

if [ "${OO_FIXTURE_FROZEN:-}" != 1 ]; then
  if [ -e "$root" ] || [ -L "$root" ]; then
    if ! oo_resume_requested; then
      echo "run root exists; pass OO_RESUME=1 to continue this run" >&2
      exit 2
    fi
    if [ -L "$root" ] || [ ! -d "$root" ]; then
      echo "run root is not a real directory" >&2
      exit 2
    fi
    if [ -L "$root/infra" ] || [ ! -d "$root/infra" ]; then
      echo "resume requested but the frozen compose tree is missing" >&2
      exit 2
    fi
    oo_exec_frozen_script "$root" compose/up.sh "$@"
    exit 2
  fi
  oo_prepare_run_root || exit 2
  mkdir -p "$root/infra"
  cp -R "$here/../." "$root/infra/"
  printf '%s\n' "$OO_RUN_ID" > "$root/infra/.run-id"
  chmod 600 "$root/infra/.run-id"
  if ! oo_seal_infra_freeze; then
    oo_clear_immutable "$root/infra.seal" 2>/dev/null || true
    if command -v chflags >/dev/null 2>&1; then
      chflags -R nouchg "$root/infra" 2>/dev/null || true
    fi
    chmod -R u+w "$root/infra" 2>/dev/null || true
    rm -rf "$root/infra"
    rm -f "$root/infra.seal" "$root/.seal-writing"
    echo "infra seal failed; frozen tree removed" >&2
    exit 2
  fi
  oo_exec_frozen_script "$root" compose/up.sh "$@"
  exit 2
fi

oo_verify_frozen_infra || exit 2

echo "evidence_session=$(oo_session_id)"
echo "run_id=$OO_RUN_ID"
secret=$(oo_secret_dir)
if [ -L "$secret" ]; then
  echo "secret dir is a symlink" >&2
  exit 2
fi
mkdir -p "$secret"
chmod 700 "$secret"
chmod 700 "$root"
present=0
missing=0
while IFS= read -r name; do
  if [ -e "$secret/$name" ] || [ -L "$secret/$name" ]; then
    present=$((present + 1))
  else
    missing=$((missing + 1))
  fi
done < <(oo_secret_names)
if [ "$present" -gt 0 ] && [ "$missing" -gt 0 ]; then
  echo "partial secret set; refusing to mix old and new passwords" >&2
  exit 2
fi
if [ "$present" -eq 8 ]; then
  while IFS= read -r name; do
    oo_validate_secret_file "$secret/$name" || exit 2
  done < <(oo_secret_names)
  echo "secrets=reused"
else
  umask 077
  while IFS= read -r name; do
    tmp="$secret/.${name}.tmp"
    openssl rand -hex 24 > "$tmp"
    chmod 600 "$tmp"
    oo_validate_secret_file "$tmp" || exit 1
    mv "$tmp" "$secret/$name"
  done < <(oo_secret_names)
  echo "secrets=generated"
fi
oo_secret_names > "$root/secret-manifest"
chmod 600 "$root/secret-manifest"
echo "secret_dir=$secret"
echo "secret_modes:"
ls -l "$secret" | awk 'NR>1 { print $1, $NF }'

mkdir -p "$root/logs"
chmod 700 "$root/logs"
docker compose -f "$here/compose.yaml" --project-directory "$here" config > "$root/logs/compose-config.txt"
hits=0
while IFS= read -r name; do
  count=$(grep -F -c -f "$secret/$name" "$root/logs/compose-config.txt" || true)
  hits=$((hits + count))
done < <(oo_secret_names)
echo "compose_config_secret_hits=$hits"
if [ "$hits" -ne 0 ]; then
  echo "compose config contains a generated secret" >&2
  exit 1
fi
grep -q "$OO_RUN_ID" "$root/logs/compose-config.txt"
if grep -q 'docker.sock\|/home/\|network_mode: host\|privileged: true' "$root/logs/compose-config.txt"; then
  echo "compose config has a forbidden mount or privilege" >&2
  exit 1
fi
docker compose -f "$here/compose.yaml" --project-directory "$here" up -d --wait --wait-timeout 480
oo_bash_frozen_sibling configure-proxies.sh
echo "fixture_up=yes"
