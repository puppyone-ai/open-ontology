#!/usr/bin/env bash
# Host side. Runs the T9 integration test in a capped rust:1.94.0 container (detached).
# Inside the container: copy the sources, build the producer and the loader, install the
# migration chain through the T7 live test, then run the ignored tests of c1_v2_driver_live.rs.
# Follow progress with: docker logs -f oo-cb-T9-run   (ends with the line T9_DONE)
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
FIX="${OO_CB_T9_HOME:-/tmp/oo-cb-T9/fixture}"
OUT="${OO_CB_T9_RUN_DIR:-/tmp/oo-cb-T9/run}"
PREP="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/in-container.sh"
NAME=oo-cb-T9-run
mkdir -p "$OUT" "$FIX"
docker rm -f "$NAME" >/dev/null 2>&1 || true
# The loader accepts ssl=disable only for 127.0.0.1. The rust container therefore shares the
# PostgreSQL container's network namespace, so PostgreSQL is at 127.0.0.1:5432 from inside.
# MySQL is reached over the bridge network, and its certificate is checked against "localhost".
MYSQL_IP="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' oo-cb-T9-mysql57)"
[[ -n "$MYSQL_IP" ]] || { echo "oo-cb-T9-mysql57 has no address; run up.sh first" >&2; exit 1; }
CMD='set -e
cd /work
apt-get update -qq >/dev/null 2>&1 && apt-get install -y -qq default-mysql-client >/dev/null 2>&1
cargo build --locked --offline --manifest-path tools/mysql-preview/Cargo.toml
cargo build --locked --offline -p oo-pipeline --bin oo-pipeline
# Step 3 installs 0000-0042 (install_everything runs before the scenarios). The T7 scenarios
# also read .arrow fixtures that are git-excluded and absent here, so their failure is expected.
OO_C1V2_HOST=127.0.0.1 OO_C1V2_PORT=5432 OO_C1V2_PASSWORD_FILE=/fixture/secrets/pg-super.pw \
  cargo test --locked --offline -p oo-pipeline --test native_c1_chunked_apply_live live_v2_loader_operations_match_the_design -- --ignored --test-threads=1 \
  || echo "T7_STEP_EXIT=$? (chain install ran; scenario fixtures absent in this checkout)"
cargo test --locked --offline --manifest-path tools/job-worker/Cargo.toml --test c1_v2_driver_live -- --ignored --test-threads=1 --nocapture
echo T9_DONE'
docker run -d --name "$NAME" --platform linux/arm64 --memory 16g --cpus 4 --network container:oo-cb-T9-pg17 \
  -v "$REPO:/source:ro" -v oo-cb-T9-target:/target -v oo-chunked-baseline-cargo-home:/cargo-vol \
  -v "$FIX:/fixture:ro" -v "$OUT:/out" -v "$PREP:/cfg/prepare.sh:ro" \
  -e OO_CB_T9_MYSQL_HOST="$MYSQL_IP" -e OO_CB_T9_MYSQL_PORT=3306 -e OO_CB_T9_MYSQL_CLI=mysql \
  -e OO_CB_T9_MYSQL_ROOT_PW_FILE=/fixture/secrets/mysql-root.pw -e OO_CB_T9_MYSQL_WRITER_PW_FILE=/fixture/secrets/writer.pw \
  -e OO_CB_T9_MYSQL_CDC_PW_FILE=/fixture/secrets/cdc.pw -e OO_CB_T9_CA=/fixture/certs/ca.pem -e OO_CB_T9_TLS_NAME=localhost \
  -e OO_CB_T9_PG_HOST=127.0.0.1 -e OO_CB_T9_PG_PORT=5432 -e OO_CB_T9_PG_DATABASE=oo_cb_t7_apply \
  -e OO_CB_T9_PG_SUPER_PW_FILE=/fixture/secrets/pg-super.pw -e OO_CB_T9_PG_RUNTIME_PW_FILE=/fixture/secrets/runtime.pw \
  -e OO_CB_T9_PRODUCER_BIN=/target/debug/mysql-preview -e OO_CB_T9_LOADER_BIN=/target/debug/oo-pipeline \
  -e OO_CB_T9_WORK=/out/t9 \
  -e CMD="$CMD" rust:1.94.0 bash /cfg/prepare.sh >/dev/null
echo "started $NAME; follow with: docker logs -f $NAME"
