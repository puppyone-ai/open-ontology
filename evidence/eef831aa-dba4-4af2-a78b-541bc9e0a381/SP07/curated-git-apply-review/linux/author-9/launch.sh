#!/bin/bash
# Private archive for the nine author cases. Git metadata is created only here.
# The workspace Git directory is not the target of init.
set -eu
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a381/SP07/curated-git-apply-review
EVIDENCE=${REVIEW}/linux/author-9
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6
NAME=oo-a381-author9
NET=oo-a381-author9-net
VOL_CARGO=oo-a381-author9-cargo
VOL_TARGET=oo-a381-author9-target
ARCHIVE=/tmp/a381-eef831aa-archive-author9
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite author-9 console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a381 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  echo "named author-9 container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
{
  echo "head=$(git -C "$SRC" rev-parse HEAD)"
  echo "fixed_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "tool_tree=$(git -C "$SRC" rev-parse "${FIXED}:tools/curated-git-apply")"
} > "$EVIDENCE/workspace-before.txt"
git -C "$SRC" cat-file -e "${FIXED}^{commit}"
git -C "$SRC" ls-tree -r "$FIXED" -- \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  tools/description-review tools/curated-git-apply \
  > "$EVIDENCE/product-ls-tree.txt"
rm -rf "$ARCHIVE"
mkdir -p "$ARCHIVE"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  tools/description-review tools/curated-git-apply \
  | tar -x -C "$ARCHIVE"
test ! -e "$ARCHIVE/.git"
test ! -e "$ARCHIVE/tools/curated-git-apply/tests/a381_review.rs"
python3 - "$ARCHIVE" "$EVIDENCE/bytes-before.txt" <<'PY'
import hashlib, os, sys
root, dest = sys.argv[1:]
rows = []
for dirpath, dirnames, filenames in os.walk(root):
    dirnames[:] = [d for d in dirnames if d != ".git"]
    for name in filenames:
        path = os.path.join(dirpath, name)
        if os.path.islink(path):
            raise SystemExit("symlink before init: " + path)
        rel = os.path.relpath(path, root)
        digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
        rows.append(f"{digest}  {rel}")
rows.sort()
open(dest, "w").write("\n".join(rows) + "\n")
print(len(rows))
PY
(
  cd /tmp
  env \
    GIT_DIR="$ARCHIVE/.git" \
    GIT_WORK_TREE="$ARCHIVE" \
    GIT_CONFIG_GLOBAL=/dev/null \
    GIT_CONFIG_SYSTEM=/dev/null \
    GIT_CONFIG_NOSYSTEM=1 \
    git init --quiet
)
test -d "$ARCHIVE/.git"
python3 - "$ARCHIVE" "$EVIDENCE/bytes-after.txt" <<'PY'
import hashlib, os, sys
root, dest = sys.argv[1:]
rows = []
for dirpath, dirnames, filenames in os.walk(root):
    dirnames[:] = [d for d in dirnames if d != ".git"]
    for name in filenames:
        path = os.path.join(dirpath, name)
        if os.path.islink(path):
            raise SystemExit("symlink after init: " + path)
        rel = os.path.relpath(path, root)
        digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
        rows.append(f"{digest}  {rel}")
rows.sort()
open(dest, "w").write("\n".join(rows) + "\n")
print(len(rows))
PY
cmp "$EVIDENCE/bytes-before.txt" "$EVIDENCE/bytes-after.txt"
python3 - "$ARCHIVE" "$EVIDENCE/product-ls-tree.txt" "$EVIDENCE/bytes-match.txt" <<'PY'
import os, subprocess, sys
archive, listing, dest = sys.argv[1:]
expected = {}
for line in open(listing):
    line = line.rstrip("\n")
    if not line:
        continue
    meta, path = line.split("\t", 1)
    mode, typ, blob = meta.split()
    if mode not in ("100644", "100755") or typ != "blob":
        raise SystemExit("unexpected ls-tree line")
    expected[path] = blob
mismatch = []
for path, blob in expected.items():
    full = os.path.join(archive, path)
    got = subprocess.check_output(
        ["git", "hash-object", "--no-filters", "--", full], text=True
    ).strip()
    if got != blob:
        mismatch.append(path)
found = set()
for dirpath, dirnames, filenames in os.walk(archive):
    dirnames[:] = [d for d in dirnames if d != ".git"]
    for name in filenames:
        found.add(os.path.relpath(os.path.join(dirpath, name), archive))
extra = sorted(found - set(expected))
missing = sorted(set(expected) - found)
text = (
    f"files={len(expected)}\n"
    f"mismatch={len(mismatch)}\n"
    f"extra={len(extra)}\n"
    f"missing={len(missing)}\n"
)
open(dest, "w").write(text)
if mismatch or extra or missing:
    raise SystemExit(text)
PY
{
  echo "head=$(git -C "$SRC" rev-parse HEAD)"
  echo "fixed_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "tool_tree=$(git -C "$SRC" rev-parse "${FIXED}:tools/curated-git-apply")"
} > "$EVIDENCE/workspace-after.txt"
cmp "$EVIDENCE/workspace-before.txt" "$EVIDENCE/workspace-after.txt"
{
  echo "git_dir=${ARCHIVE}/.git"
  echo "init=unborn"
  if git --git-dir="$ARCHIVE/.git" rev-parse --verify HEAD >/dev/null 2>&1; then
    echo "head=present"
  else
    echo "head=unborn"
  fi
  echo "workspace_git_not_target=yes"
} > "$EVIDENCE/git-metadata.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create --driver bridge "$NET" > "$EVIDENCE/network-created.txt"
else
  printf '%s\n' "$NET" > "$EVIDENCE/network-created.txt"
fi
if ! docker volume inspect "$VOL_CARGO" >/dev/null 2>&1; then
  docker volume create "$VOL_CARGO" > "$EVIDENCE/volume-cargo.txt"
else
  printf '%s\n' "$VOL_CARGO" > "$EVIDENCE/volume-cargo.txt"
fi
if ! docker volume inspect "$VOL_TARGET" >/dev/null 2>&1; then
  docker volume create "$VOL_TARGET" > "$EVIDENCE/volume-target.txt"
else
  printf '%s\n' "$VOL_TARGET" > "$EVIDENCE/volume-target.txt"
fi
own=$(docker ps -q --filter label=oo.owner=a381 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity-before-run.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  rm -rf "$ARCHIVE"
  exit 3
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e TMPDIR=/target/tmp \
  -e CARGO_BUILD_JOBS=1 \
  -e MAKEFLAGS=-j1 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ARCHIVE},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${VOL_CARGO},target=/cargo-home" \
  --mount "type=volume,source=${VOL_TARGET},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a381 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a381 \
  --label oo.task=curated-git-apply-review-author9 \
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
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
docker volume rm "$VOL_CARGO" > "$EVIDENCE/volume-cargo-removed.txt" 2>/dev/null || true
docker volume rm "$VOL_TARGET" > "$EVIDENCE/volume-target-removed.txt" 2>/dev/null || true
docker network rm "$NET" > "$EVIDENCE/network-removed.txt" 2>/dev/null || true
rm -rf "$ARCHIVE"
exit "$run_exit"
