#!/bin/bash
# Docker TLS reachability. Public CA only. No token, no private key, no cargo.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/host
ROOT=/tmp/oo-a324-eef831aa-dba4-4af2-a78b-541bc9e0a324/host
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a324-eef831aa-net
TARGET_VOL=oo-a324-eef831aa-target
RELAY="$ROOT/target/debug/oo-local-llm-relay"
if [ -f "$EVIDENCE/reach.exit" ]; then
  echo "refusing to overwrite reach.exit" >&2
  exit 2
fi
if lsof -nP -iTCP:18317 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "18317 already listening" >&2
  printf '%s\n' 1 > "$EVIDENCE/reach.exit"
  exit 1
fi
"$RELAY" --listen 127.0.0.1:18317 --cert "$ROOT/cert/leaf.pem" --key "$ROOT/cert/leaf.key" \
  > "$EVIDENCE/reach-relay.stdout" 2> "$EVIDENCE/reach-relay.stderr" &
relay_pid=$!
printf '%s\n' "$relay_pid" > "$EVIDENCE/reach-relay.pid"
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if lsof -nP -iTCP:18317 -sTCP:LISTEN >/dev/null 2>&1; then
    ready=1
    break
  fi
  if ! kill -0 "$relay_pid" 2>/dev/null; then
    break
  fi
  sleep 0.2
done
if [ "$ready" -ne 1 ]; then
  kill "$relay_pid" 2>/dev/null || true
  echo "relay did not listen" >&2
  printf '%s\n' 1 > "$EVIDENCE/reach.exit"
  exit 1
fi
set +e
docker run --name oo-a324-reach \
  --platform linux/arm64 \
  --pull never \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 256m \
  --memory-swap 256m \
  --cpus 1 \
  --pids-limit 128 \
  --network "$NET" \
  --mount "type=volume,source=${TARGET_VOL},target=/target,readonly" \
  --mount "type=bind,source=${EVIDENCE}/ca.pem,target=/ca.pem,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a324 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a324 \
  --label oo.kind=reachability \
  "$IMAGE" \
  /target/debug/oo-local-llm-models --origin "https://host.docker.internal:18317" --ca /ca.pem --out /evidence/docker-models.json \
  > "$EVIDENCE/docker-reach-2.stdout" 2> "$EVIDENCE/docker-reach-2.stderr"
docker_code=$?
printf '%s\n' "$docker_code" > "$EVIDENCE/docker-reach-2.exit"
docker rm oo-a324-reach >/dev/null 2>&1 || true
kill "$relay_pid" 2>/dev/null || true
wait "$relay_pid" 2>/dev/null || true
if [ -s "$EVIDENCE/docker-models.json" ]; then
  printf '%s\n' 0 > "$EVIDENCE/reach.exit"
  exit 0
fi
printf '%s\n' 1 > "$EVIDENCE/reach.exit"
exit 1
