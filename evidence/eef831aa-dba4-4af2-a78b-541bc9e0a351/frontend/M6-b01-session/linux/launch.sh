#!/bin/bash
# One private PostgreSQL, one Rust builder, then one Node browser. No host network.
set -eu
export PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a351
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/frontend/M6-b01-session
SEC=/tmp/oo-fe01-${RUN_ID}/m6-secrets
NET=oo-fe01-net-m6-eef831aa
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NODE_IMAGE=docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
HOME_DIR=/tmp/oo-fe01-${RUN_ID}/home
mkdir -p "$ROOT/raw" "$ROOT/screenshots"
cd "$REPO"

own_rust=$(docker ps -q --filter name=^/oo-fe01-rust-m6$ --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_rust=$(docker ps --format '{{.Image}}' | grep -c '0f24958fa693' || true)
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter name=^/oo-fe01-pg-m6$ --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
own_node=$(docker ps --format '{{.Image}}' | grep -c '481d53bf381928' || true)
printf 'own_rust=%s global_rust=%s host_cargo=%s own_pg=%s node_image=%s\n' "$own_rust" "$global_rust" "$host_cargo" "$own_pg" "$own_node" > "$ROOT/raw/capacity-before.txt"
if [ "$own_rust" -ge 1 ] || [ "$global_rust" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ] || [ "$own_node" -ge 2 ]; then
  echo "deferred" >&2
  exit 3
fi
rm -f "$ROOT/ready.json" "$ROOT/stop"

cleanup() {
  touch "$ROOT/stop" || true
  docker logs --tail 400 oo-fe01-rust-m6 > "$ROOT/raw/rust.log" 2>&1 || true
  docker logs --tail 200 oo-fe01-node-m6 > "$ROOT/raw/node.log" 2>&1 || true
  docker rm -f oo-fe01-node-m6 >/dev/null 2>&1 || true
  docker rm -f oo-fe01-rust-m6 >/dev/null 2>&1 || true
  docker rm -f oo-fe01-pg-m6 >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  docker volume rm oo-fe01-m6-cargo oo-fe01-m6-work oo-fe01-m6-pgdata >/dev/null 2>&1 || true
  rm -rf "$SEC"
}
trap cleanup EXIT

umask 077
mkdir -p "$SEC"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/login.pass"
openssl rand -hex 32 > "$SEC/client.secret"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 2 -subj "/CN=oo-fe01-m6-ca" -addext "basicConstraints=CA:TRUE" >/dev/null 2>&1
sign() {
  name=$1
  san=$2
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/$name.key" -out "$SEC/$name.csr" -subj "/CN=$name" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=%s\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' "$san" > "$SEC/$name.ext"
  openssl x509 -req -in "$SEC/$name.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/$name.crt" -days 2 -extfile "$SEC/$name.ext" >/dev/null 2>&1
  rm -f "$SEC/$name.csr" "$SEC/$name.ext"
}
sign server "DNS:oo-fe01-pg"
sign idp "DNS:oo-fe01-idp"
sign svc "DNS:oo-fe01-proof"
sign edge "DNS:oo-fe01-edge"
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

docker volume create --label oo.owner=a351 --label oo.session="${RUN_ID}" --label oo.kind=postgres oo-fe01-m6-pgdata > "$ROOT/raw/volume-pg.txt"
docker run -d --name oo-fe01-pg-m6 \
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
  --network-alias oo-fe01-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=oo-fe01-m6-pgdata,target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${ROOT}/pg,target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --entrypoint /fixture/pg-wrap.sh \
  "$PG_IMAGE" > "$ROOT/raw/pg.id"
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres oo-fe01-pg-m6 pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" != 1 ]; then
  docker logs oo-fe01-pg-m6 > "$ROOT/raw/pg-start.log" 2>&1 || true
  exit 1
fi
docker exec -u root oo-fe01-pg-m6 /fixture/init-roles.sh > "$ROOT/raw/init-roles.txt"
grep -q '^ok$' "$ROOT/raw/init-roles.txt"
docker inspect oo-fe01-pg-m6 --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} CapAdd={{json .HostConfig.CapAdd}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}}' > "$ROOT/raw/pg-inspect.txt"

docker volume create --label oo.owner=a351 --label oo.session="${RUN_ID}" --label oo.kind=cargo oo-fe01-m6-cargo > "$ROOT/raw/volume-cargo.txt"
docker volume create --label oo.owner=a351 --label oo.session="${RUN_ID}" --label oo.kind=work oo-fe01-m6-work > "$ROOT/raw/volume-work.txt"
chmod 755 "$ROOT/linux/run-rust.sh" "$ROOT/linux/run-node.sh" "$ROOT/pg/pg-wrap.sh" "$ROOT/pg/init-roles.sh"
docker run -d --name oo-fe01-rust-m6 \
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
  --network-alias oo-fe01-proof \
  --network-alias oo-fe01-idp \
  -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/work/target \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
  --mount "type=bind,source=${ROOT},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets" \
  --mount "type=volume,source=oo-fe01-m6-cargo,target=/cargo" \
  --mount "type=volume,source=oo-fe01-m6-work,target=/work" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --entrypoint /evidence/linux/run-rust.sh \
  "$RUST_IMAGE" > "$ROOT/raw/rust.id"
docker inspect oo-fe01-rust-m6 --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}}' > "$ROOT/raw/rust-inspect.txt"
ready=0
for _ in $(seq 1 180); do
  if [ -f "$ROOT/ready.json" ]; then
    ready=1
    break
  fi
  if ! docker ps --format '{{.Names}}' | grep -qx oo-fe01-rust-m6; then
    break
  fi
  status=$(docker inspect -f '{{.State.Status}}' oo-fe01-rust-m6)
  if [ "$status" = exited ] || [ "$status" = dead ]; then
    break
  fi
  sleep 5
done
if [ "$ready" != 1 ]; then
  docker logs oo-fe01-rust-m6 > "$ROOT/raw/rust.log" 2>&1 || true
  exit 1
fi
printf '%s\n' 0 > "$ROOT/raw/rust-ready.exit"

docker run -d --name oo-fe01-node-m6 \
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
  --network-alias oo-fe01-edge \
  -e HOME=/home/fe01 \
  -e PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin \
  --mount "type=bind,source=${REPO},target=/src" \
  --mount "type=bind,source=${ROOT},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${HOME_DIR},target=/home/fe01" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=node-builder \
  --entrypoint /evidence/linux/run-node.sh \
  "$NODE_IMAGE" > "$ROOT/raw/node.id"
docker inspect oo-fe01-node-m6 --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}}' > "$ROOT/raw/node-inspect.txt"
docker wait oo-fe01-node-m6 > "$ROOT/raw/node-wait.txt"
docker logs --tail 200 oo-fe01-node-m6 > "$ROOT/raw/node.log" 2>&1 || true
node_code=$(cat "$ROOT/raw/node-wait.txt")
if [ "$node_code" != 0 ]; then
  exit "$node_code"
fi
printf '%s\n' 0 > "$ROOT/raw/launch.exit"
