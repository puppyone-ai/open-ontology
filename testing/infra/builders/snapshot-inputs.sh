#!/bin/bash
# Copy an allowlisted build input into an immutable snapshot and hash it.
# Two stable copies must match the source. A mismatch is not promoted.
# Trust anchor: the shipped fingerprint literals in this file, plus the
# readonly seal written by the parent before exec. Environment variables are
# not provenance. A rewritten copy of this script is outside this check.
set -euo pipefail

oo_boot_drop_stage() {
  local p base
  for p in "${oo_boot_stage_script:-}" "${oo_boot_stage_seal:-}"; do
    [ -n "$p" ] || continue
    base=${p##*/}
    case "$base" in
      oo-snapshot-script.[A-Za-z0-9]*|oo-snapshot-script.[A-Za-z0-9]*.seal) ;;
      *) continue ;;
    esac
    case "$p" in
      /tmp/"$base"|/private/tmp/"$base") ;;
      *) continue ;;
    esac
    if [ -L "$p" ] || [ -d "$p" ]; then
      continue
    fi
    if command -v chflags >/dev/null 2>&1; then
      chflags nouchg "$p" 2>/dev/null || true
    fi
    rm -f "$p" 2>/dev/null || true
  done
  return 0
}

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

if printf '%s\n' "$oo_boot_phys" | grep -Eq '^/(private/)?tmp/oo-snapshot-script\.[A-Za-z0-9]+$'; then
  if [ "${OO_SNAPSHOT_SCRIPT_FROZEN:-}" != 1 ]; then
    echo "refusing untrusted common path" >&2
    exit 2
  fi
  perl -e 'my @s = lstat($ARGV[0]); exit 2 if !@s || -l _ || !-f _; exit 3 if $s[3] != 1; exit 0' -- "$oo_boot_phys" || {
    oo_boot_rc=$?
    if [ "$oo_boot_rc" -eq 3 ]; then
      echo "refusing hardlinked invocation" >&2
    else
      echo "refusing untrusted common path" >&2
    fi
    exit 2
  }
  perl -MDigest::SHA=sha256_hex -MFcntl -MCwd=abs_path - "$oo_boot_phys" "${OO_COMMON_SH:-}" << 'PERL' || exit 2
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
sub slurp_nlink1 {
  my ($path) = @_;
  die_msg("refusing untrusted common path") unless ancestors_ok($path);
  my @st = lstat($path);
  die_msg("refusing untrusted common path") if !@st || -l _ || !-f _;
  die_msg("refusing hardlinked invocation") if $st[3] != 1;
  sysopen(my $fh, $path, O_RDONLY | O_NOFOLLOW) or die_msg("refusing untrusted common path");
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  close $fh;
  return defined $raw ? $raw : "";
}
my ($script, $common_env) = @ARGV;
die_msg("refusing unbound snapshot seal") if !defined $script || $script !~ m{\A/};
die_msg("refusing untrusted common path") if !defined $common_env || $common_env !~ m{\A/} || $common_env =~ /[[:cntrl:]]/;
die_msg("refusing untrusted common path") if -l $common_env;
my $seal = "$script.seal";
die_msg("refusing unbound snapshot seal") unless ancestors_ok($seal);
die_msg("refusing unbound snapshot seal") if -l $seal || !-f $seal;
my @sst = lstat($seal);
die_msg("refusing unbound snapshot seal") if !@sst || -l _ || !-f _;
die_msg("refusing unbound snapshot seal") if $sst[4] != $> || $sst[3] != 1;
die_msg("refusing unbound snapshot seal") unless sprintf("%o", $sst[2] & 07777) eq "400";
die_msg("refusing unbound snapshot seal") unless immutable_ok($seal);
sysopen(my $sfh, $seal, O_RDONLY | O_NOFOLLOW) or die_msg("refusing unbound snapshot seal");
binmode $sfh;
local $/;
my $seal_raw = <$sfh>;
close $sfh;
my @lines = split(/\n/, $seal_raw // "");
die_msg("refusing unbound snapshot seal") unless @lines == 9;
die_msg("refusing unbound snapshot seal") unless $lines[0] eq "oo-snapshot-bind-v1";
die_msg("refusing unbound snapshot seal") unless $lines[1] =~ /\Ascript (\S+)\z/;
my $sealed_script = $1;
die_msg("refusing unbound snapshot seal") unless $lines[2] =~ /\Ascript_sha256 ([0-9a-f]{64})\z/;
my $sealed_script_hash = $1;
die_msg("refusing unbound snapshot seal") unless $lines[3] =~ /\Acommon (\S+)\z/;
my $sealed_common = $1;
die_msg("refusing unbound snapshot seal") unless $lines[4] =~ /\Asha256 ([0-9a-f]{64})\z/;
my $sealed_hash = $1;
die_msg("refusing unbound snapshot seal") unless $lines[5] =~ /\Auid ([0-9]+)\z/;
my $sealed_uid = $1;
die_msg("refusing unbound snapshot seal") unless $lines[6] =~ /\Amode ([0-7]+)\z/;
my $sealed_mode = $1;
die_msg("refusing unbound snapshot seal") unless $lines[7] =~ /\Adev ([0-9]+)\z/;
my $sealed_dev = $1;
die_msg("refusing unbound snapshot seal") unless $lines[8] =~ /\Aino ([0-9]+)\z/;
my $sealed_ino = $1;
die_msg("refusing unbound snapshot seal") unless $sealed_script eq $script;
my $script_bytes = slurp_nlink1($script);
die_msg("refusing unbound snapshot seal") unless sha256_hex($script_bytes) eq $sealed_script_hash;
my $common_phys = abs_path($common_env);
die_msg("refusing untrusted common path") if !defined $common_phys || $common_phys ne $sealed_common;
my @cst = lstat($common_phys);
die_msg("refusing untrusted common path") if !@cst || -l _ || !-f _;
die_msg("refusing hardlinked invocation") if $cst[3] != 1;
my $common_bytes = slurp_nlink1($common_phys);
my $common_hex = sha256_hex($common_bytes);
die_msg("refusing substituted library") unless $common_hex eq $SHIPPED_COMMON_SHA256;
die_msg("refusing unbound snapshot seal") unless $sealed_hash eq $SHIPPED_COMMON_SHA256 && $sealed_hash eq $common_hex;
die_msg("refusing unbound snapshot seal") unless $sealed_uid eq "$cst[4]" && $sealed_uid == $>;
die_msg("refusing unbound snapshot seal") unless $sealed_mode eq sprintf("%o", $cst[2] & 07777);
die_msg("refusing unbound snapshot seal") unless $sealed_dev == $cst[0] && $sealed_ino == $cst[1];
exit 0;
PERL
  oo_boot_stage_script=$oo_boot_phys
  oo_boot_stage_seal=${oo_boot_phys}.seal
  trap oo_boot_drop_stage EXIT
  # shellcheck disable=SC1090
  source "$OO_COMMON_SH"
else
  case "$oo_boot_phys" in
    */builders/snapshot-inputs.sh) ;;
    *)
      echo "refusing invocation path" >&2
      exit 2
      ;;
  esac
  if [ "${OO_SNAPSHOT_SCRIPT_FROZEN:-}" = 1 ]; then
    echo "refusing snapshot reentry flags on an unstaged invocation" >&2
    exit 2
  fi
  oo_boot_base=${oo_boot_phys%/builders/snapshot-inputs.sh}
  oo_boot_sibling=$oo_boot_base/lib/common.sh
  OO_COMMON_SH=$(perl -MDigest::SHA=sha256_hex -MFcntl -MCwd=abs_path - "$oo_boot_phys" "$oo_boot_sibling" "${OO_COMMON_SH:-}" << 'PERL'
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
sub identity {
  my ($path) = @_;
  die_msg("refusing untrusted common path") if !defined $path || $path eq "" || $path =~ /[[:cntrl:]]/;
  die_msg("refusing untrusted common path") if -l $path;
  die_msg("refusing untrusted common path") unless ancestors_ok($path);
  my $phys = abs_path($path);
  die_msg("refusing untrusted common path") if !defined $phys;
  die_msg("refusing untrusted common path") if -l $phys;
  my @st = lstat($phys);
  die_msg("refusing untrusted common path") if !@st || -l _ || !-f _;
  die_msg("refusing hardlinked invocation") if $st[3] != 1;
  sysopen(my $fh, $phys, O_RDONLY | O_NOFOLLOW) or die_msg("refusing untrusted common path");
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  close $fh;
  $raw = "" unless defined $raw;
  return (sha256_hex($raw), $st[4], sprintf("%o", $st[2] & 07777), $st[0], $st[1], $phys);
}
my ($script, $sibling, $claimed) = @ARGV;
die_msg("refusing invocation path") if !defined $script || -l $script;
die_msg("refusing invocation path") unless ancestors_ok($script);
my @script_st = lstat($script);
die_msg("refusing invocation path") if !@script_st || -l _ || !-f _;
die_msg("refusing hardlinked invocation") if $script_st[3] != 1;
my $lib = $sibling;
$lib =~ s{/common\.sh\z}{};
die_msg("refusing untrusted common path") if -l $lib || -l $sibling;
my @sib = identity($sibling);
die_msg("refusing substituted library") unless $sib[0] eq $SHIPPED_COMMON_SHA256;
die_msg("refusing untrusted common path") unless $sib[1] == $>;
if (defined $claimed && $claimed ne "") {
  my @got = identity($claimed);
  die_msg("refusing untrusted common path") if join(" ", @got) ne join(" ", @sib);
}
print $sib[5];
exit 0;
PERL
) || exit 2
  export OO_COMMON_SH
  # shellcheck disable=SC1090
  source "$OO_COMMON_SH"
  oo_require_run_id || exit 2
  oo_boot_frozen=$(mktemp /tmp/oo-snapshot-script.XXXXXX) || exit 2
  oo_boot_stage_script=$(perl -MCwd=abs_path -e 'my $p = abs_path($ARGV[0]); exit 2 if !defined $p || $p !~ m{\A/(?:private/)?tmp/oo-snapshot-script\.[A-Za-z0-9]+\z}; print $p' -- "$oo_boot_frozen") || {
    rm -f "$oo_boot_frozen"
    unset oo_boot_stage_script
    exit 2
  }
  oo_boot_stage_seal=${oo_boot_stage_script}.seal
  cp "$oo_boot_phys" "$oo_boot_stage_script" || {
    oo_boot_drop_stage
    exit 2
  }
  chmod a-w "$oo_boot_stage_script" || {
    oo_boot_drop_stage
    exit 2
  }
  if ! perl -MDigest::SHA=sha256_hex -MFcntl - "$oo_boot_stage_script" "$oo_boot_stage_seal" "$OO_COMMON_SH" << 'PERL'
use strict;
use warnings;
use Digest::SHA qw(sha256_hex);
use Fcntl;
my $SHIPPED_COMMON_SHA256 = "faee8f0564d5105b3fbfc3fbe27d2f5e61917d0e2b0fdebd1d593fd6bd667520";
sub die_msg {
  my ($msg) = @_;
  print STDERR "$msg\n";
  exit 2;
}
my ($script, $seal, $common) = @ARGV;
die_msg("refusing invocation path") if !defined $script || !defined $seal || !defined $common;
my @sst = lstat($script);
die_msg("refusing hardlinked invocation") if !@sst || -l _ || !-f _ || $sst[3] != 1;
my @cst = lstat($common);
die_msg("refusing untrusted common path") if !@cst || -l _ || !-f _;
die_msg("refusing hardlinked invocation") if $cst[3] != 1;
sysopen(my $sf, $script, O_RDONLY | O_NOFOLLOW) or die_msg("refusing invocation path");
binmode $sf;
local $/;
my $script_raw = <$sf>;
close $sf;
sysopen(my $cf, $common, O_RDONLY | O_NOFOLLOW) or die_msg("refusing untrusted common path");
binmode $cf;
my $common_raw = <$cf>;
close $cf;
$script_raw = "" unless defined $script_raw;
$common_raw = "" unless defined $common_raw;
my $common_hex = sha256_hex($common_raw);
die_msg("refusing substituted library") unless $common_hex eq $SHIPPED_COMMON_SHA256;
die_msg("refusing untrusted common path") unless $cst[4] == $>;
my $body = join("\n",
  "oo-snapshot-bind-v1",
  "script $script",
  "script_sha256 " . sha256_hex($script_raw),
  "common $common",
  "sha256 $common_hex",
  "uid $cst[4]",
  "mode " . sprintf("%o", $cst[2] & 07777),
  "dev $cst[0]",
  "ino $cst[1]",
) . "\n";
sysopen(my $out, $seal, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0400) or die_msg("refusing unbound snapshot seal");
syswrite($out, $body) == length($body) or die_msg("refusing unbound snapshot seal");
close $out or die_msg("refusing unbound snapshot seal");
exit 0;
PERL
  then
    oo_boot_drop_stage
    exit 2
  fi
  if command -v chflags >/dev/null 2>&1; then
    chflags uchg "$oo_boot_stage_seal" || {
      oo_boot_drop_stage
      exit 2
    }
  fi
  export OO_SNAPSHOT_SCRIPT_FROZEN=1
  export OO_COMMON_SH
  exec bash "$oo_boot_stage_script" "$@"
  oo_boot_drop_stage
  exit 2
fi
unset oo_boot_phys oo_boot_base oo_boot_sibling oo_boot_frozen

usage() {
  echo "usage: OO_RUN_ID=<uuid> snapshot-inputs.sh <repo> <dest>" >&2
  exit 2
}

[ $# -eq 2 ] || usage
repo=$1
dest=$2
oo_require_run_id || exit 2
oo_reject_secret_override || exit 2

root=$(oo_run_root)
case "$dest" in
  "$root"/snapshot) ;;
  *)
    echo "snapshot dest must be $root/snapshot" >&2
    exit 2
    ;;
esac
case "$repo" in
  *..*) echo "repo path refused" >&2; exit 2 ;;
esac
if [ -e "$dest" ] || [ -L "$dest" ]; then
  echo "snapshot dest already exists" >&2
  exit 2
fi
if [ ! -d "$repo" ] || [ -L "$repo" ]; then
  echo "repo is not a real directory" >&2
  exit 2
fi

# Exact build and test inputs. A path that is not listed here is not copied.
# crates/ holds the seventeen workspace members. contracts/evidence/evidence-v1.json
# is embedded by xtask. The other exact paths are files cargo test reads.
# bench/datagen/Cargo.lock is the second shipping lock. supply::tests::both_shipping_locks_clear_the_age_floor
# reads it with fs::read_to_string. The bench manifest is not an input: that test does not open
# bench/datagen/Cargo.toml, and cargo-deny --manifest-path is only reached from supply --run-tools.
# Copying the lock does not build the bench binary and does not prove the bench MSRV.
# Evidence session trees, the rest of bench/datagen, docs, specs, and governance are not inputs.
required_files() {
  printf '%s\n' \
    Cargo.toml \
    Cargo.lock \
    bench/datagen/Cargo.lock \
    rust-toolchain.toml \
    contracts/evidence/evidence-v1.json \
    ci/deny.toml \
    ci/gates.yaml \
    ci/baseline/gates-baseline.yaml \
    .github/workflows/nightly.yml \
    .github/workflows/pr.yml \
    .github/workflows/release.yml \
    supply-chain/harness-dependency-delta.json \
    testing/infra/images.pins \
    testing/infra/images.lock.json \
    testing/infra/builders/container-cargo.sh \
    testing/infra/builders/cargo-locked.sh \
    testing/infra/builders/snapshot-inputs.sh \
    testing/infra/builders/run-matrix.sh \
    testing/infra/lib/common.sh \
    testing/infra/compose/up.sh \
    testing/infra/compose/smoke.sh \
    testing/infra/compose/fault.sh \
    testing/infra/compose/cleanup.sh \
    testing/infra/compose/debezium/pending.sh
}

required_dirs() {
  printf '%s\n' \
    crates \
    contracts/schema \
    contracts/fixtures \
    contracts/evidence \
    examples/config \
    examples/schema
}

reject_name() {
  case "$1" in
    .env|.env.*|*.pem|*.key|id_rsa|id_rsa.*|id_ed25519|id_ed25519.*|*.p12|*.pfx|credentials|credentials.*)
      return 0
      ;;
  esac
  return 1
}

reject_input_shape() {
  local path=$1 rel=$2 rc
  case "$rel" in
    /*|*../*|../*|*/..|*/../*|*[[:cntrl:]]*)
      echo "control character or parent segment in path: $rel" >&2
      return 2
      ;;
  esac
  perl -e 'my @s = lstat($ARGV[0]); exit 3 if !@s || -l _ || !-f _; exit 4 if $s[4] != $>; exit 0' -- "$path"
  rc=$?
  if [ "$rc" -eq 4 ]; then
    echo "unexpected owner: $rel" >&2
    return 2
  fi
  if [ "$rc" -ne 0 ]; then
    echo "not a regular file: $rel" >&2
    return 2
  fi
  return 0
}

reject_tree_shape() {
  perl - "$1" << 'PERL'
use strict;
use warnings;
my $root = $ARGV[0];
my $uid = $>;
if (!-d $root || -l $root) {
  print STDERR "snapshot tree missing\n";
  exit 2;
}
my @stack = ($root);
while (@stack) {
  my $dir = pop @stack;
  opendir(my $dh, $dir) or do {
    print STDERR "snapshot tree unreadable\n";
    exit 2;
  };
  my @names = readdir $dh;
  closedir $dh;
  for my $name (@names) {
    next if $name eq "." || $name eq "..";
    if ($name =~ /[[:cntrl:]]/) {
      print STDERR "control character in snapshot path\n";
      exit 2;
    }
    my $path = "$dir/$name";
    my @st = lstat($path);
    if (!@st) {
      print STDERR "snapshot entry missing\n";
      exit 2;
    }
    if (-l _) {
      print STDERR "snapshot copy contains a symlink\n";
      exit 2;
    }
    if ($st[4] != $uid) {
      print STDERR "unexpected owner\n";
      exit 2;
    }
    if (-d _) {
      push @stack, $path;
      next;
    }
    if (!-f _) {
      print STDERR "snapshot entry is not a regular file\n";
      exit 2;
    }
  }
}
exit 0;
PERL
}

collect_list() {
  local out=$1 rel base dir
  : > "$out"
  while IFS= read -r rel; do
    if [ ! -f "$repo/$rel" ] || [ -L "$repo/$rel" ]; then
      echo "required file missing: $rel" >&2
      return 2
    fi
    base=$(basename "$rel")
    if reject_name "$base"; then
      echo "credential-shaped name: $rel" >&2
      return 2
    fi
    if ! reject_input_shape "$repo/$rel" "$rel"; then
      return 2
    fi
    printf '%s\n' "$rel" >> "$out"
  done < <(required_files)
  while IFS= read -r dir; do
    if [ ! -d "$repo/$dir" ] || [ -L "$repo/$dir" ]; then
      echo "required directory missing: $dir" >&2
      return 2
    fi
    local found=0
    while IFS= read -r -d '' path; do
      rel=${path#"$repo/"}
      base=$(basename "$rel")
      case "$rel" in
        */.git/*|*/.git|*/target/*|*/target) continue ;;
      esac
      if [ -L "$path" ]; then
        echo "symlink refused: $rel" >&2
        return 2
      fi
      if [ ! -f "$path" ]; then
        echo "not a regular file: $rel" >&2
        return 2
      fi
      if reject_name "$base"; then
        echo "credential-shaped name: $rel" >&2
        return 2
      fi
      if ! reject_input_shape "$path" "$rel"; then
        return 2
      fi
      printf '%s\n' "$rel" >> "$out"
      found=1
    done < <(find "$repo/$dir" -name .git -prune -o -name target -prune -o \( -type f -o -type l \) -print0)
    if [ "$found" -ne 1 ]; then
      echo "required directory has no files: $dir" >&2
      return 2
    fi
  done < <(required_dirs)
  sort -u "$out" -o "$out"
}

hash_list() {
  local root_dir=$1 list=$2 out=$3 rel digest
  : > "$out"
  while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    if [ -L "$root_dir/$rel" ] || [ ! -f "$root_dir/$rel" ]; then
      echo "hash input missing: $rel" >&2
      HASH_FATAL=1
      return 1
    fi
    if ! digest=$(shasum -a 256 "$root_dir/$rel" | awk '{print $1}'); then
      echo "hash failed: $rel" >&2
      HASH_FATAL=1
      return 1
    fi
    if ! printf '%s' "$digest" | grep -Eq '^[0-9a-f]{64}$'; then
      echo "hash failed: $rel" >&2
      HASH_FATAL=1
      return 1
    fi
    printf '%s  %s\n' "$digest" "$rel" >> "$out"
  done < "$list"
}

copy_list() {
  local list=$1 out=$2 rel
  rm -rf "$out"
  mkdir -p "$out"
  while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    mkdir -p "$out/$(dirname "$rel")"
    cp -p "$repo/$rel" "$out/$rel" || return 1
  done < "$list"
  reject_tree_shape "$out" || return 2
}

assert_exact_listed() {
  local manifest=$1 rel got
  while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    got=$(awk -v p="$rel" '$2 == p { print $1; c++ } END { if (c != 1) exit 1 }' "$manifest") || {
      echo "required input omitted: $rel" >&2
      return 2
    }
    if ! printf '%s' "$got" | grep -Eq '^[0-9a-f]{64}$'; then
      echo "required input hash missing: $rel" >&2
      return 2
    fi
  done < <(required_files)
}

verify_exact_source() {
  local manifest=$1 rel expect got
  while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    if [ -L "$repo/$rel" ] || [ ! -f "$repo/$rel" ]; then
      echo "required input missing: $rel" >&2
      return 2
    fi
    if ! expect=$(shasum -a 256 "$repo/$rel" | awk '{print $1}'); then
      echo "hash failed: $rel" >&2
      HASH_FATAL=1
      return 1
    fi
    got=$(awk -v p="$rel" '$2 == p { print $1; c++ } END { if (c != 1) exit 1 }' "$manifest") || {
      echo "required input omitted: $rel" >&2
      return 2
    }
    if [ "$expect" != "$got" ]; then
      echo "required input differs from source: $rel" >&2
      return 1
    fi
  done < <(required_files)
}

fail_cleanup() {
  local code=$1
  chmod -R u+w "$staging" 2>/dev/null || true
  rm -rf "$staging"
  if [ -e "$dest" ] || [ -L "$dest" ]; then
    chmod -R u+w "$dest" 2>/dev/null || true
    rm -rf "$dest"
  fi
  rm -f "$dest.sha256" "$dest.tree" "$root/snapshot.owner"
  exit "$code"
}

oo_prepare_run_root || exit 2
if [ -e "$root/snapshot.sha256" ] || [ -e "$root/snapshot.tree" ] || [ -e "$root/snapshot.owner" ]; then
  echo "snapshot metadata already exists" >&2
  exit 2
fi

staging=$(mktemp -d /tmp/oo-snap-stage.XXXXXX)
trap 'oo_boot_drop_stage; chmod -R u+w "$staging" 2>/dev/null || true; rm -rf "$staging"' EXIT

list=$staging/files.txt
if ! collect_list "$list"; then
  fail_cleanup 2
fi

produce_stable() {
  local name=$1
  local before=$staging/$name.before
  local after=$staging/$name.after
  local copied=$staging/$name.copied
  hash_list "$repo" "$list" "$before" || return 1
  assert_exact_listed "$before" || return 2
  local copy_rc=0
  copy_list "$list" "$staging/$name" || copy_rc=$?
  if [ "$copy_rc" -ne 0 ]; then
    return "$copy_rc"
  fi
  hash_list "$staging/$name" "$list" "$copied" || return 1
  if [ -n "${OO_SNAPSHOT_MUTATE_HOOK:-}" ]; then
    bash "$OO_SNAPSHOT_MUTATE_HOOK" "$repo" || return 1
  fi
  hash_list "$repo" "$list" "$after" || return 1
  cmp -s "$before" "$after" || return 1
  cmp -s "$before" "$copied" || return 1
}

matched=0
attempts=0
HASH_FATAL=0
while [ "$attempts" -lt 3 ]; do
  attempts=$((attempts + 1))
  rm -rf "$staging/a" "$staging/b"
  set +e
  produce_stable a
  rc_a=$?
  produce_stable b
  rc_b=$?
  set -e
  if [ "$HASH_FATAL" -eq 1 ]; then
    echo "snapshot hash failed" >&2
    fail_cleanup 1
  fi
  if [ "$rc_a" -eq 2 ] || [ "$rc_b" -eq 2 ]; then
    fail_cleanup 2
  fi
  if [ "$rc_a" -eq 0 ] && [ "$rc_b" -eq 0 ] && cmp -s "$staging/a.before" "$staging/b.before"; then
    matched=1
    break
  fi
  echo "snapshot attempt $attempts differed" >&2
done

if [ "$matched" -ne 1 ]; then
  echo "snapshot inputs did not match across two stable copies" >&2
  fail_cleanup 1
fi

mkdir -p "$(dirname "$dest")"
if [ -e "$dest" ] || [ -L "$dest" ]; then
  fail_cleanup 2
fi
mv "$staging/b" "$dest"
final=$staging/final.sha
if ! hash_list "$dest" "$list" "$final"; then
  fail_cleanup 1
fi
if ! cmp -s "$staging/b.before" "$final"; then
  echo "promoted snapshot hash does not match the stable copy" >&2
  fail_cleanup 1
fi
verify_exact_source "$final" || fail_cleanup $?
reject_tree_shape "$dest" || fail_cleanup 2
cp "$final" "$dest.sha256"
if ! shasum -a 256 "$dest.sha256" | awk '{print $1}' > "$dest.tree"; then
  echo "tree hash failed" >&2
  fail_cleanup 1
fi
printf '%s\n' "$OO_RUN_ID" > "$root/snapshot.owner"
chmod 600 "$root/snapshot.owner"
chmod a-w "$dest.sha256" "$dest.tree" "$root/snapshot.owner"
chmod -R a-w "$dest"
chmod -R u+w "$staging" 2>/dev/null || true
rm -rf "$staging"
trap oo_boot_drop_stage EXIT

echo "snapshot=$dest"
echo "files=$(wc -l < "$dest.sha256" | tr -d ' ')"
echo "tree_sha256=$(cat "$dest.tree")"
echo "copy_attempts=$attempts"
echo "copies_matched=yes"
echo "includes_container_cargo=$(grep -c 'testing/infra/builders/container-cargo.sh$' "$dest.sha256" | tr -d ' ')"
echo "includes_image_pins=$(grep -c 'testing/infra/images.pins$' "$dest.sha256" | tr -d ' ')"
echo "includes_evidence_v1=$(awk '$2 == "contracts/evidence/evidence-v1.json" { c++ } END { print c + 0 }' "$dest.sha256")"
echo "includes_bench_lock=$(awk '$2 == "bench/datagen/Cargo.lock" { c++ } END { print c + 0 }' "$dest.sha256")"
echo "includes_bench_manifest=$(awk '$2 == "bench/datagen/Cargo.toml" { c++ } END { print c + 0 }' "$dest.sha256")"
echo "required_exact=$(required_files | wc -l | tr -d ' ')"
