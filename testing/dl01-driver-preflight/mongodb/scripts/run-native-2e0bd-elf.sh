#!/bin/bash
# Call the fixed native ELF with packs already written by run-native-2e0bd.sh.
# The first script execution stored the subcommand in the stdin path. Those phase files stay.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
BASE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="$BASE/raw/native-2e0bd"
CALL="$OUT/phases/elf2"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PIPELINE="/tmp/oo-u06-194/debug/oo-pipeline"
PG_NAME="oo-dl01w1-41-pg-native"
PASS_DIR="/tmp/oo-dl01w1-41-native"
ELF_NAME="oo-dl01w1-41-native-elf"
ENVELOPE="$BASE/raw/docseal-80/baseline-0.envelope.json"

if [[ ! -f "$OUT/gen1/request.json" || ! -f "$OUT/gen2/request.json" ]]; then
  echo "capture packs are absent" >&2
  exit 2
fi
if [[ -e "$CALL" ]]; then
  echo "elf2 evidence already exists" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$ELF_NAME"; then
  echo "elf name is present" >&2
  exit 2
fi
docker inspect "$PG_NAME" --format '{{.State.Status}}' | grep -qx running
mkdir -p "$CALL"

run_elf() {
  local subcommand="$1"
  local input="$2"
  docker run --rm -i --platform linux/arm64 --pull never \
    --name "$ELF_NAME" \
    --network "container:$PG_NAME" \
    --memory 4g --memory-swap 4g \
    -v "$PIPELINE:/opt/oo-pipeline:ro" \
    -v "$OUT:$OUT:ro" \
    -v "$BASE/raw/docseal-80:$BASE/raw/docseal-80:ro" \
    -v "$PASS_DIR:$PASS_DIR:ro" \
    "$RUST_IMAGE" \
    /opt/oo-pipeline "$subcommand" <"$input"
}

phase() {
  local name="$1"
  shift
  set +e
  "$@" >"$CALL/${name}.stdout" 2>"$CALL/${name}.stderr"
  local status=$?
  set -e
  printf '%s\n' "$status" >"$CALL/${name}.exit"
  return "$status"
}

python3 - <<PY
import json, pathlib
src = json.loads(pathlib.Path("$OUT/gen1/request.json").read_text())
src["target"]["port"] = 1
pathlib.Path("$CALL/preflight-port1.json").write_text(json.dumps(src) + "\n")
req = json.loads(pathlib.Path("$OUT/gen1/request.json").read_text())
install = {"protocol": "oo.pipeline.install-native-update/v1", "target": req["target"]}
pathlib.Path("$CALL/install.json").write_text(json.dumps(install) + "\n")
catalog = json.loads(pathlib.Path("$OUT/gen1/catalog.json").read_text())
document = json.loads(pathlib.Path("$ENVELOPE").read_text())
body = {
    "protocol": "oo.pipeline.jsonb-document-import/v1",
    "engine": "mongodb",
    "source_id": "oo_w141",
    "catalog": catalog,
    "document": document,
}
pathlib.Path("$CALL/jsonb-document.request.json").write_text(json.dumps(body) + "\n")
PY

phase cpu-max docker run --rm --platform linux/arm64 --pull never \
  --name "$ELF_NAME" \
  --network "container:$PG_NAME" \
  --memory 4g --memory-swap 4g \
  "$RUST_IMAGE" \
  cat /sys/fs/cgroup/cpu.max || true
phase preflight run_elf import-native-update "$CALL/preflight-port1.json" || true
phase install run_elf install-native-update "$CALL/install.json" || true
phase import-gen1 run_elf import-native-update "$OUT/gen1/request.json" || true
phase import-gen2 run_elf import-native-update "$OUT/gen2/request.json" || true
phase import-jsonb-document run_elf import-jsonb-document "$CALL/jsonb-document.request.json" || true

python3 - <<PY
import json, pathlib
call = pathlib.Path("$CALL")

def load(name):
    path = call / f"{name}.stdout"
    text = path.read_text() if path.exists() else ""
    try:
        body = json.loads(text) if text.strip() else {}
    except json.JSONDecodeError:
        body = {"raw": text[:500]}
    return int((call / f"{name}.exit").read_text().strip()), body

summary = {}
for name in ("preflight", "install", "import-gen1", "import-gen2", "import-jsonb-document"):
    status, body = load(name)
    summary[name] = {
        "exit": status,
        "load_state": body.get("load_state"),
        "reason": body.get("reason"),
        "installed": body.get("installed"),
        "source_bound": body.get("source_bound"),
        "code": body.get("code"),
        "physical_name": body.get("physical_name"),
        "row_count": body.get("row_count"),
        "target_generation": body.get("target_generation"),
    }
(call / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
PY
