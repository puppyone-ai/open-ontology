#!/bin/bash
# One a117 Rust builder. Does not stop, remove, or prune any other run.
set -eu
REPO="/Users/redux/puppy-one/open-ontology"
EVID="$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a117/SP00/supported-target-license-gate"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
BASE_COMMIT="a6e002aad0aa79c4bae788a84fc62c65283e0c04"
BASE_TREE="ca17b5cd44a123c613227357c61ab934560fdbbe"
BASE_PARENT="f7d90e2d7914c4c3b61dc3b38b83f42ad59ec649"
DENY_BEFORE_SHA="4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b"
LOCK_SHA="d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f"
UUID=$(uuidgen | tr '[:upper:]' '[:lower:]')
BASE="/tmp/oo-a117-${UUID}"
NAME="eef831aa-dba4-4af2-a78b-541bc9e0a117-rust"
NET="oo-a117-${UUID}-net"
VOLC="oo-a117-${UUID}-cargo"
VOLT="oo-a117-${UUID}-target"
LOG="$EVID/linux/host-launch.log"
mkdir -p "$BASE" "$EVID/linux" "$EVID/raw" "$EVID/identity"
printf '%s\n' "$UUID" >"$EVID/linux/run-id.txt"
printf '%s\n' "$NAME" >"$EVID/linux/container-name.txt"
printf '%s\n' "$$" >"$EVID/linux/driver.pid"
date -u +%Y-%m-%dT%H:%M:%SZ >"$EVID/linux/registered-at.txt"

say() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$LOG" >/dev/null; }

cleanup() {
  local status=$?
  trap - EXIT
  if [ -n "${NAME:-}" ]; then
    docker stop "$NAME" >"$EVID/linux/docker-stop.txt" 2>&1 || true
    docker rm "$NAME" >"$EVID/linux/docker-rm.txt" 2>&1 || true
  fi
  if [ -n "${NET:-}" ]; then
    docker network rm "$NET" >"$EVID/linux/network-rm.txt" 2>&1 || true
  fi
  if [ -n "${VOLC:-}" ]; then
    docker volume rm "$VOLC" "$VOLT" >"$EVID/linux/volume-rm.txt" 2>&1 || true
  fi
  docker ps -a --filter "name=eef831aa-dba4-4af2-a78b-541bc9e0a117-rust" --format '{{.ID}} {{.Names}} {{.Status}}' >"$EVID/linux/docker-ps-owned-after.txt" || true
  docker ps -a --format '{{.ID}} {{.Names}} {{.Status}}' >"$EVID/linux/docker-ps-after.txt" || true
  if [ -n "${BASE:-}" ] && [ -d "$BASE" ]; then
    rm -rf "$BASE"
    echo "removed" >"$EVID/linux/tmp-removed.txt"
  fi
  echo "$status" >"$EVID/linux/driver.exit"
  if [ "$status" -eq 0 ]; then
    echo "DONE" | tee -a "$LOG"
  else
    echo "FAILED exit=$status" | tee -a "$LOG"
  fi
  exit "$status"
}
trap cleanup EXIT

cd "$REPO"
{
  echo "HEAD=$(git rev-parse HEAD)"
  echo "HEAD_TREE=$(git rev-parse 'HEAD^{tree}')"
  echo "HEAD_PARENT=$(git rev-parse 'HEAD^')"
  echo "BASE=$(git rev-parse "$BASE_COMMIT")"
  echo "BASE_TREE=$(git rev-parse "$BASE_COMMIT^{tree}")"
  echo "BASE_PARENT=$(git rev-parse "$BASE_COMMIT^")"
} >"$EVID/identity/git-at-launch.txt"

got_base=$(git rev-parse "$BASE_COMMIT")
got_tree=$(git rev-parse "$BASE_COMMIT^{tree}")
got_parent=$(git rev-parse "$BASE_COMMIT^")
if [ "$got_base" != "$BASE_COMMIT" ] || [ "$got_tree" != "$BASE_TREE" ] || [ "$got_parent" != "$BASE_PARENT" ]; then
  say "FAILED baseline identity"
  exit 4
fi
if ! git merge-base --is-ancestor "$BASE_COMMIT" HEAD; then
  say "FAILED baseline is not an ancestor"
  exit 4
fi
lock_now=$(shasum -a 256 Cargo.lock | awk '{print $1}')
deny_now=$(shasum -a 256 ci/deny.toml | awk '{print $1}')
if [ "$lock_now" != "$LOCK_SHA" ] || [ "$deny_now" != "$DENY_BEFORE_SHA" ]; then
  say "FAILED pre-edit lock or deny hash"
  exit 4
fi
cp ci/deny.toml "$EVID/identity/deny.toml.before"
cp Cargo.lock "$EVID/identity/Cargo.lock.before"
# lock rows for the two retained crates
awk '
  $0 ~ /^\[\[package\]\]/ { if (keep) { print buf; print ""; n++ } buf=$0; keep=0; next }
  { buf = buf "\n" $0 }
  $0 ~ /^name = "tiny-keccak"/ || $0 ~ /^name = "webpki-root-certs"/ { keep=1 }
  END { if (keep) { print buf; n++ } }
' Cargo.lock >"$EVID/identity/lock-rows-offending.txt"

# product paths must be clean before the config edit
if [ -n "$(git diff --name-only -- Cargo.lock Cargo.toml crates bench rust-toolchain.toml ci/deny.toml)" ]; then
  say "FAILED tracked product diff before edit"
  git diff --name-only -- Cargo.lock Cargo.toml crates bench rust-toolchain.toml ci/deny.toml >"$EVID/linux/pre-edit-diff.txt"
  exit 4
fi

say "capacity recheck"
ps -ax -o pid=,comm= >"$EVID/linux/processes-launch.txt"
docker ps --format '{{.ID}} {{.Image}} {{.Names}} {{.Status}}' >"$EVID/linux/docker-ps-launch.txt"
host_rust=$(awk '$2=="cargo" || $2=="rustc" || $2=="rustup" || $2=="rustdoc" { c++ } END { print c+0 }' "$EVID/linux/processes-launch.txt")
rust_containers=$(awk 'BEGIN{c=0} { if (tolower($2) ~ /rust/ || tolower($3) ~ /rust/) c++ } END { print c+0 }' "$EVID/linux/docker-ps-launch.txt")
total=$((host_rust + rust_containers))
{
  echo "host_rust_comm=$host_rust"
  echo "rust_named_or_image_containers=$rust_containers"
  echo "total=$total"
} >"$EVID/linux/builder-count-launch.txt"
if [ "$total" -ge 2 ]; then
  say "FAILED builder cap total=$total"
  exit 6
fi

say "create network and volumes"
docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a117 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a117 \
  --label oo.task=supported-target-license-gate \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$NET" >"$EVID/linux/network-id.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a117 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a117 \
  --label oo.task=supported-target-license-gate \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$VOLC" >"$EVID/linux/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a117 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a117 \
  --label oo.task=supported-target-license-gate \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$VOLT" >"$EVID/linux/volume-target.txt"

say "start container"
docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=2g \
  --tmpfs /work:rw,nosuid,nodev,size=256m \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/a117 \
  -e CARGO_BUILD_JOBS=2 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/work/home \
  -e TMPDIR=/tmp \
  -e GIT_TERMINAL_PROMPT=0 \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
  --mount "type=bind,source=${EVID},target=/evidence" \
  --mount "type=volume,source=${VOLC},target=/cargo-home" \
  --mount "type=volume,source=${VOLT},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a117 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a117 \
  --label oo.task=supported-target-license-gate \
  --label oo.kind=rust-builder \
  --label oo.slot=B \
  --label oo.run="$UUID" \
  "$IMAGE" \
  sleep 21600 >"$EVID/linux/container.id"

docker inspect --format 'Id={{.Id}} Image={{.Image}} Platform={{.Platform}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}}' "$NAME" >"$EVID/linux/hostconfig.txt"

say "phase1"
set +e
docker exec "$NAME" bash /evidence/linux/inside.sh phase1
phase1_code=$?
set -e
printf '%s\n' "$phase1_code" >"$EVID/linux/phase1.exit"
if [ "$phase1_code" -ne 0 ]; then
  say "FAILED phase1 exit=$phase1_code"
  exit "$phase1_code"
fi

# Confirm smoke from the container before touching deny.toml.
host_line=$(awk -F': ' '$1=="host"{print $2}' "$EVID/linux/rustc-vv.txt")
rel_line=$(awk -F': ' '$1=="release"{print $2}' "$EVID/linux/rustc-vv.txt")
if [ "$host_line" != "aarch64-unknown-linux-gnu" ] || [ "$rel_line" != "1.94.0" ]; then
  say "FAILED smoke recorded identity"
  exit 4
fi

say "edit deny.toml targets"
awk '
  BEGIN { done=0 }
  {
    print
    if (!done && $0 == "no-default-features = false") {
      print "targets = ["
      print "    \"aarch64-unknown-linux-gnu\","
      print "    \"x86_64-unknown-linux-gnu\","
      print "]"
      done=1
    }
  }
  END { if (!done) exit 3 }
' "$EVID/identity/deny.toml.before" >"$EVID/identity/deny.toml.after"
# Reject any edit that is not exactly those four added lines.
diff -u "$EVID/identity/deny.toml.before" "$EVID/identity/deny.toml.after" >"$EVID/identity/deny.toml.diff" || true
added=$(grep -E '^\+[^+]' "$EVID/identity/deny.toml.diff" | wc -l | awk '{print $1}')
removed=$(grep -c -E '^-[^-]' "$EVID/identity/deny.toml.diff" || true)
if [ "$added" != "4" ] || [ "$removed" != "0" ]; then
  say "FAILED deny diff shape added=$added removed=$removed"
  exit 5
fi
grep -F -x '+targets = [' "$EVID/identity/deny.toml.diff" >/dev/null
grep -F -x '+    "aarch64-unknown-linux-gnu",' "$EVID/identity/deny.toml.diff" >/dev/null
grep -F -x '+    "x86_64-unknown-linux-gnu",' "$EVID/identity/deny.toml.diff" >/dev/null
grep -F -x '+]' "$EVID/identity/deny.toml.diff" >/dev/null
cp "$EVID/identity/deny.toml.after" ci/deny.toml
after_sha=$(shasum -a 256 ci/deny.toml | awk '{print $1}')
printf '%s\n' "$after_sha" >"$EVID/identity/deny.toml.after.sha256"
# lock must still match
lock_mid=$(shasum -a 256 Cargo.lock | awk '{print $1}')
if [ "$lock_mid" != "$LOCK_SHA" ]; then
  say "FAILED lock changed at edit"
  cp "$EVID/identity/Cargo.lock.before" Cargo.lock
  exit 5
fi

say "phase2"
set +e
docker exec "$NAME" bash /evidence/linux/inside.sh phase2
phase2_code=$?
set -e
printf '%s\n' "$phase2_code" >"$EVID/linux/phase2.exit"
if [ "$phase2_code" -ne 0 ]; then
  say "FAILED phase2 exit=$phase2_code"
  exit "$phase2_code"
fi

# Host-side product identity after the run. deny.toml is the only allowed tracked change.
git diff --name-only -- Cargo.lock Cargo.toml crates bench rust-toolchain.toml >"$EVID/identity/product-diff-after.txt"
if [ -s "$EVID/identity/product-diff-after.txt" ]; then
  say "FAILED product paths changed"
  exit 5
fi
lock_end=$(shasum -a 256 Cargo.lock | awk '{print $1}')
if [ "$lock_end" != "$LOCK_SHA" ]; then
  say "FAILED lock hash at end"
  exit 5
fi
say "validation commands finished"
exit 0
