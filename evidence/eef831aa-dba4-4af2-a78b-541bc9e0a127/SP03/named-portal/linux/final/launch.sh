#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder over a fresh archive of the fixed SHA plus the overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a127/SP03/named-portal/linux/final
ROOT=/tmp/oo-a127-eef831aa
OVERLAY=/tmp/oo-a127-draft/overlay
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a127-msrv-final
NET=oo-a127-eef831aa-net
CARGO_VOL=oo-a127-eef831aa-cargo
TARGET_VOL=oo-a127-eef831aa-target
PGLOG=oo-a127-eef831aa-pglog
SEC=/tmp/oo-a127-eef831aa-dba4-4af2-a78b-541bc9e0a127/secrets
FIXED=b8e7bf1b1c25fbc0f8213898eb7de84b248f8211
TREE=e8f408d6f66ef5361dc7c6d6185ca7942eaf1819
PARENT=578a55231e4b4373db014ff1450f8e1bb17ba639

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
rm -f "$EVIDENCE"/*.exit "$EVIDENCE"/console.log "$EVIDENCE"/hostconfig.txt

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a127 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
pg_up=$(docker ps -q --filter name=^/oo-a127-pg$ --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s own_pg_running=%s\n' "$running_rust" "$own" "$host_cargo" "$pg_up" > "$EVIDENCE/capacity.txt"
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
if [ "$pg_up" -ne 1 ]; then
  echo "owned postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
test -f "$SEC/pointer.txt"
test -f "$OVERLAY/crates/oo-connectors/vendor/sqlx-postgres/src/connection/portal.rs"

git -C "$SRC" rev-parse "$FIXED" > "$EVIDENCE/source-head.txt"
git -C "$SRC" rev-parse "$FIXED^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$SRC" rev-parse "$FIXED^" > "$EVIDENCE/source-parent.txt"
test "$(cat "$EVIDENCE/source-head.txt")" = "$FIXED"
test "$(cat "$EVIDENCE/source-tree.txt")" = "$TREE"
test "$(cat "$EVIDENCE/source-parent.txt")" = "$PARENT"

SNAP=$ROOT/final-snapshot
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive --format=tar "$FIXED" | tar -x -C "$SNAP"
test -f "$SNAP/contracts/source/pgsql/empty-catalog.json"
test ! -f "$SNAP/crates/oo-connectors/vendor/sqlx-postgres/src/connection/portal.rs"
python3 - <<'PY'
import hashlib, pathlib, shutil, sys
snap = pathlib.Path("/tmp/oo-a127-eef831aa/final-snapshot")
overlay = pathlib.Path("/tmp/oo-a127-draft/overlay")
expect = {
    "crates/oo-connectors/vendor/sqlx-postgres/src/connection/executor.rs": "721ead17743d912c625a56b5728669d8111c750797b6fee24e234226a5b6fbe6",
    "crates/oo-connectors/vendor/sqlx-postgres/src/connection/mod.rs": "a2057da12adbf73d83e20282bdcea21ce373d1b0c00f63595ace2124710afdf5",
    "crates/oo-connectors/vendor/sqlx-postgres/src/connection/establish.rs": "bd68ddaf75db5239b4701f03ff90efddf4259835ccb10c58e12454ecaadeb3a1",
    "crates/oo-connectors/vendor/sqlx-postgres/src/io/mod.rs": "5ffb01f5b17c83c11d10de097142c514b17e1bb8544d767d238539bcaeda9ab9",
    "crates/oo-connectors/vendor/sqlx-postgres/src/lib.rs": "bbf43d753b0f86b0b8105cac9bcf6ae282e81194ae26eb2856f67355529c0093",
    "crates/oo-connectors/vendor/PATCH.md": "24ea8ccbc3e3596b76891401bffecbd2679ca7ebaff949a2218dc5454c4c3720",
}
lines = []
for rel, want in expect.items():
    got = hashlib.sha256((snap / rel).read_bytes()).hexdigest()
    if got != want:
        sys.exit(f"pre-overlay hash mismatch {rel} {got}")
    lines.append(f"pre {rel} {got}")
owned = [
    "crates/oo-connectors/vendor/sqlx-postgres/src/connection/executor.rs",
    "crates/oo-connectors/vendor/sqlx-postgres/src/connection/mod.rs",
    "crates/oo-connectors/vendor/sqlx-postgres/src/connection/establish.rs",
    "crates/oo-connectors/vendor/sqlx-postgres/src/connection/portal.rs",
    "crates/oo-connectors/vendor/sqlx-postgres/src/io/mod.rs",
    "crates/oo-connectors/vendor/sqlx-postgres/src/lib.rs",
    "crates/oo-connectors/vendor/PATCH.md",
    "crates/oo-connectors/tests/pg_portal_bounds.rs",
    "crates/oo-connectors/tests/pg_portal_live.rs",
    "contracts/source/pgsql/named-portal-v1.md",
]
for rel in owned:
    src = overlay / rel
    dest = snap / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dest)
    digest = hashlib.sha256(dest.read_bytes()).hexdigest()
    lines.append(f"overlay {rel} {digest}")
out = pathlib.Path("/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a127/SP03/named-portal/linux/final/overlay-sha256.txt")
out.write_text("\n".join(lines) + "\n")
print("overlay_applied")
PY

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
test -f "$SNAP/crates/oo-connectors/vendor/sqlx-postgres/src/connection/portal.rs"

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
source_commit=${FIXED}
source_tree=${TREE}
source_parent=${PARENT}
snapshot_method=complete git archive of the fixed SHA plus exact overlay
prior_attempt=10
prior_attempt_exit=101
prior_attempt_cause=live error branch borrowed the reader while the transaction was still alive
snapshot=${SNAP}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
cargo_offline=yes
network=${NET}
postgres=oo-a127-pg
container=${NAME}
privileged=no
host_network=no
docker_socket=no
home_mount=no
EOF

docker run --name "$NAME" \
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
  -e OO_A127_FIXTURE=/secrets/pointer.txt \
  -e OO_A127_LOG_REPORT=/evidence/sql-log-class.txt \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a127 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a127 \
  --label oo.task=named-portal-final \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1 || code=$?
code=${code:-0}
printf '%s\n' "$code" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} user={{.Config.User}}' > "$EVIDENCE/hostconfig.txt" || true
exit "$code"
