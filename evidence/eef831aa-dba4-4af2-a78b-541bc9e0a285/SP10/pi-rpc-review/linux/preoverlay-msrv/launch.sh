#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable git archive of c551afea.
# The spike bytes at that commit are the b0cc1d24 fix. Network none. 6g. 1 CPU.
# Origin recipes are copied into origin-files/ and hashed inside the container.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a285/SP10/pi-rpc-review/linux/preoverlay-msrv
ROOT=/tmp/oo-a285-eef831aa-dba4-4af2-a78b-541bc9e0a285
FIXED=c551afea55d5a9164760878a5d6a76835b869875
FIX=b0cc1d24149f172d50cbcc7319bd7e6cc1aa1679
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a285-preoverlay-msrv
CARGO_VOL=oo-a285-eef831aa-cargo
TARGET_VOL=oo-a285-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite preoverlay-msrv console.log" >&2
  exit 2
fi
mkdir -p "$ROOT" "$EVIDENCE/origin-files"
rm -rf "$ROOT/archive" "$EVIDENCE/spike"
mkdir -p "$ROOT/archive" "$EVIDENCE/spike"
if ! git -C "$SRC" cat-file -e "${FIXED}:testing/spikes/p09-pi-rpc/Cargo.toml"; then
  echo "fixed commit has no spike Cargo.toml" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" archive --format=tar "$FIXED" testing/spikes/p09-pi-rpc | tar -x -C "$ROOT/archive"
cp -a "$ROOT/archive/testing/spikes/p09-pi-rpc/." "$EVIDENCE/spike/"
if [ ! -f "$EVIDENCE/spike/tests/boundary.rs" ]; then
  echo "archive is missing boundary.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
printf '%s\n' "$FIX" > "$EVIDENCE/fix-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
set +e
git -C "$SRC" merge-base --is-ancestor "$FIX" "$FIXED"
printf '%s\n' "$?" > "$EVIDENCE/ancestor.exit"
git -C "$SRC" diff --exit-code "$FIX" "$FIXED" -- testing/spikes/p09-pi-rpc > "$EVIDENCE/spike-diff.stdout" 2> "$EVIDENCE/spike-diff.stderr"
printf '%s\n' "$?" > "$EVIDENCE/spike-diff.exit"
git -C "$SRC" show "${FIX}:testing/spikes/p09-pi-rpc/src/drive.rs" > "$EVIDENCE/drive-from-b0cc1d24.rs"
cmp "$EVIDENCE/spike/src/drive.rs" "$EVIDENCE/drive-from-b0cc1d24.rs" > "$EVIDENCE/drive-cmp.stdout" 2> "$EVIDENCE/drive-cmp.stderr"
printf '%s\n' "$?" > "$EVIDENCE/drive-cmp.exit"
set -e
if [ "$(cat "$EVIDENCE/ancestor.exit")" != 0 ] || [ "$(cat "$EVIDENCE/spike-diff.exit")" != 0 ] || [ "$(cat "$EVIDENCE/drive-cmp.exit")" != 0 ]; then
  echo "archive is not the b0cc1d24 spike integrated at c551afea" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
copy_origin() {
  cp "$2" "$EVIDENCE/origin-files/$1"
  printf '%s %s\n' "$1" "$2" >> "$EVIDENCE/origin-files/SOURCES.txt"
}
: > "$EVIDENCE/origin-files/SOURCES.txt"
copy_origin a204-launch.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay/launch.sh"
copy_origin a204-inside.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay/inside.sh"
copy_origin a200-launch.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
copy_origin a200-inside.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/inside.sh"
copy_origin a199-launch.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh"
copy_origin a199-inside.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/inside.sh"
copy_origin a282-preedit-launch.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/preedit-boundary/launch.sh"
copy_origin a282-preedit-inside.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/preedit-boundary/inside.sh"
copy_origin a282-clippy-launch.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/clippy-after-1/launch.sh"
copy_origin a282-clippy-inside.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/clippy-after-1/inside.sh"
copy_origin a282-reinstall-launch.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/pi-reinstall/launch.sh"
copy_origin a282-reinstall-inside.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/pi-reinstall/inside.sh"
copy_origin a282-live-launch.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/live-rerun/launch.sh"
copy_origin a282-live-inside.sh "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/live-rerun/inside.sh"
copy_origin a204-sha256_manifest.rs "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/tools/sha256_manifest.rs"
copy_origin a279-mock_openai.rs "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a279/SP10/pi-rpc-probe/linux/pi-live-src/mock_openai.rs"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a285 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a285 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a285 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a285 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a285 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named preoverlay container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  --network none \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${EVIDENCE}/spike,target=/work,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/proof" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a285/SP10/pi-rpc-review/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/origin-files,target=/origins,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a279/SP10/pi-rpc-probe/linux/adapter-proof-4,target=/historical/a279,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix/linux/hash-verify,target=/historical/a282,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a285 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a285 \
  --label oo.task=pi-rpc-review-preoverlay-msrv \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
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
exit "$run_exit"
