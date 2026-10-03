#!/bin/bash
# a142 independent review. One Linux arm64 Rust 1.94 builder and one private PostgreSQL 17.10.
# Immutable git archive of b5415544c9eb8d608eafd411884627dda9ee7c84. No pull, prune, host network,
# socket, HOME mount, or foreign cleanup.
set -eu
RUN_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
COMMIT=b5415544c9eb8d608eafd411884627dda9ee7c84
ROOT=/tmp/oo-a142-eef831aa-dba4-4af2-a78b-541bc9e0a142
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a142/SP04/physical-mapping-review/linux/fixed
PROBE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a142/SP04/physical-mapping-review/linux/probe
REPO=/Users/redux/puppy-one/open-ontology
NAME=oo-a142-msrv-${RUN_ID}
PG=oo-a142-pg-${RUN_ID}
NET=oo-a142-${RUN_ID}-net
CARGO_VOL=oo-a142-eef831aa-dba4-4af2-a78b-541bc9e0a142-cargo
TARGET_VOL=oo-a142-eef831aa-dba4-4af2-a78b-541bc9e0a142-target
PG_VOL=oo-a142-eef831aa-dba4-4af2-a78b-541bc9e0a142-pg
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a142
SUCCESS=0
CLEANED=0

mkdir -p "$EVIDENCE" "$PROBE" "$ROOT"
chmod +x "$EVIDENCE/inside.sh" "$PROBE/probe.py"
printf '%s\n' "$NAME" > "$EVIDENCE/container-name.txt"
printf '%s\n' "$PG" > "$EVIDENCE/postgres-container.txt"
printf '%s\n' "$NET" > "$EVIDENCE/network-name.txt"
printf '%s\n' "$CARGO_VOL" > "$EVIDENCE/cargo-volume.txt"
printf '%s\n' "$TARGET_VOL" > "$EVIDENCE/target-volume.txt"
printf '%s\n' "$PG_VOL" > "$EVIDENCE/postgres-volume.txt"
printf '%s\n' "$RUN_ID" > "$EVIDENCE/run-id.txt"

cleanup() {
  if [ "$CLEANED" = 1 ]; then
    return
  fi
  CLEANED=1
  if [ -n "${PG:-}" ]; then
    docker logs "$PG" > "$EVIDENCE/postgres.log" 2>&1 || true
    docker inspect "$PG" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}}' > "$EVIDENCE/postgres-inspect.txt" 2>/dev/null || true
  fi
  if [ -n "${NAME:-}" ]; then
    docker rm -f "$NAME" > "$EVIDENCE/cleanup-rm.txt" 2>&1 || true
  fi
  if [ -n "${PG:-}" ]; then
    docker rm -f "$PG" > "$EVIDENCE/cleanup-postgres-rm.txt" 2>&1 || true
  fi
  if [ -n "${NET:-}" ]; then
    docker network rm "$NET" > "$EVIDENCE/cleanup-network.txt" 2>&1 || true
  fi
  if [ "$SUCCESS" = 1 ]; then
    docker volume rm "$CARGO_VOL" > "$EVIDENCE/cleanup-cargo-volume.txt" 2>&1 || true
    docker volume rm "$TARGET_VOL" > "$EVIDENCE/cleanup-target-volume.txt" 2>&1 || true
    docker volume rm "$PG_VOL" > "$EVIDENCE/cleanup-postgres-volume.txt" 2>&1 || true
  else
    printf 'retained_for_rerun\n' > "$EVIDENCE/cleanup-cargo-volume.txt"
    printf 'retained_for_rerun\n' > "$EVIDENCE/cleanup-target-volume.txt"
    docker volume rm "$PG_VOL" > "$EVIDENCE/cleanup-postgres-volume.txt" 2>&1 || true
  fi
  {
    echo "success=$SUCCESS"
    echo "rm=$(cat "$EVIDENCE/cleanup-rm.txt" 2>/dev/null || true)"
    echo "postgres_rm=$(cat "$EVIDENCE/cleanup-postgres-rm.txt" 2>/dev/null || true)"
    echo "network=$(cat "$EVIDENCE/cleanup-network.txt" 2>/dev/null || true)"
  } > "$EVIDENCE/cleanup.txt"
}
trap cleanup EXIT

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
own_pg=$(docker ps --filter label=oo.owner=a142 --filter label=oo.kind=postgres --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
printf 'builder_containers=%s host_cargo=%s own_postgres=%s\n' "$count" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$((count + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builders already at global max" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a142 --filter label=oo.kind=rust-builder --format '{{.Names}}')
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
docker image inspect "$RUST_IMAGE" --format '{{.Id}} {{.Os}}/{{.Architecture}}' > "$EVIDENCE/rust-image-id.txt"
docker image inspect "$PG_IMAGE" --format '{{.Id}} {{.Os}}/{{.Architecture}}' > "$EVIDENCE/postgres-image-id.txt"

rm -rf "$ROOT/fixed-snapshot" "$ROOT/fixed-archive.tar"
mkdir -p "$ROOT/fixed-snapshot"
git -C "$REPO" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  contracts/pipeline/physical-mapping-v1.md \
  contracts/pipeline/target-fence-receipts \
  > "$ROOT/fixed-archive.tar"
tar -x -f "$ROOT/fixed-archive.tar" -C "$ROOT/fixed-snapshot"
shasum -a 256 "$ROOT/fixed-archive.tar" | awk '{print $1}' > "$EVIDENCE/archive.tar.sha256"
git -C "$REPO" rev-parse "$COMMIT" > "$EVIDENCE/source-commit.txt"
git -C "$REPO" rev-parse "$COMMIT^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$REPO" rev-parse "$COMMIT^" > "$EVIDENCE/source-parent.txt"
git -C "$REPO" rev-parse HEAD > "$EVIDENCE/workspace-head-at-archive.txt"
python3 - "$REPO" "$COMMIT" "$ROOT/fixed-snapshot" "$EVIDENCE/archive-verify.txt" "$EVIDENCE/archive-mismatch.txt" << 'PY'
import subprocess, sys
repo, commit, snapshot, verify_path, mismatch_path = sys.argv[1:]
paths = subprocess.check_output(
    ["git", "-C", repo, "ls-tree", "-r", "--name-only", commit, "--",
     "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", "crates",
     "contracts/pipeline/physical-mapping-v1.md",
     "contracts/pipeline/target-fence-receipts"],
    text=True,
).splitlines()
listing = subprocess.check_output(
    ["git", "-C", repo, "ls-tree", "-r", commit, "--",
     "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", "crates",
     "contracts/pipeline/physical-mapping-v1.md",
     "contracts/pipeline/target-fence-receipts"],
    text=True,
)
expected = {}
for line in listing.splitlines():
    meta, path = line.split("\t", 1)
    expected[path] = meta.split()[2]
files = 0
links = 0
mismatch = 0
missing = 0
mismatches = []
import os
for dirpath, dirnames, filenames in os.walk(snapshot):
    for name in filenames:
        full = os.path.join(dirpath, name)
        rel = os.path.relpath(full, snapshot)
        if os.path.islink(full):
            links += 1
            mismatches.append(f"symlink {rel}")
            continue
        files += 1
        got = subprocess.check_output(["git", "hash-object", full], text=True).strip()
        exp = expected.get(rel)
        if exp is None or got != exp:
            mismatch += 1
            mismatches.append(f"mismatch {rel}")
    for name in dirnames:
        full = os.path.join(dirpath, name)
        if os.path.islink(full):
            links += 1
            rel = os.path.relpath(full, snapshot)
            mismatches.append(f"symlink {rel}")
for path in expected:
    if not os.path.isfile(os.path.join(snapshot, path)):
        missing += 1
        mismatches.append(f"missing {path}")
open(mismatch_path, "w").write("\n".join(mismatches) + ("\n" if mismatches else ""))
open(verify_path, "w").write(
    f"files={files}\nlinks={links}\nmismatched={mismatch}\nmissing={missing}\nexpected={len(expected)}\n"
)
if mismatch or links or missing or files != len(expected):
    sys.exit(2)
PY

cat > "$EVIDENCE/invocation.txt" << EOF
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
snapshot_method=git archive of ${COMMIT}
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
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings; then unchanged physical_mapping_live; then host probe
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a142 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=fixture \
  "$NET" > "$EVIDENCE/network-create.txt"
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a142 \
    --label oo.session="${SESSION}" \
    --label oo.run_id="${RUN_ID}" \
    --label oo.kind=rust-builder \
    "$CARGO_VOL" >/dev/null
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a142 \
    --label oo.session="${SESSION}" \
    --label oo.run_id="${RUN_ID}" \
    --label oo.kind=rust-builder \
    "$TARGET_VOL" >/dev/null
fi
if docker volume inspect "$PG_VOL" >/dev/null 2>&1; then
  echo "refusing a pre-existing postgres volume" >&2
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a142 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$PG_VOL" >/dev/null

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
  --label oo.owner=a142 \
  --label oo.session="${SESSION}" \
  --label oo.task=physical-mapping-review \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-physical-mapping-review \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  "$PG_IMAGE" \
  -c listen_addresses='*' -c max_connections=40 -c shared_preload_libraries= \
  > "$EVIDENCE/postgres-start.txt"

ready=0
for _ in $(seq 1 60); do
  if docker exec "$PG" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
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
  --label oo.owner=a142 \
  --label oo.session="${SESSION}" \
  --label oo.task=physical-mapping-review \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-physical-mapping-review \
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
docker inspect "$PG" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}}' > "$EVIDENCE/postgres-hostconfig.txt" || true

probe_exit=9
if [ "$run_exit" -eq 0 ]; then
  set +e
  python3 "$PROBE/probe.py" "$ROOT/fixed-snapshot" "$PROBE" "$PG" > "$PROBE/probe-console.log" 2>&1
  probe_exit=$?
  set -e
else
  echo "probe skipped because smoke or the unchanged live test failed" > "$PROBE/probe-console.log"
fi
printf '%s\n' "$probe_exit" > "$PROBE/probe.exit.host"

restart_exit=0
if [ "$run_exit" -eq 0 ] && [ "$probe_exit" -eq 0 ]; then
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
    docker exec -i "$PG" psql -U oo_a140_runtime -d oo_pm -h 127.0.0.1 -v ON_ERROR_STOP=1 -At --no-psqlrc \
      -c "SELECT _oo.lookup_physical_mapping('src_main', 'abababababababababababababababababababababababababababababababab', 1, 'namespace', 'customer', '', '');" \
      > "$EVIDENCE/restart-golden-lookup.txt" 2> "$EVIDENCE/restart-golden-lookup.err"
    golden_lookup_exit=$?
    docker exec -i "$PG" psql -U postgres -d oo_pm -h 127.0.0.1 -v ON_ERROR_STOP=1 -At --no-psqlrc \
      -c "SELECT physical_name || '|' || encode(identity_digest, 'hex') || '|' || current_setting('server_version') FROM _oo.physical_mappings WHERE source = 'src_main' AND config_digest = 'abababababababababababababababababababababababababababababababab' AND run_generation = 1 AND level = 'n' AND logical_namespace = 'customer' AND logical_table = '' AND logical_column = '';" \
      > "$EVIDENCE/restart-golden-select.txt" 2> "$EVIDENCE/restart-golden-select.err"
    golden_select_exit=$?
    docker exec -i "$PG" psql -U oo_a142_runtime -d oo_pm_review -h 127.0.0.1 -v ON_ERROR_STOP=1 -At --no-psqlrc \
      < "$PROBE/restart-lookup.sql" > "$EVIDENCE/restart-fresh-lookup.txt" 2> "$EVIDENCE/restart-fresh-lookup.err"
    fresh_lookup_exit=$?
    docker exec -i "$PG" psql -U postgres -d oo_pm_review -h 127.0.0.1 -v ON_ERROR_STOP=1 -At --no-psqlrc \
      < "$PROBE/restart-select.sql" > "$EVIDENCE/restart-fresh-select.txt" 2> "$EVIDENCE/restart-fresh-select.err"
    fresh_select_exit=$?
    set -e
    expected_name=$(awk -F= '$1=="physical_name" {print $2}' "$EVIDENCE/durable-name.txt")
    expected_digest=$(awk -F= '$1=="identity_digest" {print $2}' "$EVIDENCE/durable-name.txt")
    fresh_expected=$(awk -F= '$1=="physical" {print $2}' "$PROBE/expected.txt")
    fresh_digest=$(awk -F= '$1=="digest" {print $2}' "$PROBE/expected.txt")
    golden_lookup=$(head -n 1 "$EVIDENCE/restart-golden-lookup.txt")
    golden_select=$(head -n 1 "$EVIDENCE/restart-golden-select.txt")
    fresh_lookup=$(head -n 1 "$EVIDENCE/restart-fresh-lookup.txt")
    fresh_select=$(head -n 1 "$EVIDENCE/restart-fresh-select.txt")
    golden_select_name=${golden_select%%|*}
    golden_rest=${golden_select#*|}
    golden_select_digest=${golden_rest%%|*}
    fresh_select_name=${fresh_select%%|*}
    fresh_rest=${fresh_select#*|}
    fresh_select_digest=${fresh_rest%%|*}
    if [ "$golden_lookup_exit" -eq 0 ] && [ "$golden_select_exit" -eq 0 ] \
      && [ "$fresh_lookup_exit" -eq 0 ] && [ "$fresh_select_exit" -eq 0 ] \
      && [ "$golden_lookup" = "$expected_name" ] && [ "$golden_select_name" = "$expected_name" ] \
      && [ "$golden_select_digest" = "$expected_digest" ] \
      && [ "$fresh_lookup" = "$fresh_expected" ] && [ "$fresh_select_name" = "$fresh_expected" ] \
      && [ "$fresh_select_digest" = "$fresh_digest" ]; then
      printf 'match=1\ngolden_lookup=%s\ngolden_select=%s\nfresh_lookup=%s\nfresh_select=%s\n' \
        "$golden_lookup" "$golden_select" "$fresh_lookup" "$fresh_select" > "$EVIDENCE/restart-compare.txt"
    else
      printf 'match=0\ngolden_lookup_exit=%s\ngolden_select_exit=%s\nfresh_lookup_exit=%s\nfresh_select_exit=%s\ngolden_lookup=%s\ngolden_select=%s\nfresh_lookup=%s\nfresh_select=%s\nexpected_golden=%s\nexpected_digest=%s\nexpected_fresh=%s\nexpected_fresh_digest=%s\n' \
        "$golden_lookup_exit" "$golden_select_exit" "$fresh_lookup_exit" "$fresh_select_exit" \
        "$golden_lookup" "$golden_select" "$fresh_lookup" "$fresh_select" \
        "$expected_name" "$expected_digest" "$fresh_expected" "$fresh_digest" > "$EVIDENCE/restart-compare.txt"
      restart_exit=5
    fi
  fi
else
  printf 'skipped\n' > "$EVIDENCE/restart-compare.txt"
fi
printf '%s\n' "$restart_exit" > "$EVIDENCE/restart.exit"
if [ "$run_exit" -eq 0 ] && [ "$probe_exit" -eq 0 ] && [ "$restart_exit" -eq 0 ]; then
  SUCCESS=1
fi
echo "docker_run_exit=${run_exit} probe_exit=${probe_exit} restart_exit=${restart_exit}"
if [ "$SUCCESS" -ne 1 ]; then
  exit 1
fi
exit 0
