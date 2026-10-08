#!/bin/bash
set -euo pipefail
SHA=b12b5b41924754d1683c92d5a34ec1f49767839f
ROOT=/tmp/oo-a353-m11
NAME=oo-a353-node-m11
IMAGE=docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413
REPO=/Users/redux/puppy-one/open-ontology
cd "$REPO"
if [ -e "$ROOT" ]; then
  echo "ABORT root exists"
  exit 1
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "ABORT container name exists"
  exit 1
fi
node_like=$(docker ps --format '{{.Names}} {{.Image}}' | awk 'tolower($0) ~ /node|oo-fe0|oo-a353-node/ {c++} END{print c+0}')
echo "node_like=$node_like"
if [ "$node_like" -ge 1 ]; then
  echo "ABORT node slot taken"
  exit 1
fi
mkdir -p "$ROOT/src" "$ROOT/raw" "$ROOT/home"
cp "$0" "$ROOT/raw/launch.sh"
docker ps -a --format '{{.Names}} {{.Image}} {{.Status}}' > "$ROOT/raw/containers-before.txt"
{
  echo "archive=$SHA"
  git rev-parse "$SHA"
  echo "workspace=$(git rev-parse HEAD)"
  if git merge-base --is-ancestor "$SHA" HEAD; then echo m12_ancestor=yes; else echo m12_ancestor=no; fi
  git diff --stat "$SHA" HEAD -- apps/web packages/sdk-ts pnpm-lock.yaml
} > "$ROOT/raw/source-head.txt"
git archive "$SHA" \
  package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc .node-version \
  apps/web packages/sdk-ts tools/control-identity tools/selfbuild-control-api \
  | tar -x -C "$ROOT/src"
cp -cR /tmp/oo-a353-m0/src/node_modules "$ROOT/src/node_modules"
cp -cR /tmp/oo-a353-m0/src/apps/web/node_modules "$ROOT/src/apps/web/node_modules"
mkdir -p "$ROOT/corepack"
cp -cR /tmp/oo-a353-m0/corepack/. "$ROOT/corepack/"
files=(
  apps/web/src/features/admin-overview/session-example.test.tsx
  apps/web/src/features/permissions/APIBINDING.md
)
: > "$ROOT/raw/source-inventory.txt"
for f in "${files[@]}"; do
  mkdir -p "$ROOT/src/$(dirname "$f")"
  cp "$REPO/$f" "$ROOT/src/$f"
  shasum -a 256 "$ROOT/src/$f" >> "$ROOT/raw/source-inventory.txt"
done
frozen=(
  apps/web/src/app/router.tsx
  apps/web/src/app/admin-gate.tsx
  apps/web/src/api/identity-session.ts
  apps/web/src/app/identity-context.tsx
  packages/sdk-ts/src/bindings.ts
  packages/sdk-ts/src/client.ts
  apps/web/config/eslint.config.mjs
  apps/web/deploy/b01-adapter.mjs
  apps/web/src/styles/fonts.css
  apps/web/design/font-manifest.json
  apps/web/src/components/shared/app-shell.tsx
  apps/web/src/components/shared/review-decision-form.tsx
  apps/web/src/features/admin-overview/blocks.tsx
  apps/web/src/features/admin-overview/nav.ts
  apps/web/src/features/admin-overview/admin-frame.tsx
  pnpm-lock.yaml
)
{
  echo "archive=$SHA"
  for f in "${frozen[@]}"; do
    git show "$SHA:$f" | shasum -a 256 | awk -v p="$ROOT/src/$f" '{print $1, p}'
    git show "$SHA:$f" | cmp - "$ROOT/src/$f"
  done
  echo frozen_cmp=ok
} > "$ROOT/raw/frozen-sha256.txt"
shasum -a 256 "$ROOT/src/pnpm-lock.yaml" > "$ROOT/raw/lock.txt"
if grep -n 'kind: "secret"' "$ROOT/src/apps/web/src/features/admin-overview/blocks.tsx"; then
  echo "ABORT secret kind"
  exit 1
fi
if grep -n 'ReviewDecisionForm' "$ROOT/src/apps/web/src/features/admin-overview/blocks.tsx" "$ROOT/src/apps/web/src/features/reviews/pages.tsx"; then
  echo "ABORT review form"
  exit 1
fi
if grep -n 'to: ".*:' "$ROOT/src/apps/web/src/features/admin-overview/nav.ts"; then
  echo "ABORT nav id"
  exit 1
fi
if ! grep -q 'Buffer: "readonly"' "$ROOT/src/apps/web/config/eslint.config.mjs"; then
  echo "ABORT missing Buffer global"
  exit 1
fi
cat > "$ROOT/raw/inside.sh" << 'EOF'
#!/bin/bash
set -u
export HOME=/home/fe02
export COREPACK_HOME=/corepack
export CI=1
export TMPDIR=/tmp
cd /src/apps/web
{
  echo "node=$(node -v)"
  echo "pnpm=$(corepack pnpm --version)"
  uname -m
  id
} > /raw/identity.txt 2>&1
cat /raw/identity.txt
worst=0
phase() {
  name="$1"
  shift
  "$@" > "/raw/${name}.log" 2>&1
  ec=$?
  printf '%s\n' "$ec" > "/raw/${name}.exit"
  echo "phase=${name} exit=${ec}"
  if [ "$ec" -gt "$worst" ]; then worst=$ec; fi
}
phase types corepack pnpm exec tsc -p tsconfig.json --noEmit --pretty false
phase types-node corepack pnpm exec tsc -p tsconfig.node.json --noEmit --pretty false
phase types-sdk corepack pnpm exec tsc -p ../../packages/sdk-ts/tsconfig.json --noEmit --pretty false
phase lint corepack pnpm exec eslint --config config/eslint.config.mjs .
phase test corepack pnpm exec vitest run --config config/vitest.config.ts
echo DONE
exit "$worst"
EOF
chmod 755 "$ROOT/raw/inside.sh"
echo "launching $NAME"
set +e
docker run --name "$NAME" \
  --pull never \
  --platform linux/arm64 \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  --memory 4g \
  --memory-swap 4g \
  --cpus 1 \
  --pids-limit 2048 \
  --network none \
  -e HOME=/home/fe02 \
  -e COREPACK_HOME=/corepack \
  -e CI=1 \
  -e TMPDIR=/tmp \
  -v "$ROOT/src:/src" \
  -v "$ROOT/corepack:/corepack" \
  -v "$ROOT/home:/home/fe02" \
  -v "$ROOT/raw:/raw" \
  -w /src/apps/web \
  "$IMAGE" \
  bash /raw/inside.sh
docker_ec=$?
set -e
echo "DOCKER_EC:$docker_ec"
docker inspect "$NAME" --format 'name={{.Name}} exit={{.State.ExitCode}} mem={{.HostConfig.Memory}} swap={{.HostConfig.MemorySwap}} cpu={{.HostConfig.NanoCpus}} pids={{.HostConfig.PidsLimit}} capdrop={{json .HostConfig.CapDrop}} privileged={{.HostConfig.Privileged}} net={{.HostConfig.NetworkMode}} image={{.Image}}' > "$ROOT/raw/inspect.txt" || true
docker rm "$NAME" > "$ROOT/raw/cleanup.txt"
echo "removed=$NAME" >> "$ROOT/raw/cleanup.txt"
docker ps -a --filter "name=$NAME" --format '{{.Names}}' > "$ROOT/raw/gone.txt" || true
printf '%s\n' "$docker_ec" > "$ROOT/raw/docker.exit"
exit "$docker_ec"
