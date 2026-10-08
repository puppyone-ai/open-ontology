#!/bin/bash
set -eu
export PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a351
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/frontend/M8-deploy-routes
NODE_IMAGE=docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413
HOME_DIR=/tmp/oo-fe01-${RUN_ID}/home
ARCH=/tmp/oo-fe01-${RUN_ID}/m8-src
VITE_TMP=/tmp/oo-fe01-${RUN_ID}/m8-vite
mkdir -p "$ROOT/raw" "$VITE_TMP"
cd "$REPO"
node_running=$(docker ps --filter ancestor="$NODE_IMAGE" --format '{{.Names}}' | wc -l | tr -d ' ')
printf 'node_running=%s\n' "$node_running" > "$ROOT/raw/capacity-before.txt"
if [ "$node_running" -ge 1 ]; then
  echo "deferred" >&2
  exit 3
fi
if docker ps -a --format '{{.Names}}' | grep -qx 'oo-fe01-node-m8v'; then
  echo "name in use" >&2
  exit 3
fi
head=$(git rev-parse HEAD)
printf '%s\n' "$head" > "$ROOT/raw/archive-base.txt"
rm -rf "$ARCH"
mkdir -p "$ARCH"
git archive --format=tar "$head" | tar -x -C "$ARCH"
cp apps/web/deploy/b01-adapter.mjs apps/web/deploy/b01-adapter.test.mjs apps/web/deploy/b01-adapter.example.json apps/web/deploy/README.md "$ARCH/apps/web/deploy/"
cp apps/web/versions.md "$ARCH/apps/web/versions.md"
cp apps/web/design/APIBINDING.md "$ARCH/apps/web/design/APIBINDING.md"
chmod 755 "$ROOT/linux/run-suite.sh"
cleanup() {
  docker rm -f oo-fe01-node-m8v >/dev/null 2>&1 || true
  rm -rf "$ARCH" "$VITE_TMP"
}
trap cleanup EXIT
docker run -d --name oo-fe01-node-m8v \
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
  --mount "type=bind,source=${ROOT},target=/evidence" \
  --mount "type=bind,source=${HOME_DIR},target=/home/fe01" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=node-builder \
  --entrypoint /evidence/linux/run-suite.sh \
  "$NODE_IMAGE" > "$ROOT/raw/node.id"
docker inspect oo-fe01-node-m8v --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}} PidMode={{.HostConfig.PidMode}}' > "$ROOT/raw/node-inspect.txt"
docker wait oo-fe01-node-m8v > "$ROOT/raw/node-wait.txt"
docker logs --tail 200 oo-fe01-node-m8v > "$ROOT/raw/node.log" 2>&1 || true
code=$(cat "$ROOT/raw/node-wait.txt")
printf '%s\n' "$code" > "$ROOT/raw/launch-suite.exit"
if [ "$code" != 0 ]; then
  echo "FAILED exit=$code"
  exit "$code"
fi
echo "DONE exit=0"
