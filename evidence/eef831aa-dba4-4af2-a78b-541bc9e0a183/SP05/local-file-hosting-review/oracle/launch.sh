#!/bin/bash
# Oracle run. Archive of 755fb84, then two review tests copied only into the snap.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a183/SP05/local-file-hosting-review/oracle
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a183/SP05/local-file-hosting-review
ROOT=/tmp/oo-a183-eef831aa-dba4-4af2-a78b-541bc9e0a183
FIXED=755fb84acc7f80e5ba2fba1c2b80412dab7fab26
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a183-review-oracle
NET=oo-a183-eef831aa-net
CARGO_VOL=oo-a183-eef831aa-cargo
TARGET_VOL=oo-a183-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a183
SNAP=${ROOT}/oracle-src

mkdir -p "$EVIDENCE" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite oracle console.log" >&2
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
cp "$EVIDENCE/review_oracle.rs" "$SNAP/crates/oo-control/tests/review_oracle.rs"
cp "$EVIDENCE/review_memory.rs" "$SNAP/crates/oo-control/tests/review_memory.rs"
shasum -a 256 "$EVIDENCE/review_oracle.rs" "$EVIDENCE/review_memory.rs" > "$EVIDENCE/overlay-sha256.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$RUST_IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker network inspect "$NET" --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' > "$EVIDENCE/network.txt"
grep -q 'internal=false' "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named oracle container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
fixed=${FIXED}
tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
composition=git archive of ${FIXED} plus review_oracle.rs and review_memory.rs
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
  --label oo.owner=a183 \
  --label oo.session="$SESSION" \
  --label oo.task=local-file-hosting-review-oracle \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"

{
  echo "product_files_after_run:"
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    case "$path" in
      \#*) continue ;;
    esac
    sha=$(shasum -a 256 "$SNAP/$path" | awk '{print $1}')
    printf '%s  %s\n' "$sha" "$path"
  done < "$REVIEW/source/blobs.txt"
} > "$EVIDENCE/product-files-after.txt"
python3 - <<'PY' "$REVIEW/source/blobs.txt" "$EVIDENCE/product-files-after.txt" "$EVIDENCE/product-files-match.txt"
import sys
blobs, after, out = sys.argv[1:]
want = {}
for line in open(blobs):
    line = line.strip()
    if not line or line.startswith("#"):
        continue
    _blob, sha, path = line.split("  ")
    want[path] = sha
got = {}
for line in open(after):
    line = line.strip()
    if not line or line.endswith(":"):
        continue
    sha, path = line.split("  ")
    got[path] = sha
bad = [path for path, sha in want.items() if got.get(path) != sha]
open(out, "w").write("product_files_match=%s\n" % (0 if bad else 1))
if bad:
    sys.exit("product files changed: %s" % bad)
PY

exit "$run_exit"
