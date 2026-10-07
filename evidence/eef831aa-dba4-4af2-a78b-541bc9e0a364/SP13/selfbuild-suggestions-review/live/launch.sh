#!/bin/bash
# One builder runs the sealed command through the compiled query-pg binary.
set -u
SRC=/Users/redux/puppy-one/open-ontology
ROOT=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a364/SP13/selfbuild-suggestions-review
EVIDENCE=${ROOT}/live
PROBE=${ROOT}/probe
GOLD=${ROOT}/gold
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a364-sbs-live
NET=oo-a364-sbs-net
SBS_TARGET=oo-a364-sbs-target
GW_TARGET=oo-a364-gw-target
SEC=/tmp/oo-a364-sbs-secrets
ORIGIN_SHA=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/sha256.rs

own=$(docker ps -q --filter label=oo.owner=a364 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" | tee "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if ! docker ps --filter name=^/oo-a364-pg$ --filter status=running --format '{{.ID}}' | grep -q .; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$ROOT/build/live-command.json" "$EVIDENCE/live-command.json"
cp "$ROOT/pg/apply.txt" "$EVIDENCE/apply.txt"
chmod 755 "$EVIDENCE/wrapper.sh"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named builder already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94" > "$EVIDENCE/invocation.txt"
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
  --label oo.kind=rust-builder \
  --label oo.owner=a364 \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e "OO_A364_JUDGE_ONLY=${OO_A364_JUDGE_ONLY:-0}" \
  -v "${SBS_TARGET}:/sbs-target:ro" \
  -v "${GW_TARGET}:/gw-target:ro" \
  -v "${SEC}:/secrets:ro" \
  -v "${GOLD}:/gold:ro" \
  -v "${PROBE}:/probe:ro" \
  -v "${ORIGIN_SHA}:/tools/sha256.rs:ro" \
  -v "${EVIDENCE}:/out" \
  "$IMAGE" \
  bash /out/inside.sh
rc=$?
docker inspect "$NAME" --format 'exit={{.State.ExitCode}} oom={{.State.OOMKilled}} status={{.State.Status}}' > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' "$rc" > "$EVIDENCE/docker-run.exit"
exit "$rc"
