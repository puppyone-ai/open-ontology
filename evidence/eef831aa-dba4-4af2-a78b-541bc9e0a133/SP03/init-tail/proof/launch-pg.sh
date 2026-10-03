#!/bin/bash
# One owned builder for the private TLS read. PostgreSQL is started separately.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a133/SP03/init-tail
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a133-init-tail-pgread
PGNAME=oo-a133-init-tail-pg
NET=oo-a133-eef831aa-net
CARGO_VOL=oo-a133-eef831aa-cargo
TARGET_VOL=oo-a133-eef831aa-target
PGLOG_VOL=oo-a133-eef831aa-pglog
SNAP=/tmp/oo-a133-eef831aa/proof-snapshot
SEC=/tmp/oo-a133-eef831aa/secrets

mkdir -p "$EVIDENCE/proof/attempt7"
if [ -f "$EVIDENCE/proof/pg-console.log" ]; then
  cp "$EVIDENCE/proof/pg-console.log" "$EVIDENCE/proof/attempt7/pg-console.log"
  cp "$EVIDENCE/proof"/pg-*.exit "$EVIDENCE/proof/attempt7/" 2>/dev/null || true
  printf '%s\n' 'reader SET log_statement failed: the parameter is superuser-only. The admin now records log_statement=all on the reader role after the password statement.' > "$EVIDENCE/proof/attempt7/reason.txt"
fi

running_rust=$(docker ps --no-trunc --format '{{.Image}} {{.Label "oo.kind"}}' | awk '$1 ~ /rust/ || $1 ~ /0f24958fa693/ || $1 ~ /f747a9f2b58b/ || $1 ~ /218765d808d3/ || $1 ~ /f31fa9eaac4e/ || $2=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a133 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/proof/pg-capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/proof/pg-docker-run.exit"
  exit 3
fi
if ! docker ps --filter name="^/${PGNAME}$" --filter status=running --format '{{.Names}}' | grep -q .; then
  echo "postgres is not running" >&2
  printf '%s\n' 4 > "$EVIDENCE/proof/pg-docker-run.exit"
  exit 4
fi
test -f "$SEC/pointer.json"
test -f "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
if [ -f "$SNAP/crates/oo-connectors/src/stream.rs" ]; then
  echo "snapshot contains stream.rs; refusing to compile it" >&2
  printf '%s\n' 4 > "$EVIDENCE/proof/pg-docker-run.exit"
  exit 4
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/proof/pg-image.txt"
grep -q 'arch=arm64' "$EVIDENCE/proof/pg-image.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/proof/pg-docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

rm -f "$EVIDENCE/proof/pg-docker-run.exit" "$EVIDENCE/proof/pg-worst.exit"

cat > "$EVIDENCE/proof/pg-invocation.txt" <<EOF
phase=tls_pg_sixty_thousand_small_rows
image=${IMAGE}
network=${NET}
container=${NAME}
postgres=${PGNAME}
privileged=no
cap_drop=ALL
secrets_in_evidence=no
stream_rs=absent
EOF

set +e
docker run --name "$NAME" \
  --pull never \
  --platform linux/arm64 \
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
  -e OO_A133_TLS_FIXTURE=/secrets/pointer.json \
  -e OO_A133_PRODUCT=/src/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs \
  -e OO_A133_REQUIRE_PRODUCT=1 \
  --mount "type=bind,src=${SNAP},dst=/src,readonly" \
  --mount "type=bind,src=${EVIDENCE},dst=/evidence" \
  --mount "type=bind,src=${SEC},dst=/secrets,readonly" \
  --mount "type=volume,src=${PGLOG_VOL},dst=/pglog,readonly" \
  --mount "type=volume,src=${CARGO_VOL},dst=/cargo-home" \
  --mount "type=volume,src=${TARGET_VOL},dst=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a133 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 \
  --label oo.kind=rust-builder \
  --label oo.task=init-tail-pgread \
  "$IMAGE" \
  bash /evidence/proof/inside-pg.sh \
  > "$EVIDENCE/proof/pg-console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/proof/pg-docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}}
Status={{.State.Status}}
OOMKilled={{.State.OOMKilled}}
Privileged={{.HostConfig.Privileged}}
NetworkMode={{.HostConfig.NetworkMode}}
Memory={{.HostConfig.Memory}}
MemorySwap={{.HostConfig.MemorySwap}}
NanoCpus={{.HostConfig.NanoCpus}}
PidsLimit={{.HostConfig.PidsLimit}}
CapDrop={{json .HostConfig.CapDrop}}
CapAdd={{json .HostConfig.CapAdd}}
SecurityOpt={{json .HostConfig.SecurityOpt}}
{{range .Mounts}}mount dest={{.Destination}} type={{.Type}} rw={{.RW}}
{{end}}' > "$EVIDENCE/proof/pg-hostconfig.txt" || true
docker rm "$NAME" > "$EVIDENCE/proof/pg-container-rm.txt"
echo "HOST_DONE pg_docker_run_exit=${run_exit}"
exit "$run_exit"
