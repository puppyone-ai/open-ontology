#!/bin/bash
# Recompute resolve membership from saved metadata. One short builder.
# The first facts files matched package ids as "name version", which Cargo 1.94 does not emit.
set -eu
REPO="/Users/redux/puppy-one/open-ontology"
EVID="$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a117/SP00/supported-target-license-gate"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
NAME="eef831aa-dba4-4af2-a78b-541bc9e0a117-rust"
LOG="$EVID/linux/rescan.log"
RAW="$EVID/raw"

say() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$LOG"; }

ps -ax -o pid=,comm= >"$EVID/linux/processes-rescan.txt"
docker ps --format '{{.ID}} {{.Image}} {{.Names}} {{.Status}}' >"$EVID/linux/docker-ps-rescan.txt"
host_rust=$(awk '$2=="cargo" || $2=="rustc" || $2=="rustup" || $2=="rustdoc" { c++ } END { print c+0 }' "$EVID/linux/processes-rescan.txt")
rust_containers=$(awk 'BEGIN{c=0} { if (tolower($2) ~ /rust/ || tolower($3) ~ /rust/) c++ } END { print c+0 }' "$EVID/linux/docker-ps-rescan.txt")
total=$((host_rust + rust_containers))
printf 'host_rust_comm=%s\nrust_named_or_image_containers=%s\ntotal=%s\n' "$host_rust" "$rust_containers" "$total" >"$EVID/linux/builder-count-rescan.txt"
if [ "$total" -ge 2 ]; then
  say "FAILED builder cap total=$total"
  exit 6
fi

for phase in before after; do
  for name in aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu wasm32-unknown-unknown all-target; do
    src="$RAW/$phase/$name/facts.txt"
    if [ -f "$src" ] && [ ! -f "$RAW/$phase/$name/facts.broken-id-match.txt" ]; then
      mv "$src" "$RAW/$phase/$name/facts.broken-id-match.txt"
    fi
  done
done

cleanup() {
  local status=$?
  trap - EXIT
  docker stop "$NAME" >"$EVID/linux/rescan-docker-stop.txt" 2>&1 || true
  docker rm "$NAME" >"$EVID/linux/rescan-docker-rm.txt" 2>&1 || true
  echo "$status" >"$EVID/linux/rescan.exit"
  if [ "$status" -eq 0 ]; then say "DONE rescan"; else say "FAILED rescan exit=$status"; fi
  exit "$status"
}
trap cleanup EXIT

say "start rescan container"
docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 2g \
  --memory-swap 2g \
  --cpus 2 \
  --pids-limit 512 \
  --network none \
  --workdir /evidence \
  --tmpfs /work:rw,exec,nosuid,nodev,size=256m \
  -e CARGO_TERM_COLOR=never \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/work/home \
  -e TMPDIR=/work \
  --mount "type=bind,source=${EVID},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a117 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a117 \
  --label oo.task=supported-target-license-gate \
  --label oo.kind=rust-builder \
  --label oo.slot=B \
  "$IMAGE" \
  sleep 600 >"$EVID/linux/rescan.container.id"

say "compile and scan"
docker exec "$NAME" bash -c '
set -eu
rustc -vV > /evidence/linux/rescan-rustc-vv.txt
rustc --edition 2021 -D warnings -o /work/meta_facts /evidence/probe/meta_facts.rs
/work/meta_facts test
for phase in before after; do
  for name in aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu wasm32-unknown-unknown all-target; do
    /work/meta_facts scan "/evidence/raw/$phase/$name/metadata.json" "/evidence/raw/$phase/$name/facts.txt"
    printf "%s\n" 0 > "/evidence/raw/$phase/$name/facts.exit"
  done
done
'
say "scan exec done"
