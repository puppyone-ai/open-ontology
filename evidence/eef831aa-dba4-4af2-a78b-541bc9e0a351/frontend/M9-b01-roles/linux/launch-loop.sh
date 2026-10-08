#!/bin/bash
# One private PostgreSQL, one Rust builder, then one Node browser. No host network.
set -eu
export PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a351
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/frontend/M9-b01-roles
SEC=/tmp/oo-fe01-${RUN_ID}/m9-secrets
NET=oo-fe01-net-m9-eef831aa
SERVICE=a6f92da8a0bf872f8a30efb116c2c7b14463505b
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NODE_IMAGE=docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
HOME_DIR=/tmp/oo-fe01-${RUN_ID}/home
SOURCE=/tmp/oo-fe01-${RUN_ID}/m9-source
ARCH=/tmp/oo-fe01-${RUN_ID}/m9-src
VITE_TMP=/tmp/oo-fe01-${RUN_ID}/m9-vite
VITE_CACHE=/tmp/oo-fe01-${RUN_ID}/m9-vite-cache
mkdir -p "$ROOT/raw" "$ROOT/screenshots" "$VITE_TMP" "$VITE_CACHE"
cd "$REPO"
own_rust=$(docker ps -q --filter name=^/oo-fe01-rust-m9$ --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_rust=$(docker ps --format '{{.Image}}' | grep -c '0f24958fa693' || true)
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter name=^/oo-fe01-pg-m9$ --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
node_image=$(docker ps --format '{{.Image}}' | grep -c '481d53bf381928' || true)
own_node=$(docker ps --format '{{.Names}}' | grep -c '^oo-fe01-node' || true)
printf 'own_rust=%s global_rust=%s host_cargo=%s own_pg=%s node_image=%s own_node=%s\n' \
  "$own_rust" "$global_rust" "$host_cargo" "$own_pg" "$node_image" "$own_node" > "$ROOT/raw/loop-capacity.txt"
if [ "$own_rust" -ge 1 ] || [ "$global_rust" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ] || [ "$node_image" -ge 2 ] || [ "$own_node" -ge 1 ]; then
  echo "deferred" >&2
  exit 3
fi
latest=$(git log -1 --format=%H -- tools/control-identity)
printf '%s\n' "$latest" > "$ROOT/raw/identity-head.txt"
if [ "$latest" != "$SERVICE" ]; then
  echo "identity head moved" >&2
  exit 1
fi
git diff --stat -- tools/control-identity > "$ROOT/raw/producer-worktree.diffstat" || true
head=$(git rev-parse HEAD)
printf '%s\n' "$head" > "$ROOT/raw/archive-base.txt"
rm -rf "$SOURCE" "$ARCH"
mkdir -p "$SOURCE" "$ARCH"
git archive --format=tar "$SERVICE" tools/control-identity | tar -x -C "$SOURCE"
git archive --format=tar "$head" | tar -x -C "$ARCH"
cp apps/web/src/api/identity-session.test.ts "$ARCH/apps/web/src/api/identity-session.test.ts"
mkdir -p "$ARCH/apps/web/design/e2e"
cp apps/web/design/e2e/b01.roles.spec.ts "$ARCH/apps/web/design/e2e/b01.roles.spec.ts"
cp apps/web/playwright.b01-roles.config.ts "$ARCH/apps/web/playwright.b01-roles.config.ts"
printf '%s\n' "$SERVICE" > "$ROOT/raw/consumed-service.txt"
rm -f "$ROOT/ready.json" "$ROOT/stop" "$ROOT/subject.request" "$ROOT/subject.applied"

cleanup() {
  touch "$ROOT/stop" || true
  docker logs --tail 400 oo-fe01-rust-m9 > "$ROOT/raw/rust.log" 2>&1 || true
  docker logs --tail 200 oo-fe01-node-m9 > "$ROOT/raw/node-loop.log" 2>&1 || true
  docker rm -f oo-fe01-node-m9 >/dev/null 2>&1 || true
  docker rm -f oo-fe01-rust-m9 >/dev/null 2>&1 || true
  docker rm -f oo-fe01-pg-m9 >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  docker volume rm oo-fe01-m9-cargo oo-fe01-m9-work oo-fe01-m9-pgdata >/dev/null 2>&1 || true
  rm -rf "$SEC" "$SOURCE" "$ARCH" "$VITE_TMP" "$VITE_CACHE"
}
trap cleanup EXIT

umask 077
mkdir -p "$SEC"
cd "$SEC"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/login.pass"
openssl rand -hex 32 > "$SEC/client.secret"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 2 -subj "/CN=oo-fe01-m9-ca" -addext "basicConstraints=CA:TRUE" >/dev/null 2>&1
sign() {
  name=$1
  san=$2
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/$name.key" -out "$SEC/$name.csr" -subj "/CN=$name" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=%s\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' "$san" > "$SEC/$name.ext"
  openssl x509 -req -in "$SEC/$name.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/$name.crt" -days 2 -extfile "$SEC/$name.ext" >/dev/null 2>&1
  rm -f "$SEC/$name.csr" "$SEC/$name.ext"
}
sign server "DNS:oo-fe01-m9-pg"
sign idp "DNS:oo-fe01-m9-idp"
sign svc "DNS:oo-fe01-m9-proof"
sign edge "DNS:oo-fe01-m9-edge"
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$SEC/k1.pem"
openssl pkcs8 -topk8 -nocrypt -in "$SEC/k1.pem" -outform DER -out "$SEC/k1.der"
rm -f "$SEC/k1.pem" "$SEC"/*.csr
chmod 700 "$SEC"
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt" "$SEC/idp.crt" "$SEC/svc.crt" "$SEC/edge.crt"

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" > "$ROOT/raw/network.id"
docker volume create --label oo.owner=a351 --label oo.session="${RUN_ID}" --label oo.kind=postgres oo-fe01-m9-pgdata > "$ROOT/raw/volume-pg.txt"
docker run -d --name oo-fe01-pg-m9 \
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
  --network-alias oo-fe01-m9-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=oo-fe01-m9-pgdata,target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${ROOT}/pg,target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --entrypoint /fixture/pg-wrap.sh \
  "$PG_IMAGE" > "$ROOT/raw/pg.id"
docker inspect oo-fe01-pg-m9 --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} CapAdd={{json .HostConfig.CapAdd}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}}' > "$ROOT/raw/pg-inspect.txt"
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres oo-fe01-pg-m9 pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" != 1 ]; then
  docker logs --tail 80 oo-fe01-pg-m9 > "$ROOT/raw/pg-start.log" 2>&1 || true
  exit 1
fi
docker exec -u root oo-fe01-pg-m9 /fixture/init-roles.sh > "$ROOT/raw/init-roles.txt"
grep -q '^ok$' "$ROOT/raw/init-roles.txt"

docker volume create --label oo.owner=a351 --label oo.session="${RUN_ID}" --label oo.kind=cargo oo-fe01-m9-cargo > "$ROOT/raw/volume-cargo.txt"
docker volume create --label oo.owner=a351 --label oo.session="${RUN_ID}" --label oo.kind=work oo-fe01-m9-work > "$ROOT/raw/volume-work.txt"
chmod 755 "$ROOT/linux/run-rust.sh" "$ROOT/linux/run-node.sh" "$ROOT/pg/pg-wrap.sh" "$ROOT/pg/init-roles.sh"
docker run -d --name oo-fe01-rust-m9 \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 1024 \
  --network "$NET" \
  --network-alias oo-fe01-m9-proof \
  --network-alias oo-fe01-m9-idp \
  -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/work/target \
  --mount "type=bind,source=${ROOT},target=/evidence" \
  --mount "type=bind,source=${SOURCE},target=/snapshot,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets" \
  --mount "type=volume,source=oo-fe01-m9-cargo,target=/cargo" \
  --mount "type=volume,source=oo-fe01-m9-work,target=/work" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --entrypoint /evidence/linux/run-rust.sh \
  "$RUST_IMAGE" > "$ROOT/raw/rust.id"
docker inspect oo-fe01-rust-m9 --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}}' > "$ROOT/raw/rust-inspect.txt"
ready=0
for _ in $(seq 1 180); do
  if [ -f "$ROOT/ready.json" ]; then
    ready=1
    break
  fi
  status=$(docker inspect -f '{{.State.Status}}' oo-fe01-rust-m9 2>/dev/null || echo missing)
  if [ "$status" = exited ] || [ "$status" = dead ] || [ "$status" = missing ]; then
    break
  fi
  sleep 5
done
if [ "$ready" != 1 ]; then
  docker logs --tail 200 oo-fe01-rust-m9 > "$ROOT/raw/rust-fail.log" 2>&1 || true
  exit 1
fi
printf '%s\n' 0 > "$ROOT/raw/rust-ready.exit"
docker run -d --name oo-fe01-node-m9 \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 4g \
  --memory-swap 4g \
  --cpus 1 \
  --pids-limit 512 \
  --network "$NET" \
  --network-alias oo-fe01-m9-edge \
  -e HOME=/home/fe01 \
  -e PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin \
  --mount "type=bind,source=${ARCH},target=/src" \
  --mount "type=bind,source=${REPO}/node_modules,target=/src/node_modules,readonly" \
  --mount "type=bind,source=${REPO}/apps/web/node_modules,target=/src/apps/web/node_modules,readonly" \
  --mount "type=bind,source=${VITE_TMP},target=/src/apps/web/node_modules/.vite-temp" \
  --mount "type=bind,source=${VITE_CACHE},target=/src/apps/web/node_modules/.vite" \
  --mount "type=bind,source=${ROOT},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${HOME_DIR},target=/home/fe01" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=node-builder \
  --entrypoint /evidence/linux/run-node.sh \
  "$NODE_IMAGE" > "$ROOT/raw/node-loop.id"
docker inspect oo-fe01-node-m9 --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}}' > "$ROOT/raw/node-loop-inspect.txt"
docker wait oo-fe01-node-m9 > "$ROOT/raw/node-loop-wait.txt"
code=$(cat "$ROOT/raw/node-loop-wait.txt")
printf '%s\n' "$code" > "$ROOT/raw/launch-loop.exit"
if [ "$code" != 0 ]; then
  echo "FAILED exit=$code"
  exit "$code"
fi
echo "DONE exit=0"
