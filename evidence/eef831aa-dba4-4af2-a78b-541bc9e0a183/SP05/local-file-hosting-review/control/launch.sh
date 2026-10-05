#!/bin/bash
# Pre-overlay Linux 1.94 control smoke of commit 755fb84. No product overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a183/SP05/local-file-hosting-review/control
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a183/SP05/local-file-hosting-review
ROOT=/tmp/oo-a183-eef831aa-dba4-4af2-a78b-541bc9e0a183
FIXED=755fb84acc7f80e5ba2fba1c2b80412dab7fab26
PARENT=72b5136ba23d134916425132523383947046aeff
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a183-review-control
NET=oo-a183-eef831aa-net
CARGO_VOL=oo-a183-eef831aa-cargo
TARGET_VOL=oo-a183-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a183
SNAP=${ROOT}/control-src

mkdir -p "$EVIDENCE" "$REVIEW/source" "$REVIEW/diff" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite control console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

cleanup() {
  docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt" 2>/dev/null || true
  docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} readonly_root={{.HostConfig.ReadonlyRootfs}} memory={{.HostConfig.Memory}} memoryswap={{.HostConfig.MemorySwap}}' > "$EVIDENCE/hostconfig.txt" 2>/dev/null || true
  docker inspect "$NAME" --format '{{json .HostConfig.Binds}}{{json .HostConfig.Mounts}}' > "$EVIDENCE/mounts.txt" 2>/dev/null || true
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

{
  echo "fixed=$FIXED"
  echo "parent=$PARENT"
  echo "tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "workspace_head=$(git -C "$SRC" rev-parse HEAD)"
  git -C "$SRC" diff-tree --no-commit-id --name-status -r "$FIXED"
} > "$EVIDENCE/archive-base.txt"

git -C "$SRC" diff --binary "$PARENT" "$FIXED" > "$REVIEW/diff/parent.diff"
git -C "$SRC" archive "$FIXED" -- \
  Cargo.lock \
  contracts/control/local-file-hosting-v1.md \
  crates/oo-control/Cargo.toml \
  crates/oo-control/src/lib.rs \
  crates/oo-control/src/file_hosting.rs \
  crates/oo-control/tests/file_hosting.rs \
  | tar -x -C "$REVIEW/source"
{
  echo "# fixed $FIXED"
  echo "# parent $PARENT"
  echo "# tree $(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "# columns: git blob, sha256, path"
  while IFS= read -r path; do
    blob=$(git -C "$SRC" rev-parse "${FIXED}:${path}")
    sha=$(git -C "$SRC" cat-file blob "$blob" | shasum -a 256 | awk '{print $1}')
    printf '%s  %s  %s\n' "$blob" "$sha" "$path"
  done <<'PATHS'
Cargo.lock
contracts/control/local-file-hosting-v1.md
crates/oo-control/Cargo.toml
crates/oo-control/src/lib.rs
crates/oo-control/src/file_hosting.rs
crates/oo-control/tests/file_hosting.rs
PATHS
} > "$REVIEW/source/blobs.txt"

author_sums=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a181/SP05/local-file-hosting/SHA256SUMS
{
  echo "author_sha256sums=$author_sums"
  echo "author_data_lines=$(grep -cEv '^(#|$)' "$author_sums" || true)"
  echo "author_file_count=$(find "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a181/SP05/local-file-hosting" -type f | wc -l | tr -d ' ')"
  echo "compare_to_fixed_blobs:"
  mismatch=0
  while read -r blob sha path; do
    case "$blob" in
      ''|\#*) continue ;;
    esac
    got_blob=$(git -C "$SRC" rev-parse "${FIXED}:${path}")
    got_sha=$(git -C "$SRC" cat-file blob "$got_blob" | shasum -a 256 | awk '{print $1}')
    if [ "$got_blob" = "$blob" ] && [ "$got_sha" = "$sha" ]; then
      echo "match $path"
    else
      echo "MISMATCH $path author_blob=$blob git_blob=$got_blob author_sha=$sha git_sha=$got_sha"
      mismatch=1
    fi
  done < "$author_sums"
  echo "source_hash_mismatches=$mismatch"
  echo "inventory=source_commit_files_only"
  echo "full_evidence_manifest=no"
} > "$REVIEW/author-sha256sums-gap.txt"

git -C "$SRC" show "${PARENT}:Cargo.lock" > /tmp/oo-a183-parent-Cargo.lock
git -C "$SRC" show "${FIXED}:Cargo.lock" > /tmp/oo-a183-fixed-Cargo.lock
awk '
function flush() {
  if (name == "") return
  if (checksum == "") checksum = "-"
  print name "\t" version "\t" checksum
}
/^\[\[package\]\]/ { flush(); name = ""; version = ""; checksum = ""; next }
/^name = "/ && name == "" { name = substr($0, 9, length($0) - 9); next }
/^version = "/ && name != "" && version == "" { version = substr($0, 12, length($0) - 12); next }
/^checksum = "/ && name != "" && checksum == "" { checksum = substr($0, 13, length($0) - 13); next }
END { flush() }
' /tmp/oo-a183-parent-Cargo.lock | sort > /tmp/oo-a183-old.tsv
awk '
function flush() {
  if (name == "") return
  if (checksum == "") checksum = "-"
  print name "\t" version "\t" checksum
}
/^\[\[package\]\]/ { flush(); name = ""; version = ""; checksum = ""; next }
/^name = "/ && name == "" { name = substr($0, 9, length($0) - 9); next }
/^version = "/ && name != "" && version == "" { version = substr($0, 12, length($0) - 12); next }
/^checksum = "/ && name != "" && checksum == "" { checksum = substr($0, 13, length($0) - 13); next }
END { flush() }
' /tmp/oo-a183-fixed-Cargo.lock | sort > /tmp/oo-a183-new.tsv
: > "$EVIDENCE/lock-added.txt"
: > "$EVIDENCE/lock-version-added.txt"
: > "$EVIDENCE/lock-delta-errors.txt"
set +e
awk -F '\t' '
NR == FNR { old[$1] = old[$1] " " $2; pair[$1 "\t" $2] = $3; next }
{
  key = $1 "\t" $2
  if (!(key in pair)) {
    print $1 "\t" $2 "\t" $3 >> added
    if ($1 in old) print "added version alongside existing: " $1 " " $2 >> alongside
  } else if (pair[key] != $3) {
    print "checksum changed: " $1 " " $2 >> errors
    bad = 1
  }
}
END { if (bad) exit 1 }
' added="$EVIDENCE/lock-added.txt" alongside="$EVIDENCE/lock-version-added.txt" errors="$EVIDENCE/lock-delta-errors.txt" \
  /tmp/oo-a183-old.tsv /tmp/oo-a183-new.tsv
delta_new=$?
awk -F '\t' '
NR == FNR { seen[$1 "\t" $2] = 1; next }
{
  if (!(($1 "\t" $2) in seen)) {
    print "existing package version removed: " $1 " " $2 >> errors
    bad = 1
  }
}
END { if (bad) exit 1 }
' errors="$EVIDENCE/lock-delta-errors.txt" /tmp/oo-a183-new.tsv /tmp/oo-a183-old.tsv
delta_old=$?
set -e
if [ "$delta_new" -ne 0 ] || [ "$delta_old" -ne 0 ]; then
  echo "host lock delta has errors" >&2
  printf '%s\n' 4 > "$EVIDENCE/phases-host-lock.exit"
else
  printf '%s\n' 0 > "$EVIDENCE/phases-host-lock.exit"
fi

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}'
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver|rustup)$/ {print}' || true
} > "$EVIDENCE/builders-before.txt"

running_rust=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a183 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'running_rust_builders=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 3 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
(
  cd "$SNAP"
  find . -type f ! -path './.git/*' -print0 | sort -z | xargs -0 shasum -a 256
) > "$EVIDENCE/tree-before.sha256"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/archive-tree.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$RUST_IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a183 \
    --label oo.session="$SESSION" \
    "$NET" > "$EVIDENCE/network-create.txt"
fi
docker network inspect "$NET" --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' > "$EVIDENCE/network.txt"
grep -q 'internal=false' "$EVIDENCE/network.txt"

if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create --label oo.project=open-ontology --label oo.owner=a183 --label oo.session="$SESSION" "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create --label oo.project=open-ontology --label oo.owner=a183 --label oo.session="$SESSION" "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named control container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
fixed=${FIXED}
parent=${PARENT}
tree=$(cat "$EVIDENCE/archive-tree.txt")
composition=git archive of ${FIXED} with no overlay
image=${RUST_IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
postgres=none
phases=fmt-check fetch lock-unchanged tree test clippy arch
package=oo-control
EOF

set +e
docker run \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a183 \
  --label oo.session="$SESSION" \
  --label oo.task=local-file-hosting-review-control \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"

(
  cd "$SNAP"
  find . -type f ! -path './.git/*' -print0 | sort -z | xargs -0 shasum -a 256
) > "$EVIDENCE/tree-after.sha256"
if cmp -s "$EVIDENCE/tree-before.sha256" "$EVIDENCE/tree-after.sha256"; then
  echo "archive_hashes_match=1" > "$EVIDENCE/archive-match.txt"
else
  echo "archive_hashes_match=0" > "$EVIDENCE/archive-match.txt"
fi

exit "$run_exit"
