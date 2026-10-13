#!/bin/bash
# Build the private Pi runtime prefix. Downloads happen only in this container.
# The prefix is the export volume. It is not copied into git.
set -euo pipefail
export HOME=/var/pi-home
export GIT_TERMINAL_PROMPT=0
export NPM_CONFIG_CACHE=/npm-cache
export NPM_CONFIG_UPDATE_NOTIFIER=false
export NPM_CONFIG_FUND=false
export NPM_CONFIG_AUDIT=false
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_HOME=/cargo-home
export CARGO_INCREMENTAL=0
umask 022
: "${KIT_VOLUME:?}"
mkdir -p "$HOME" /var/pi-home/agent /cargo-home/bin /cargo-home/tmp /cargo-home/downloads /opt/pi-runtime-kit/bin /opt/pi-runtime-kit/lib /opt/pi-runtime-kit/fonts /opt/pi-runtime-kit/licenses /npm-cache
export TMPDIR=/cargo-home/tmp
# shellcheck disable=SC1091
. /pins.env
PREFIX=/opt/pi-runtime-kit
DL=/cargo-home/downloads

trap 'code=$?; if [ ! -s /evidence/inside.exit ]; then echo "$code" > /evidence/inside.exit; fi' EXIT

phase() { echo "PHASE $1"; }

die() {
  echo "$1" > /evidence/inside.exit
  echo "fail $2" >&2
  exit "$1"
}

fetch() {
  url=$1
  dest=$2
  sha=$3
  label=$4
  if [ -f "$dest" ]; then
    got=$(sha256sum "$dest" | awk '{print $1}')
    if [ "$got" = "$sha" ]; then
      echo "reuse $label"
      return 0
    fi
    rm -f "$dest"
  fi
  curl -fsSL --retry 5 --retry-delay 2 -o "$dest.partial" "$url"
  mv "$dest.partial" "$dest"
  got=$(sha256sum "$dest" | awk '{print $1}')
  if [ "$got" != "$sha" ]; then
    echo "checksum mismatch $label got=$got want=$sha" >&2
    return 1
  fi
}

phase toolchain
rustc --version | tee /evidence/rustc-version.txt
uname -m | tee /evidence/uname.txt
grep -q '^rustc 1.94.0' /evidence/rustc-version.txt
grep -qx aarch64 /evidence/uname.txt
bash --version | head -n 1 > /evidence/bash-version.txt
{
  command -v curl
  command -v git
  command -v tar
  command -v sha256sum
  command -v openssl
  command -v unzip
  command -v gzip
  command -v dpkg-deb
  command -v rustup
} > /evidence/image-tools.txt
echo 0 > /evidence/toolchain.exit

phase clippy-component
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
echo 0 > /evidence/clippy-component.exit

phase kit-test
export CARGO_TARGET_DIR=/target/kit
(
  cd /src/deploy/pi-runtime-kit
  cargo test --locked --offline -- --test-threads=1
) > /evidence/test.stdout 2> /evidence/test.stderr
echo 0 > /evidence/test.exit
(
  cd /src/deploy/pi-runtime-kit
  cargo clippy --locked --offline --all-targets -- -D warnings
) > /evidence/clippy.stdout 2> /evidence/clippy.stderr
echo 0 > /evidence/clippy.exit
(
  cd /src/deploy/pi-runtime-kit
  cargo build --locked --offline
) > /evidence/kit-build.stdout 2> /evidence/kit-build.stderr
cp /target/kit/debug/kit /cargo-home/bin/kit
chmod 755 /cargo-home/bin/kit
echo 0 > /evidence/kit-build.exit

phase source
git -c safe.directory=/src -c core.filemode=false -C /src rev-parse HEAD | tee /evidence/source-commit.txt
git -c safe.directory=/src -c core.filemode=false -C /src status --porcelain -- crates/oo-cli crates/oo-gateway crates/oo-core crates/oo-sdk tools/gateway-token-store Cargo.toml Cargo.lock > /evidence/source-status.txt
if [ -s /evidence/source-status.txt ]; then
  die 2 source-dirty
fi
sha256sum \
  /src/deploy/trusted-demo/tools/select-debs.rs \
  /src/deploy/trusted-demo/tools/sha256.rs \
  /src/deploy/trusted-demo/linux/extract-libclang.sh \
  /src/tools/local-analysis/native/font.json \
  /src/testing/spikes/p09-pi-rpc/PIN.md \
  > /evidence/origin-files.sha256
echo 0 > /evidence/source.exit

phase tools
rustc --edition 2021 /src/deploy/trusted-demo/tools/sha256.rs -o /cargo-home/bin/sha256 > /evidence/sha256-tool.stdout 2> /evidence/sha256-tool.stderr
printf abc > /cargo-home/sha256-abc.txt
/cargo-home/bin/sha256 file /cargo-home/sha256-abc.txt > /evidence/sha256-vector.txt
grep -q ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad /evidence/sha256-vector.txt
rustc --edition 2021 /src/deploy/trusted-demo/tools/select-debs.rs -o /cargo-home/bin/select-debs > /evidence/select-debs.stdout 2> /evidence/select-debs.stderr
echo 0 > /evidence/tools.exit

phase node
fetch "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt" "$DL/SHASUMS256.txt" "$NODE_SUMS_SHA" node-sums
fetch "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-arm64.tar.gz" "$DL/node.tar.gz" "$NODE_TARBALL_SHA" node-tarball
grep -F "node-v${NODE_VERSION}-linux-arm64.tar.gz" "$DL/SHASUMS256.txt" | awk '{print $1}' > /evidence/node-sums-line.sha
grep -qx "$NODE_TARBALL_SHA" /evidence/node-sums-line.sha
rm -rf "$PREFIX/node"
tar --no-same-owner -xzf "$DL/node.tar.gz" -C "$PREFIX"
mv "$PREFIX/node-v${NODE_VERSION}-linux-arm64" "$PREFIX/node"
"$PREFIX/node/bin/node" --version | tee /evidence/node-version.txt
grep -qx "v${NODE_VERSION}" /evidence/node-version.txt
cp "$PREFIX/node/LICENSE" "$PREFIX/licenses/node-LICENSE"
sha256sum "$PREFIX/licenses/node-LICENSE" | awk '{print $1}' > /evidence/node-license.sha
grep -qx "$NODE_LICENSE_SHA" /evidence/node-license.sha
test ! -L "$PREFIX/node/bin/node"
export PATH="$PREFIX/node/bin:${PATH}"
echo 0 > /evidence/node.exit

phase pi
if [ ! -d /cargo-home/pi/.git ]; then
  rm -rf /cargo-home/pi
  git -c safe.directory=/cargo-home/pi clone --depth 1 --branch "v${PI_VERSION}" https://github.com/earendil-works/pi.git /cargo-home/pi
fi
git -c safe.directory=/cargo-home/pi -C /cargo-home/pi rev-parse HEAD | tee /evidence/pi-git-commit.txt
grep -qx "$PI_COMMIT" /evidence/pi-git-commit.txt
cp /cargo-home/pi/packages/coding-agent/docs/rpc.md /evidence/upstream-rpc.md
cp /cargo-home/pi/packages/coding-agent/docs/rpc-commands.md /evidence/upstream-rpc-commands.md
sha256sum /evidence/upstream-rpc.md /evidence/upstream-rpc-commands.md > /evidence/upstream-docs.sha256
cp /cargo-home/pi/LICENSE "$PREFIX/licenses/pi-LICENSE"
sha256sum "$PREFIX/licenses/pi-LICENSE" | awk '{print $1}' > /evidence/pi-license.sha
grep -qx "$PI_LICENSE_SHA" /evidence/pi-license.sha
cp /cargo-home/pi/packages/coding-agent/install-lock/package-lock.json /evidence/package-lock.json
sha256sum /evidence/package-lock.json | awk '{print $1}' > /evidence/package-lock.sha
grep -qx "$PI_LOCK_SHA" /evidence/package-lock.sha
fetch "https://github.com/earendil-works/pi/archive/refs/tags/v${PI_VERSION}.tar.gz" "$DL/pi-tag.tar.gz" "$PI_GITHUB_TARBALL_SHA" pi-github
curl -fsSL --retry 5 --retry-delay 2 -o /evidence/npm-registry.json "https://registry.npmjs.org/@earendil-works/pi-coding-agent/${PI_VERSION}"
PI_VERSION="$PI_VERSION" PI_COMMIT="$PI_COMMIT" NPM_INTEGRITY="$NPM_INTEGRITY" "$PREFIX/node/bin/node" - <<'JS' > /evidence/npm-metadata.stdout 2> /evidence/npm-metadata.stderr
const fs = require("fs");
const m = JSON.parse(fs.readFileSync("/evidence/npm-registry.json", "utf8"));
const want = {
  version: process.env.PI_VERSION,
  gitHead: process.env.PI_COMMIT,
  integrity: process.env.NPM_INTEGRITY
};
function bad(msg) { console.error(msg); process.exit(2); }
if (m.version !== want.version) bad("version");
if (m.license !== "MIT") bad("license");
if (m.gitHead !== want.gitHead) bad("gitHead");
if (!m.engines || m.engines.node !== ">=22.19.0") bad("engine");
if (!m.dist || m.dist.integrity !== want.integrity) bad("integrity");
if (!m.repository || m.repository.url !== "git+https://github.com/earendil-works/pi.git") bad("repo");
fs.writeFileSync("/evidence/npm-tarball.url", m.dist.tarball + "\n");
JS
tarball_url=$(cat /evidence/npm-tarball.url)
if [ -f "$DL/pi.tgz" ] && [ "$(sha256sum "$DL/pi.tgz" | awk '{print $1}')" = "$NPM_TARBALL_SHA" ]; then
  echo "reuse pi-npm"
else
  curl -fsSL --retry 5 --retry-delay 2 -o "$DL/pi.tgz.partial" "$tarball_url"
  mv "$DL/pi.tgz.partial" "$DL/pi.tgz"
fi
sha256sum "$DL/pi.tgz" | awk '{print $1}' > /evidence/npm-tarball.sha
grep -qx "$NPM_TARBALL_SHA" /evidence/npm-tarball.sha
got=$(openssl dgst -sha512 -binary "$DL/pi.tgz" | openssl base64 -A)
if [ "sha512-${got}" != "$NPM_INTEGRITY" ]; then
  die 2 npm-integrity
fi
rm -rf "$PREFIX/install"
mkdir -p "$PREFIX/install"
cp /cargo-home/pi/packages/coding-agent/install-lock/package.json "$PREFIX/install/package.json"
cp /cargo-home/pi/packages/coding-agent/install-lock/package-lock.json "$PREFIX/install/package-lock.json"
(
  cd "$PREFIX/install"
  "$PREFIX/node/bin/npm" ci --ignore-scripts --no-audit --no-fund --cache /npm-cache
) > /evidence/npm-ci.stdout 2> /evidence/npm-ci.stderr
if ! cmp -s /evidence/package-lock.json "$PREFIX/install/package-lock.json"; then
  die 2 package-lock-drift
fi
PI_JS="$PREFIX/install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js"
test -f "$PI_JS"
test ! -L "$PI_JS"
chmod 755 "$PI_JS"
head -n 1 "$PI_JS" > /evidence/pi-shebang.txt
grep -q node /evidence/pi-shebang.txt
export PI_CODING_AGENT_DIR=/var/pi-home/agent
"$PREFIX/node/bin/node" "$PI_JS" --version > /evidence/pi-version.stdout 2> /evidence/pi-version.stderr
grep -qx "$PI_VERSION" /evidence/pi-version.stdout
echo 0 > /evidence/pi.exit

phase rg-font-bash
fetch "https://github.com/BurntSushi/ripgrep/releases/download/${RG_VERSION}/${RG_TARBALL}" "$DL/$RG_TARBALL" "$RG_TARBALL_SHA" rg
rm -rf /cargo-home/rg
mkdir -p /cargo-home/rg
tar --no-same-owner -xzf "$DL/$RG_TARBALL" -C /cargo-home/rg
cp "/cargo-home/rg/ripgrep-${RG_VERSION}-aarch64-unknown-linux-gnu/rg" "$PREFIX/bin/rg"
chmod 755 "$PREFIX/bin/rg"
cp "$PREFIX/bin/rg" "$PREFIX/node/bin/rg"
license=$(find "/cargo-home/rg/ripgrep-${RG_VERSION}-aarch64-unknown-linux-gnu" -maxdepth 1 -type f \( -name 'LICENSE*' -o -name 'COPYING*' -o -name 'UNLICENSE' \) -print -quit)
test -n "$license"
cp "$license" "$PREFIX/licenses/rg-LICENSE"
"$PREFIX/bin/rg" --version > /evidence/rg-version.txt
grep -q "$RG_VERSION" /evidence/rg-version.txt
fetch "https://github.com/notofonts/noto-cjk/releases/download/${FONT_VERSION}/08_NotoSansCJKsc.zip" "$DL/noto.zip" "$FONT_ARCHIVE_SHA" font-zip
rm -rf /cargo-home/font
mkdir -p /cargo-home/font
unzip -q "$DL/noto.zip" -d /cargo-home/font
font=$(find /cargo-home/font -name "$FONT_FILE" -type f -print -quit)
test -n "$font"
sha256sum "$font" | awk '{print $1}' > /evidence/font-otf.sha
grep -qx "$FONT_OTF_SHA" /evidence/font-otf.sha
cp "$font" "$PREFIX/fonts/$FONT_FILE"
ofl=$(find /cargo-home/font -type f \( -iname 'LICENSE*' -o -iname 'OFL*' \) -print -quit)
if [ -n "$ofl" ]; then
  cp "$ofl" "$PREFIX/licenses/font-OFL"
fi
cp /usr/bin/bash "$PREFIX/bin/bash"
cp /usr/bin/bash "$PREFIX/node/bin/bash"
chmod 755 "$PREFIX/bin/bash" "$PREFIX/node/bin/bash"
if [ -f /usr/share/doc/bash/copyright ]; then
  cp /usr/share/doc/bash/copyright "$PREFIX/licenses/bash-copyright"
fi
cmp -s "$PREFIX/bin/bash" "$PREFIX/node/bin/bash"
cmp -s "$PREFIX/bin/rg" "$PREFIX/node/bin/rg"
echo 0 > /evidence/rg-font-bash.exit

phase libclang
if [ ! -s /evidence/libclang-dir.txt ]; then
  OO_PHASE=/evidence bash /libclang.sh > /evidence/libclang.stdout 2> /evidence/libclang.stderr
fi
libdir=$(cat /evidence/libclang-dir.txt)
test -n "$libdir"
llvm_inc=$(find /cargo-home/clang-root/usr/lib -type d -path '*/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
ld_path="$libdir:$multiarch"
if [ -d /cargo-home/clang-root/usr/lib/llvm-19/lib ]; then
  ld_path="$ld_path:/cargo-home/clang-root/usr/lib/llvm-19/lib"
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="$ld_path"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
printf 'libdir=%s\nllvm_inc=%s\ngcc_inc=%s\n' "$libdir" "$llvm_inc" "$gcc_inc" > /evidence/libclang-env.txt
echo 0 > /evidence/libclang.exit

phase bins
export CARGO_TARGET_DIR=/target/ws
export CARGO_NET_OFFLINE=false
cargo fetch --manifest-path /src/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
echo 0 > /evidence/fetch.exit
export CARGO_NET_OFFLINE=true
cargo build --manifest-path /src/Cargo.toml --locked --offline -p oo-cli --bin oo > /evidence/oo-build.stdout 2> /evidence/oo-build.stderr
echo 0 > /evidence/oo-build.exit
cargo build --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --bin query-pg > /evidence/query-build.stdout 2> /evidence/query-build.stderr
echo 0 > /evidence/query-build.exit
cp /target/ws/debug/oo "$PREFIX/bin/oo"
cp /target/ws/debug/oo "$PREFIX/node/bin/oo"
cp /target/ws/debug/query-pg "$PREFIX/bin/query-pg"
cp /cargo-home/bin/kit "$PREFIX/bin/kit"
chmod 755 "$PREFIX/bin/oo" "$PREFIX/node/bin/oo" "$PREFIX/bin/query-pg" "$PREFIX/bin/kit"
cmp -s "$PREFIX/bin/oo" "$PREFIX/node/bin/oo"
static=$(find /target/ws -name 'libpg_query.a' -type f -print)
count=$(printf '%s\n' "$static" | sed '/^$/d' | wc -l | tr -d ' ')
if [ "$count" -lt 1 ]; then
  find /target/ws -name '*.a' -type f > /evidence/static-candidates.txt
  die 2 static-missing
fi
first=$(printf '%s\n' "$static" | sed '/^$/d' | head -n 1)
sha=$(sha256sum "$first" | awk '{print $1}')
printf '%s\n' "$static" | sed '/^$/d' | while read -r one; do
  got=$(sha256sum "$one" | awk '{print $1}')
  if [ "$got" != "$sha" ]; then
    echo "static libraries differ" >&2
    exit 1
  fi
done
cp "$first" "$PREFIX/lib/libpg_query.a"
ldd "$PREFIX/bin/query-pg" > /evidence/query-pg-ldd.txt || true
if [ ! -s /evidence/query-pg-ldd.txt ]; then
  die 2 ldd-empty
fi
if grep -q 'libpg_query\.so' /evidence/query-pg-ldd.txt; then
  die 2 static-shared
fi
ldd "$PREFIX/bin/oo" > /evidence/oo-ldd.txt || true
"$PREFIX/bin/oo" --help > /evidence/oo-help.stdout 2> /evidence/oo-help.stderr
echo $? > /evidence/oo-help.exit
grep -q 'Only query is implemented' /evidence/oo-help.stdout
"$PREFIX/bin/query-pg" --help > /evidence/query-help.stdout 2> /evidence/query-help.stderr
echo $? > /evidence/query-help.exit
grep -q 'query-pg — run one trusted local PostgreSQL SELECT' /evidence/query-help.stdout
echo 0 > /evidence/bins.exit

phase manifest
PI_JS="$PREFIX/install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js"
{
  sha256sum "$PI_JS"
  sha256sum "$PREFIX/node/bin/node"
  sha256sum "$PREFIX/bin/oo"
  sha256sum "$PREFIX/bin/query-pg"
  sha256sum "$PREFIX/node/bin/bash"
  sha256sum "$PREFIX/node/bin/rg"
  sha256sum "$PREFIX/lib/libpg_query.a"
  sha256sum "$PREFIX/fonts/$FONT_FILE"
  sha256sum "$PREFIX/bin/kit"
} | awk -v prefix="$PREFIX/" '{ sub(prefix, "", $2); print $1 "  " $2 }' > "$PREFIX/manifest.sha256"
cp "$PREFIX/manifest.sha256" /evidence/manifest.sha256
/cargo-home/bin/kit verify "$PREFIX" "$PREFIX/manifest.sha256" > /evidence/verify.stdout 2> /evidence/verify.stderr
echo 0 > /evidence/verify.exit
/cargo-home/bin/kit cmap "$PREFIX/fonts/$FONT_FILE" /evidence/cjk.json > /evidence/cmap.stdout 2> /evidence/cmap.stderr
echo 0 > /evidence/cmap.exit
bash_token=$(sed -n 's/.*version \([0-9][0-9.]*\).*/\1/p' /evidence/bash-version.txt | head -n 1)
if [ -z "$bash_token" ]; then
  die 2 bash-version-token
fi
{
  echo "source_commit=$(cat /evidence/source-commit.txt)"
  echo "pi_commit=$PI_COMMIT"
  echo "pi_version=$PI_VERSION"
  echo "node_version=$(cat /evidence/node-version.txt)"
  echo "image=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
  echo "profile=debug"
  echo "bash_version=$bash_token"
  echo "rg_version=$RG_VERSION"
  echo "volume=${KIT_VOLUME}"
  echo "pi_executable=$PI_JS"
  echo "pi_executable_sha256=$(sha256sum "$PI_JS" | awk '{print $1}')"
  echo "pi_runtime=$PREFIX/node/bin/node"
  echo "pi_runtime_sha256=$(sha256sum "$PREFIX/node/bin/node" | awk '{print $1}')"
  echo "oo_bin=$PREFIX/bin/oo"
  echo "oo_bin_sha256=$(sha256sum "$PREFIX/bin/oo" | awk '{print $1}')"
  echo "query_pg_bin=$PREFIX/bin/query-pg"
  echo "query_pg_bin_sha256=$(sha256sum "$PREFIX/bin/query-pg" | awk '{print $1}')"
  echo "bash_bin=$PREFIX/node/bin/bash"
  echo "bash_bin_sha256=$(sha256sum "$PREFIX/node/bin/bash" | awk '{print $1}')"
  echo "rg_bin=$PREFIX/node/bin/rg"
  echo "rg_bin_sha256=$(sha256sum "$PREFIX/node/bin/rg" | awk '{print $1}')"
  echo "static_library=$PREFIX/lib/libpg_query.a"
  echo "static_library_sha256=$(sha256sum "$PREFIX/lib/libpg_query.a" | awk '{print $1}')"
  echo "cjk_font=$PREFIX/fonts/$FONT_FILE"
  echo "cjk_font_sha256=$(sha256sum "$PREFIX/fonts/$FONT_FILE" | awk '{print $1}')"
  echo "kit_bin=$PREFIX/bin/kit"
  echo "kit_bin_sha256=$(sha256sum "$PREFIX/bin/kit" | awk '{print $1}')"
} > /evidence/facts.env
cat > "$PREFIX/NOTICE" <<'EOF'
This prefix is a development runtime kit. It is not a production sandbox
and it does not accept SP10-W04, SP10-W05, or the product goal.

Pi coding agent 1.0.4 is MIT. The published npm tarball does not include
the license file; the git tag license is licenses/pi-LICENSE.
Node.js 22.23.3 is under licenses/node-LICENSE.
ripgrep 14.1.1 is under licenses/rg-LICENSE.
Noto Sans CJK SC Sans2.004 is OFL-1.1 under licenses/font-OFL when present.
GNU bash is the binary from the pinned Rust image; its copyright is
licenses/bash-copyright when that file is in the image.
oo and query-pg are Apache-2.0 binaries built from the recorded source commit.
libpg_query.a is the static library produced by pg_query 6.2.1. Its wrapper
is MIT. Vendored PostgreSQL sources are the PostgreSQL license and the rest
of libpg_query is BSD-3-Clause.
The public proxy marker is not a credential.
EOF
cp "$PREFIX/NOTICE" /evidence/prefix-NOTICE
if command -v sync >/dev/null; then
  sync || true
fi
echo 0 > /evidence/inside.exit
echo "install-ok"
