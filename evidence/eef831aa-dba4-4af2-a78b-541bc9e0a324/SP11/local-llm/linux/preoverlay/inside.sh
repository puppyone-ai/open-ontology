#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of fixed e8d297f.
# Confirms the archive identity and the dialogue seam. No product overlay.
# TCP probes open a socket and send no bytes and no token.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy NO_PROXY no_proxy
unset OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE
cd /src

phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/${name}.stdout" 2> "/evidence/${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/${name}.exit"
  echo "${name}_exit=${code}"
  return 0
}
set -e

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
rustup show | tee /evidence/rustup-show.txt
uname -a | tee /evidence/uname.txt
id | tee /evidence/id.txt
echo "=== toolchain file ==="
if [ -f rust-toolchain.toml ]; then
  cp rust-toolchain.toml /evidence/rust-toolchain.toml
fi
echo "=== archive seam ==="
if [ -e tools/ontology-dialogue ]; then
  echo "present" > /evidence/ontology-dialogue-in-archive.txt
else
  echo "absent" > /evidence/ontology-dialogue-in-archive.txt
fi
if [ -e testing/real-db/local-llm ]; then
  echo "present" > /evidence/local-llm-in-archive.txt
else
  echo "absent" > /evidence/local-llm-in-archive.txt
fi
if [ -f tools/description-provider/CONTRACT.md ]; then
  echo "present" > /evidence/description-provider-in-archive.txt
  sha256sum tools/description-provider/CONTRACT.md tools/description-provider/src/transport.rs \
    > /evidence/provider-sha256.txt
else
  echo "absent" > /evidence/description-provider-in-archive.txt
fi
echo "=== proxy env names only ==="
env | awk -F= 'BEGIN{n=0} $1 ~ /PROXY|OPENAI|TOKEN|KEY|SECRET|PASSWORD/ {print $1; n++} END{print "sensitive_names=" n}' \
  > /evidence/env-names.txt
echo "=== openssl ==="
if command -v openssl >/dev/null 2>&1; then
  openssl version > /evidence/openssl-version.txt
else
  echo "absent" > /evidence/openssl-version.txt
fi

probe() {
  local label="$1"
  local host="$2"
  local port="$3"
  set +e
  timeout 3 bash -c "exec 3<>/dev/tcp/${host}/${port}" >/dev/null 2>/tmp/probe.err
  local code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/probe-${label}.exit"
  if [ "$code" -eq 0 ]; then
    echo "${label}=open"
  else
    echo "${label}=closed"
  fi
  # The error text is a connect failure. It has no token. Keep it.
  if [ -s /tmp/probe.err ]; then
    cp /tmp/probe.err "/evidence/probe-${label}.stderr"
  fi
}

echo "=== tcp probes ==="
probe container-loopback-8317 127.0.0.1 8317
probe host-gateway-8317 host.docker.internal 8317
probe lan-8317 192.168.1.3 8317

echo "=== rustc pin ==="
if grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "msrv_image=1.94.0" | tee /evidence/msrv.txt
else
  echo "msrv_image=mismatch" | tee /evidence/msrv.txt
  exit 1
fi
if [ "$(cat /evidence/ontology-dialogue-in-archive.txt)" != "absent" ]; then
  echo "dialogue unexpectedly present in the fixed archive" >&2
  exit 1
fi
if [ "$(cat /evidence/local-llm-in-archive.txt)" != "absent" ]; then
  echo "local-llm unexpectedly present before the overlay" >&2
  exit 1
fi
echo "preoverlay_ok"
