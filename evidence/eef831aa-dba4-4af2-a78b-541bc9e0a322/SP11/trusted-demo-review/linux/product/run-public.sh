#!/bin/bash
# Run the frozen product entry once. Evidence stays in the private clone until copied.
set -eu
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a322/SP11/trusted-demo-review
CLONE=/tmp/oo-a322-eef831aa-dba4-4af2-a78b-541bc9e0a322-repo
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a322
HOST=${REVIEW}/linux/product-host
START=${CLONE}/deploy/trusted-demo/start.sh
GOOD=${REVIEW}/boundaries/good.json
SRC_EVIDENCE=${CLONE}/evidence/${RUN}/SP11/trusted-demo-identity

if [ ! -f "$REVIEW/linux/preoverlay/docker-run.exit" ] || [ "$(cat "$REVIEW/linux/preoverlay/docker-run.exit")" != "0" ]; then
  echo "preoverlay smoke has not passed" >&2
  printf '%s\n' 2 > "$HOST/refused.exit"
  exit 2
fi
if [ ! -f "$REVIEW/linux/preoverlay/smoke-ok.txt" ]; then
  echo "preoverlay marker is absent" >&2
  printf '%s\n' 2 > "$HOST/refused.exit"
  exit 2
fi
if docker ps -q --filter name=^/oo-a322-preoverlay$ --filter status=running | grep -q .; then
  echo "preoverlay builder is still running" >&2
  printf '%s\n' 3 > "$HOST/refused.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a322 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
mkdir -p "$HOST"
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$HOST/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$HOST/refused.exit"
  exit 3
fi
if [ -e "$REVIEW/product/live/run" ]; then
  echo "refusing to overwrite copied product evidence" >&2
  printf '%s\n' 2 > "$HOST/refused.exit"
  exit 2
fi

copy_out() {
  if [ -d "$SRC_EVIDENCE" ]; then
    mkdir -p "$REVIEW/product"
    cp -R "$SRC_EVIDENCE/." "$REVIEW/product/"
  fi
}
trap copy_out EXIT

set +e
bash "$START" --manifest "$GOOD" --public-fixture > "$HOST/public-fixture.stdout" 2> "$HOST/public-fixture.stderr"
code=$?
set -e
printf '%s\n' "$code" > "$HOST/public-fixture.exit"
copy_out
exit "$code"
