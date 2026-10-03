#!/bin/bash
# One owned builder. Reuses the third-run snapshot. Does not repeat smoke or
# the green integration phases. Does not start PostgreSQL.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a133/SP03/init-tail
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a133-init-tail-embed
NET=oo-a133-eef831aa-net
CARGO_VOL=oo-a133-eef831aa-cargo
TARGET_VOL=oo-a133-eef831aa-target
SNAP=/tmp/oo-a133-eef831aa/proof-snapshot

mkdir -p "$EVIDENCE/proof/attempt5"
if [ -f "$EVIDENCE/proof/embed-console.log" ]; then
  cp "$EVIDENCE/proof/embed-console.log" "$EVIDENCE/proof/attempt5/embed-console.log"
  cp "$EVIDENCE/proof"/embed-*.exit "$EVIDENCE/proof/attempt5/" 2>/dev/null || true
  cp "$EVIDENCE/proof"/embed-*.txt "$EVIDENCE/proof/attempt5/" 2>/dev/null || true
  printf '%s\n' 'embed exit 101: offline lockfile resolution looked up optional async-fs, which the production features do not enable and which is not in the warm cache. The next run drops unused optional tables from the /tmp copy only.' > "$EVIDENCE/proof/attempt5/reason.txt"
fi

mkdir -p "$EVIDENCE/proof/attempt4"
if [ -f "$EVIDENCE/proof/rest-console.log" ]; then
  cp "$EVIDENCE/proof/rest-console.log" "$EVIDENCE/proof/attempt4/rest-console.log"
fi
cp "$EVIDENCE/proof"/rest-*.exit "$EVIDENCE/proof/attempt4/" 2>/dev/null || true
cp "$EVIDENCE/proof"/rest-*.txt "$EVIDENCE/proof/attempt4/" 2>/dev/null || true
printf '%s\n' 'rest exit 101: cargo test -p sqlx-core refused because the package has dev-dependencies and is not a workspace member. That command is not repeated.' > "$EVIDENCE/proof/attempt4/reason.txt"

running_rust=$(docker ps --no-trunc --format '{{.Image}} {{.Label "oo.kind"}}' | awk '$1 ~ /rust/ || $1 ~ /0f24958fa693/ || $1 ~ /f747a9f2b58b/ || $1 ~ /218765d808d3/ || $1 ~ /f31fa9eaac4e/ || $2=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a133 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/proof/embed-capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/proof/embed-docker-run.exit"
  exit 3
fi

test -f "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
grep -q 'fn committed_init_len' "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
grep -q 'AssertSqlSafe' "$SNAP/crates/oo-connectors/tests/pg_wire_tls_tail.rs"
if [ -f "$SNAP/crates/oo-connectors/src/stream.rs" ]; then
  echo "snapshot contains stream.rs; refusing to compile it" >&2
  printf '%s\n' 4 > "$EVIDENCE/proof/embed-docker-run.exit"
  exit 4
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/proof/embed-image.txt"
grep -q 'arch=arm64' "$EVIDENCE/proof/embed-image.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/proof/embed-docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

rm -f "$EVIDENCE/proof/embed-docker-run.exit" "$EVIDENCE/proof/embed-worst.exit"

cat > "$EVIDENCE/proof/embed-invocation.txt" <<EOF
continuation_of=rest exit 101 on the third-run snapshot
phases=isolated-sqlx-core-lib-tests,scoped-clippy
not_repeated=smoke,lib,bounds,accounting,portal,view,tail
sqlx_invocation=cargo test --manifest-path /tmp/sqlx-core-iso/Cargo.toml --offline --locked --lib
bytes_pin==1.12.1
vendored_lock=discarded (bytes 1.11.1)
workspace_member=not added
smoke=not repeated
image=${IMAGE}
network=${NET}
container=${NAME}
privileged=no
cap_drop=ALL
postgres=no
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
  --mount "type=bind,src=${SNAP},dst=/src,readonly" \
  --mount "type=bind,src=${EVIDENCE},dst=/evidence" \
  --mount "type=volume,src=${CARGO_VOL},dst=/cargo-home" \
  --mount "type=volume,src=${TARGET_VOL},dst=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a133 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 \
  --label oo.kind=rust-builder \
  --label oo.task=init-tail-embed \
  "$IMAGE" \
  bash /evidence/proof/inside-embed.sh \
  > "$EVIDENCE/proof/embed-console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/proof/embed-docker-run.exit"
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
{{end}}' > "$EVIDENCE/proof/embed-hostconfig.txt" || true
docker rm "$NAME" > "$EVIDENCE/proof/embed-container-rm.txt"
echo "HOST_DONE embed_docker_run_exit=${run_exit}"
exit "$run_exit"
