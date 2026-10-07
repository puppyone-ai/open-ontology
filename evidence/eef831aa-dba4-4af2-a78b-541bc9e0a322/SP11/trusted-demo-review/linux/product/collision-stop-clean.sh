#!/bin/bash
# After a ready run: collision must keep volumes, then stop and clean only a322.
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
OUT=${REVIEW}/cleanup
mkdir -p "$OUT"

copy_out() {
  if [ -d "$SRC_EVIDENCE" ]; then
    mkdir -p "$REVIEW/product"
    cp -R "$SRC_EVIDENCE/." "$REVIEW/product/"
  fi
}

snap() {
  name=$1
  docker volume inspect oo-a322-eef831aa-pgdata oo-a322-eef831aa-cargo oo-a322-eef831aa-target \
    --format '{{.Name}} {{.CreatedAt}}' > "$OUT/$name-volumes.txt"
  docker network inspect oo-a322-eef831aa-net --format '{{.Id}} {{.Created}}' > "$OUT/$name-network.txt" 2>/dev/null || true
  docker inspect oo-a322-pg --format '{{.Id}} {{.State.Status}}' > "$OUT/$name-pg.txt" 2>/dev/null || true
}

if [ ! -f "$HOST/public-fixture.exit" ] || [ "$(cat "$HOST/public-fixture.exit")" != "0" ]; then
  echo "public fixture did not exit 0" >&2
  exit 2
fi
snap before-collision
set +e
bash "$START" --manifest "$GOOD" --public-fixture > "$OUT/collision.stdout" 2> "$OUT/collision.stderr"
code=$?
set -e
printf '%s\n' "$code" > "$OUT/collision.exit"
snap after-collision
if [ "$code" != "2" ]; then
  echo "collision exit was $code" >&2
  copy_out
  exit 1
fi
if ! cmp -s "$OUT/before-collision-volumes.txt" "$OUT/after-collision-volumes.txt"; then
  echo "collision changed a volume timestamp" >&2
  copy_out
  exit 1
fi
set +e
bash "$START" --manifest "$GOOD" --stop > "$OUT/stop.stdout" 2> "$OUT/stop.stderr"
code=$?
set -e
printf '%s\n' "$code" > "$OUT/stop.exit"
snap after-stop
if [ "$code" != "0" ]; then
  echo "stop exit was $code" >&2
  copy_out
  exit 1
fi
if ! cmp -s "$OUT/before-collision-volumes.txt" "$OUT/after-stop-volumes.txt"; then
  echo "stop changed a volume timestamp" >&2
  copy_out
  exit 1
fi
set +e
bash "$START" --manifest "$GOOD" --clean > "$OUT/clean.stdout" 2> "$OUT/clean.stderr"
code=$?
set -e
printf '%s\n' "$code" > "$OUT/clean.exit"
docker volume ls --filter name=oo-a322 --format '{{.Name}}' > "$OUT/volumes-after-clean.txt"
docker network ls --filter name=oo-a322 --format '{{.Name}}' > "$OUT/networks-after-clean.txt"
docker ps -a --filter name=oo-a322 --format '{{.ID}} {{.Names}} {{.Status}}' > "$OUT/containers-after-clean.txt"
if [ -e "/tmp/oo-a322-${RUN}" ]; then
  echo "private tmp remains" > "$OUT/tmp-after-clean.txt"
else
  echo "private tmp absent" > "$OUT/tmp-after-clean.txt"
fi
copy_out
if [ "$code" != "0" ]; then
  echo "clean exit was $code" >&2
  exit 1
fi
if [ -s "$OUT/volumes-after-clean.txt" ] || [ -s "$OUT/networks-after-clean.txt" ] || [ -s "$OUT/containers-after-clean.txt" ]; then
  echo "owned docker objects remain" >&2
  exit 1
fi
set +e
bash "$START" --manifest "$GOOD" --public-fixture > "$OUT/collision-after-clean.stdout" 2> "$OUT/collision-after-clean.stderr"
code=$?
set -e
printf '%s\n' "$code" > "$OUT/collision-after-clean.exit"
copy_out
if [ "$code" != "2" ]; then
  echo "post-clean public fixture exit was $code" >&2
  exit 1
fi
set +e
bash "$START" --manifest "$GOOD" --clean > "$OUT/clean-again.stdout" 2> "$OUT/clean-again.stderr"
code=$?
set -e
printf '%s\n' "$code" > "$OUT/clean-again.exit"
copy_out
docker ps --format '{{.ID}} {{.Names}} {{.Status}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$OUT/foreign-after.txt"
if [ "$code" != "0" ]; then
  echo "second clean exit was $code" >&2
  exit 1
fi
echo "cleanup=ok"
