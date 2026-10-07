#!/bin/bash
# Immutable archive of 9d5df65, then one 6GiB builder. Does not pull.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a366
REPO=/Users/redux/puppy-one/open-ontology
EVID=${REPO}/evidence/${RUN_ID}/SP08/release-coordinator-review
ROOT=/tmp/oo-a366-${RUN_ID}
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a366-build
NET=oo-a366-eef831aa-net
COMMIT=9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3
OUT=${EVID}/linux/build-1
mkdir -p "$OUT" "$ROOT/archive" "$ROOT/build" "$ROOT/review"
global=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a366 --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
host=0
for n in cargo rustc rustdoc rustup; do
  c=$( (pgrep -x "$n" || true) | wc -l | tr -d ' ')
  host=$((host + c))
done
printf 'own=%s global=%s host_tools=%s\n' "$own" "$global" "$host" | tee "$OUT/capacity.txt"
if [ "$global" -ge 10 ] || [ "$own" -ge 1 ] || [ "$host" -ge 1 ]; then
  echo "builder capacity exceeded" >&2
  printf '%s\n' 3 > "$OUT/docker-run.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "build container already exists" >&2
  exit 2
fi
git -C "$REPO" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-builder crates/oo-core crates/oo-jobs \
  tools/release-coordinator \
  | tar -x -C "$ROOT/archive"
tar -C "$ROOT/archive" -cf - . | shasum -a 256 | awk '{print $1}' > "$EVID/origin/archive.sha256"
{
  echo "commit=$COMMIT"
  echo "archive_sha256=$(cat "$EVID/origin/archive.sha256")"
  mismatch=0
  while IFS= read -r path; do
    blob=$(git -C "$REPO" rev-parse "$COMMIT:$path")
    file=$(git -C "$REPO" hash-object "$ROOT/archive/$path")
    if [ "$blob" = "$file" ]; then
      printf 'match %s %s\n' "$blob" "$path"
    else
      printf 'MISMATCH %s %s %s\n' "$blob" "$file" "$path"
      mismatch=1
    fi
  done < <(git -C "$REPO" ls-tree -r --name-only "$COMMIT" -- crates/oo-builder crates/oo-core crates/oo-jobs tools/release-coordinator Cargo.toml Cargo.lock rust-toolchain.toml)
  printf 'mismatches=%s\n' "$mismatch"
} > "$EVID/origin/source-inventory.txt"
grep -q 'mismatches=0' "$EVID/origin/source-inventory.txt"

rm -rf "$ROOT/build" "$ROOT/review"
mkdir -p "$ROOT/build" "$ROOT/review"
tar -C "$ROOT/archive" -cf - crates tools rust-toolchain.toml Cargo.lock | tar -x -C "$ROOT/build"
cat > "$ROOT/build/Cargo.toml" <<'EOF'
[workspace]
resolver = "3"
members = [
    "crates/oo-builder",
    "crates/oo-core",
    "crates/oo-jobs",
]
[workspace.package]
edition = "2024"
license = "Apache-2.0"
publish = false
rust-version = "1.94.0"
version = "0.1.0"
[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"
[workspace.lints.clippy]
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
EOF
tar -C "$ROOT/build" -cf - . | tar -x -C "$ROOT/review"
awk '
  { print }
  $0 == "mod tls;" { print "#[cfg(test)]"; print "mod review_oracle;" }
' "$ROOT/review/tools/release-coordinator/src/lib.rs" > "$ROOT/review/tools/release-coordinator/src/lib.rs.new"
mv "$ROOT/review/tools/release-coordinator/src/lib.rs.new" "$ROOT/review/tools/release-coordinator/src/lib.rs"
cp "$EVID/oracle/review_oracle.rs" "$ROOT/review/tools/release-coordinator/src/review_oracle.rs"
diff -rq "$ROOT/build/tools/release-coordinator" "$ROOT/review/tools/release-coordinator" > "$OUT/review-tree.diff" || true

docker volume create --label oo.project=open-ontology --label oo.owner=a366 --label oo.session="$RUN_ID" --label oo.kind=cargo oo-a366-cargo >/dev/null
docker volume create --label oo.project=open-ontology --label oo.owner=a366 --label oo.session="$RUN_ID" --label oo.kind=target oo-a366-target >/dev/null

docker image inspect "$IMAGE" --format 'id={{.Id}} arch={{.Architecture}}' > "$OUT/image.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --network "$NET" \
  -e HOME=/tmp \
  -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e CARGO_HOME=/cargo \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e OO_A359_PG_HOST=oo-a366-pg \
  -e OO_A359_PG_PORT=5432 \
  -e OO_A359_PG_DATABASE=oo_a366_author \
  -e OO_A359_PASSWORD_FILE=/secrets/release-admin.pass \
  -e OO_A359_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A359_CA_FILE=/secrets/ca.crt \
  -e OO_A359_OBSERVATION=/out/author-observation.json \
  -e OO_A366_PG_HOST=oo-a366-pg \
  -e OO_A366_PG_PORT=5432 \
  -e OO_A366_PG_DATABASE=oo_a366 \
  -e OO_A366_PASSWORD_FILE=/secrets/release-admin.pass \
  -e OO_A366_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A366_CA_FILE=/secrets/ca.crt \
  -e OO_A366_HAND=/hand/hand.txt \
  -e OO_A366_FIXTURE=/out/fixture.json \
  -e OO_A366_AUTHORITY=/out/authority.txt \
  -e OO_A366_AUDIT=/out/audit.txt \
  -e OO_A366_OBSERVATION=/out/observation.json \
  --mount "type=bind,source=${ROOT}/archive,target=/immutable,readonly" \
  --mount "type=bind,source=${ROOT}/build,target=/build,readonly" \
  --mount "type=bind,source=${ROOT}/review,target=/review,readonly" \
  --mount "type=bind,source=${EVID}/oracle,target=/hand,readonly" \
  --mount "type=bind,source=${EVID}/linux/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${OUT},target=/out" \
  --mount "type=bind,source=${ROOT}/secrets,target=/secrets,readonly" \
  --mount "type=volume,source=oo-a366-cargo,target=/cargo" \
  --mount "type=volume,source=oo-a366-target,target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a366 \
  --label oo.session="$RUN_ID" \
  --label oo.kind=rust-builder \
  --label oo.task=release-coordinator-review \
  --entrypoint /bin/bash \
  "$IMAGE" /inside.sh > "$OUT/console.log" 2>&1
code=$?
printf '%s\n' "$code" > "$OUT/docker-run.exit"
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} exit={{.State.ExitCode}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}} network={{.HostConfig.NetworkMode}}' > "$OUT/hostconfig.txt"
exit "$code"
