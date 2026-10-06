#!/bin/bash
# Hash the reused recipe files with the std-only manifest tool.
# Stdout stays outside this evidence directory. Does not prune.
# Origin: a265 manifest launch
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a265/SP11/session-transition/linux/manifest/launch.sh
# Delta: owner a267, edition 2021, pids 2048, origin-file walk only.
set -u
if [ "$#" -ne 1 ]; then
  echo "usage: origins.sh <console-outside-evidence-dir>" >&2
  exit 2
fi
CONSOLE=$1
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite
EVIDENCE=${REVIEW}/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a267-manifest-origins
case "$CONSOLE" in
  "$REVIEW"/*)
    echo "console path is inside the evidence directory" >&2
    exit 2
    ;;
esac
if [ -f "$EVIDENCE/origins-console.log" ]; then
  echo "refusing to overwrite origins-console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
mkdir -p "$EVIDENCE"
own=$(docker ps -q --filter label=oo.owner=a267 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/origins-capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/origins-docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named origins container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/origins-docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/origins-image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc --edition 2021 -D warnings sha256_manifest.rs" > "$EVIDENCE/origins-invocation.txt"
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
  --pids-limit 2048 \
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${SRC},target=/repo,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a267 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a267 \
  --label oo.task=question-bank-suite-manifest-origins \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
set -u
rustc --edition 2021 -D warnings /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest
rustc_exit=$?
printf "rustc_exit=%s\n" "$rustc_exit"
if [ "$rustc_exit" -ne 0 ]; then exit "$rustc_exit"; fi
cmp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/tools/sha256_manifest.rs
printf "cmp_a199_exit=%s\n" "$?"
mkdir -p /tmp/origins
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh /tmp/origins/a204-preoverlay-launch.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh /tmp/origins/a204-preoverlay-inside.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh /tmp/origins/a200-smoke-launch.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs /tmp/origins/a199-sha256_manifest.rs
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/impl-5/launch.sh /tmp/origins/a256-impl5-launch.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/impl-5/inside.sh /tmp/origins/a256-impl5-inside.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/tools/sha256_manifest.rs /tmp/origins/a267-sha256_manifest.rs
/usr/local/bin/sha256_manifest /tmp/origins /tmp/origins.sha256
printf "manifest_exit=%s\n" "$?"
printf "rustc=%s\n" "$(rustc --version)"
' \
  > "$CONSOLE" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/origins-docker-run.exit"
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
} > "$EVIDENCE/origins-hostconfig.txt"
cp "$CONSOLE" "$EVIDENCE/origins-console.log"
docker rm "$NAME" > "$EVIDENCE/origins-container-removed.txt"
exit "$run_exit"
