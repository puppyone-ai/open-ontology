#!/bin/bash
# Host relay with the client-token boundary. One models GET. No chat completion.
# The upstream token file is read by the relay. Its bytes are not arguments.
set -u
SRC=/Users/redux/puppy-one/open-ontology
if [ -x /opt/homebrew/opt/openssl@3/bin/openssl ]; then
  OPENSSL=/opt/homebrew/opt/openssl@3/bin/openssl
else
  OPENSSL=openssl
fi
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/host-3
ROOT=/var/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334/host-3
TOKEN=/tmp/oo-customer-demo-20261007/llm-token
CLIENT=$ROOT/client-token
LISTEN=127.0.0.1:18317
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
  "$OPENSSL" req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
    -keyout "$ROOT/cert/ca.key" -out "$ROOT/cert/ca.pem" -days 1 -nodes \
    -subj "/CN=oo-a334-ca" \
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
  stat -f 'mode=%OLp bytes=%z' "$TOKEN" > "$EVIDENCE/token-meta.txt"
else
  echo "absent" > "$EVIDENCE/token-meta.txt"
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
stat -f 'mode=%OLp bytes=%z' "$CLIENT" > "$EVIDENCE/client-token-meta.txt"
nohup "$RELAY" --listen "$LISTEN" --cert "$ROOT/cert/leaf.pem" --key "$ROOT/cert/leaf.key" \
  --client-token-file "$CLIENT" --upstream-token-file "$TOKEN" \
  > "$EVIDENCE/relay.stdout" 2> "$EVIDENCE/relay.stderr" < /dev/null &
relay_pid=$!
disown "$relay_pid" 2>/dev/null || true
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
"$MODELS" --origin "https://127.0.0.1:18317" --ca "$EVIDENCE/ca.pem" \
  --bearer-file "$CLIENT" --out "$EVIDENCE/models.json" \
  > "$EVIDENCE/models.stdout" 2> "$EVIDENCE/models.stderr"
models_code=$?
printf '%s\n' "$models_code" > "$EVIDENCE/models.exit"
printf '%s\n' "$models_code" > "$EVIDENCE/smoke.exit"
exit "$models_code"
