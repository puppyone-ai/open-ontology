#!/bin/bash
# Emit capture packs with the Linux 1.94 producer and call the fixed native ELF.
# Does not start MongoDB, does not touch the platform Postgres, and does not set a CPU quota.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
BASE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="$BASE/raw/native-2e0bd"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PIPELINE="/tmp/oo-u06-194/debug/oo-pipeline"
PGBUILDER="/tmp/oo-u06-194/debug/pg-builder"
PG_NAME="oo-dl01w1-41-pg-native"
PASS_DIR="/tmp/oo-dl01w1-41-native"
PASS_FILE="$PASS_DIR/pg.pass"
SEAL1="$BASE/raw/docseal-80/baseline.seal.json"
SEAL2="$BASE/raw/docseal-80/cycle-b.seal.json"
AUDIT="$BASE/raw/c5-cycle-79/audit-profiled-named.json"
ENVELOPE="$BASE/raw/docseal-80/baseline-0.envelope.json"
OWNER="41a14141-1414-4414-8414-000000000041"
PRODUCER_NAME="oo-dl01w1-41-native-producer"
ELF_NAME="oo-dl01w1-41-native-elf"

if [[ -e "$OUT" ]]; then
  echo "native evidence already exists" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$PRODUCER_NAME"; then
  echo "producer name is present" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$ELF_NAME"; then
  echo "elf name is present" >&2
  exit 2
fi
[[ -x "$PIPELINE" && -x "$PGBUILDER" ]]
docker inspect "$PG_NAME" --format '{{.State.Status}}' | grep -qx running
mkdir -p "$OUT/gen1" "$OUT/gen2" "$OUT/phases" "$PASS_DIR"
python3 - <<'PY'
from pathlib import Path
Path("/tmp/oo-dl01w1-41-native/pg.pass").write_bytes(b"local\n")
PY
chmod 600 "$PASS_FILE"

run_producer() {
  docker run --rm --platform linux/arm64 --pull never \
    --name "$PRODUCER_NAME" \
    --user "$(id -u):$(id -g)" \
    --memory 4g --memory-swap 4g \
    -v oo-dl01w1-41-native-target:/target:ro \
    -v "$BASE/raw/docseal-80:$BASE/raw/docseal-80:ro" \
    -v "$BASE/raw/c5-cycle-79:$BASE/raw/c5-cycle-79:ro" \
    -v "$OUT:$OUT" \
    -v "$PASS_DIR:$PASS_DIR:ro" \
    "$RUST_IMAGE" \
    bash -c 'cat /sys/fs/cgroup/cpu.max > "$1/producer.cpu.max"; exec /target/debug/mongodb-preview "${@:2}"' \
    bash "$OUT" "$@"
}

phase() {
  local name="$1"
  shift
  set +e
  "$@" >"$OUT/phases/${name}.stdout" 2>"$OUT/phases/${name}.stderr"
  local status=$?
  set -e
  printf '%s\n' "$status" >"$OUT/phases/${name}.exit"
  return "$status"
}

phase bind-gen1 run_producer \
  --bind-pack \
  --seal "$SEAL1" \
  --audit "$AUDIT" \
  --generation 1 \
  --out-dir "$OUT/gen1" \
  --arrow-path "$OUT/gen1/rows.arrow" \
  --receipt-path "$OUT/gen1/receipt.json" \
  --password-file "$PASS_FILE" \
  --database oo_w141 \
  --port 5432 \
  --shard shard_w141 \
  --owner "$OWNER"
phase bind-gen2 run_producer \
  --bind-pack \
  --seal "$SEAL2" \
  --audit "$AUDIT" \
  --generation 2 \
  --parent 1 \
  --full-snapshot \
  --out-dir "$OUT/gen2" \
  --arrow-path "$OUT/gen2/rows.arrow" \
  --receipt-path "$OUT/gen2/receipt.json" \
  --password-file "$PASS_FILE" \
  --database oo_w141 \
  --port 5432 \
  --shard shard_w141 \
  --owner "$OWNER"

python3 - <<PY
import json, pathlib
out = pathlib.Path("$OUT")
env = json.loads(pathlib.Path("$ENVELOPE").read_text())
req = json.loads((out / "gen1/request.json").read_text())
seal = json.loads(pathlib.Path("$SEAL1").read_text())
truth = {row[0]: row[1] for row in req["truth"]}
key = "c141d04141eef04100000000"
document = next(row["extended_json"] for row in seal["rows"] if row["extended_json"]["_id"]["\$oid"] == key)
sql = "SELECT (\$mongo\$" + json.dumps(document, separators=(",", ":")) + "\$mongo\$::jsonb)::text;\n"
(out / "phases/text-live.sql").write_text(sql)
(out / "phases/text-probe-partial.json").write_text(json.dumps({"key": key, "truth": truth[key], "envelope": env["pg_jsonb_text"]}) + "\n")
PY
phase text-live docker exec -i "$PG_NAME" psql -U postgres -d oo_w141 -At -v ON_ERROR_STOP=1 -f - <"$OUT/phases/text-live.sql" || true
python3 - <<PY
import json, pathlib
out = pathlib.Path("$OUT")
partial = json.loads((out / "phases/text-probe-partial.json").read_text())
live_path = out / "phases/text-live.stdout"
live = live_path.read_text().strip() if live_path.exists() else ""
body = {
    "key": partial["key"],
    "predictor_equals_envelope": partial["truth"] == partial["envelope"],
    "predictor_equals_live_pg": partial["truth"] == live,
    "live_exit": int((out / "phases/text-live.exit").read_text().strip()),
}
(out / "phases/text-probe.json").write_text(json.dumps(body, indent=2) + "\n")
print(json.dumps(body))
PY

run_elf() {
  local bin="$1"
  local subcommand="$2"
  local input="$3"
  docker run --rm -i --platform linux/arm64 --pull never \
    --name "$ELF_NAME" \
    --network "container:$PG_NAME" \
    --memory 4g --memory-swap 4g \
    -v "$PIPELINE:/opt/oo-pipeline:ro" \
    -v "$PGBUILDER:/opt/pg-builder:ro" \
    -v "$OUT:$OUT:ro" \
    -v "$BASE/raw/docseal-80:$BASE/raw/docseal-80:ro" \
    -v "$PASS_DIR:$PASS_DIR:ro" \
    "$RUST_IMAGE" \
    "$bin" "$subcommand" <"$input"
}

set +e
python3 - <<PY
import json, pathlib, sys
body = json.loads(pathlib.Path("$OUT/phases/text-probe.json").read_text())
sys.exit(0 if body.get("predictor_equals_live_pg") and body.get("predictor_equals_envelope") else 4)
PY
PROBE_OK=$?
set -e
if [[ "$PROBE_OK" -ne 0 ]]; then
  printf '%s\n' 4 >"$OUT/phases/preflight-port1.exit"
  printf '%s\n' 4 >"$OUT/phases/install.exit"
  printf '%s\n' 4 >"$OUT/phases/import-gen1.exit"
  printf '%s\n' 4 >"$OUT/phases/import-gen2.exit"
  echo "text probe did not match; imports were not sent" >"$OUT/phases/import-gen1.stderr"
else
python3 - <<PY
import json, pathlib
src = json.loads(pathlib.Path("$OUT/gen1/request.json").read_text())
src["target"]["port"] = 1
pathlib.Path("$OUT/phases/preflight-port1.json").write_text(json.dumps(src) + "\n")
PY
phase preflight-port1 run_elf /opt/oo-pipeline import-native-update "$OUT/phases/preflight-port1.json" || true

python3 - <<PY
import json, pathlib
req = json.loads(pathlib.Path("$OUT/gen1/request.json").read_text())
install = {"protocol": "oo.pipeline.install-native-update/v1", "target": req["target"]}
pathlib.Path("$OUT/phases/install.json").write_text(json.dumps(install) + "\n")
PY
phase install run_elf /opt/oo-pipeline install-native-update "$OUT/phases/install.json" || true

phase import-gen1 run_elf /opt/oo-pipeline import-native-update "$OUT/gen1/request.json" || true
phase import-gen2 run_elf /opt/oo-pipeline import-native-update "$OUT/gen2/request.json" || true
fi

python3 - <<PY
import json, pathlib
out = pathlib.Path("$OUT")
catalog = json.loads((out / "gen1/catalog.json").read_text())
document = json.loads(pathlib.Path("$ENVELOPE").read_text())
body = {
    "protocol": "oo.pipeline.jsonb-document-import/v1",
    "engine": "mongodb",
    "source_id": "oo_w141",
    "catalog": catalog,
    "document": document,
}
(out / "phases/jsonb-document.request.json").write_text(json.dumps(body) + "\n")
PY
phase import-jsonb-document run_elf /opt/oo-pipeline import-jsonb-document "$OUT/phases/jsonb-document.request.json" || true

python3 - <<PY
import hashlib, json, pathlib, subprocess
out = pathlib.Path("$OUT")
base = pathlib.Path("$BASE")

def load_exit(name):
    text = (out / "phases" / f"{name}.exit").read_text().strip()
    return int(text)

def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

def load_json(path):
    return json.loads(pathlib.Path(path).read_text())

gen1 = {}
gen2 = {}
try:
    gen1 = load_json(out / "phases/import-gen1.stdout")
except Exception as exc:
    gen1 = {"parse_error": str(exc)}
try:
    gen2 = load_json(out / "phases/import-gen2.stdout")
except Exception as exc:
    gen2 = {"parse_error": str(exc)}
summary = {
    "protocol": "oo.mongodb-preview.native-2e0bd-run/v1",
    "source_acceptance": False,
    "model_called": False,
    "ready": False,
    "fulldoc_complete": False,
    "generation_created_on_platform": False,
    "bind_gen1_exit": load_exit("bind-gen1"),
    "bind_gen2_exit": load_exit("bind-gen2"),
    "preflight_exit": load_exit("preflight-port1"),
    "install_exit": load_exit("install"),
    "import_gen1_exit": load_exit("import-gen1"),
    "import_gen2_exit": load_exit("import-gen2"),
    "jsonb_document_exit": load_exit("import-jsonb-document"),
    "import_gen1": gen1,
    "import_gen2": gen2,
    "producer_sha256": "501f2d1a5de071005f015035623fedebce5c3b5d75837242d23e5f17d8cedcf6",
    "pipeline_sha256": sha("$PIPELINE"),
    "pg_builder_sha256": sha("$PGBUILDER"),
    "text_probe": load_json(out / "phases/text-probe.json"),
}
(out / "run-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps({
    "bind1": summary["bind_gen1_exit"],
    "bind2": summary["bind_gen2_exit"],
    "preflight": summary["preflight_exit"],
    "install": summary["install_exit"],
    "gen1": summary["import_gen1_exit"],
    "gen2": summary["import_gen2_exit"],
    "doc": summary["jsonb_document_exit"],
    "g1_state": gen1.get("load_state"),
    "g1_reason": gen1.get("reason"),
    "g1_bound": gen1.get("source_bound"),
    "g2_state": gen2.get("load_state"),
    "g2_reason": gen2.get("reason"),
    "g2_bound": gen2.get("source_bound"),
    "probe": summary["text_probe"],
}, indent=2))
PY
