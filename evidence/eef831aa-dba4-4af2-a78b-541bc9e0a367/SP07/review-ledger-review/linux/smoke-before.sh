#!/bin/bash
# Pre-edit toolchain smoke. No repo mount. No login shell. No pull.
# Recipe shape: a366 evidence/eef831aa-dba4-4af2-a78b-541bc9e0a366/SP08/release-coordinator-review/linux/smoke-before.sh
set -euo pipefail
EVID=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a367/SP07/review-ledger-review
OUT=$EVID/linux/smoke-before
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a367-smoke
mkdir -p "$OUT"
global=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a367 --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
printf 'own=%s global=%s\n' "$own" "$global" | tee "$OUT/capacity-at-launch.txt"
if [ "$global" -ge 10 ] || [ "$own" -ge 1 ]; then
  echo "builder capacity exceeded" >&2
  printf '%s\n' 3 > "$OUT/docker-run.exit"
  exit 3
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} arch={{.Architecture}}' | tee "$OUT/image.txt"
docker rm -f "$NAME" >/dev/null 2>&1 || true
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --network none \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=64m \
  -e HOME=/tmp \
  -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e CARGO_HOME=/usr/local/cargo \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --label oo.project=open-ontology \
  --label oo.owner=a367 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a367 \
  --label oo.kind=rust-builder \
  --label oo.task=review-ledger-review-smoke \
  --entrypoint /bin/bash \
  "$IMAGE" \
  -c 'rustc --version; cargo --version; uname -m; rustup show active-toolchain' \
  > "$OUT/console.log" 2> "$OUT/stderr.txt"
code=$?
set -e
printf '%s\n' "$code" > "$OUT/docker-run.exit"
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} exit={{.State.ExitCode}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}} network={{.HostConfig.NetworkMode}}' > "$OUT/hostconfig.txt"
docker rm -f "$NAME" > "$OUT/container-removed.txt"
exit "$code"
