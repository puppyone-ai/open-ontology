#!/bin/bash
# One loopback relay and one observer. One models GET. No chat completion here.
# Upstream token bytes stay in the file the relay reads. They are not arguments.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a346/SP11/actual-model-receipt-review/host
ROOT=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346/host
TOKEN=/tmp/oo-customer-demo-20261007/llm-token
CLIENT=$ROOT/client-token
RELAY=$ROOT/bin/oo-local-llm-relay
MODELS=$ROOT/bin/oo-local-llm-models
OBSERVE=$ROOT/bin/a346-observe
A334=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/host-3/smoke.sh
if [ -x /opt/homebrew/opt/openssl@3/bin/openssl ]; then
  OPENSSL=/opt/homebrew/opt/openssl@3/bin/openssl
else
  echo "openssl 3 is required" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi

mkdir -p "$EVIDENCE" "$ROOT/cert" "$ROOT/observe-out"
chmod 755 "$EVIDENCE/listen.sh"
if [ -f "$EVIDENCE/smoke.exit" ]; then
  echo "refusing to overwrite host smoke.exit" >&2
  exit 2
fi
if [ ! -x "$RELAY" ] || [ ! -x "$MODELS" ] || [ ! -x "$OBSERVE" ]; then
  echo "host binaries missing" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
if [ ! -f "$EVIDENCE/build.exit" ] || [ "$(cat "$EVIDENCE/build.exit")" != 0 ]; then
  echo "host build is not green" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
if lsof -nP -iTCP:18446 -sTCP:LISTEN >/dev/null 2>&1 || lsof -nP -iTCP:18447 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "18446 or 18447 is already listening" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
{
  echo "a334 host-3 smoke ${A334} $(shasum -a 256 "$A334" | awk '{print $1}')"
  echo "changes ports 18446 relay and 18447 observer models-get-only no-chat-out"
} > "$EVIDENCE/listen-origins.txt"

for name in HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE; do
  if eval "[ -n \"\${${name}+x}\" ]"; then
    unset "$name"
  fi
done

umask 077
{
  "$OPENSSL" req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
    -keyout "$ROOT/cert/ca.key" -out "$ROOT/cert/ca.pem" -days 1 -nodes \
    -subj "/CN=oo-a346-ca" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign"
  printf 'subjectAltName=IP:127.0.0.1,DNS:host.docker.internal\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature\nextendedKeyUsage=serverAuth\n' > "$ROOT/cert/leaf.ext"
  "$OPENSSL" req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
    -keyout "$ROOT/cert/leaf.key" -out "$ROOT/cert/leaf.csr" -nodes \
    -subj "/CN=127.0.0.1"
  "$OPENSSL" x509 -req -in "$ROOT/cert/leaf.csr" -CA "$ROOT/cert/ca.pem" -CAkey "$ROOT/cert/ca.key" \
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
"$OPENSSL" x509 -in "$EVIDENCE/leaf.pem" -noout -ext subjectAltName > "$EVIDENCE/cert-san.txt"
if [ -f "$TOKEN" ]; then
  stat -f 'mode=%OLp%nbytes=%z' "$TOKEN" > "$EVIDENCE/token-meta.txt"
else
  echo "absent" > "$EVIDENCE/token-meta.txt"
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
mode=$(awk -F= '/^mode=/ {print $2}' "$EVIDENCE/token-meta.txt")
bytes=$(awk -F= '/^bytes=/ {print $2}' "$EVIDENCE/token-meta.txt")
if [ "$mode" != 600 ] || [ "$bytes" != 6 ]; then
  echo "upstream token file is not the expected mode and length" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
if [ -e "$CLIENT" ]; then
  echo "client token path already exists" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
"$RELAY" --mint-client-token "$CLIENT" > "$ROOT/mint.stdout" 2> "$ROOT/mint.stderr"
mint_code=$?
printf '%s\n' "$mint_code" > "$EVIDENCE/mint.exit"
mint_out=$(wc -c < "$ROOT/mint.stdout" | tr -d ' ')
mint_err=$(wc -c < "$ROOT/mint.stderr" | tr -d ' ')
printf 'stdout_bytes=%s stderr_bytes=%s\n' "$mint_out" "$mint_err" > "$EVIDENCE/mint-meta.txt"
if [ "$mint_code" -ne 0 ] || [ "$mint_out" -ne 0 ] || [ "$mint_err" -ne 0 ]; then
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
stat -f 'mode=%OLp%nbytes=%z' "$CLIENT" > "$EVIDENCE/client-token-meta.txt"
client_mode=$(awk -F= '/^mode=/ {print $2}' "$EVIDENCE/client-token-meta.txt")
client_bytes=$(awk -F= '/^bytes=/ {print $2}' "$EVIDENCE/client-token-meta.txt")
if [ "$client_mode" != 600 ] || [ "$client_bytes" -lt 32 ] || [ "$client_bytes" -gt 256 ]; then
  echo "client token metadata is outside the relay boundary" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi

nohup "$RELAY" --listen 127.0.0.1:18446 --cert "$ROOT/cert/leaf.pem" --key "$ROOT/cert/leaf.key" \
  --client-token-file "$CLIENT" --upstream-token-file "$TOKEN" \
  > "$EVIDENCE/relay.stdout" 2> "$EVIDENCE/relay.stderr" < /dev/null &
relay_pid=$!
disown "$relay_pid" 2>/dev/null || true
printf '%s\n' "$relay_pid" > "$EVIDENCE/relay.pid"

nohup "$OBSERVE" --listen 127.0.0.1:18447 --cert "$ROOT/cert/leaf.pem" --key "$ROOT/cert/leaf.key" \
  --upstream-origin https://127.0.0.1:18446 --ca "$ROOT/cert/ca.pem" --out "$ROOT/observe-out" \
  > "$EVIDENCE/observer.stdout" 2> "$EVIDENCE/observer.stderr" < /dev/null &
observe_pid=$!
disown "$observe_pid" 2>/dev/null || true
printf '%s\n' "$observe_pid" > "$EVIDENCE/observer.pid"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  relay_up=0
  observe_up=0
  if lsof -nP -iTCP:18446 -sTCP:LISTEN >/dev/null 2>&1; then
    relay_up=1
  fi
  if lsof -nP -iTCP:18447 -sTCP:LISTEN >/dev/null 2>&1; then
    observe_up=1
  fi
  if [ "$relay_up" -eq 1 ] && [ "$observe_up" -eq 1 ]; then
    ready=1
    break
  fi
  if ! kill -0 "$relay_pid" 2>/dev/null || ! kill -0 "$observe_pid" 2>/dev/null; then
    break
  fi
  sleep 0.2
done
if [ "$ready" -ne 1 ]; then
  kill "$relay_pid" "$observe_pid" 2>/dev/null || true
  echo "relay or observer did not listen" >&2
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi

set +e
"$MODELS" --origin "https://127.0.0.1:18446" --ca "$EVIDENCE/ca.pem" \
  --bearer-file "$CLIENT" --out "$EVIDENCE/models.json" \
  > "$EVIDENCE/models.stdout" 2> "$EVIDENCE/models.stderr"
models_code=$?
printf '%s\n' "$models_code" > "$EVIDENCE/models.exit"
if [ "$models_code" -ne 0 ]; then
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
if ! grep -q '"model_available":true' "$EVIDENCE/models.json" || ! grep -q '"selected_model_id":"grok-4.7"' "$EVIDENCE/models.json"; then
  echo "grok-4.7 is not available; no chat will be sent" > "$EVIDENCE/models-gate.txt"
  printf '%s\n' 1 > "$EVIDENCE/smoke.exit"
  exit 1
fi
printf 'model_available=true selected_model_id=grok-4.7 chat_out=absent\n' > "$EVIDENCE/models-gate.txt"
printf '%s\n' 0 > "$EVIDENCE/smoke.exit"
exit 0
