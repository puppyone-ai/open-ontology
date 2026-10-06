#!/bin/bash
# Independent review smoke of fixed commit c551afea and pre-seam parent f57e430f.
# Immutable git archives. No product overlay. No PostgreSQL. No prune.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh
# Immediate shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/smoke/launch.sh
# Delta: owner a286, integrated seam archive plus the parent adapter archive,
# memory 6 GiB, one CPU, global builder cap 10.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a286/SP12/question-bank-suite-review
EVIDENCE=${REVIEW}/linux/smoke
ROOT=/tmp/oo-a286-eef831aa-dba4-4af2-a78b-541bc9e0a286/smoke
FIXED=c551afea55d5a9164760878a5d6a76835b869875
OLD=f57e430f51b9f44175ffab5ae876cd64dd558c78
SEAM=eb0bbbe7b82789cbdc362771e1b89226fc1af832
SUITE=c9b3c6fffef507afc2e4c54d1fc5933f008ae04a
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a286-smoke
NET=oo-a286-eef831aa-net
CARGO_VOL=oo-a286-eef831aa-cargo
TARGET_VOL=oo-a286-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi

shasum -a 256 \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/tools/sha256.rs" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/tools/select-debs.rs" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/smoke/launch.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/live/launch.sh" \
  > "$EVIDENCE/origin-recipes.txt" || exit 2

git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt" || exit 2
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt" || exit 2
git -C "$SRC" rev-parse "${OLD}^{tree}" > "$EVIDENCE/old-tree.txt" || exit 2
git -C "$SRC" merge-base --is-ancestor "$SEAM" "$FIXED"
printf '%s\n' "$?" > "$EVIDENCE/seam-ancestor.exit"
git -C "$SRC" merge-base --is-ancestor "$FIXED" HEAD
printf '%s\n' "$?" > "$EVIDENCE/fixed-ancestor.exit"
git -C "$SRC" diff --exit-code "${SUITE}^" "$FIXED" -- Cargo.toml Cargo.lock
printf '%s\n' "$?" > "$EVIDENCE/root-lock-since-suite.exit"
git -C "$SRC" diff --exit-code "${SUITE}^" "$FIXED" -- \
  benchmarks/query-result-adapter/Cargo.lock \
  benchmarks/sql-runner/Cargo.lock \
  benchmarks/question-bank/Cargo.lock \
  benchmarks/result-comparator/Cargo.lock
printf '%s\n' "$?" > "$EVIDENCE/bench-locks-since-suite.exit"
git -C "$SRC" diff --exit-code "$SUITE" "$FIXED" -- benchmarks/question-bank-suite/Cargo.lock
printf '%s\n' "$?" > "$EVIDENCE/suite-lock-since-introduction.exit"
git -C "$SRC" diff --name-only "${SEAM}^" "$SEAM" -- ':(glob)**/Cargo.toml' ':(glob)**/Cargo.lock' > "$EVIDENCE/seam-lock-names.txt" || true
if [ -s "$EVIDENCE/seam-lock-names.txt" ]; then
  printf '%s\n' 1 > "$EVIDENCE/seam-lock-diff.exit"
else
  printf '%s\n' 0 > "$EVIDENCE/seam-lock-diff.exit"
fi
git -C "$SRC" diff --exit-code "$OLD" "$FIXED" -- \
  benchmarks/query-result-adapter/Cargo.lock \
  benchmarks/sql-runner/Cargo.lock
printf '%s\n' "$?" > "$EVIDENCE/adapter-runner-lock-vs-parent.exit"
git -C "$SRC" diff --exit-code "$FIXED" HEAD -- \
  Cargo.toml Cargo.lock \
  benchmarks/question-bank-suite \
  benchmarks/sql-runner \
  benchmarks/query-result-adapter
printf '%s\n' "$?" > "$EVIDENCE/head-vs-fixed-benches.exit"

archive_one() {
  local commit="$1"
  local dest="$2"
  rm -rf "$dest"
  mkdir -p "$dest"
  git -C "$SRC" archive --format=tar "$commit" \
    Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
    benchmarks/question-bank benchmarks/sql-runner \
    benchmarks/query-result-adapter benchmarks/question-bank-suite \
    benchmarks/result-comparator \
    | tar -x -C "$dest"
  if [ ! -f "$dest/benchmarks/query-result-adapter/src/parse.rs" ]; then
    echo "archive $commit missing adapter parse.rs" >&2
    return 2
  fi
  if [ ! -f "$dest/benchmarks/question-bank-suite/src/compile.rs" ]; then
    echo "archive $commit missing suite compile.rs" >&2
    return 2
  fi
  shasum -a 256 "$dest/rust-toolchain.toml" > "$dest.toolchain.sha256"
  mv "$dest/rust-toolchain.toml" "$dest/rust-toolchain.toml.repo"
}

mkdir -p "$ROOT"
archive_one "$FIXED" "$ROOT/archive" || exit $?
archive_one "$OLD" "$ROOT/old-archive" || exit $?
mv "$ROOT/archive.toolchain.sha256" "$EVIDENCE/toolchain-file.sha256"
mv "$ROOT/old-archive.toolchain.sha256" "$EVIDENCE/old-toolchain-file.sha256"
if ! grep -q 'not_found_if_complete_empty' "$ROOT/archive/benchmarks/query-result-adapter/src/parse.rs"; then
  echo "fixed archive is missing the empty_result opt-in" >&2
  exit 2
fi
if grep -q 'not_found_if_complete_empty' "$ROOT/old-archive/benchmarks/query-result-adapter/src/parse.rs"; then
  echo "parent archive already contains the empty_result opt-in" >&2
  exit 2
fi

BLOB=$REVIEW/source/blobs
mkdir -p \
  "$BLOB/benchmarks/query-result-adapter/src" \
  "$BLOB/benchmarks/query-result-adapter/tests" \
  "$BLOB/benchmarks/question-bank-suite/src" \
  "$BLOB/benchmarks/question-bank-suite/tests/support" \
  "$BLOB/benchmarks/sql-runner/src" \
  "$BLOB/benchmarks/sql-runner/tests"
copy_blob() {
  cp "$ROOT/archive/$1" "$BLOB/$1"
}
copy_blob benchmarks/query-result-adapter/CONTRACT.md
copy_blob benchmarks/query-result-adapter/src/parse.rs
copy_blob benchmarks/query-result-adapter/tests/adapt.rs
copy_blob benchmarks/question-bank-suite/CONTRACT.md
copy_blob benchmarks/question-bank-suite/src/compile.rs
copy_blob benchmarks/question-bank-suite/tests/compile.rs
copy_blob benchmarks/question-bank-suite/tests/proof.rs
copy_blob benchmarks/question-bank-suite/tests/support/mod.rs
copy_blob benchmarks/sql-runner/CONTRACT.md
copy_blob benchmarks/sql-runner/src/parse.rs
copy_blob benchmarks/sql-runner/tests/cli.rs
(
  cd "$BLOB"
  shasum -a 256 \
    benchmarks/query-result-adapter/CONTRACT.md \
    benchmarks/query-result-adapter/src/parse.rs \
    benchmarks/query-result-adapter/tests/adapt.rs \
    benchmarks/question-bank-suite/CONTRACT.md \
    benchmarks/question-bank-suite/src/compile.rs \
    benchmarks/question-bank-suite/tests/compile.rs \
    benchmarks/question-bank-suite/tests/proof.rs \
    benchmarks/question-bank-suite/tests/support/mod.rs \
    benchmarks/sql-runner/CONTRACT.md \
    benchmarks/sql-runner/src/parse.rs \
    benchmarks/sql-runner/tests/cli.rs \
    > "$EVIDENCE/blob-sha256.txt"
)

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a286 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a286 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a286 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a286 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a286 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a286 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a286 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/old-archive,target=/old,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW}/oracle,target=/oracle" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a286 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a286 \
  --label oo.task=question-bank-suite-review-smoke \
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
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
