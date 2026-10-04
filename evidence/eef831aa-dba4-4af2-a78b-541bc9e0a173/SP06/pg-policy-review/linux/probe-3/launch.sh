#!/bin/bash
# One Linux arm64 Rust 1.94 builder against a copy of the fixed archive plus the review overlay.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a173/SP06/pg-policy-review/linux/probe-3
ROOT=/tmp/oo-a173-eef831aa-dba4-4af2-a78b-541bc9e0a173
OVERLAY=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a173/SP06/pg-policy-review/overlay/policy_pg_a173_review.rs
SECRETS=/tmp/oo-a173-eef831aa-dba4-4af2-a78b-541bc9e0a173/secrets
CLIENT=/tmp/oo-a173-eef831aa-dba4-4af2-a78b-541bc9e0a173/client-secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a173-probe3
NET=oo-a173-net
CARGO_VOL=oo-a173-cargo
TARGET_VOL=oo-a173-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite probe console.log" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/policy.rs" ] || [ ! -f "$OVERLAY" ]; then
  echo "archive or overlay missing" >&2
  exit 2
fi
if [ -e "$ROOT/archive/crates/oo-gateway/tests/policy_pg_a173_review.rs" ]; then
  echo "immutable archive contains the overlay" >&2
  exit 2
fi
rm -rf "$ROOT/work"
mkdir -p "$ROOT/work"
cp -a "$ROOT/archive/." "$ROOT/work/"
cp "$OVERLAY" "$ROOT/work/crates/oo-gateway/tests/policy_pg_a173_review.rs"
if [ ! -f "$SECRETS/ca.crt" ] || [ ! -f "$SECRETS/admin.pass" ] || [ ! -f "$SECRETS/other-ca.crt" ]; then
  echo "server secrets are absent" >&2
  exit 2
fi
rm -rf "$CLIENT"
mkdir -p "$CLIENT"
cp "$SECRETS/ca.crt" "$SECRETS/other-ca.crt" "$SECRETS/admin.pass" "$SECRETS/login.pass" "$CLIENT/"
chmod 600 "$CLIENT/admin.pass" "$CLIENT/login.pass"
chmod 644 "$CLIENT/ca.crt" "$CLIENT/other-ca.crt"
find "$CLIENT" -name '*.key' -print | grep -q . && { echo "client dir has a key" >&2; exit 2; }
own=$(docker ps -q --filter label=oo.owner=a173 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a173 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s running_rust=%s host_cargo=%s global=%s own_pg=%s\n' "$own" "$running_rust" "$host_cargo" "$global_builders" "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ] || [ "$global_builders" -ge 2 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: builder or postgres capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named probe container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
set +e
docker run --name "$NAME" \
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
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/work,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${CLIENT},target=/run/oo-a173/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --label oo.project=open-ontology \
  --label oo.owner=a173 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a173 \
  --label oo.task=pg-policy-review-probe-3 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
admin=$(cat "$CLIENT/admin.pass")
login=$(cat "$CLIENT/login.pass")
if grep -R -F -q -e "$admin" -e "$login" "$EVIDENCE" --exclude launch.sh --exclude inside.sh; then
  echo "secret text appeared in probe evidence" > "$EVIDENCE/secret-scan.txt"
  printf '%s\n' 4 > "$EVIDENCE/secret-scan.exit"
else
  echo "password_hits=0" > "$EVIDENCE/secret-scan.txt"
  printf '%s\n' 0 > "$EVIDENCE/secret-scan.exit"
fi
if grep -R -n -E 'BEGIN (RSA |OPENSSH |EC )?PRIVATE KEY' "$EVIDENCE" >/dev/null 2>&1; then
  echo "private key block in probe evidence" >> "$EVIDENCE/secret-scan.txt"
  printf '%s\n' 4 > "$EVIDENCE/secret-scan.exit"
fi
exit "$run_exit"
