#!/bin/bash
# Decode the captured customer BSON with the fixed private decoder.
# Restores jsonb_doc.rs before exit. Does not replace the production ELF.
set -euo pipefail

SRC_FILE=/src/crates/oo-core/src/jsonb_doc.rs
ORIG=/tmp/jsonb_doc.rs.orig
mkdir -p /elf
copy_elf() {
  local name="$1"
  local source="$2"
  if [[ ! -s "/elf/$name" ]]; then
    cp "$source" "/elf/$name"
  fi
}
copy_elf oo-pipeline /target/jsonb-631720c5/pipeline/debug/oo-pipeline
copy_elf pg-builder /target/jsonb-631720c5/pg-builder/debug/pg-builder
copy_elf source-semantics /target/jsonb-631720c5/source-semantics/debug/source-semantics
sha256sum /elf/oo-pipeline /elf/pg-builder /elf/source-semantics | tee /evidence/elf.sha256
grep -q '^f2894da67ec5877c2c6322f17bcb8377e4ca1f03c3f99d4b39236d22b34a9631  ' /evidence/elf.sha256
grep -q '^1e52beeb6020bee2a402bd16bdf22aa50274b1790b1218438ce3726ded94daee  ' /evidence/elf.sha256
grep -q '^e2f73180499c683b951cdba541428fbdc1376f37bf99b2c23292eab3f72e0ffe  ' /evidence/elf.sha256
cp "$SRC_FILE" "$ORIG"
restore() {
  cp "$ORIG" "$SRC_FILE"
}
trap restore EXIT

sha256sum "$SRC_FILE" | tee /evidence/jsonb-doc-before.sha256
grep -q '^38369c072bd09ce09efca48ed86811210d4761b1ddc179c73d253c7af21408e2  ' /evidence/jsonb-doc-before.sha256
if grep -q 'mod w141_customer_decode' "$SRC_FILE"; then
  echo "decode test already present" >&2
  exit 2
fi
printf '\n' >> "$SRC_FILE"
cat /snippet >> "$SRC_FILE"

printf '%s\n' "phase decode-test" > /evidence/phase.txt
CARGO_TARGET_DIR=/target/jsonb-631720c5/helper \
  cargo test --jobs 1 --locked --offline --manifest-path /src/Cargo.toml -p oo-core --lib write_customer_decode -- --test-threads=1 \
  | tee /evidence/decode-test.txt
grep -q '1 passed' /evidence/decode-test.txt
printf '%s\n' 0 > /evidence/decode-test.exit

restore
trap - EXIT
sha256sum "$SRC_FILE" | tee /evidence/jsonb-doc-restored.sha256
cmp /evidence/jsonb-doc-before.sha256 /evidence/jsonb-doc-restored.sha256
test -s /evidence/decode-full.jsonl
test -s /evidence/decode-seal.jsonl
if [[ -n "${HOST_UID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence /elf || true
fi
printf '%s\n' DONE > /evidence/status.txt
