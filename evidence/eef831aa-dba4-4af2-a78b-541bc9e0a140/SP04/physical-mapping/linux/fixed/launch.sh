#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder and one private PostgreSQL 17.10.
# Immutable git archive of 02b9847159a68ced1443615cbb47bcb5872724b0 plus the owned overlay.
# No pull, prune, host network, socket, HOME mount, or foreign cleanup.
set -eu
RUN_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
COMMIT=02b9847159a68ced1443615cbb47bcb5872724b0
ROOT=/tmp/oo-a140-eef831aa-dba4-4af2-a78b-541bc9e0a140
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a140/SP04/physical-mapping/linux/fixed
REPO=/Users/redux/puppy-one/open-ontology
NAME=oo-a140-msrv-${RUN_ID}
PG=oo-a140-pg-${RUN_ID}
NET=oo-a140-${RUN_ID}-net
CARGO_VOL=oo-a140-eef831aa-dba4-4af2-a78b-541bc9e0a140-cargo
TARGET_VOL=oo-a140-eef831aa-dba4-4af2-a78b-541bc9e0a140-target
PG_VOL=oo-a140-eef831aa-dba4-4af2-a78b-541bc9e0a140-pg
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a140

mkdir -p "$EVIDENCE" "$ROOT"
chmod +x "$EVIDENCE/inside.sh"
printf '%s\n' "$NAME" > "$EVIDENCE/container-name.txt"
printf '%s\n' "$PG" > "$EVIDENCE/postgres-container.txt"
printf '%s\n' "$NET" > "$EVIDENCE/network-name.txt"
printf '%s\n' "$CARGO_VOL" > "$EVIDENCE/cargo-volume.txt"
printf '%s\n' "$TARGET_VOL" > "$EVIDENCE/target-volume.txt"
printf '%s\n' "$PG_VOL" > "$EVIDENCE/postgres-volume.txt"
printf '%s\n' "$RUN_ID" > "$EVIDENCE/run-id.txt"

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/ {print}' || true
  echo "rust_builder_labels:"
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}}' || true
  echo "postgres_labels:"
  docker ps --filter label=oo.kind=postgres --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}}' || true
} > "$EVIDENCE/builders-before.txt"

running=$(docker ps --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder --format '{{.Names}}')
count=$(printf '%s\n' "$running" | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -ax -o comm= | awk '$1 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/' | wc -l | tr -d ' ')
own_pg=$(docker ps --filter label=oo.owner=a140 --filter label=oo.kind=postgres --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
printf 'builder_containers=%s host_cargo=%s own_postgres=%s\n' "$count" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$((count + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builders already at global max" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a140 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a postgres" >&2
  exit 3
fi
if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
if ! docker image inspect "$PG_IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  exit 1
fi
docker image inspect "$RUST_IMAGE" --format '{{.Id}}' > "$EVIDENCE/rust-image-id.txt"
docker image inspect "$PG_IMAGE" --format '{{.Id}}' > "$EVIDENCE/postgres-image-id.txt"

rm -rf "$ROOT/fixed-snapshot" "$ROOT/fixed-archive.tar"
mkdir -p "$ROOT/fixed-snapshot"
git -C "$REPO" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  contracts/pipeline/target-fence-receipts \
  > "$ROOT/fixed-archive.tar"
tar -x -f "$ROOT/fixed-archive.tar" -C "$ROOT/fixed-snapshot"
overlay_files=(
  crates/oo-pipeline/src/lib.rs
  crates/oo-pipeline/src/physical_mapping.rs
  crates/oo-pipeline/tests/physical_mapping_live.rs
  crates/oo-pipeline/migrations/0004_physical_mapping.sql
  contracts/pipeline/physical-mapping-v1.md
)
for rel in "${overlay_files[@]}"; do
  mkdir -p "$ROOT/fixed-snapshot/$(dirname "$rel")"
  cp "$REPO/$rel" "$ROOT/fixed-snapshot/$rel"
done
shasum -a 256 "$ROOT/fixed-archive.tar" | awk '{print $1}' > "$EVIDENCE/archive.tar.sha256"
: > "$EVIDENCE/overlay.sha256"
for rel in "${overlay_files[@]}"; do
  shasum -a 256 "$REPO/$rel" >> "$EVIDENCE/overlay.sha256"
done
git -C "$REPO" rev-parse "$COMMIT" > "$EVIDENCE/source-commit.txt"
git -C "$REPO" rev-parse "$COMMIT^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$REPO" rev-parse "$COMMIT^" > "$EVIDENCE/source-parent.txt"
git -C "$REPO" rev-parse HEAD > "$EVIDENCE/workspace-head-at-archive.txt"

is_overlay() {
  case "$1" in
    crates/oo-pipeline/src/lib.rs|\
    crates/oo-pipeline/src/physical_mapping.rs|\
    crates/oo-pipeline/tests/physical_mapping_live.rs|\
    crates/oo-pipeline/migrations/0004_physical_mapping.sql|\
    contracts/pipeline/physical-mapping-v1.md)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

files=0
links=0
mismatch=0
overlay_seen=0
: > "$EVIDENCE/archive-mismatch.txt"
while IFS= read -r path; do
  [ -n "$path" ] || continue
  links=$((links + 1))
  echo "symlink $path" >> "$EVIDENCE/archive-mismatch.txt"
done < <(find "$ROOT/fixed-snapshot" -type l | sort)
while IFS= read -r path; do
  [ -n "$path" ] || continue
  rel=${path#"$ROOT/fixed-snapshot/"}
  files=$((files + 1))
  if is_overlay "$rel"; then
    overlay_seen=$((overlay_seen + 1))
    if ! cmp -s "$path" "$REPO/$rel"; then
      mismatch=$((mismatch + 1))
      echo "overlay mismatch $rel" >> "$EVIDENCE/archive-mismatch.txt"
    fi
  else
    got=$(git hash-object "$path")
    exp=$(git -C "$REPO" rev-parse "$COMMIT:$rel")
    if [ "$got" != "$exp" ]; then
      mismatch=$((mismatch + 1))
      echo "mismatch $rel" >> "$EVIDENCE/archive-mismatch.txt"
    fi
  fi
done < <(find "$ROOT/fixed-snapshot" -type f | sort)
printf 'files=%s\nlinks=%s\nmismatched=%s\noverlay=%s\n' "$files" "$links" "$mismatch" "$overlay_seen" > "$EVIDENCE/archive-verify.txt"
if [ "$mismatch" -ne 0 ] || [ "$links" -ne 0 ] || [ "$overlay_seen" -ne 5 ]; then
  echo "archive verification failed" >&2
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
source_commit=${COMMIT}
source_tree=$(cat "$EVIDENCE/source-tree.txt")
source_parent=$(cat "$EVIDENCE/source-parent.txt")
workspace_head=$(cat "$EVIDENCE/workspace-head-at-archive.txt")
rust_image=${RUST_IMAGE}
rust_image_id=$(cat "$EVIDENCE/rust-image-id.txt")
postgres_image=${PG_IMAGE}
postgres_image_id=$(cat "$EVIDENCE/postgres-image-id.txt")
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
postgres_version=17.10
network=${NET}
container=${NAME}
postgres_container=${PG}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
postgres_volume=${PG_VOL}
snapshot=${ROOT}/fixed-snapshot
archive_tar=${ROOT}/fixed-archive.tar
archive_sha256=$(cat "$EVIDENCE/archive.tar.sha256")
snapshot_method=git archive of ${COMMIT} plus five owned overlay files
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
rust_cap_drop=ALL
rust_cap_add=none
postgres_cap_drop=ALL
postgres_cap_add=CHOWN,DAC_OVERRIDE,FOWNER,SETGID,SETUID
init=yes
no_new_privileges=yes
auth=trust
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings; cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test physical_mapping_live physical_mapping_live -- --ignored --exact
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a140 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=fixture \
  "$NET"
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a140 \
    --label oo.session="${SESSION}" \
    --label oo.run_id="${RUN_ID}" \
    --label oo.kind=rust-builder \
    "$CARGO_VOL" >/dev/null
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a140 \
    --label oo.session="${SESSION}" \
    --label oo.run_id="${RUN_ID}" \
    --label oo.kind=rust-builder \
    "$TARGET_VOL" >/dev/null
fi
if docker volume inspect "$PG_VOL" >/dev/null 2>&1; then
  echo "refusing a pre-existing postgres volume" >&2
  docker network rm "$NET" > "$EVIDENCE/cleanup-network.txt" 2>&1 || true
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a140 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$PG_VOL" >/dev/null

cleanup_partial() {
  docker rm -f "$PG" > "$EVIDENCE/cleanup-postgres-rm.txt" 2>&1 || true
  docker network rm "$NET" > "$EVIDENCE/cleanup-network.txt" 2>&1 || true
  docker volume rm "$PG_VOL" > "$EVIDENCE/cleanup-postgres-volume.txt" 2>&1 || true
}

set +e
docker run -d \
  --name "$PG" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add SETGID --cap-add SETUID \
  --memory 1g --cpus 1 --pids-limit 256 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,nodev,size=256m \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  --label oo.project=open-ontology \
  --label oo.owner=a140 \
  --label oo.session="${SESSION}" \
  --label oo.task=physical-mapping-fixed \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-physical-mapping \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  "$PG_IMAGE" \
  -c listen_addresses='*' -c max_connections=40 -c shared_preload_libraries= \
  > "$EVIDENCE/postgres-start.txt"
pg_start=$?
set -e
if [ "$pg_start" -ne 0 ]; then
  cleanup_partial
  echo "postgres did not start" >&2
  exit 1
fi

ready=0
for _ in $(seq 1 60); do
  if docker exec "$PG" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  docker logs "$PG" > "$EVIDENCE/postgres.log" 2>&1 || true
  cleanup_partial
  echo "postgres did not become ready" >&2
  exit 1
fi
printf 'ready=1\n' > "$EVIDENCE/postgres-ready.txt"

set +e
docker run \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e OO_PHYSICAL_MAPPING_HOST="$PG" \
  -e OO_PHYSICAL_MAPPING_PORT=5432 \
  -e OO_PHYSICAL_MAPPING_EVIDENCE=/evidence \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=bind,source=${ROOT}/fixed-snapshot,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a140 \
  --label oo.session="${SESSION}" \
  --label oo.task=physical-mapping-fixed \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-physical-mapping \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt" || true
docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} readonly_root={{.HostConfig.ReadonlyRootfs}}' > "$EVIDENCE/hostconfig.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.Binds}}{{json .HostConfig.Mounts}}' > "$EVIDENCE/mounts.txt" || true
docker inspect "$PG" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}}' > "$EVIDENCE/postgres-hostconfig.txt" || true

restart_exit=0
if [ "$run_exit" -eq 0 ]; then
  set +e
  docker restart "$PG" > "$EVIDENCE/postgres-restart.txt" 2>&1
  restart_exit=$?
  set -e
  ready=0
  for _ in $(seq 1 60); do
    if docker exec "$PG" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  if [ "$ready" -ne 1 ]; then
    echo "postgres did not return after restart" > "$EVIDENCE/restart-lookup.txt"
    restart_exit=1
  else
    set +e
    docker exec "$PG" psql -U postgres -d oo_pm -v ON_ERROR_STOP=1 -At -F '|' -c "SELECT physical_name, encode(identity_digest, 'hex'), current_setting('server_version') FROM _oo.physical_mappings WHERE source = 'src_main' AND config_digest = 'abababababababababababababababababababababababababababababababab' AND run_generation = 1 AND level = 'n' AND logical_namespace = 'customer' AND logical_table = '' AND logical_column = '';" > "$EVIDENCE/restart-lookup.txt" 2> "$EVIDENCE/restart-lookup.err"
    lookup_exit=$?
    set -e
    if [ "$lookup_exit" -ne 0 ]; then
      restart_exit=$lookup_exit
    else
      expected_name=$(awk -F= '$1=="physical_name" {print $2}' "$EVIDENCE/durable-name.txt")
      expected_digest=$(awk -F= '$1=="identity_digest" {print $2}' "$EVIDENCE/durable-name.txt")
      got_line=$(head -n 1 "$EVIDENCE/restart-lookup.txt")
      got_name=${got_line%%|*}
      rest=${got_line#*|}
      got_digest=${rest%%|*}
      got_version=${rest#*|}
      if [ "$got_name" = "$expected_name" ] && [ "$got_digest" = "$expected_digest" ] && [ "${got_version#17.10}" != "$got_version" ]; then
        printf 'match=1\nphysical_name=%s\nidentity_digest=%s\nserver_version=%s\n' "$got_name" "$got_digest" "$got_version" > "$EVIDENCE/restart-compare.txt"
      else
        printf 'match=0\nphysical_name=%s\nidentity_digest=%s\nserver_version=%s\nexpected_name=%s\nexpected_digest=%s\n' "$got_name" "$got_digest" "$got_version" "$expected_name" "$expected_digest" > "$EVIDENCE/restart-compare.txt"
        restart_exit=5
      fi
    fi
  fi
else
  printf 'skipped\n' > "$EVIDENCE/restart-compare.txt"
fi
printf '%s\n' "$restart_exit" > "$EVIDENCE/restart.exit"
docker logs "$PG" > "$EVIDENCE/postgres.log" 2>&1 || true
docker inspect "$PG" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}}' > "$EVIDENCE/postgres-inspect.txt" || true

set +e
docker rm -f "$NAME" > "$EVIDENCE/cleanup-rm.txt" 2>&1
rm_exit=$?
docker rm -f "$PG" > "$EVIDENCE/cleanup-postgres-rm.txt" 2>&1
pg_rm_exit=$?
docker network rm "$NET" > "$EVIDENCE/cleanup-network.txt" 2>&1
net_exit=$?
if [ "$run_exit" -eq 0 ] && [ "$restart_exit" -eq 0 ]; then
  docker volume rm "$CARGO_VOL" > "$EVIDENCE/cleanup-cargo-volume.txt" 2>&1
  cargo_exit=$?
  docker volume rm "$TARGET_VOL" > "$EVIDENCE/cleanup-target-volume.txt" 2>&1
  target_exit=$?
  docker volume rm "$PG_VOL" > "$EVIDENCE/cleanup-postgres-volume.txt" 2>&1
  pg_vol_exit=$?
else
  printf 'retained_for_rerun\n' > "$EVIDENCE/cleanup-cargo-volume.txt"
  printf 'retained_for_rerun\n' > "$EVIDENCE/cleanup-target-volume.txt"
  docker volume rm "$PG_VOL" > "$EVIDENCE/cleanup-postgres-volume.txt" 2>&1
  pg_vol_exit=$?
  cargo_exit=0
  target_exit=0
fi
set -e
printf 'rm_exit=%s\npostgres_rm_exit=%s\nnetwork_exit=%s\ncargo_volume_exit=%s\ntarget_volume_exit=%s\npostgres_volume_exit=%s\n' \
  "$rm_exit" "$pg_rm_exit" "$net_exit" "$cargo_exit" "$target_exit" "$pg_vol_exit" > "$EVIDENCE/cleanup.txt"
final=$run_exit
if [ "$restart_exit" -gt "$final" ]; then
  final=$restart_exit
fi
echo "docker_run_exit=${run_exit} restart_exit=${restart_exit}"
exit "$final"
