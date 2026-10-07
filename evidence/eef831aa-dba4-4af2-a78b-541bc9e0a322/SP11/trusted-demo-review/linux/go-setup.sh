#!/bin/bash
# Archive the frozen demo and run manifest refusals. Does not start Docker.
set -eu
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a322/SP11/trusted-demo-review
CLONE=/tmp/oo-a322-eef831aa-dba4-4af2-a78b-541bc9e0a322-repo
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a322
PRODUCT=895864a5bf64e50cef84075d20fd7b0c9e41c264
CLI=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480
OUT=${REVIEW}/boundaries
START=""
MANIFEST=""
fail=0

mkdir -p "$OUT" "$REVIEW/source" "$REVIEW/recipes" "$REVIEW/linux/product-host"
docker ps --format '{{.ID}} {{.Names}} {{.Status}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$REVIEW/linux/foreign-before.txt"
docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ' > "$REVIEW/linux/builders-before.txt"

if [ ! -d "$CLONE/.git" ]; then
  git clone --shared --no-checkout "$SRC" "$CLONE"
fi
git -C "$CLONE" checkout "$PRODUCT" -- deploy/trusted-demo
rm -rf "$REVIEW/source/deploy"
mkdir -p "$REVIEW/source"
git -C "$SRC" archive --format=tar "$PRODUCT" deploy/trusted-demo | tar -x -C "$REVIEW/source"
if ! diff -rq "$REVIEW/source/deploy/trusted-demo" "$CLONE/deploy/trusted-demo" > "$REVIEW/source/archive-checkout.diff"; then
  echo "archive and checkout differ" >&2
  exit 2
fi
find "$REVIEW/source/deploy" -type l > "$REVIEW/source/symlinks.txt"
if [ -s "$REVIEW/source/symlinks.txt" ]; then
  echo "frozen product contains a symlink" >&2
  exit 2
fi
{
  echo "product=$PRODUCT"
  echo "product_tree=$(git -C "$SRC" rev-parse "${PRODUCT}^{tree}")"
  echo "product_deploy_tree=$(git -C "$SRC" rev-parse "${PRODUCT}^{tree}:deploy/trusted-demo")"
  echo "cli=$CLI"
  echo "cli_tree=$(git -C "$SRC" rev-parse "${CLI}^{tree}")"
  echo "head=$(git -C "$SRC" rev-parse HEAD)"
} > "$REVIEW/source/trees.txt"
cp "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh" "$REVIEW/recipes/a204-preoverlay-launch.sh"
cp "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh" "$REVIEW/recipes/a200-proof-launch.sh"
cp "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh" "$REVIEW/recipes/a199-overlay-launch.sh"
START=$CLONE/deploy/trusted-demo/start.sh
MANIFEST=$CLONE/deploy/trusted-demo/linux/manifest.sh
chmod 755 "$START"

GOOD='{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"a322","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0a322","source_id":"public_shop"}'
HIST='{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"a305","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0a305","source_id":"public_shop"}'
printf '%s\n' "$GOOD" > "$OUT/good.json"
printf '%s\n' "$HIST" > "$OUT/historical.json"
printf '%s\n' '{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"a321","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0a322","source_id":"public_shop"}' > "$OUT/owner-mismatch.json"
printf '%s\n' '{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"a322","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0a321","source_id":"public_shop"}' > "$OUT/uuid-mismatch.json"
printf '%s\n' '{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"a322","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0A322","source_id":"public_shop"}' > "$OUT/uppercase.json"
printf '%s\n\n' "$GOOD" > "$OUT/second-line.json"
printf '%s\n' '{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"a322","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0a322","source_id":"public_shop","note":"x"}' > "$OUT/extra-key.json"
printf '%s\n' '{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"0000000000000000000000000000000000000000","owner":"a322","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0a322","source_id":"public_shop"}' > "$OUT/wrong-fixed.json"
printf '%s\r\n' "$GOOD" > "$OUT/carriage.json"
dd if=/dev/zero bs=513 count=1 of="$OUT/too-large.bin" >/dev/null 2>&1
ln -s "$OUT/good.json" /tmp/oo-a322-manifest-link.json

run_case() {
  name=$1
  expect=$2
  shift 2
  set +e
  "$@" > "$OUT/$name.stdout" 2> "$OUT/$name.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "$OUT/$name.exit"
  if [ "$code" != "$expect" ]; then
    echo "case $name exit $code expected $expect" >&2
    fail=1
  fi
}

expect_err() {
  name=$1
  needle=$2
  if ! grep -F "$needle" "$OUT/$name.stderr" >/dev/null; then
    echo "case $name stderr missing: $needle" >&2
    fail=1
  fi
}

parse_only() {
  name=$1
  file=$2
  bash -c '
    set -eu
    . "$1"
    load_deployment "$2"
    bind_resources "$3"
    printf "OWNER=%s\nRUN_ID=%s\nNET=%s\nPG_NAME=%s\nCARGO_VOL=%s\nTARGET_VOL=%s\nPG_VOL=%s\nROOT=%s\nEVIDENCE=%s\nDB_SRC=%s\nDB_DST=%s\nROLE_READER=%s\nROLE_WRITER=%s\nROLE_RUNTIME=%s\nROLE_LOGIN=%s\nSECRET_REF=%s\nSHARD=%s\nPROJECT=%s\nFIXED=%s\n" \
      "$OWNER" "$RUN_ID" "$NET" "$PG_NAME" "$CARGO_VOL" "$TARGET_VOL" "$PG_VOL" "$ROOT" "$EVIDENCE" \
      "$DB_SRC" "$DB_DST" "$ROLE_READER" "$ROLE_WRITER" "$ROLE_RUNTIME" "$ROLE_LOGIN" "$SECRET_REF" "$SHARD" "$PROJECT" "$FIXED_SOURCE"
  ' bash "$MANIFEST" "$file" "$CLONE" > "$OUT/$name.stdout" 2> "$OUT/$name.stderr"
  printf '%s\n' 0 > "$OUT/$name.exit"
}

run_case no-args 2 bash "$START"
expect_err no-args "usage:"
run_case no-manifest 2 bash "$START" --public-fixture
expect_err no-manifest "usage:"
run_case missing-file 2 bash "$START" --manifest "$OUT/absent.json" --public-fixture
expect_err missing-file "deployment manifest must be a regular file"
run_case symlink 2 bash "$START" --manifest /tmp/oo-a322-manifest-link.json --public-fixture
expect_err symlink "deployment manifest must be a regular file"
run_case owner-mismatch 2 bash "$START" --manifest "$OUT/owner-mismatch.json" --public-fixture
expect_err owner-mismatch "owner is not the last four digits of run_id"
run_case uuid-mismatch 2 bash "$START" --manifest "$OUT/uuid-mismatch.json" --public-fixture
expect_err uuid-mismatch "owner is not the last four digits of run_id"
run_case uppercase 2 bash "$START" --manifest "$OUT/uppercase.json" --public-fixture
expect_err uppercase "run_id is not a lowercase uuid"
run_case second-line 2 bash "$START" --manifest "$OUT/second-line.json" --public-fixture
expect_err second-line "deployment manifest must be one object"
run_case extra-key 2 bash "$START" --manifest "$OUT/extra-key.json" --public-fixture
expect_err extra-key "deployment manifest source_id is not public_shop"
run_case wrong-fixed 2 bash "$START" --manifest "$OUT/wrong-fixed.json" --public-fixture
expect_err wrong-fixed "deployment manifest fields are not the canonical object"
run_case carriage 2 bash "$START" --manifest "$OUT/carriage.json" --public-fixture
expect_err carriage "deployment manifest must be one object"
run_case too-large 2 bash "$START" --manifest "$OUT/too-large.bin" --public-fixture
expect_err too-large "deployment manifest is too large"

parse_only good-parse "$OUT/good.json"
cat > "$OUT/good-parse.expect" <<EOF
OWNER=a322
RUN_ID=${RUN}
NET=oo-a322-eef831aa-net
PG_NAME=oo-a322-pg
CARGO_VOL=oo-a322-eef831aa-cargo
TARGET_VOL=oo-a322-eef831aa-target
PG_VOL=oo-a322-eef831aa-pgdata
ROOT=/tmp/oo-a322-${RUN}
EVIDENCE=${CLONE}/evidence/${RUN}/SP11/trusted-demo-identity
DB_SRC=oo_a322_src
DB_DST=oo_a322_dst
ROLE_READER=oo_a322_reader
ROLE_WRITER=oo_a322_writer
ROLE_RUNTIME=oo_a322_runtime
ROLE_LOGIN=oo_a322_login
SECRET_REF=secret://pg/a322/reader
SHARD=a322main
PROJECT=open-ontology
FIXED=${CLI}
EOF
if ! cmp -s "$OUT/good-parse.expect" "$OUT/good-parse.stdout"; then
  echo "good manifest names differ" >&2
  fail=1
fi
parse_only historical-parse "$OUT/historical.json"
grep -q '^OWNER=a305$' "$OUT/historical-parse.stdout"
grep -q '^PG_NAME=oo-a305-pg$' "$OUT/historical-parse.stdout"
if [ -d "$CLONE/evidence" ]; then
  echo "manifest parse created an evidence directory" >&2
  fail=1
fi
if [ -e "/tmp/oo-a322-${RUN}" ] || [ -e "/tmp/oo-a305-eef831aa-dba4-4af2-a78b-541bc9e0a305" ]; then
  echo "manifest parse created a private tmp" >&2
  fail=1
fi
docker ps -a --filter name=oo-a322 --format '{{.ID}} {{.Names}}' > "$OUT/docker-a322-after.txt"
docker ps -a --filter name=oo-a305 --format '{{.ID}} {{.Names}}' > "$OUT/docker-a305-after.txt"
if [ -s "$OUT/docker-a322-after.txt" ] || [ -s "$OUT/docker-a305-after.txt" ]; then
  echo "manifest refusal created a container" >&2
  fail=1
fi
rm -f /tmp/oo-a322-manifest-link.json
if [ "$fail" -ne 0 ]; then
  printf '%s\n' 1 > "$OUT/summary.exit"
  exit 1
fi
printf '%s\n' 0 > "$OUT/summary.exit"
echo "boundaries=ok"
