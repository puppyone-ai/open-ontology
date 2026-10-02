#!/bin/bash
# One a114 Rust builder. Does not stop, remove, or prune any other run.
set -eu
REPO="/Users/redux/puppy-one/open-ontology"
REVIEW="$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a114/SP00/helper-record-trust-review"
FIX="6d2f7cdb935c7debad0dbb5292074f325d1948cb"
TREE="dad005c861fca7fd906f49551169777a37b1c5d0"
PARENT="d67795e397657087495a32317938ecdd7a6cb028"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADOPT="f1a6005971e3dc3075094c87ed670fc9d25e9ae0"
LOCK_SHA="14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1"
UUID=$(uuidgen | tr '[:upper:]' '[:lower:]')
BASE="/tmp/oo-a114-${UUID}"
NAME="eef831aa-dba4-4af2-a78b-541bc9e0a114-rust"
NET="oo-a114-${UUID}-net"
VOLC="oo-a114-${UUID}-cargo"
VOLT="oo-a114-${UUID}-target"
mkdir -p "$BASE" "$REVIEW/linux"
printf '%s\n' "$UUID" >"$REVIEW/linux/run-id.txt"
printf '%s\n' "$BASE" >"$REVIEW/linux/tmp-base.txt"
printf '%s\n' "$NAME" >"$REVIEW/linux/container-name.txt"
printf '%s\n' "$$" >"$REVIEW/linux/driver.pid"
date -u +%Y-%m-%dT%H:%M:%SZ >"$REVIEW/linux/registered-at.txt"

cleanup() {
  local status=$?
  trap - EXIT
  if [ -n "${NAME:-}" ]; then
    docker stop "$NAME" >"$REVIEW/linux/docker-stop.txt" 2>&1 || true
    docker rm "$NAME" >"$REVIEW/linux/docker-rm.txt" 2>&1 || true
  fi
  if [ -n "${NET:-}" ]; then
    docker network rm "$NET" >"$REVIEW/linux/network-rm.txt" 2>&1 || true
  fi
  if [ -n "${VOLC:-}" ]; then
    docker volume rm "$VOLC" "$VOLT" >"$REVIEW/linux/volume-rm.txt" 2>&1 || true
  fi
  docker ps -a --filter "name=eef831aa-dba4-4af2-a78b-541bc9e0a114-rust" --format '{{.ID}} {{.Names}} {{.Status}}' >"$REVIEW/linux/docker-ps-owned-after.txt" || true
  docker ps -a --format '{{.ID}} {{.Names}} {{.Status}}' >"$REVIEW/linux/docker-ps-after.txt" || true
  if [ -n "${BASE:-}" ] && [ -f "$BASE/expected-outcomes.tsv" ]; then
    if cmp -s "$BASE/expected-outcomes.tsv" "$REVIEW/expected-outcomes.tsv" && cmp -s "$BASE/age-floor-preregistration.tsv" "$REVIEW/age-floor-preregistration.tsv"; then
      echo "expected-outcomes unchanged" >"$REVIEW/linux/expected-unchanged.txt"
    else
      echo "EXPECTED OUTCOMES CHANGED" >"$REVIEW/linux/expected-unchanged.txt"
      status=5
    fi
  fi
  if [ -n "${BASE:-}" ] && [ -d "$BASE" ]; then
    rm -rf "$BASE"
    echo "removed $BASE" >"$REVIEW/linux/tmp-removed.txt"
  fi
  echo "$status" >"$REVIEW/linux/driver.exit"
  exit "$status"
}
trap cleanup EXIT

cd "$REPO"
{
  echo "FIX=$(git rev-parse "$FIX")"
  echo "TREE=$(git rev-parse "$FIX^{tree}")"
  echo "PARENT=$(git rev-parse "$FIX^")"
  echo "HEAD=$(git rev-parse HEAD)"
  echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "adopt_author=$(git log -1 --format=%aI "$ADOPT")"
  echo "adopt_committer=$(git log -1 --format=%cI "$ADOPT")"
  echo "lock_blob=$(git rev-parse "$ADOPT:testing/harness/containment-supervisor/Cargo.lock")"
} >"$REVIEW/linux/git-identity.txt"

got_fix=$(git rev-parse "$FIX")
got_tree=$(git rev-parse "$FIX^{tree}")
got_parent=$(git rev-parse "$FIX^")
if [ "$got_fix" != "$FIX" ] || [ "$got_tree" != "$TREE" ] || [ "$got_parent" != "$PARENT" ]; then
  echo "FAILED identity fix=$got_fix tree=$got_tree parent=$got_parent" | tee "$REVIEW/linux/status.txt"
  exit 4
fi
author_time=$(git log -1 --format=%aI "$ADOPT")
committer_time=$(git log -1 --format=%cI "$ADOPT")
utc=$(python3 -c 'from datetime import datetime, timezone; import sys; print(datetime.fromisoformat(sys.argv[1]).astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))' "$author_time")
{
  echo "author=$author_time"
  echo "committer=$committer_time"
  echo "utc=$utc"
} >"$REVIEW/linux/adoption-time.txt"
if [ "$author_time" != "2026-10-02T19:10:37+08:00" ] || [ "$committer_time" != "2026-10-02T19:10:37+08:00" ] || [ "$utc" != "2026-10-02T11:10:37Z" ]; then
  echo "FAILED adoption time" | tee "$REVIEW/linux/status.txt"
  exit 4
fi

cp "$REVIEW/expected-outcomes.tsv" "$BASE/expected-outcomes.tsv"
cp "$REVIEW/age-floor-preregistration.tsv" "$BASE/age-floor-preregistration.tsv"
shasum -a 256 "$BASE/expected-outcomes.tsv" | awk '{print $1}' >"$BASE/expected.sha256"

PATHS=(
  testing/harness/helper-evidence-check/src/check.rs
  testing/harness/helper-evidence-check/src/json.rs
  testing/harness/helper-evidence-check/src/lib.rs
  testing/harness/helper-evidence-check/src/trust.rs
  testing/harness/helper-evidence-check/src/main.rs
  testing/harness/helper-evidence-check/src/fail.rs
  testing/harness/helper-evidence-check/src/sha.rs
  testing/harness/helper-evidence-check/src/timeutil.rs
  testing/harness/helper-evidence-check/tests/trust.rs
  testing/harness/helper-evidence-check/tests/closure.rs
  testing/harness/helper-evidence-check/Cargo.toml
  testing/harness/helper-evidence-check/Cargo.lock
  testing/harness/helper-evidence-check/fixtures/inventory.json
  testing/harness/containment-supervisor/Cargo.toml
  testing/harness/containment-supervisor/Cargo.lock
  ci/deny.toml
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/age.txt
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/advisory-rev.txt
  testing/harness/containment-supervisor/provenance/audit-records/bitflags-2.13.2.json
  testing/harness/containment-supervisor/provenance/audit-records/cfg-if-1.0.5.json
  testing/harness/containment-supervisor/provenance/audit-records/cfg_aliases-0.2.2.json
  testing/harness/containment-supervisor/provenance/audit-records/libc-0.2.189.json
  testing/harness/containment-supervisor/provenance/audit-records/nix-0.31.3.json
)
: >"$REVIEW/linux/source-pins.sha256"
for path in "${PATHS[@]}"; do
  git show "$FIX:$path" | shasum -a 256 | awk -v p="$path" '{print $1, p}' >>"$REVIEW/linux/source-pins.sha256"
done

mkdir -p "$BASE/archive"
git archive --format=tar "$FIX" \
  testing/harness/helper-evidence-check \
  testing/harness/containment-supervisor/Cargo.toml \
  testing/harness/containment-supervisor/Cargo.lock \
  testing/harness/containment-supervisor/provenance \
  ci/deny.toml \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw \
  | tar -C "$BASE/archive" -xf -
if [ -e "$BASE/archive/crates/oo-connectors" ]; then
  echo "FAILED archive contains connector workspace" | tee "$REVIEW/linux/status.txt"
  exit 3
fi
pin_fail=0
while read -r hash path; do
  [ -n "$hash" ] || continue
  got=$(shasum -a 256 "$BASE/archive/$path" | awk '{print $1}')
  if [ "$got" != "$hash" ]; then
    echo "PIN $path got=$got want=$hash" | tee -a "$REVIEW/linux/status.txt"
    pin_fail=1
  fi
done <"$REVIEW/linux/source-pins.sha256"
if [ "$pin_fail" != "0" ]; then
  echo "FAILED source pins" | tee -a "$REVIEW/linux/status.txt"
  exit 3
fi
archive_lock=$(shasum -a 256 "$BASE/archive/testing/harness/containment-supervisor/Cargo.lock" | awk '{print $1}')
printf '%s\n' "$archive_lock" >"$REVIEW/linux/archive-lock.sha256"
if [ "$archive_lock" != "$LOCK_SHA" ]; then
  echo "FAILED archive lock $archive_lock" | tee "$REVIEW/linux/status.txt"
  exit 3
fi
python3 - "$BASE/archive/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out" <<'PY' >"$REVIEW/linux/audit-status-key.txt"
import json, sys
doc = json.load(open(sys.argv[1]))
print("status_key=" + str("status" in doc))
print("last_commit=" + doc["database"]["last-commit"])
print("last_updated=" + doc["database"]["last-updated"])
print("advisory_count=" + str(doc["database"]["advisory-count"]))
print("dependency_count=" + str(doc["lockfile"]["dependency-count"]))
print("vulnerabilities=" + json.dumps(doc["vulnerabilities"], separators=(",", ":")))
if "status" in doc:
    raise SystemExit(3)
PY

ps -ax -o pid=,comm= >"$REVIEW/linux/processes-before.txt"
docker ps --format '{{.ID}} {{.Image}} {{.Names}} {{.Status}}' >"$REVIEW/linux/docker-ps-before.txt"
docker ps -a --filter "name=eef831aa-dba4-4af2-a78b-541bc9e0a112" --format '{{.ID}} {{.Names}} {{.Status}}' >"$REVIEW/linux/slot-a-a112.txt"
docker ps -a --filter "name=eef831aa-dba4-4af2-a78b-541bc9e0a113" --format '{{.ID}} {{.Names}} {{.Status}}' >"$REVIEW/linux/slot-b-a113.txt"
host_rust=$(awk '$2=="cargo" || $2=="rustc" || $2=="rustup" || $2=="rustdoc" { c++ } END { print c+0 }' "$REVIEW/linux/processes-before.txt")
rust_containers=$(awk 'BEGIN{c=0} tolower($0) ~ /rust/ { c++ } END{ print c+0 }' "$REVIEW/linux/docker-ps-before.txt")
total=$((host_rust + rust_containers))
{
  echo "host_rust_comm=$host_rust"
  echo "rust_image_containers=$rust_containers"
  echo "total=$total"
  echo "slot_a_reserved=eef831aa-dba4-4af2-a78b-541bc9e0a112-rust-base"
  echo "counting=running-only"
} >"$REVIEW/linux/builder-count-before.txt"
if [ "$total" -ge 2 ]; then
  echo "FAILED builder cap total=$total" | tee "$REVIEW/linux/status.txt"
  exit 6
fi
if [ -s "$REVIEW/linux/slot-b-a113.txt" ]; then
  echo "FAILED a113 container still present" | tee "$REVIEW/linux/status.txt"
  exit 6
fi

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a114 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a114 \
  --label oo.task=helper-record-trust-review \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$NET" >"$REVIEW/linux/network-id.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a114 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a114 \
  --label oo.task=helper-record-trust-review \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$VOLC" >"$REVIEW/linux/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a114 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a114 \
  --label oo.task=helper-record-trust-review \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$VOLT" >"$REVIEW/linux/volume-target.txt"

docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /archive \
  --tmpfs /tmp:rw,nosuid,nodev,size=2g \
  --tmpfs /work:rw,nosuid,nodev,size=256m \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/checker \
  -e CARGO_BUILD_JOBS=2 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/work/home \
  -e TMPDIR=/tmp \
  -e GIT_TERMINAL_PROMPT=0 \
  --mount "type=bind,source=${BASE}/archive,target=/archive,readonly" \
  --mount "type=bind,source=${REVIEW},target=/review" \
  --mount "type=volume,source=${VOLC},target=/cargo-home" \
  --mount "type=volume,source=${VOLT},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a114 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a114 \
  --label oo.task=helper-record-trust-review \
  --label oo.kind=rust-builder \
  --label oo.slot=B \
  --label oo.run="$UUID" \
  "$IMAGE" \
  sleep 7200 >"$REVIEW/linux/container.id"

docker inspect --format 'Id={{.Id}} Image={{.Image}} Platform={{.Platform}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}}' "$NAME" >"$REVIEW/linux/hostconfig.txt"

set +e
docker exec "$NAME" bash /review/linux/inside.sh
exec_code=$?
set -e
echo "$exec_code" >"$REVIEW/linux/docker-exec.exit"
if [ -f "$REVIEW/linux/phases/smoke.exit" ]; then
  smoke=$(tr -d '[:space:]' <"$REVIEW/linux/phases/smoke.exit")
else
  smoke=$exec_code
fi
echo "smoke=$smoke exec=$exec_code" >"$REVIEW/linux/status.txt"
exit "$smoke"
