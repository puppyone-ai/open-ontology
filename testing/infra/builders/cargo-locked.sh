#!/bin/bash
# Host wrapper. The container executes the snapshot's container-cargo.sh.
# Volume names include the full run id, toolchain, arch, and tree hash.
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
    echo "refusing direct execution of a snapshot wrapper" >&2
    exit 2
    ;;
esac

if [ "${OO_WRAPPER_FROM_SNAPSHOT:-}" != 1 ]; then
  case "$oo_boot_phys" in
    */builders/cargo-locked.sh) ;;
    *)
      echo "refusing invocation path" >&2
      exit 2
      ;;
  esac
  oo_boot_base=${oo_boot_phys%/builders/cargo-locked.sh}
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
  if [ $# -ne 4 ]; then
    echo "usage: OO_RUN_ID=<uuid> cargo-locked.sh <1.98.1|1.94.0> <linux/arm64|linux/amd64> <snapshot-dir> <log-dir>" >&2
    exit 2
  fi
  oo_require_run_id || exit 2
  oo_exec_snapshot_wrapper "$@"
  exit 2
fi

if ! printf '%s' "${OO_RUN_ID:-}" | grep -Eq '^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'; then
  echo "refusing untrusted common path" >&2
  exit 2
fi
if ! printf '%s\n' "$oo_boot_phys" | grep -Eq "^/(private/)?tmp/oo-trusted-${OO_RUN_ID}\\.[A-Za-z0-9]+/entry\\.sh$"; then
  echo "refusing direct execution of a snapshot wrapper" >&2
  exit 2
fi
oo_boot_dir=${oo_boot_phys%/entry.sh}
oo_boot_common=${oo_boot_dir}/common.sh
if [ -L "${OO_TRUSTED_COMMON:-}" ] || [ ! -f "${OO_TRUSTED_COMMON:-}" ]; then
  echo "trusted common is not a regular file" >&2
  exit 2
fi
if ! perl -MCwd=abs_path -e 'my $p = abs_path($ARGV[0]); exit 1 if !defined $p || $p ne $ARGV[1] || -l $ARGV[0] || -l $ARGV[1]' -- "${OO_TRUSTED_COMMON:-}" "$oo_boot_common"; then
  echo "refusing untrusted common path" >&2
  exit 2
fi
oo_boot_entry_nlink=$(perl -e 'my @s = lstat($ARGV[0]); exit 1 if !@s || -l $ARGV[0] || !-f _; print $s[3]' -- "$oo_boot_phys") || {
  echo "refusing hardlinked invocation" >&2
  exit 2
}
oo_boot_common_nlink=$(perl -e 'my @s = lstat($ARGV[0]); exit 1 if !@s || -l $ARGV[0] || !-f _; print $s[3]' -- "$oo_boot_common") || {
  echo "refusing hardlinked invocation" >&2
  exit 2
}
if [ "$oo_boot_entry_nlink" != 1 ] || [ "$oo_boot_common_nlink" != 1 ]; then
  echo "refusing hardlinked invocation" >&2
  exit 2
fi
if [ $# -ne 4 ]; then
  echo "usage: OO_RUN_ID=<uuid> cargo-locked.sh <1.98.1|1.94.0> <linux/arm64|linux/amd64> <snapshot-dir> <log-dir>" >&2
  exit 2
fi
if [ "$3" != "/tmp/oo-env-$OO_RUN_ID/snapshot" ]; then
  echo "refusing untrusted common path" >&2
  exit 2
fi
perl -MDigest::SHA=sha256_hex -MFcntl - "$oo_boot_phys" "$oo_boot_common" "$OO_RUN_ID" << 'PERL' || exit 2
use strict;
use warnings;
use Digest::SHA qw(sha256_hex);
use Fcntl;
my $SHIPPED_COMMON_SHA256 = "faee8f0564d5105b3fbfc3fbe27d2f5e61917d0e2b0fdebd1d593fd6bd667520";
my ($entry, $common, $run_id) = @ARGV;
die "refusing untrusted common path\n" unless defined $run_id && $run_id =~ /\A[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\z/;
my $manifest = "/tmp/oo-env-$run_id/snapshot.sha256";
die "refusing untrusted common path\n" if -l $manifest || -l $entry || -l $common;
sub slurp {
  my ($path) = @_;
  my @st = lstat($path) or die "trusted common is not a regular file\n";
  die "trusted common is not a regular file\n" if -l _ || !-f _;
  die "refusing untrusted common path\n" if $st[4] != $> || ($st[2] & 0222) || $st[3] != 1;
  sysopen(my $fh, $path, O_RDONLY | O_NOFOLLOW) or die "trusted common is not a regular file\n";
  binmode $fh;
  local $/;
  my $raw = <$fh>;
  close $fh;
  return defined $raw ? $raw : "";
}
my $text = slurp($manifest);
my %want;
foreach my $line (split(/\n/, $text)) {
  next if $line eq "";
  die "refusing untrusted common path\n" unless $line =~ /\A([0-9a-f]{64})  (\S+)\z/;
  $want{$2} = $1;
}
my $entry_rel = "testing/infra/builders/cargo-locked.sh";
my $common_rel = "testing/infra/lib/common.sh";
my $common_hex = sha256_hex(slurp($common));
my $entry_hex = sha256_hex(slurp($entry));
die "mismatch lib/common.sh\n" unless defined $want{$common_rel} && $common_hex eq $want{$common_rel};
die "refusing substituted library\n" unless $common_hex eq $SHIPPED_COMMON_SHA256 && $want{$common_rel} eq $SHIPPED_COMMON_SHA256;
die "infra freeze does not match the seal\n" unless defined $want{$entry_rel} && $entry_hex eq $want{$entry_rel};
exit 0;
PERL
# shellcheck disable=SC1090
source "$oo_boot_common"
unset oo_boot_phys oo_boot_dir oo_boot_common
if [ "${OO_TRUSTED_COMMON_OWNER:-}" = 1 ]; then
  trap oo_drop_trusted_common EXIT
  unset OO_TRUSTED_COMMON_OWNER
fi

if [ $# -ne 4 ]; then
  echo "usage: OO_RUN_ID=<uuid> cargo-locked.sh <1.98.1|1.94.0> <linux/arm64|linux/amd64> <snapshot-dir> <log-dir>" >&2
  exit 2
fi

toolchain=$1
platform=$2
snapshot=$3
log_dir=$4
oo_require_run_id || exit 2

case "$platform" in
  linux/arm64) arch=arm64; cpus=6; jobs=6 ;;
  linux/amd64) arch=amd64; cpus=4; jobs=2 ;;
  *) echo "unsupported platform: $platform" >&2; exit 2 ;;
esac
case "$toolchain" in
  1.98.1|1.94.0) ;;
  *) echo "unsupported toolchain: $toolchain" >&2; exit 2 ;;
esac

root=$(oo_run_root)
case "$snapshot" in
  "$root"/snapshot) ;;
  *) echo "snapshot must be the run's frozen snapshot" >&2; exit 2 ;;
esac
if [ -L "$snapshot" ] || [ ! -d "$snapshot" ]; then
  echo "snapshot is not a real directory" >&2
  exit 2
fi
if [ ! -f "$snapshot/Cargo.toml" ] || [ ! -f "$snapshot/Cargo.lock" ] || [ ! -f "$snapshot/rust-toolchain.toml" ]; then
  echo "snapshot is missing Cargo inputs" >&2
  exit 2
fi
if [ ! -f "$snapshot/testing/infra/builders/container-cargo.sh" ]; then
  echo "snapshot is missing container-cargo.sh" >&2
  exit 2
fi
pins=$snapshot/testing/infra/images.pins
if [ ! -f "$pins" ] || [ ! -f "$snapshot.tree" ] || [ ! -f "$snapshot.sha256" ]; then
  echo "snapshot is missing pins or hash files" >&2
  exit 2
fi
tree=$(tr -d '\n' < "$snapshot.tree")
if ! printf '%s' "$tree" | grep -Eq '^[0-9a-f]{64}$'; then
  echo "snapshot tree hash is not 64 hex" >&2
  exit 2
fi
if [ "$(tr -d '\n' < "$root/snapshot.owner")" != "$OO_RUN_ID" ]; then
  echo "snapshot owner marker does not match OO_RUN_ID" >&2
  exit 2
fi
oo_verify_snapshot_members "$snapshot" || exit 2

digest=$(awk -v key="rust-$toolchain" -v plat="$platform" '$1==key && $2==plat { print $3 }' "$pins")
if ! printf '%s' "$digest" | grep -Eq '^sha256:[0-9a-f]{64}$'; then
  echo "no pin for rust-$toolchain $platform" >&2
  exit 2
fi
image="docker.io/library/rust@${digest}"
safe_tc=${toolchain//./_}
vol_cargo="oo-${OO_RUN_ID}-cargo-${safe_tc}-${arch}-${tree}"
vol_target="oo-${OO_RUN_ID}-target-${safe_tc}-${arch}-${tree}"
net="oo-${OO_RUN_ID}-net-build"
cname="oo-${OO_RUN_ID}-build-${safe_tc}-${arch}"
if [ "${#vol_cargo}" -gt 200 ] || [ "${#vol_target}" -gt 200 ] || [ "${#cname}" -gt 200 ]; then
  echo "resource name exceeds the length budget" >&2
  exit 2
fi

engine_arch=$(docker info --format '{{.Architecture}}')
mkdir -p "$log_dir"
chmod 700 "$log_dir"

common_labels=(
  --label oo.project=open-ontology
  --label oo.owner=env
  --label "oo.run_id=$OO_RUN_ID"
  --label oo.scope=sp00-infra-builder
)
identity_labels=(
  --label "oo.toolchain=$toolchain"
  --label "oo.platform=$platform"
  --label "oo.tree=$tree"
)

resource_labels_match() {
  local kind=$1 name=$2 got
  if [ "$kind" = "network" ]; then
    got=$(docker network inspect --format '{{index .Labels "oo.project"}} {{index .Labels "oo.owner"}} {{index .Labels "oo.run_id"}} {{index .Labels "oo.scope"}}' "$name")
    [ "$got" = "open-ontology env $OO_RUN_ID sp00-infra-builder" ]
    return
  fi
  if [ "$kind" = "container" ]; then
    got=$(docker container inspect --format '{{index .Config.Labels "oo.project"}} {{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.run_id"}} {{index .Config.Labels "oo.scope"}} {{index .Config.Labels "oo.toolchain"}} {{index .Config.Labels "oo.platform"}} {{index .Config.Labels "oo.tree"}}' "$name")
  else
    got=$(docker volume inspect --format '{{index .Labels "oo.project"}} {{index .Labels "oo.owner"}} {{index .Labels "oo.run_id"}} {{index .Labels "oo.scope"}} {{index .Labels "oo.toolchain"}} {{index .Labels "oo.platform"}} {{index .Labels "oo.tree"}}' "$name")
  fi
  [ "$got" = "open-ontology env $OO_RUN_ID sp00-infra-builder $toolchain $platform $tree" ]
}

ensure_volume() {
  local name=$1 role=$2
  if docker volume inspect "$name" >/dev/null 2>&1; then
    if ! resource_labels_match volume "$name"; then
      echo "refusing volume $name: labels do not match this run" >&2
      exit 2
    fi
    if ! oo_resume_requested; then
      echo "volume $name already exists; pass OO_RESUME=1 to reuse it" >&2
      exit 2
    fi
    echo "volume_reuse=$name"
  else
    docker volume create "${common_labels[@]}" "${identity_labels[@]}" --label "oo.fixture=$role" "$name" >/dev/null
  fi
}

oo_verify_snapshot_members "$snapshot" || exit 2
if ! docker network inspect "$net" >/dev/null 2>&1; then
  docker network create "${common_labels[@]}" --label oo.fixture=builder "$net" >/dev/null 2>&1 || true
fi
if ! docker network inspect "$net" >/dev/null 2>&1 || ! resource_labels_match network "$net"; then
  echo "refusing network $net: missing or labels do not match this run" >&2
  exit 2
fi
ensure_volume "$vol_cargo" cargo-home
ensure_volume "$vol_target" cargo-target

if docker container inspect "$cname" >/dev/null 2>&1; then
  if ! resource_labels_match container "$cname"; then
    echo "refusing to remove foreign container $cname" >&2
    exit 2
  fi
  if ! oo_resume_requested; then
    echo "container $cname already exists; pass OO_RESUME=1 to replace it" >&2
    exit 2
  fi
  docker rm -f "$cname" >/dev/null
fi

{
  echo "evidence_session=$(oo_session_id)"
  echo "run_id=$OO_RUN_ID"
  echo "image=$image"
  echo "platform=$platform"
  echo "engine_arch=$engine_arch"
  echo "cpus=$cpus"
  echo "cargo_build_jobs=$jobs"
  echo "memory=8g"
  echo "snapshot=$snapshot"
  echo "snapshot_tree_sha256=$tree"
  echo "script_source=snapshot"
  echo "airgap_claim=no"
  echo "parallel_cap=2"
} > "$log_dir/invocation.txt"

oo_verify_snapshot_members "$snapshot" || exit 2
set +e
docker run --pull never \
  --name "$cname" \
  --platform "$platform" \
  --network "$net" \
  --init \
  "${common_labels[@]}" \
  "${identity_labels[@]}" \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus "$cpus" \
  --pids-limit 4096 \
  --mount "type=bind,src=$snapshot,dst=/src,readonly" \
  --mount "type=volume,src=$vol_cargo,dst=/cargo-home" \
  --mount "type=volume,src=$vol_target,dst=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,size=2g \
  -w /src \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e "CARGO_BUILD_JOBS=$jobs" \
  -e "OO_TOOLCHAIN=$toolchain" \
  -e "OO_PLATFORM=$platform" \
  -e "OO_ENGINE_ARCH=$engine_arch" \
  "$image" \
  bash /src/testing/infra/builders/container-cargo.sh \
  > "$log_dir/console.log" 2>&1
container_exit=$?
set -e
printf '%s\n' "$container_exit" > "$log_dir/container.exit"
printf '%s\n' "$container_exit" > "$log_dir/wrapper.exit"
inspect_exit=$(docker inspect --format '{{.State.ExitCode}}' "$cname" 2>/dev/null || echo missing)
printf '%s\n' "$inspect_exit" > "$log_dir/inspect.exit"
offline_downloads=$(awk '
  /^=== cargo build --workspace --locked --offline ===/ { phase="offline" }
  phase=="offline" && /Downloaded/ { count++ }
  END { print count+0 }
' "$log_dir/console.log")
printf '%s\n' "$offline_downloads" > "$log_dir/offline-downloads.count"
echo "container_exit=$container_exit"
echo "wrapper_exit=$container_exit"
echo "inspect_exit=$inspect_exit"
echo "offline_download_lines=$offline_downloads"
echo "airgap_claim=no"
if [ "$offline_downloads" -ne 0 ] && [ "$container_exit" -eq 0 ]; then
  echo "offline section downloaded crates; wrapper fails closed" >&2
  printf '%s\n' 1 > "$log_dir/wrapper.exit"
  echo "wrapper_exit=1"
  exit 1
fi
exit "$container_exit"
