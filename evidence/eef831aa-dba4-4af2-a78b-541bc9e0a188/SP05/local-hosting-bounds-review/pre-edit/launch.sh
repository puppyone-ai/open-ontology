#!/bin/bash
# Pre-overlay smoke. Immutable git archive of 3a251dea. No review test is copied in.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a188/SP05/local-hosting-bounds-review/pre-edit
ROOT=/tmp/oo-a188-eef831aa-dba4-4af2-a78b-541bc9e0a188
FIXED=3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a188-pre-edit
NET=oo-a188-eef831aa-net
CARGO_VOL=oo-a188-eef831aa-cargo
TARGET_VOL=oo-a188-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a188
SNAP=${ROOT}/archive

mkdir -p "$EVIDENCE/phases" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite pre-edit console.log" >&2
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
  echo "fixed_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "fixed_parent=$(git -C "$SRC" rev-parse "${FIXED}^")"
  echo "workspace_head=$(git -C "$SRC" rev-parse HEAD)"
  echo "archive_source=git archive ${FIXED}"
  echo "worktree_not_used=1"
} > "$EVIDENCE/archive-base.txt"

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}'
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver|rustup)$/ {print}' || true
} > "$EVIDENCE/builders-before.txt"
running_rust=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a188 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/archive-tree.txt"
(
  cd "$SNAP"
  find . -type f ! -path './.git/*' -print0 | sort -z | xargs -0 shasum -a 256
) > "$EVIDENCE/tree-before.sha256"

python3 - <<'PY' "$SRC" "$FIXED" "$SNAP" "$EVIDENCE/archive-match.txt"
import hashlib, subprocess, sys
src, fixed, snap, out = sys.argv[1:]
paths = [
    "crates/oo-control/src/file_hosting.rs",
    "crates/oo-control/tests/file_hosting.rs",
    "crates/oo-control/src/lib.rs",
    "crates/oo-control/Cargo.toml",
    "contracts/control/local-file-hosting-v1.md",
    "Cargo.lock",
]
lines = []
ok = True
for path in paths:
    blob = subprocess.check_output(["git", "-C", src, "rev-parse", f"{fixed}:{path}"], text=True).strip()
    data = open(f"{snap}/{path}", "rb").read()
    sha = hashlib.sha256(data).hexdigest()
    show = subprocess.check_output(["git", "-C", src, "show", f"{fixed}:{path}"])
    match = show == data
    ok = ok and match
    lines.append(f"path={path} blob={blob} sha256={sha} match={int(match)}")
lines.append(f"archive_hashes_match={int(ok)}")
open(out, "w").write("\n".join(lines) + "\n")
if not ok:
    sys.exit("archive product bytes differ from the fixed commit")
PY

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$RUST_IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a188 \
    --label oo.session="$SESSION" \
    --label oo.kind=network \
    "$NET" > "$EVIDENCE/network-create.txt"
fi
docker network inspect "$NET" --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' > "$EVIDENCE/network.txt"
grep -q 'internal=false' "$EVIDENCE/network.txt"

if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a188 \
    --label oo.session="$SESSION" \
    --label oo.kind=cargo-home \
    "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a188 \
    --label oo.session="$SESSION" \
    --label oo.kind=cargo-target \
    "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
fi
docker volume inspect "$CARGO_VOL" --format '{{.Name}}' > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" --format '{{.Name}}' > "$EVIDENCE/volume-target.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named pre-edit container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
fixed=${FIXED}
tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
composition=git archive of ${FIXED} with no review overlay
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
  --tmpfs /tmp:rw,nosuid,nodev,size=2g \
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
  --label oo.owner=a188 \
  --label oo.session="$SESSION" \
  --label oo.task=local-hosting-bounds-review-pre-edit \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
exit "$run_exit"
