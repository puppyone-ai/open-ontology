#!/bin/bash
# Linux 1.94 product tests, then the pinned pg-builder import refusal.
set -euo pipefail
bash /work/tools/mongodb-preview/scripts/build-product-inside.sh
mkdir -p /evidence/pg-builder /evidence/probe
rustc --version | tee /evidence/pg-builder/rustc-version.txt
grep -q '^rustc 1.94.0 ' /evidence/pg-builder/rustc-version.txt
(
  cd /deps/tools/pg-builder
  CARGO_TARGET_DIR=/target/pg-builder-194 cargo build --jobs 1 --locked --bin pg-builder
) > /evidence/pg-builder/cargo-build.txt 2>&1
cp /target/pg-builder-194/debug/pg-builder /evidence/pg-builder/pg-builder
sha256sum /evidence/pg-builder/pg-builder | tee /evidence/pg-builder/pg-builder.sha256
set +e
cargo run --jobs 1 --locked --offline --quiet --manifest-path /work/tools/mongodb-preview/Cargo.toml -- \
  --customer /work/tools/mongodb-preview/examples/customer.shipments.json \
  --source-id oo_w141 \
  --secret-ref secret://oo-dl01w1-41/reader \
  --driver-digest 622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8 \
  --permission read \
  --budget-unit Rows \
  --budget-limit 16 \
  --out /evidence/probe/decision.json \
  --gap-out /evidence/probe/gap.json \
  --pg-builder /evidence/pg-builder/pg-builder \
  --work-dir /evidence/probe/work \
  --arrow /frozen/batch.arrow \
  --census /frozen/field-census.json > /evidence/probe/cli.stdout 2> /evidence/probe/cli.stderr
cli_rc=$?
set -e
printf '%s\n' "$cli_rc" > /evidence/probe/cli.exit
if [[ "$cli_rc" -ne 5 ]]; then
  exit "$cli_rc"
fi
grep -q '"admitted": true' /evidence/probe/decision.json
grep -q '"engine": "mongodb"' /evidence/probe/gap.json
grep -q '"shared_probe": "shared_contract"' /evidence/probe/gap.json
grep -q '"import_started": false' /evidence/probe/gap.json
grep -q '"model_called": false' /evidence/probe/gap.json
grep -q '"sealed_import_called": false' /evidence/probe/gap.json
grep -q '"postgres_started": false' /evidence/probe/gap.json
grep -q 'the manifest protocol is not the sealed import manifest' /evidence/probe/gap.json
grep -q 'the catalog source engine is not mysql' /evidence/probe/gap.json
if grep -E 'mongodb://|CANARY-SECRET|postgres-native' /evidence/probe/decision.json /evidence/probe/work/manifest.json; then
  echo "secret or retag in probe" >&2
  exit 1
fi
if grep -E '"engine": "postgres-native"|"engine": "mysql"' /evidence/probe/gap.json /evidence/probe/work/manifest.json; then
  echo "engine retag in probe" >&2
  exit 1
fi
test ! -f /evidence/probe/work/password-not-read
test ! -f /evidence/probe/work/import-out.json
cat > /evidence/probe/productmodel-provider-gate.json << 'EOF'
{
  "endpoint": "http://127.0.0.1:8317",
  "token_file": "/tmp/oo-customer-demo-20261007/llm-token",
  "token_read": false,
  "called": false,
  "http_status": null,
  "reason": "pg-builder import refused oo.mongodb-preview.manifest/v1 before a sealed catalog existed"
}
EOF
if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence || true
fi
echo "chain probe ok"
