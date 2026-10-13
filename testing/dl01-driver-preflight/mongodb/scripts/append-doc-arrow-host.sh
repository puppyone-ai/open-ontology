#!/bin/bash
# One Rust slot. Builds only the W41 probe doc-arrow bin and writes a new Arrow file.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-72-doc-arrow"
FROZEN="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/batch.arrow"
CANONICAL="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-67-canonical/canonical.json"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
FROZEN_SHA="4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-41
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd
  --label task=DL01-W1-41
  --label kind=rust-builder
)

if [[ -e "$EVIDENCE" ]]; then
  echo "evidence dir already exists" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$RUST"; then
  echo "named container $RUST still present" >&2
  exit 2
fi
python3 - << 'PY'
import hashlib, json
from pathlib import Path
frozen = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/batch.arrow").read_bytes()
digest = hashlib.sha256(frozen).hexdigest()
expect = "4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8"
if digest != expect:
    raise SystemExit(f"frozen arrow {digest}")
canonical = json.loads(Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-67-canonical/canonical.json").read_text())
rows = canonical["rows"]
if len(rows) != 16:
    raise SystemExit(f"rows {len(rows)}")
hidden = ("weight", "notApproved", "price_odd", "mixed_marks")
for index, row in enumerate(rows):
    if row["index"] != index:
        raise SystemExit(f"index {row['index']}")
    text = row["pg_jsonb_text"]
    send = hashlib.sha256(b"\x01" + text.encode()).hexdigest()
    if send != row["pg_jsonb_sha256"]:
        raise SystemExit(f"jsonb send sha {index}")
    value = json.loads(text)
    def walk(node):
        if isinstance(node, dict):
            for key, child in node.items():
                if key in hidden:
                    raise SystemExit(f"hidden {key} at {index}")
                walk(child)
        elif isinstance(node, list):
            for child in node:
                walk(child)
    walk(value)
print("precheck-ok", len(frozen))
PY
before="$(shasum -a 256 "$FROZEN" | awk '{print $1}')"
[[ "$before" == "$FROZEN_SHA" ]]
mkdir -p "$EVIDENCE"
docker image inspect "$RUST_IMAGE" >/dev/null
python3 "$ADMIT" acquire rust "$EVIDENCE/admission.json"
cleanup() {
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}
trap cleanup EXIT

set +e
docker run --name "$RUST" \
  --platform linux/arm64 --pull never \
  --memory 6g --memory-swap 6g --cpus 1 \
  --pids-limit 1024 \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_INCREMENTAL=1 \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$SRC:/src" \
  -v "$FROZEN:/frozen/batch.arrow:ro" \
  -v "$CANONICAL:/canonical/canonical.json:ro" \
  -v "$EVIDENCE:/evidence" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  bash /src/scripts/append-doc-arrow-inside.sh
rc=$?
set -e
printf '%s\n' "$rc" >"$EVIDENCE/docker.exit"
after="$(shasum -a 256 "$FROZEN" | awk '{print $1}')"
[[ "$after" == "$FROZEN_SHA" ]]
if [[ "$rc" -ne 0 ]]; then
  exit "$rc"
fi
python3 - << 'PY'
import hashlib
from pathlib import Path
root = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-72-doc-arrow")
arrow = (root / "shipments-doc.arrow").read_bytes()
digest = hashlib.sha256(arrow).hexdigest()
frozen = "4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8"
if digest == frozen:
    raise SystemExit("new arrow reused the frozen digest")
(root / "arrow.sha256").write_text(digest + "\n")
identity = (root / "arrow-identity.json").read_text()
if '"frozen_arrow_replaced":false' not in identity.replace(" ", ""):
    raise SystemExit("identity does not record frozen_arrow_replaced false")
print(digest)
PY
