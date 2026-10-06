#!/bin/bash
# Two builders, one at a time, network none. Std-only sha256_manifest.
# Origin: a288 linux/manifest/launch.sh. Inspect block follows pi-live-1.
# Does not prune. Starts no PostgreSQL. Does not cargo fetch.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a287/SP10/pi-harness
EVIDENCE=${REVIEW}/linux/hash-1
CRATE=${SRC}/tools/pi-harness
TOOL=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/tools/sha256_manifest.rs
ROOT=/tmp/oo-a287-eef831aa-dba4-4af2-a78b-541bc9e0a287
OUT=${ROOT}/hash-outside
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a287-hash
REFRESH=oo-a287-hash-refresh
EMPTY=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
ABC=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad

chmod 755 "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite hash-1 console.log" >&2
  exit 2
fi
if find "$REVIEW" "$CRATE" -type l | grep -q .; then
  echo "symlink refused before hash" >&2
  exit 2
fi
mkdir -p "$OUT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a287 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_task=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || exit 1
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
if docker container inspect "$NAME" >/dev/null 2>&1 || docker container inspect "$REFRESH" >/dev/null 2>&1; then
  echo "named hash container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc --edition 2024 -D warnings sha256_manifest.rs" > "$EVIDENCE/invocation.txt"
printf '%s\n' "docker start ${REFRESH} after docker create --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc --edition 2024 -D warnings sha256_manifest.rs" >> "$EVIDENCE/invocation.txt"

run_hash='export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /tool/sha256_manifest.rs -o /usr/local/bin/sha256_manifest >/tmp/rustc.out 2>/tmp/rustc.err || { cat /tmp/rustc.err >&2; exit 1; }; /usr/local/bin/sha256_manifest /src /evidence/source-pi-harness.sha256 && /usr/local/bin/sha256_manifest /evidence/origin /evidence/recipe-origin.sha256 && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256'
labels=(
  --label oo.project=open-ontology
  --label oo.owner=a287
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a287
  --label oo.toolchain=1.94.0
  --label oo.platform=linux/arm64
  --label oo.kind=rust-builder
)
common=(
  --platform linux/arm64
  --pull never
  --init
  --user 0:0
  --security-opt no-new-privileges
  --cap-drop ALL
  --memory 6g
  --memory-swap 6g
  --cpus 1
  --pids-limit 4096
  --network none
  -e HOME=/tmp
  -e RUSTUP_TOOLCHAIN=1.94.0
  --mount "type=bind,source=${CRATE},target=/src,readonly"
  --mount "type=bind,source=${REVIEW},target=/evidence"
  --mount "type=bind,source=${TOOL},target=/tool/sha256_manifest.rs,readonly"
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m
)

set +e
docker run --name "$NAME" "${common[@]}" \
  --label oo.task=pi-harness-hash \
  "${labels[@]}" \
  "$IMAGE" \
  bash -c "$run_hash" > "$OUT/first-console.txt" 2> "$OUT/first-stderr.txt"
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
cp "$OUT/first-console.txt" "$EVIDENCE/console.log"
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
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
if [ "$run_exit" -ne 0 ]; then
  echo "first hash container failed" >&2
  exit "$run_exit"
fi

cleanup_exit=0
{
  echo "volume-rm-output is under the private tmp hash-outside directory and is removed with that directory after the rewrite compare"
  for vol in oo-a287-eef831aa-cargo oo-a287-eef831aa-target oo-a287-eef831aa-harness-target oo-a287-eef831aa-pi-prefix oo-a287-eef831aa-npm-cache; do
    docker volume rm "$vol" > "$OUT/volume-${vol}.txt" 2>&1
    rc=$?
    echo "volume-rm ${vol} exit=${rc}"
    if [ "$rc" -ne 0 ]; then
      cleanup_exit=1
    fi
  done
  docker network rm oo-a287-eef831aa-net > "$OUT/network-rm.txt" 2>&1
  rc=$?
  echo "network-rm oo-a287-eef831aa-net exit=${rc}"
  if [ "$rc" -ne 0 ]; then
    cleanup_exit=1
  fi
  echo "left-volumes-begin"
  docker volume ls --format '{{.Name}}' | awk '/^oo-a287-/ {print}'
  echo "left-volumes-end"
  echo "left-networks-begin"
  docker network ls --format '{{.Name}}' | awk '/^oo-a287-/ {print}'
  echo "left-networks-end"
  echo "tmp-children-before-snapshot-rm:"
  find "$ROOT" -mindepth 1 -maxdepth 1 -print
  find "$ROOT" -mindepth 1 -maxdepth 1 ! -name hash-outside -exec rm -rf {} +
  rc=$?
  echo "snapshot-rm exit=${rc}"
  if [ "$rc" -ne 0 ]; then
    cleanup_exit=1
  fi
  echo "tmp-children-after-snapshot-rm:"
  find "$ROOT" -mindepth 1 -maxdepth 1 -print
  echo "docker-prune=not-run"
  echo "hash-outside remains until the rewrite compare; launch.sh then removes ${ROOT}. That removal is outside this file."
} > "$REVIEW/CLEANUP.txt"
if [ "$cleanup_exit" -ne 0 ]; then
  echo "cleanup failed" >&2
  exit 1
fi
if docker volume ls --format '{{.Name}}' | awk '/^oo-a287-/ {found=1} END{exit !found}'; then
  echo "a287 volume still present" >&2
  exit 1
fi
if docker network ls --format '{{.Name}}' | awk '/^oo-a287-/ {found=1} END{exit !found}'; then
  echo "a287 network still present" >&2
  exit 1
fi

own=$(docker ps -q --filter label=oo.owner=a287 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'refresh own=%s global=%s host_cargo=%s cap_global=10 cap_task=1\n' "$own" "$global_builders" "$host_cargo" >> "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: refresh builder capacity" >&2
  exit 3
fi

rewrite='export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /tool/sha256_manifest.rs -o /usr/local/bin/sha256_manifest >/tmp/rustc.out 2>/tmp/rustc.err || { cat /tmp/rustc.err >&2; exit 1; }; /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256'
docker create --name "$REFRESH" "${common[@]}" \
  --label oo.task=pi-harness-hash-refresh \
  "${labels[@]}" \
  "$IMAGE" \
  bash -c "$rewrite" > "$OUT/refresh-create.txt"
create_exit=$?
if [ "$create_exit" -ne 0 ]; then
  echo "docker create refresh failed" >&2
  exit "$create_exit"
fi
docker inspect -f '{{.Id}}' "$REFRESH" > "$EVIDENCE/refresh-id.txt"

n=$(find "$REVIEW" -type f ! -path "$REVIEW/MANIFEST.sha256" | wc -l | tr -d ' ')
N=$((n + 1))
printf 'files=%s\nempty_sha256=%s\nabc_sha256=%s\n' "$N" "$EMPTY" "$ABC" > "$EVIDENCE/tool-stdout.txt"
n2=$(find "$REVIEW" -type f ! -path "$REVIEW/MANIFEST.sha256" | wc -l | tr -d ' ')
if [ "$n2" != "$N" ]; then
  docker rm "$REFRESH" > "$OUT/refresh-removed-predict.txt" 2>&1
  echo "predicted file count ${N} but found ${n2}" >&2
  exit 2
fi
find "$REVIEW" -type f ! -path "$REVIEW/MANIFEST.sha256" | sort > "$OUT/file-list.txt"
echo "predict files=${N}"

set +e
docker start -a "$REFRESH" > "$OUT/refresh-stdout.txt" 2> "$OUT/refresh-stderr.txt"
refresh_exit=$?
set +e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$REFRESH")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$REFRESH")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$REFRESH")"
} > "$OUT/refresh-hostconfig.txt"
docker rm "$REFRESH" > "$OUT/refresh-removed.txt"
if [ "$refresh_exit" -ne 0 ]; then
  echo "rewrite container failed" >&2
  exit "$refresh_exit"
fi
if ! cmp -s "$EVIDENCE/tool-stdout.txt" "$OUT/refresh-stdout.txt"; then
  echo "rewrite stdout does not match tool-stdout.txt" >&2
  echo "---- expected ----" >&2
  cat "$EVIDENCE/tool-stdout.txt" >&2
  echo "---- actual ----" >&2
  cat "$OUT/refresh-stdout.txt" >&2
  exit 1
fi
rm -rf "$ROOT"
rm_exit=$?
if [ "$rm_exit" -ne 0 ]; then
  echo "tmp removal failed" >&2
  exit "$rm_exit"
fi
if [ -e "$ROOT" ]; then
  echo "tmp still present" >&2
  exit 1
fi
exit 0
