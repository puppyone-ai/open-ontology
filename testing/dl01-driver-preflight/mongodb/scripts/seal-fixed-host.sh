#!/bin/bash
# One Rust slot. Seals the canonical customer envelopes. Does not import.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
ARCHIVE="/tmp/oo-w141-archive"
CANON="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-67-canonical"
EVIDENCE="${SEAL_EVIDENCE:-$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-68-seal}"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
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
if [[ ! -f "$CANON/status.txt" ]] || ! grep -qx 'DONE' "$CANON/status.txt"; then
  echo "canonical jsonb is not done" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$RUST"; then
  echo "named container $RUST still present" >&2
  exit 2
fi

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
  -e CARGO_INCREMENTAL=1 \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$ARCHIVE:/src" \
  -v "$SRC/scripts/seal-fixed-inside.sh:/inside.sh:ro" \
  -v "$SRC/scripts/w141-seal-test.rs.txt:/snippet:ro" \
  -v "$CANON/envelopes:/envelopes:ro" \
  -v "$EVIDENCE:/evidence" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  bash /inside.sh >"$EVIDENCE/docker.stdout" 2>"$EVIDENCE/docker.stderr"
rc=$?
set -e
printf '%s\n' "$rc" >"$EVIDENCE/docker.exit"
if [[ "$rc" -ne 0 && ! -f "$EVIDENCE/status.txt" ]]; then
  printf '%s\n' "FAILED docker $rc" >"$EVIDENCE/status.txt"
fi
got="$(sha256sum "$ARCHIVE/crates/oo-core/src/jsonb_doc.rs" | awk '{print $1}')"
if [[ "$got" != "38369c072bd09ce09efca48ed86811210d4761b1ddc179c73d253c7af21408e2" ]]; then
  echo "archive jsonb_doc.rs was not restored" >&2
  exit 3
fi
exit "$rc"
