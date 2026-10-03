#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Immutable git archive of the fixed commit.
set -euo pipefail
COMMIT=a90c86b405febebfdc70f8169be42dc4de4c3dfc
PARENT=6de125ab01df6564024005dcc74fbf9d0a96b392
TREE=6e7eb46c09470a8163a324063beef48c6254bbb1
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a122-eef831aa
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a122/SP03/wire-bounds-review
EVIDENCE=${REVIEW}/linux/review-1
NAME=oo-a122-msrv-review
NET=oo-a122-eef831aa-net
CARGO_VOL=oo-a122-eef831aa-cargo
TARGET_VOL=oo-a122-eef831aa-target

mkdir -p "$EVIDENCE" "$ROOT"
cp "$REVIEW/product-blobs.sha256" "$EVIDENCE/product-blobs.sha256"
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"
chmod 755 "$ROOT/inside.sh" "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a122 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

git -C "$SRC" rev-parse "$COMMIT" > "$EVIDENCE/source-head.txt"
git -C "$SRC" rev-parse "${COMMIT}^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$SRC" rev-parse "${COMMIT}^" > "$EVIDENCE/source-parent.txt"
test "$(cat "$EVIDENCE/source-head.txt")" = "$COMMIT"
test "$(cat "$EVIDENCE/source-tree.txt")" = "$TREE"
test "$(cat "$EVIDENCE/source-parent.txt")" = "$PARENT"
git -C "$SRC" diff-tree --no-commit-id --name-status -r "$PARENT" "$COMMIT" > "$EVIDENCE/source-diff-names.txt"

rm -rf "$ROOT/snapshot"
mkdir -p "$ROOT/snapshot"
git -C "$SRC" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
  contracts/source/pgsql/empty-catalog.json \
  | tar -C "$ROOT/snapshot" -xf -
test -f "$ROOT/snapshot/contracts/source/pgsql/empty-catalog.json"
test -f "$ROOT/snapshot/crates/oo-connectors/tests/pg_wire_bounds.rs"
test ! -f "$ROOT/snapshot/crates/oo-connectors/tests/pg_wire_bounds_review.rs"
(
  cd "$ROOT/snapshot"
  shasum -a 256 -c "$EVIDENCE/product-blobs.sha256"
) > "$EVIDENCE/product-blobs.host-check.txt"
printf '%s\n' 0 > "$EVIDENCE/snapshot-prep.exit"

{
  echo "commit=$COMMIT"
  echo "tree=$TREE"
  echo "parent=$PARENT"
  while IFS= read -r line; do
    sum=${line%% *}
    path=${line#*  }
    blob=$(git -C "$SRC" rev-parse "${COMMIT}:${path}")
    echo "blob=$blob sha256=$sum path=$path"
  done < "$EVIDENCE/product-blobs.sha256"
} > "$EVIDENCE/blob-ids.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"

if docker network inspect "$NET" >/dev/null 2>&1; then
  echo "network_exists=$NET" > "$EVIDENCE/network.txt"
else
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a122 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a122 \
    "$NET" > "$EVIDENCE/network.txt"
fi
docker volume create --label oo.owner=a122 --label oo.project=open-ontology "$CARGO_VOL" > "$EVIDENCE/cargo-volume.txt"
docker volume create --label oo.owner=a122 --label oo.project=open-ontology "$TARGET_VOL" > "$EVIDENCE/target-volume.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  printf 'prior_status=%s\n' "$prior" > "$EVIDENCE/prior-container.txt"
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >> "$EVIDENCE/prior-container.txt"
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${COMMIT}
source_parent=${PARENT}
source_tree=${TREE}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
network_mode=bridge_not_host
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_method=git archive of the fixed commit; review probe copied after pristine lib, clippy, and author pg_wire_bounds
privileged=no
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
memory=8g
memory_swap=8g
cpus=4
pids_limit=4096
user=0:0
fetch=cargo fetch --manifest-path Cargo.toml --locked
lib=cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
clippy=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
author=cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_bounds -- --nocapture --test-threads=1
review=cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_bounds_review -- --nocapture --test-threads=1
review_clippy=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_bounds_review -- -D warnings
airgap_claim=no
EOF

shasum -a 256 "$ROOT/inside.sh" "$REVIEW/probe/pg_wire_bounds_review.rs" > "$EVIDENCE/inputs.sha256"

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
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ROOT}/snapshot,target=/src" \
  --mount "type=bind,source=${ROOT}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW}/probe,target=/probe,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a122 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a122 \
  --label oo.task=wire-bounds-review-1 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
python3 - "$EVIDENCE" "$NAME" <<'PY'
import json, subprocess, sys
from pathlib import Path
evidence = Path(sys.argv[1])
name = sys.argv[2]
raw = subprocess.check_output(["docker", "inspect", name])
data = json.loads(raw)[0]
hc = data["HostConfig"]
state = data["State"]
lines = []
lines.append(f"ExitCode={state.get('ExitCode')}")
lines.append(f"Status={state.get('Status')}")
lines.append(f"OOMKilled={state.get('OOMKilled')}")
lines.append(f"Privileged={hc.get('Privileged')}")
lines.append(f"NetworkMode={hc.get('NetworkMode')}")
lines.append(f"Memory={hc.get('Memory')}")
lines.append(f"MemorySwap={hc.get('MemorySwap')}")
lines.append(f"NanoCpus={hc.get('NanoCpus')}")
lines.append(f"PidsLimit={hc.get('PidsLimit')}")
lines.append(f"ReadonlyRootfs={hc.get('ReadonlyRootfs')}")
lines.append(f"CapDrop={hc.get('CapDrop')}")
lines.append(f"CapAdd={hc.get('CapAdd')}")
lines.append(f"SecurityOpt={hc.get('SecurityOpt')}")
lines.append(f"Tmpfs={hc.get('Tmpfs')}")
for mount in data.get("Mounts") or []:
    lines.append(
        "mount dest={Destination} mode={Mode} type={Type} rw={RW}".format(**mount)
    )
(evidence / "hostconfig.txt").write_text("\n".join(lines) + "\n")
PY
inspect_exit=$?
printf '%s\n' "$inspect_exit" > "$EVIDENCE/inspect.exit"
exit "$run_exit"
