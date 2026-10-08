#!/bin/bash
# Run the compiled loop after the builder container is gone. One container, no compile.
set -euo pipefail
REPO=/Users/redux/puppy-one/open-ontology
BASE=${REPO}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a399/SP09/pack-dialogue-compat
BINS=/tmp/oo-a399-eef831aa-dba4-4af2-a78b-541bc9e0a399/bins
SEC=/tmp/oo-a399-eef831aa-dba4-4af2-a78b-541bc9e0a399/client-secrets
TOKEN=/tmp/oo-customer-demo-20261007/llm-token
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
mkdir -p "$BASE/live"
if docker ps -q --filter name=^/oo-a399-build$ --filter status=running | grep -q .; then
  echo "builder still running" >&2
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a399 --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
if [ "$own" -ge 1 ]; then
  echo "own builder still running" >&2
  exit 3
fi
umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
cp /tmp/oo-a399-eef831aa-dba4-4af2-a78b-541bc9e0a399/secrets/ca.crt "$SEC/ca.crt"
cp /tmp/oo-a399-eef831aa-dba4-4af2-a78b-541bc9e0a399/secrets/admin.pass "$SEC/admin.pass"
cp /tmp/oo-a399-eef831aa-dba4-4af2-a78b-541bc9e0a399/secrets/login.pass "$SEC/login.pass"
chmod 644 "$SEC/ca.crt"
chmod 600 "$SEC/admin.pass" "$SEC/login.pass"
test -f "$BINS/query-pg"
test -f "$BINS/ontology-dialogue"
test -f "$TOKEN"
set +e
docker run --name oo-a399-loop \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 2g \
  --memory-swap 2g \
  --cpus 1 \
  --pids-limit 256 \
  --network oo-a399-eef831aa-net \
  --add-host host.docker.internal:host-gateway \
  -e HOME=/tmp \
  --mount "type=bind,source=${BASE},target=/evidence" \
  --mount "type=bind,source=${BINS},target=/bins,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${TOKEN},target=/token/llm-token,readonly" \
  --tmpfs /work:rw,nosuid,nodev,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a399 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a399 \
  --label oo.kind=loop \
  --label oo.task=pack-dialogue-compat \
  "$IMAGE" \
  bash /evidence/live/loop.sh > "$BASE/live/console.log" 2>&1
loop_code=$?
set -e
printf '%s\n' "$loop_code" > "$BASE/live/docker-run.exit"
docker inspect oo-a399-loop --format 'id={{.Id}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}}' > "$BASE/live/inspect.txt"
docker rm oo-a399-loop >/dev/null
echo removed > "$BASE/live/container-removed.txt"
