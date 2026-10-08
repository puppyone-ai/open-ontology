#!/bin/bash
set -u
UUID_HOME=/tmp/oo-fe01-eef831aa-dba4-4af2-a78b-541bc9e0a351
NAME=oo-fe01-node-m5b
NET=oo-fe01-net-m5b-eef831aa
IMG=docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413
ROOT=/Users/redux/puppy-one/open-ontology
OUT=$ROOT/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M5-admin-guard
mkdir -p "$OUT/raw" "$UUID_HOME/home/tmp"
cp "$UUID_HOME/m5/run-focused.sh" "$OUT/raw/run-focused.sh"
cp "$UUID_HOME/m5/launch-focused.sh" "$OUT/raw/launch-focused.sh"
chmod +x "$UUID_HOME/m5/run-focused.sh"

cleanup() {
  if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
    docker rm -f "$NAME" >> "$OUT/raw/focused-docker-rm.txt" 2>&1 || true
  fi
  if docker network ls --format '{{.Name}}' | grep -qx "$NET"; then
    docker network rm "$NET" >> "$OUT/raw/focused-network-rm.txt" 2>&1 || true
  fi
}
trap cleanup EXIT

node_count=0
while read -r id; do
  [ -n "$id" ] || continue
  image=$(docker inspect --format '{{.Image}}' "$id")
  if [ "$image" = "sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413" ]; then
    node_count=$((node_count + 1))
  fi
done < <(docker ps -q)
printf 'node_image_running=%s\n' "$node_count" > "$OUT/raw/focused-capacity.txt"
if [ "$node_count" -ge 2 ]; then
  echo "node cap full"
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "container name already exists"
  exit 2
fi

docker network create --driver bridge "$NET" > "$OUT/raw/focused-network-create.txt"
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
  -e FE01_EVIDENCE=/src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M5-admin-guard \
  -v "$UUID_HOME/home:/home/fe01" \
  -v "$ROOT:/src" \
  -v "$UUID_HOME/m5/run-focused.sh:/run-focused.sh:ro" \
  -w /src \
  "$IMG" \
  bash /run-focused.sh > "$OUT/raw/focused-docker-run.txt"

docker inspect "$NAME" --format 'Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} CapDrop={{json .HostConfig.CapDrop}} NetworkMode={{.HostConfig.NetworkMode}}' \
  > "$OUT/raw/focused-inspect.txt"
code=$(docker wait "$NAME")
printf '%s\n' "$code" > "$OUT/raw/focused-docker-wait.txt"
exit "$code"
