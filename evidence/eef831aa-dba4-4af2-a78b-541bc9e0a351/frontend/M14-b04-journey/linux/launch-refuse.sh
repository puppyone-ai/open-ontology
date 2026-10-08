#!/bin/bash
set -eu
export PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a351
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/frontend/M14-b04-journey
NODE_IMAGE=docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413
HOME_DIR=/tmp/oo-fe01-${RUN_ID}/home
ARCH=/tmp/oo-fe01-${RUN_ID}/m14-refuse-src
VITE_TMP=/tmp/oo-fe01-${RUN_ID}/m14-refuse-vite
VITE_CACHE=/tmp/oo-fe01-${RUN_ID}/m14-refuse-vite-cache
mkdir -p "$VITE_TMP" "$VITE_CACHE"
cd "$REPO"
node_running=$(docker ps --filter ancestor="$NODE_IMAGE" --format '{{.Names}}' | wc -l | tr -d ' ')
own_node=$(docker ps --format '{{.Names}}' | grep -c '^oo-fe01-node' || true)
if [ "$node_running" -ge 1 ] || [ "$own_node" -ge 1 ]; then
  echo "deferred" >&2
  exit 3
fi
head=$(git rev-parse HEAD)
printf '%s\n' "$head" > "$ROOT/raw/refuse-archive-base.txt"
rm -rf "$ARCH"
mkdir -p "$ARCH"
git archive --format=tar "$head" | tar -x -C "$ARCH"
mkdir -p "$ARCH/packages/sdk-ts/src" "$ARCH/apps/web/src/api" "$ARCH/apps/web/deploy"
cp packages/sdk-ts/src/client.ts "$ARCH/packages/sdk-ts/src/client.ts"
cp packages/sdk-ts/src/index.ts "$ARCH/packages/sdk-ts/src/index.ts"
cp apps/web/src/api/sdk-client.test.ts "$ARCH/apps/web/src/api/sdk-client.test.ts"
cp apps/web/deploy/b01-adapter.mjs "$ARCH/apps/web/deploy/b01-adapter.mjs"
cp apps/web/deploy/b01-adapter.example.json "$ARCH/apps/web/deploy/b01-adapter.example.json"
cp apps/web/deploy/b01-adapter.test.mjs "$ARCH/apps/web/deploy/b01-adapter.test.mjs"
chmod 755 "$ROOT/linux/run-refuse.sh"
cleanup() {
  docker logs --tail 80 oo-fe01-node-m14-refuse > "$ROOT/raw/refuse-node.log" 2>&1 || true
  docker rm -f oo-fe01-node-m14-refuse >/dev/null 2>&1 || true
  rm -rf "$ARCH" "$VITE_TMP" "$VITE_CACHE"
}
trap cleanup EXIT
docker run -d --name oo-fe01-node-m14-refuse \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 4g \
  --memory-swap 4g \
  --cpus 1 \
  --pids-limit 512 \
  -e HOME=/home/fe01 \
  -e PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin \
  --mount "type=bind,source=${ARCH},target=/src" \
  --mount "type=bind,source=${REPO}/node_modules,target=/src/node_modules,readonly" \
  --mount "type=bind,source=${REPO}/apps/web/node_modules,target=/src/apps/web/node_modules,readonly" \
  --mount "type=bind,source=${VITE_TMP},target=/src/apps/web/node_modules/.vite-temp" \
  --mount "type=bind,source=${VITE_CACHE},target=/src/apps/web/node_modules/.vite" \
  --mount "type=bind,source=${ROOT},target=/evidence" \
  --mount "type=bind,source=${HOME_DIR},target=/home/fe01" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=node-builder \
  --entrypoint /evidence/linux/run-refuse.sh \
  "$NODE_IMAGE" > "$ROOT/raw/refuse-node.id"
docker inspect oo-fe01-node-m14-refuse --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}}' > "$ROOT/raw/refuse-inspect.txt"
docker wait oo-fe01-node-m14-refuse > "$ROOT/raw/refuse-wait.txt"
code=$(cat "$ROOT/raw/refuse-wait.txt")
printf '%s\n' "$code" > "$ROOT/raw/launch-refuse.exit"
if [ "$code" != 0 ]; then
  echo "FAILED exit=$code"
  exit "$code"
fi
echo "DONE exit=0"
