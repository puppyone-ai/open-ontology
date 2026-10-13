#!/bin/bash
# Run the rebuilt probe once against the existing MongoDB. Fresh output only.
# Does not replace the frozen Arrow and does not delete a volume.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight"
OUT="${CAPTURE_PROBE_OUT:-$EVIDENCE/raw/repair-64-fulldoc}"
BIN="$EVIDENCE/raw/repair-64-probe-build/bin"
SO="$EVIDENCE/raw/repair-60-cancel-build/bin/liboo-opmsg-names.so"
SECRETS="/tmp/oo-dl01w1-41-secrets"
MONGO="oo-dl01w1-41-mongodb"
PROBE="oo-dl01w1-41-read"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
CUSTOMER="$ROOT/tools/mongodb-preview/examples/customer.shipments.json"
REGISTRY="$ROOT/tools/mongodb-preview/examples/secret-registry.json"
FROZEN="$EVIDENCE/batch.arrow"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-41
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd
  --label task=DL01-W1-41
  --label kind=rust-builder
)

if [[ -e "$OUT" ]]; then
  echo "fresh directory already exists: $OUT" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$PROBE"; then
  echo "named container $PROBE still present" >&2
  exit 2
fi
mkdir -p "$OUT/probe"
test -x "$BIN/oo-mongodb-driver-probe"
test -x "$BIN/oo-mongodb-arrow-decode"
test -f "$SO"
cp "$SO" "$BIN/liboo-opmsg-names.so"
test -f "$CUSTOMER"
test -f "$FROZEN"
docker inspect "$MONGO" >/dev/null
if [[ "$(docker inspect -f '{{.State.Status}}' "$MONGO")" != "running" ]]; then
  echo "mongodb is not running" >&2
  exit 2
fi

cleanup() {
  docker rm -f "$PROBE" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}
trap cleanup EXIT

if ! python3 "$ADMIT" acquire rust "$OUT/admission-rust.json"; then
  echo "rust capacity" >&2
  exit 3
fi

set +e
docker run --name "$PROBE" \
  --platform linux/arm64 --pull never \
  --network "container:${MONGO}" \
  --memory 2g --memory-swap 2g --cpus 1 \
  --pids-limit 512 \
  --entrypoint bash \
  -e OO_OUT=/out \
  -e OO_READER_PASSWORD_FILE=/run/secrets/reader \
  -e OO_W141_OPMSG_LOG=/out/opmsg-names.jsonl \
  -e OO_W141_CUSTOMER=/run/customer.json \
  -e OO_W141_SECRET_REGISTRY=/run/secret-registry.json \
  -v "$OUT/probe:/out" \
  -v "$SECRETS/reader:/run/secrets/reader:ro" \
  -v "$CUSTOMER:/run/customer.json:ro" \
  -v "$REGISTRY:/run/secret-registry.json:ro" \
  -v "$BIN:/binaries:ro" \
  -v "$SRC/scripts/probe-inside.sh:/probe-inside.sh:ro" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  /probe-inside.sh \
  >"$OUT/probe/docker.stdout" 2>"$OUT/probe/docker.stderr"
probe_rc=$?
set -e
printf '%s\n' "$probe_rc" >"$OUT/probe/docker.exit"
if [[ "$probe_rc" -ne 0 ]]; then
  echo "probe container $probe_rc" >&2
  exit 1
fi
python3 - "$FROZEN" "$OUT" <<'PY'
import hashlib, json, sys
from pathlib import Path
frozen_arrow, out = map(Path, sys.argv[1:])
partial = out / "probe" / "batch.arrow.partial"
capture = out / "probe" / "fulldoc-capture.json"
if not partial.is_file() or not capture.is_file():
    raise SystemExit("capture files missing")
partial_sha = hashlib.sha256(partial.read_bytes()).hexdigest()
frozen_sha = hashlib.sha256(frozen_arrow.read_bytes()).hexdigest()
body = json.loads(capture.read_text())
report = {
    "partial_sha256": partial_sha,
    "partial_bytes": partial.stat().st_size,
    "frozen_arrow_sha256": frozen_sha,
    "partial_matches_frozen_arrow": partial_sha == frozen_sha,
    "frozen_arrow_replaced": False,
    "captured_rows": body.get("captured_rows"),
    "hidden_key_in_capture": body.get("hidden_key_in_capture"),
    "weight_in_full_bson": body.get("weight_in_full_bson"),
    "weight_in_seal_bson": body.get("weight_in_seal_bson"),
    "probe_exit": (out / "probe" / "probe.exit").read_text().strip(),
    "decode_exit": (out / "probe" / "decode.exit").read_text().strip(),
}
(out / "frozen-compare.json").write_text(json.dumps(report, indent=2) + "\n")
if report["probe_exit"] != "0" or report["decode_exit"] != "0":
    raise SystemExit("probe exit")
if not report["partial_matches_frozen_arrow"]:
    raise SystemExit("arrow drifted")
if body.get("captured_rows") != 16 or body.get("hidden_key_in_capture") is not False:
    raise SystemExit("capture")
PY
printf 'DONE\n' > "$OUT/status.txt"
