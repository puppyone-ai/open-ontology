#!/bin/bash
# One continuation builder and one new private PostgreSQL.
# Reuses the smoke cargo and target volumes. Does not rerun passed suites.
# Does not overwrite linux/smoke or the historical pg/ proof.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a360/SP06/gateway-tls-review
PRIOR=${BASE}/linux/smoke
EVIDENCE=${BASE}/linux/smoke-2
PGOUT=${EVIDENCE}/pg
ROOT=/tmp/oo-a360-eef831aa-dba4-4af2-a78b-541bc9e0a360/smoke-2
SEC=/tmp/oo-a360-eef831aa-dba4-4af2-a78b-541bc9e0a360/secrets
FIXTURE=/tmp/oo-a360-eef831aa-dba4-4af2-a78b-541bc9e0a360/builder-fixture
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PGIMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NAME=oo-a360-smoke-2
PG_NAME=oo-a360-pg
NET=oo-a360-eef831aa-net
CARGO_VOL=oo-a360-eef831aa-cargo
TARGET_VOL=oo-a360-eef831aa-target
PG_VOL=oo-a360-eef831aa-pgdata
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a360

chmod 755 "$BASE/run/inside-continue.sh" "$BASE/run/continue.sh" "$BASE/pg/pg-wrap.sh" "$BASE/pg/init-roles.sh"
mkdir -p "$EVIDENCE" "$PGOUT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke-2 console.log" >&2
  exit 2
fi
if [ ! -f "$PRIOR/docker-run.exit" ] || [ "$(cat "$PRIOR/docker-run.exit")" != "101" ]; then
  echo "smoke proof is not the preserved clippy failure" >&2
  exit 2
fi
if [ ! -f "$PRIOR/a350-live.exit" ] || [ "$(cat "$PRIOR/a350-live.exit")" != "0" ]; then
  echo "a350 live proof is not a preserved pass" >&2
  exit 2
fi

trap 'code=$?; if [ ! -f "$EVIDENCE/docker-run.exit" ]; then printf "%s\n" "$code" > "$EVIDENCE/docker-run.exit"; fi' EXIT

finish_pg() {
  docker rm -f "$PG_NAME" >/dev/null 2>&1 || true
  docker volume rm "$PG_VOL" >/dev/null 2>&1 || true
  if [ -d "$SEC" ]; then
    python3 - "$EVIDENCE" "$SEC" <<'PY'
import pathlib, re, sys
root = pathlib.Path(sys.argv[1])
sec = pathlib.Path(sys.argv[2])
needles = []
for path in sec.iterdir():
    if path.is_file() and path.stat().st_size <= 128:
        text = path.read_bytes().strip()
        if len(text) >= 16:
            needles.append(text)
pattern = re.compile(rb"ooct_[A-Za-z0-9._:-]+|oost_[A-Za-z0-9._:-]+")
skip = {"work-sha256.txt", "work-sha256-after.txt", "lock-sha256.txt", "review-test-sha256.txt"}
for path in root.rglob("*"):
    if not path.is_file() or path.name in skip:
        continue
    if path.suffix not in {".txt", ".log", ".stdout", ".stderr", ".json"} and path.name != "console.log":
        continue
    data = path.read_bytes()
    updated = pattern.sub(lambda m: m.group(0).split(b"_", 1)[0] + b"_REDACTED", data)
    for needle in needles:
        updated = updated.replace(needle, b"REDACTED")
    if updated != data:
        path.write_bytes(updated)
PY
  fi
  rm -rf "$SEC" "$FIXTURE" "$ROOT"
}

capacity() {
  own=$(docker ps -q --filter label=oo.owner=a360 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
  printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo"
  if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
    return 3
  fi
  return 0
}

{
  echo "reused_attempt=linux/smoke"
  echo "not_rerun=cargo_check author_clippy loopback tls_default fixture bundle a350_live"
  for f in rustc.exit pre-edit.exit archive-match.exit libclang.exit smoke.exit clippy-component.exit clippy.exit loopback-test.exit tls-test.exit tls-count.exit fixture-rustc.exit bundle.exit a350-live.exit review-clippy.exit docker-run.exit; do
    printf 'prior_%s=%s\n' "$f" "$(tr -d '\n' < "$PRIOR/$f")"
  done
  printf 'prior_pg_up_exit=%s\n' "$(tr -d '\n' < "$BASE/pg/up.exit")"
  printf 'prior_clippy_component_note=%s\n' "$(head -n 1 "$PRIOR/clippy-component.stderr" | tr -d '\r')"
} > "$EVIDENCE/reused-from-smoke.txt"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker image inspect "$PGIMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  echo "smoke cargo or target volume is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker volume inspect "$CARGO_VOL" "$TARGET_VOL" > "$EVIDENCE/reused-volumes.txt"
cap_line=$(capacity) || cap_rc=$?
cap_rc=${cap_rc:-0}
printf '%s\n' "$cap_line" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$cap_rc" -ne 0 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

docker image inspect "$PGIMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PGOUT/image.txt"
grep -q 'arch=arm64' "$PGOUT/image.txt"
own_pg=$(docker ps -q --filter label=oo.owner=a360 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$PGOUT/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${PG_NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "pg container name already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$PGOUT/network.txt"

umask 077
mkdir -p "$SEC" "$FIXTURE"
chmod 700 "$SEC" "$FIXTURE"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/token-admin.pass"
openssl rand -hex 24 > "$SEC/token-runtime.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a360-ca" >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/wrong.key" -out "$SEC/wrong.crt" -days 7 -subj "/CN=oo-a360-wrong" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a360-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a360-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt" "$SEC/wrong.crt"
cp "$SEC/ca.crt" "$SEC/wrong.crt" "$SEC/admin.pass" "$SEC/token-admin.pass" "$SEC/token-runtime.pass" "$FIXTURE/"
chmod 644 "$FIXTURE/ca.crt" "$FIXTURE/wrong.crt"
chmod 600 "$FIXTURE/admin.pass" "$FIXTURE/token-admin.pass" "$FIXTURE/token-runtime.pass"
find "$FIXTURE" -type f -print | sort > "$PGOUT/builder-fixture-files.txt"
if find "$FIXTURE" -type f \( -name '*.key' -o -name 'server.crt' \) | grep -q .; then
  echo "builder fixture contains a private or server key" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PGOUT/server-san.txt"
if find "$PGOUT" -type f \( -name '*.key' -o -name '*.pass' -o -name 'server.crt' \) | grep -q .; then
  echo "evidence pg dir contains secret material" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if docker volume inspect "$PG_VOL" >/dev/null 2>&1; then
  echo "postgres volume already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a360 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  "$PG_VOL" > "$PGOUT/volume-data.txt"

docker run -d --name "$PG_NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETUID \
  --cap-add SETGID \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias oo-a360-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=oo_a360 \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${BASE}/pg,target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a360 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=gateway-tls-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$PGIMAGE" > "$PGOUT/container.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40; do
  if docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d oo_a360 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$PG_NAME" > "$PGOUT/postgres-start.log" 2>&1 || true
  finish_pg
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker exec "$PG_NAME" /fixture/init-roles.sh > "$PGOUT/roles.txt" 2> "$PGOUT/roles-err.txt" || {
  sed -E "s/PASSWORD '[0-9a-fA-F]+'/PASSWORD 'redacted'/g" "$PGOUT/roles-err.txt" > "$PGOUT/roles-err-redacted.txt" || true
  rm -f "$PGOUT/roles-err.txt"
  echo "role creation failed" >&2
  finish_pg
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
}
rm -f "$PGOUT/roles-err.txt"
docker inspect "$PG_NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$PGOUT/inspect.txt"
docker inspect "$PG_NAME" --format '{{json .HostConfig.PortBindings}}' > "$PGOUT/ports.json"
docker inspect "$PG_NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PGOUT/caps.json"
printf '%s\n' 0 > "$PGOUT/up.exit"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"

cap_line=$(capacity) || cap_rc=$?
cap_rc=${cap_rc:-0}
printf '%s\n' "$cap_line" > "$EVIDENCE/capacity-before-rust.txt"
if [ "$cap_rc" -ne 0 ]; then
  echo "deferred: rust builder capacity after postgres start" >&2
  finish_pg
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 reused-target=oo-a360-eef831aa-target bash /inside-continue.sh" > "$EVIDENCE/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A360_LIVE=1 \
  -e OO_A360_PG_HOST=oo-a360-pg \
  -e OO_A360_PG_PORT=5432 \
  -e OO_A360_PG_DATABASE=oo_a360 \
  -e OO_A360_ADMIN_PASSWORD_FILE=/pg-fixture/token-admin.pass \
  -e OO_A360_RUNTIME_PASSWORD_FILE=/pg-fixture/token-runtime.pass \
  -e OO_A360_CA_FILE=/pg-fixture/ca.crt \
  -e OO_A360_MARKER=/evidence/a360-review-marker.txt \
  -e OO_A360_SUMMARY=/evidence/review-summary.txt \
  -e OO_A360_FIXTURE_DIR=/target/work/testing/real-db/gateway-compiled-pack/fixtures \
  --mount "type=bind,source=${BASE}/run/inside-continue.sh,target=/inside-continue.sh,readonly" \
  --mount "type=bind,source=${BASE}/tools/review_tls.rs,target=/review/review_tls.rs,readonly" \
  --mount "type=bind,source=${PRIOR},target=/prior,readonly" \
  --mount "type=bind,source=${FIXTURE},target=/pg-fixture,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a360 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=gateway-tls-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside-continue.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
if docker container inspect "$NAME" >/dev/null 2>&1; then
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
    echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
    echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
  } > "$EVIDENCE/hostconfig.txt"
  docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
  docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
  docker rm -f "$NAME" > "$EVIDENCE/container-removed.txt"
fi
finish_pg
if [ "$run_exit" -eq 0 ]; then
  docker volume rm "$CARGO_VOL" > "$EVIDENCE/cargo-volume-removed.txt" 2>&1 || true
  docker volume rm "$TARGET_VOL" > "$EVIDENCE/target-volume-removed.txt" 2>&1 || true
  docker network rm "$NET" > "$EVIDENCE/network-removed.txt" 2>&1 || true
fi
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
exit "$run_exit"
