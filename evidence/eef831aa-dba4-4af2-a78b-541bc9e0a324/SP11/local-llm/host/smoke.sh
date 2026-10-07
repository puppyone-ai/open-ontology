#!/bin/bash
# Host relay plus one models call. Token bytes stay in the private file.
# Docker reachability uses the Linux client and the public certificate only.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/host
ROOT=/tmp/oo-a324-eef831aa-dba4-4af2-a78b-541bc9e0a324/host
TOKEN=/tmp/oo-customer-demo-20261007/llm-token
LISTEN=127.0.0.1:18317
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a324-eef831aa-net
TARGET_VOL=oo-a324-eef831aa-target
RELAY="$ROOT/target/debug/oo-local-llm-relay"
MODELS="$ROOT/target/debug/oo-local-llm-models"
mkdir -p "$EVIDENCE" "$ROOT/cert"
if [ -f "$EVIDENCE/smoke.exit" ]; then
  echo "refusing to overwrite host smoke.exit" >&2
  exit 2
fi
if [ ! -x "$RELAY" ] || [ ! -x "$MODELS" ]; then
  echo "host binaries missing" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
if lsof -nP -iTCP:18317 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "18317 already listening" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
umask 077
{
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
    -keyout "$ROOT/cert/ca.key" -out "$ROOT/cert/ca.pem" -days 1 -nodes \
    -subj "/CN=oo-a324-ca" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign"
  printf 'subjectAltName=IP:127.0.0.1,DNS:host.docker.internal\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature\nextendedKeyUsage=serverAuth\n' > "$ROOT/cert/leaf.ext"
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
    -keyout "$ROOT/cert/leaf.key" -out "$ROOT/cert/leaf.csr" -nodes \
    -subj "/CN=127.0.0.1"
  openssl x509 -req -in "$ROOT/cert/leaf.csr" -CA "$ROOT/cert/ca.pem" -CAkey "$ROOT/cert/ca.key" \
    -CAcreateserial -out "$ROOT/cert/leaf.pem" -days 1 -extfile "$ROOT/cert/leaf.ext"
} > "$EVIDENCE/openssl.stdout" 2> "$EVIDENCE/openssl.stderr"
openssl_code=$?
printf '%s\n' "$openssl_code" > "$EVIDENCE/openssl.exit"
if [ "$openssl_code" -ne 0 ]; then
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
chmod 600 "$ROOT/cert/ca.key" "$ROOT/cert/leaf.key"
cp "$ROOT/cert/ca.pem" "$EVIDENCE/ca.pem"
cp "$ROOT/cert/leaf.pem" "$EVIDENCE/leaf.pem"
shasum -a 256 "$EVIDENCE/ca.pem" "$EVIDENCE/leaf.pem" > "$EVIDENCE/cert.sha256"
openssl x509 -in "$EVIDENCE/leaf.pem" -noout -ext subjectAltName > "$EVIDENCE/cert-san.txt"
# Metadata only. Do not read the token.
if [ -f "$TOKEN" ]; then
  stat -f 'mode=%OLp bytes=%z' "$TOKEN" > "$EVIDENCE/token-meta.txt"
else
  echo "absent" > "$EVIDENCE/token-meta.txt"
fi
"$RELAY" --listen "$LISTEN" --cert "$ROOT/cert/leaf.pem" --key "$ROOT/cert/leaf.key" \
  > "$EVIDENCE/relay.stdout" 2> "$EVIDENCE/relay.stderr" &
relay_pid=$!
printf '%s\n' "$relay_pid" > "$EVIDENCE/relay.pid"
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
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
set +e
if [ -f "$TOKEN" ]; then
  "$MODELS" --origin "https://127.0.0.1:18317" --ca "$EVIDENCE/ca.pem" \
    --bearer-file "$TOKEN" --out "$EVIDENCE/models.json" --chat-out "$EVIDENCE/chat.json" \
    > "$EVIDENCE/models.stdout" 2> "$EVIDENCE/models.stderr"
  models_code=$?
else
  "$MODELS" --origin "https://127.0.0.1:18317" --ca "$EVIDENCE/ca.pem" \
    --out "$EVIDENCE/models.json" \
    > "$EVIDENCE/models.stdout" 2> "$EVIDENCE/models.stderr"
  models_code=$?
fi
printf '%s\n' "$models_code" > "$EVIDENCE/models.exit"
# Docker client: public cert and Linux binary only. No token, no key, no cargo.
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
  > "$EVIDENCE/docker-reach.stdout" 2> "$EVIDENCE/docker-reach.stderr"
docker_code=$?
printf '%s\n' "$docker_code" > "$EVIDENCE/docker-reach.exit"
docker rm oo-a324-reach >/dev/null 2>&1 || true
kill "$relay_pid" 2>/dev/null || true
wait "$relay_pid" 2>/dev/null || true
if [ -s "$EVIDENCE/models.json" ] && [ -s "$EVIDENCE/docker-models.json" ]; then
  smoke_code=0
else
  smoke_code=1
fi
printf '%s\n' "$smoke_code" > "$EVIDENCE/smoke.exit"
exit "$smoke_code"
