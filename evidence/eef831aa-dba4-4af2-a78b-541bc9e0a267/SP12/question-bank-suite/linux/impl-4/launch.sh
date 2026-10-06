#!/bin/bash
# Build question-bank-suite on the sealed 567d621e archive.
# Does not overwrite an existing console.log. Does not prune.
# Origin: a204 preoverlay launch b2b3adabd05de94723d8ed13b33be0144a0de1f9
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh
# sha256 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841
# Immediate shape: a256 impl-5 launch
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/impl-5/launch.sh
# Delta: owner a267, fixed commit 567d621e, memory 6 GiB, one CPU.
# The archive keeps question-bank, sql-runner, query-result-adapter, and
# result-comparator. Only benchmarks/question-bank-suite is overlaid.
# a256 removed benchmarks; this launch does not.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/linux/impl-4
ROOT=/tmp/oo-a267-eef831aa-dba4-4af2-a78b-541bc9e0a267/impl-4
CRATE=${SRC}/benchmarks/question-bank-suite
FIXED=567d621e2e52b7e0011bdb025dc9337e92ecc5a6
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a267-impl-4
NET=oo-a267-eef831aa-net
CARGO_VOL=oo-a267-eef831aa-cargo
TARGET_VOL=oo-a267-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite impl-2 console.log" >&2
  exit 2
fi
if [ -f "$CRATE/Cargo.lock" ]; then
  echo "crate lock must be generated inside the fixed archive" >&2
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  benchmarks/question-bank benchmarks/sql-runner \
  benchmarks/query-result-adapter benchmarks/result-comparator \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/benchmarks/question-bank/src/lib.rs" ]; then
  echo "archive missing question-bank" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/benchmarks/sql-runner/src/lib.rs" ]; then
  echo "archive missing sql-runner" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/benchmarks/query-result-adapter/src/lib.rs" ]; then
  echo "archive missing query-result-adapter" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/benchmarks/result-comparator/src/lib.rs" ]; then
  echo "archive missing result-comparator" >&2
  exit 2
fi
if [ -e "$ROOT/archive/benchmarks/question-bank-suite" ]; then
  echo "archive already contains question-bank-suite" >&2
  exit 2
fi
cp -R "$CRATE" "$ROOT/archive/benchmarks/question-bank-suite"
rm -rf "$ROOT/archive/benchmarks/question-bank-suite/target"
if [ -f "$ROOT/archive/benchmarks/question-bank-suite/Cargo.lock" ]; then
  echo "overlay already contains Cargo.lock" >&2
  exit 2
fi
shasum -a 256 "$ROOT/archive/rust-toolchain.toml" > "$EVIDENCE/toolchain-file.sha256"
mv "$ROOT/archive/rust-toolchain.toml" "$ROOT/archive/rust-toolchain.toml.repo"
(
  cd "$ROOT/archive/benchmarks/question-bank-suite"
  find . -type f -print | sort | while IFS= read -r file; do
    shasum -a 256 "$file"
  done
) > "$EVIDENCE/source-sha256.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a267 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a267 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a267 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a267 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a267 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a267 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a267 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  echo "named impl-2 container already exists" >&2
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
  --pids-limit 2048 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a267 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a267 \
  --label oo.task=question-bank-suite-impl-4 \
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
if [ -f "$ROOT/archive/benchmarks/question-bank-suite/Cargo.lock" ]; then
  cp "$ROOT/archive/benchmarks/question-bank-suite/Cargo.lock" "$EVIDENCE/Cargo.lock"
fi
if [ -d "$ROOT/archive/benchmarks/question-bank-suite" ]; then
  (
    cd "$ROOT/archive/benchmarks/question-bank-suite"
    find . -type f -print | sort | while IFS= read -r file; do
      case "$file" in
        ./Cargo.lock)
          if [ "$run_exit" -eq 0 ]; then
            cp "$file" "$CRATE/Cargo.lock"
          fi
          ;;
        *)
          mkdir -p "$CRATE/$(dirname "$file")"
          cp "$file" "$CRATE/$file"
          ;;
      esac
    done
  )
fi
exit "$run_exit"
