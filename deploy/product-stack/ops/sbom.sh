#!/bin/bash
# Input SBOM from lock text. recipe_only units stay unlocked. Not an image scan.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run"
python3 - "$ROOT" "$EVIDENCE/run/images.txt" <<'PY' > "${STATE}/sbom-input.json"
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
images = pathlib.Path(sys.argv[2]).read_text() if pathlib.Path(sys.argv[2]).exists() else ""
units = [
    ("control-identity", "a6f92da8a0bf872f8a30efb116c2c7b14463505b", "tools/control-identity/Cargo.lock", "oo-a395-identity"),
    ("audit-ingest", "1078d69e8c3cf8dacb145198f1e4123c767a196b", "tools/audit-ingest/Cargo.lock", "oo-a395-audit"),
    ("session-control-api", "53017ed29", "tools/session-control-api/Cargo.lock", "oo-a395-session"),
    ("selfbuild-control-api", "HEAD", "tools/selfbuild-control-api/Cargo.lock", "oo-a395-selfbuild"),
    ("oo-product-stack", "workspace", "deploy/product-stack/Cargo.lock", "oo-product-stack"),
    ("source-control-api", "uncommitted", "", "oo-a395-source"),
    ("release-control-api", "uncommitted", "", "oo-a395-release"),
]
out = []
for name, commit, lock, tag in units:
    if name in ("source-control-api", "release-control-api"):
        status = "recipe_only"
        text = ""
    elif f"built {tag}:local" in images or (tag == "oo-product-stack"):
        status = "image_built" if tag != "oo-product-stack" else "fixed"
        text = (root / lock).read_text() if lock and (root / lock).exists() else ""
    elif lock and (root / lock).exists():
        status = "fixed"
        text = (root / lock).read_text()
    else:
        status = "not_started"
        text = ""
    if name == "selfbuild-control-api" and f"built {tag}:local" not in images:
        status = "not_started"
        text = (root / lock).read_text() if (root / lock).exists() else ""
        if text and status == "not_started":
            status = "fixed"
    out.append({"name": name, "commit": commit, "status": status, "lock_text": text})
print(json.dumps({"units": out}))
PY
set +e
oracle sbom < "${STATE}/sbom-input.json" > "${EVIDENCE}/run/sbom.json"
code=$?
set -e
printf '%s\n' "$code" > "${EVIDENCE}/run/sbom.exit"
rm -f "${STATE}/sbom-input.json"
exit 0
