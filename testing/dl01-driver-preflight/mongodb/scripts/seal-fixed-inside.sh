#!/bin/bash
# Seal customer envelopes with the fixed public function. Restores jsonb_doc.rs.
set -euo pipefail

SRC_FILE=/src/crates/oo-core/src/jsonb_doc.rs
ORIG=/tmp/jsonb_doc.rs.orig
cp "$SRC_FILE" "$ORIG"
restore() {
  cp "$ORIG" "$SRC_FILE"
}
trap restore EXIT

sha256sum "$SRC_FILE" | tee /evidence/jsonb-doc-before.sha256
grep -q '^38369c072bd09ce09efca48ed86811210d4761b1ddc179c73d253c7af21408e2  ' /evidence/jsonb-doc-before.sha256
if grep -q 'mod w141_customer_seal' "$SRC_FILE"; then
  echo "seal test already present" >&2
  exit 2
fi
printf '\n' >> "$SRC_FILE"
cat /snippet >> "$SRC_FILE"

printf '%s\n' "phase seal-test" > /evidence/phase.txt
CARGO_TARGET_DIR=/target/jsonb-631720c5/helper \
  cargo test --jobs 1 --locked --offline --manifest-path /src/Cargo.toml -p oo-core --lib seal_customer_envelopes -- --test-threads=1 \
  | tee /evidence/seal-test.txt
grep -q '1 passed' /evidence/seal-test.txt
printf '%s\n' 0 > /evidence/seal-test.exit

restore
trap - EXIT
sha256sum "$SRC_FILE" | tee /evidence/jsonb-doc-restored.sha256
cmp /evidence/jsonb-doc-before.sha256 /evidence/jsonb-doc-restored.sha256
test -s /evidence/seal-results.jsonl
if [[ -n "${HOST_UID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence || true
fi
printf '%s\n' DONE > /evidence/status.txt
