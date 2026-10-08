#!/bin/bash
set -eu
export PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a351
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/frontend/M7-b01-adapter
NODE_IMAGE=docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413
HOME_DIR=/tmp/oo-fe01-${RUN_ID}/home
FONT_SRC=/tmp/oo-fe01-${RUN_ID}/fonts/src
mkdir -p "$ROOT/raw"
cd "$REPO"
node_image=$(docker ps --format '{{.Image}}' | grep -c '481d53bf381928' || true)
printf 'node_image=%s\n' "$node_image" > "$ROOT/raw/capacity-before.txt"
if [ "$node_image" -ge 2 ]; then
  echo "deferred" >&2
  exit 3
fi
if docker ps -a --format '{{.Names}}' | grep -qx 'oo-fe01-node-m7v'; then
  echo "name in use" >&2
  exit 3
fi
chmod 755 "$ROOT/linux/run-verify.sh"
docker run -d --name oo-fe01-node-m7v \
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
  --mount "type=bind,source=${REPO},target=/src" \
  --mount "type=bind,source=${ROOT},target=/evidence" \
  --mount "type=bind,source=${FONT_SRC},target=/fonts,readonly" \
  --mount "type=bind,source=${HOME_DIR},target=/home/fe01" \
  --label oo.project=open-ontology \
  --label oo.owner=a351 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=node-builder \
  --entrypoint /evidence/linux/run-verify.sh \
  "$NODE_IMAGE" > "$ROOT/raw/node.id"
docker inspect oo-fe01-node-m7v --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} Network={{.HostConfig.NetworkMode}} Privileged={{.HostConfig.Privileged}} PidMode={{.HostConfig.PidMode}}' > "$ROOT/raw/node-inspect.txt"
docker wait oo-fe01-node-m7v > "$ROOT/raw/node-wait.txt"
docker logs --tail 200 oo-fe01-node-m7v > "$ROOT/raw/node.log" 2>&1 || true
code=$(cat "$ROOT/raw/node-wait.txt")
docker rm -f oo-fe01-node-m7v >/dev/null 2>&1 || true
printf '%s\n' "$code" > "$ROOT/raw/launch-verify.exit"
if [ "$code" != 0 ]; then
  echo "FAILED exit=$code"
  exit "$code"
fi
echo "DONE exit=0"
