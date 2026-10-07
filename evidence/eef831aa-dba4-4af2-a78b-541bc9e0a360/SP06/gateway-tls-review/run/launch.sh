#!/bin/bash
# Independent E48 review. One private PostgreSQL 17.10 and one 6GiB arm64 builder.
# Does not prune, pull, or write product files.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a360/SP06/gateway-tls-review
ATTEMPT=${1:-smoke}
case "$ATTEMPT" in
  smoke|smoke-2|smoke-3) ;;
  *) echo "attempt must be smoke, smoke-2, or smoke-3" >&2; exit 2 ;;
esac
EVIDENCE=${BASE}/linux/${ATTEMPT}
ROOT=/tmp/oo-a360-eef831aa-dba4-4af2-a78b-541bc9e0a360/${ATTEMPT}
SEC=/tmp/oo-a360-eef831aa-dba4-4af2-a78b-541bc9e0a360/secrets
FIXTURE=/tmp/oo-a360-eef831aa-dba4-4af2-a78b-541bc9e0a360/builder-fixture
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
COMMIT=97b6a9eb9caa2b6059d82bdb98b703bf5ee202d8
NAME=oo-a360-${ATTEMPT}
PG_NAME=oo-a360-pg
NET=oo-a360-eef831aa-net
CARGO_VOL=oo-a360-eef831aa-cargo
TARGET_VOL=oo-a360-eef831aa-target
PG_VOL=oo-a360-eef831aa-pgdata
ORIGIN_EXTRACT=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/extract-libclang.sh
ORIGIN_TOOLS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools

chmod 755 "$BASE/run/inside.sh" "$BASE/run/launch.sh" "$BASE/pg/up.sh" "$BASE/pg/pg-wrap.sh" "$BASE/pg/init-roles.sh"
mkdir -p "$EVIDENCE"
trap 'code=$?; if [ ! -f "$EVIDENCE/docker-run.exit" ]; then printf "%s\n" "$code" > "$EVIDENCE/docker-run.exit"; fi' EXIT
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite ${ATTEMPT} console.log" >&2
  exit 2
fi

finish_pg() {
  docker rm -f "$PG_NAME" >/dev/null 2>&1 || true
  docker volume rm "$PG_VOL" >/dev/null 2>&1 || true
  if [ -d "$SEC" ]; then
    python3 - "$BASE" "$SEC" <<'PY'
import pathlib, re, sys
root = pathlib.Path(sys.argv[1])
sec = pathlib.Path(sys.argv[2])
needles = []
for path in sec.iterdir():
    if path.is_file() and path.stat().st_size <= 128:
        text = path.read_bytes().strip()
        if len(text) >= 16:
            needles.append(text)
pattern = re.compile(rb"ooct_[A-Za-z0-9._:-]+|oost_[A-Za-z0-9._:-]+")
skip = {"source-sha256.txt", "lock-sha256.txt", "origin-sha256.txt", "archive-sha256.txt", "work-sha256.txt", "review-test-sha256.txt"}
folders = []
linux = root / "linux"
if linux.is_dir():
    folders.extend(path for path in linux.iterdir() if path.is_dir())
pg = root / "pg"
if pg.is_dir():
    folders.append(pg)
for folder in folders:
    for path in folder.rglob("*"):
        if not path.is_file() or path.name in skip:
            continue
        if path.suffix not in {".txt", ".log", ".stdout", ".stderr", ".json"} and path.name != "console.log":
            continue
        data = path.read_bytes()
        updated = pattern.sub(lambda m: m.group(0).split(b"_", 1)[0] + b"_REDACTED", data)
        for needle in needles:
            updated = updated.replace(needle, b"REDACTED")
        if updated != data:
            path.write_bytes(updated)
PY
  fi
  rm -rf "$SEC" "$FIXTURE"
}

capacity() {
  own=$(docker ps -q --filter label=oo.owner=a360 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
  printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo"
  if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
    return 3
  fi
  return 0
}

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
cap_line=$(capacity) || cap_rc=$?
cap_rc=${cap_rc:-0}
printf '%s\n' "$cap_line" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$cap_rc" -ne 0 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store testing/real-db/gateway-compiled-pack \
  | tar -x -C "$ROOT/archive"
printf '%s\n' "$COMMIT" > "$EVIDENCE/archive-commit.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
{
  git -C "$SRC" rev-parse "${COMMIT}:crates/oo-gateway/src/http_gateway.rs"
  git -C "$SRC" rev-parse "${COMMIT}:crates/oo-gateway/tests/http_gateway_tls.rs"
  git -C "$SRC" rev-parse "${COMMIT}:crates/oo-gateway/src/bin/oo-gateway.rs"
  git -C "$SRC" rev-parse "${COMMIT}:Cargo.lock"
} > "$EVIDENCE/expected-blobs.txt"
parent_lock=$(git -C "$SRC" rev-parse "${COMMIT}^:Cargo.lock")
commit_lock=$(git -C "$SRC" rev-parse "${COMMIT}:Cargo.lock")
printf 'commit=%s\nparent=%s\n' "$commit_lock" "$parent_lock" > "$EVIDENCE/lock-pins.txt"
if [ "$parent_lock" != "$commit_lock" ]; then
  echo "Cargo.lock changed in the reviewed commit" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" hash-object \
  "$ROOT/archive/crates/oo-gateway/src/http_gateway.rs" \
  "$ROOT/archive/crates/oo-gateway/tests/http_gateway_tls.rs" \
  "$ROOT/archive/crates/oo-gateway/src/bin/oo-gateway.rs" \
  "$ROOT/archive/Cargo.lock" \
  > "$EVIDENCE/archive-blobs.txt"
if ! cmp -s "$EVIDENCE/expected-blobs.txt" "$EVIDENCE/archive-blobs.txt"; then
  echo "archive blobs do not match ${COMMIT}" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
sha256sum \
  "$ROOT/archive/crates/oo-gateway/src/http_gateway.rs" \
  "$ROOT/archive/crates/oo-gateway/src/bin/oo-gateway.rs" \
  "$ROOT/archive/crates/oo-gateway/tests/http_gateway_tls.rs" \
  "$ROOT/archive/crates/oo-gateway/Cargo.toml" \
  "$ROOT/archive/Cargo.toml" \
  "$ROOT/archive/Cargo.lock" \
  > "$EVIDENCE/archive-sha256.txt"
sha256sum "$ORIGIN_EXTRACT" "$ORIGIN_TOOLS/sha256.rs" "$ORIGIN_TOOLS/select-debs.rs" \
  "$BASE/pg/pg-wrap.sh" "$BASE/pg/up.sh" "$BASE/pg/init-roles.sh" "$BASE/tools/review_tls.rs" \
  > "$EVIDENCE/origin-sha256.txt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"

if ! bash "$BASE/pg/up.sh"; then
  echo "postgres setup failed" >&2
  finish_pg
  if [ -f "$BASE/pg/up.exit" ]; then
    cp "$BASE/pg/up.exit" "$EVIDENCE/docker-run.exit"
  else
    printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  fi
  exit 1
fi

cap_line=$(capacity) || cap_rc=$?
cap_rc=${cap_rc:-0}
printf '%s\n' "$cap_line" > "$EVIDENCE/capacity-before-rust.txt"
if [ "$cap_rc" -ne 0 ]; then
  echo "deferred: rust builder capacity after postgres start" >&2
  finish_pg
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 archive=${COMMIT} bash /inside.sh" > "$EVIDENCE/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A350_LIVE=1 \
  -e OO_A350_PG_HOST=oo-a360-pg \
  -e OO_A350_PG_PORT=5432 \
  -e OO_A350_PG_DATABASE=oo_a360 \
  -e OO_A350_ADMIN_PASSWORD_FILE=/pg-fixture/token-admin.pass \
  -e OO_A350_RUNTIME_PASSWORD_FILE=/pg-fixture/token-runtime.pass \
  -e OO_A350_CA_FILE=/pg-fixture/ca.crt \
  -e OO_A350_MARKER=/evidence/a350-live-marker.txt \
  -e OO_A350_FIXTURE_DIR=/target/work/testing/real-db/gateway-compiled-pack/fixtures \
  -e OO_A360_LIVE=1 \
  -e OO_A360_PG_HOST=oo-a360-pg \
  -e OO_A360_PG_PORT=5432 \
  -e OO_A360_PG_DATABASE=oo_a360 \
  -e OO_A360_ADMIN_PASSWORD_FILE=/pg-fixture/token-admin.pass \
  -e OO_A360_RUNTIME_PASSWORD_FILE=/pg-fixture/token-runtime.pass \
  -e OO_A360_CA_FILE=/pg-fixture/ca.crt \
  -e OO_A360_MARKER=/evidence/a360-review-marker.txt \
  -e OO_A360_SUMMARY=/evidence/review-summary.txt \
  -e OO_A360_FIXTURE_DIR=/target/work/testing/real-db/gateway-compiled-pack/fixtures \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${BASE}/run/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${BASE}/tools/review_tls.rs,target=/review/review_tls.rs,readonly" \
  --mount "type=bind,source=${ORIGIN_EXTRACT},target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${ORIGIN_TOOLS},target=/tools,readonly" \
  --mount "type=bind,source=${FIXTURE},target=/pg-fixture,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a360 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a360 \
  --label oo.task=gateway-tls-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
if docker container inspect "$NAME" >/dev/null 2>&1; then
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
    echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
    echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
  } > "$EVIDENCE/hostconfig.txt"
  docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
  docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
  docker rm -f "$NAME" > "$EVIDENCE/container-removed.txt"
fi
finish_pg
if [ "$run_exit" -eq 0 ]; then
  docker volume rm "$CARGO_VOL" > "$EVIDENCE/cargo-volume-removed.txt" 2>&1 || true
  docker volume rm "$TARGET_VOL" > "$EVIDENCE/target-volume-removed.txt" 2>&1 || true
  docker network rm "$NET" > "$EVIDENCE/network-removed.txt" 2>&1 || true
fi
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
exit "$run_exit"
