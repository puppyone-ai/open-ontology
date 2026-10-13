#!/bin/bash
# Validate the frozen report and config, then import the generation-2 Arrow.
set -euo pipefail

/admission/oo-source-admission validate \
  --report /evidence/plan/report.json \
  --config /evidence/plan/config.json \
  > /evidence/decision.bin
printf '%s\n' 0 > /evidence/validate.exit
got="$(sha256sum /evidence/decision.bin | awk '{print $1}')"
printf '%s\n' "$got" > /evidence/decision.sha256
if [[ "$got" != "$DECISION_SHA" ]]; then
  printf 'decision mismatch %s\n' "$got" >&2
  exit 3
fi
set +e
/elf/oo-pipeline import-native < /evidence/request.json > /evidence/receipt.json 2>/evidence/import.stderr
rc=$?
set -e
printf '%s\n' "$rc" > /evidence/import.exit
if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence || true
fi
exit "$rc"
