#!/bin/bash
set -u
UUID_HOME=/tmp/oo-fe01-eef831aa-dba4-4af2-a78b-541bc9e0a351
NAME=oo-fe01-node-m4b
NET=oo-fe01-net-m4b-eef831aa
IMG=docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413
ROOT=/Users/redux/puppy-one/open-ontology
OUT=$ROOT/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M4-font-motion
mkdir -p "$OUT/raw"
cp "$UUID_HOME/m4/retry-run.sh" "$OUT/raw/retry-run.sh"
cp "$UUID_HOME/m4/launch-retry.sh" "$OUT/raw/launch-retry.sh"

cleanup() {
  if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
    docker rm -f "$NAME" >> "$OUT/raw/docker-rm-retry.txt" 2>&1 || true
  fi
  if docker network ls --format '{{.Name}}' | grep -qx "$NET"; then
    docker network rm "$NET" >> "$OUT/raw/network-rm-retry.txt" 2>&1 || true
  fi
}
trap cleanup EXIT

{
  echo "ps-before-retry"
  docker ps --format '{{.Names}}\t{{.Image}}\t{{.Status}}'
} > "$OUT/raw/docker-ps-before-retry.txt"
node_count=0
while read -r id; do
  [ -n "$id" ] || continue
  image=$(docker inspect --format '{{.Image}}' "$id")
  name=$(docker inspect --format '{{.Name}}' "$id")
  printf '%s %s\n' "$name" "$image" >> "$OUT/raw/docker-ps-before-retry.txt"
  if [ "$image" = "sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413" ]; then
    node_count=$((node_count + 1))
  fi
done < <(docker ps -q)
printf 'node_image_running=%s\n' "$node_count" > "$OUT/raw/capacity-before-retry.txt"
if [ "$node_count" -ge 2 ]; then
  echo "node cap full" | tee "$OUT/raw/capacity-refused-retry.txt"
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "container name already exists" | tee "$OUT/raw/capacity-refused-retry.txt"
  exit 2
fi

docker network create --driver bridge "$NET" > "$OUT/raw/network-create-retry.txt"
docker run -d --name "$NAME" \
  --pull never \
  --platform linux/arm64 \
  --init \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 4g \
  --memory-swap 4g \
  --cpus 1 \
  --pids-limit 2048 \
  --tmpfs /tmp:rw,size=1g,mode=1777 \
  --network "$NET" \
  -e HOME=/home/fe01 \
  -e CI=1 \
  -e TMPDIR=/home/fe01/tmp \
  -e PLAYWRIGHT_BROWSERS_PATH=/home/fe01/ms-playwright \
  -e PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
  -e FE01_EVIDENCE=/src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M4-font-motion \
  -v "$UUID_HOME/home:/home/fe01" \
  -v "$ROOT:/src" \
  -v "$UUID_HOME/m4/retry-run.sh:/retry-run.sh:ro" \
  -w /src \
  "$IMG" \
  bash /retry-run.sh > "$OUT/raw/docker-run-retry.txt"

docker inspect "$NAME" --format 'Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} Privileged={{.HostConfig.Privileged}} CapDrop={{json .HostConfig.CapDrop}} SecurityOpt={{json .HostConfig.SecurityOpt}} NetworkMode={{.HostConfig.NetworkMode}} PidMode={{.HostConfig.PidMode}} PidsLimit={{.HostConfig.PidsLimit}}' \
  > "$OUT/raw/inspect-retry.txt"
code=$(docker wait "$NAME")
printf '%s\n' "$code" > "$OUT/raw/docker-wait-retry.txt"
exit "$code"
