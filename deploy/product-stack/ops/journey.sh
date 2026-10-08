#!/bin/bash
# Build the journey document from captured bodies and ask the oracle.
# Ready stays false unless every real exchange meets the contract.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run"
python3 - "$EVIDENCE/run" <<'PY'
import json, pathlib, sys
run = pathlib.Path(sys.argv[1])
names = ["oidc_current", "catalog", "pack", "release", "session_close", "audit_ack"]
exchanges = []
for name in names:
    path = run / "probes" / f"{name}.json"
    if path.exists():
        exchanges.append(json.loads(path.read_text()))
    else:
        exchanges.append({"name": name, "http": 0, "tcp_only": False, "body": json.dumps({"error": "not_captured"})})

def snap(name):
    digest = (run / f"secret-{name}.sha").read_text().strip()
    rows = json.loads((run / f"rows-{name}.txt.json").read_text())
    return {"secret_sha256": digest, "rows": rows}

def lines(path):
    text = pathlib.Path(path).read_text() if pathlib.Path(path).exists() else ""
    return [line for line in text.splitlines() if line]

doc = {
    "profile": "dev",
    "deployment": "localprocess",
    "runsc": "not_used",
    "exchanges": exchanges,
    "before": snap("before"),
    "after_repeat": snap("after_repeat"),
    "after_restart": snap("after_restart"),
    "secret_files": json.loads((run / "secret-files.json").read_text()),
    "resources_after_cleanup": lines(run / "owned-after.txt"),
    "foreign_before": lines(run / "foreign-before.txt"),
    "foreign_after": lines(run / "foreign-after.txt"),
}
(run / "journey-input.json").write_text(json.dumps(doc))
PY
set +e
oracle journey < "${EVIDENCE}/run/journey-input.json" > "${EVIDENCE}/run/journey.json"
printf '%s\n' $? > "${EVIDENCE}/run/journey.exit"
oracle health < "${EVIDENCE}/run/journey-input.json" > "${EVIDENCE}/run/health.json"
printf '%s\n' $? > "${EVIDENCE}/run/health.exit"
exit 0
