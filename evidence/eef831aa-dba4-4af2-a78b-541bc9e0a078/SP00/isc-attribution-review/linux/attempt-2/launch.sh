#!/bin/bash
# Attempt 2. Same commit, snapshot now includes contracts and examples.
# Does not rewrite attempt 1 exit files. One own builder. No prune.
set -eu
RUN_ID=4ebcaafc-8442-4e95-9706-59ca03b0b1cb
COMMIT=fb2d1d19c21b510dfd6231c3b67d0efbf4285491
PARENT=6fa6481267e5a4efac20831fa0b407a8cf028905
TREE=79f19b818d3e14ad08ad5208ce2bd0a17b4c297e
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a078-${RUN_ID}
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a078/SP00/isc-attribution-review/linux/attempt-2
NAME=oo-a078-msrv-${RUN_ID}-a2
NET=oo-a078-${RUN_ID}-a2-net
CARGO_VOL=oo-a078-${RUN_ID}-a2-cargo
TARGET_VOL=oo-a078-${RUN_ID}-a2-target

mkdir -p "$EVIDENCE" "$ROOT"
docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}} image={{.Image}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"

running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /^(rust@|docker.io\/library\/rust|0f24958fa693|218765d808d3|f31fa9eaac4e|f747a9f2b58b)/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -aq --filter label=oo.owner=a078 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_any_state=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

mkdir -p "$ROOT/snapshot-a2"
git -C "$SRC" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo contracts examples \
  | tar -C "$ROOT/snapshot-a2" -xf -
cp "$EVIDENCE/inside.sh" "$ROOT/inside-a2.sh"
chmod 755 "$ROOT/inside-a2.sh"
git -C "$SRC" rev-parse "$COMMIT" > "$EVIDENCE/source-head.txt"
git -C "$SRC" rev-parse "${COMMIT}^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$SRC" rev-parse "${COMMIT}^" > "$EVIDENCE/source-parent.txt"
(
  cd "$ROOT/snapshot-a2"
  find . -type f | sort | shasum -a 256
) > "$EVIDENCE/archive-inputs.sha256"
shasum -a 256 "$EVIDENCE/inside.sh" "$ROOT/inside-a2.sh" > "$EVIDENCE/inside.sha256"
git -C "$SRC" rev-parse "${COMMIT}:Cargo.lock" > "$EVIDENCE/lock-blob.txt"
git hash-object "$ROOT/snapshot-a2/Cargo.lock" > "$EVIDENCE/archive-lock-blob.txt"
test -f "$ROOT/snapshot-a2/contracts/fixtures/pg-mysql/cases.json"
test -f "$ROOT/snapshot-a2/examples/config/agent.yaml"
printf '%s\n' 0 > "$EVIDENCE/extra-inputs.exit"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
attempt=2
reason=attempt 1 omitted contracts and examples, so fixtures_pg_mysql.rs returned NotFound
source_commit=${COMMIT}
source_parent=$(cat "$EVIDENCE/source-parent.txt")
source_tree=$(cat "$EVIDENCE/source-tree.txt")
expected_parent=${PARENT}
expected_tree=${TREE}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
network_mode=bridge_not_host
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot-a2
snapshot_method=git archive of the pinned commit, workspace inputs plus contracts and examples
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
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-core; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-core --all-targets -- -D warnings
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a078 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a078 \
  --label oo.run_id="${RUN_ID}-a2" \
  --label oo.kind=task-net \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a078 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a078 \
  --label oo.run_id="${RUN_ID}-a2" \
  --label oo.kind=task-cargo \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a078 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a078 \
  --label oo.run_id="${RUN_ID}-a2" \
  --label oo.kind=task-target \
  "$TARGET_VOL" >/dev/null

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
  --mount "type=bind,source=${ROOT}/snapshot-a2,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/inside-a2.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a078 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a078 \
  --label oo.task=isc-attribution-review-smoke-a2 \
  --label oo.run_id="${RUN_ID}-a2" \
  --label oo.scope=sp00-isc-attribution-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" > "$EVIDENCE/inspect.json"
inspect_exit=$?
printf '%s\n' "$inspect_exit" > "$EVIDENCE/inspect.exit"
python3 - "$EVIDENCE" <<'PY'
import json, sys
from pathlib import Path
evidence = Path(sys.argv[1])
raw = json.loads((evidence / "inspect.json").read_text())
data = raw[0]
hc = data["HostConfig"]
state = data["State"]
mounts = data.get("Mounts") or []
lines = []
lines.append(f"ExitCode={state.get('ExitCode')}")
lines.append(f"Status={state.get('Status')}")
lines.append(f"OOMKilled={state.get('OOMKilled')}")
lines.append(f"Error={state.get('Error')}")
lines.append(f"Privileged={hc.get('Privileged')}")
lines.append(f"NetworkMode={hc.get('NetworkMode')}")
lines.append(f"Memory={hc.get('Memory')}")
lines.append(f"MemorySwap={hc.get('MemorySwap')}")
lines.append(f"NanoCpus={hc.get('NanoCpus')}")
lines.append(f"PidsLimit={hc.get('PidsLimit')}")
lines.append(f"ReadonlyRootfs={hc.get('ReadonlyRootfs')}")
lines.append("CapDrop=" + json.dumps(hc.get("CapDrop")))
lines.append("CapAdd=" + json.dumps(hc.get("CapAdd")))
lines.append(f"User={data.get('Config', {}).get('User')}")
lines.append("SecurityOpt=" + json.dumps(hc.get("SecurityOpt")))
lines.append(f"TmpfsKeyPresent={'Tmpfs' in hc}")
lines.append("Tmpfs=" + json.dumps(hc.get("Tmpfs")))
lines.append(f"BindsCount={len(hc.get('Binds') or [])}")
(evidence / "inspect.txt").write_text("\n".join(lines) + "\n")
(evidence / "mounts.json").write_text(json.dumps(mounts, indent=2) + "\n")
(evidence / "binds.json").write_text(json.dumps(hc.get("Binds"), indent=2) + "\n")
(evidence / "labels.json").write_text(json.dumps(data.get("Config", {}).get("Labels"), indent=2) + "\n")
(evidence / "tmpfs.json").write_text(json.dumps({"key_present": "Tmpfs" in hc, "value": hc.get("Tmpfs")}, indent=2) + "\n")
PY
book_exit=$?
printf '%s\n' "$book_exit" > "$EVIDENCE/bookkeeping.exit"
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
