#!/bin/bash
# Install the pinned public Node binary and Pi v1.0.4 from official sources.
# Checksums are verified here. No host npm cache. No product runtime image.
# a285 change from the a282 install script: compare the GitHub tarball, npm tarball,
# install lock, and Node LICENSE to the spike PIN, then a Rust pin oracle repeats
# those hashes. A missing published LICENSE stays recorded and is not a failed install.
# The git tag MIT text is still checked. Then compile the a279 mock_openai with rustc.
set -eu
export HOME=/var/pi-home
export GIT_TERMINAL_PROMPT=0
export NPM_CONFIG_CACHE=/npm-cache
export NPM_CONFIG_UPDATE_NOTIFIER=false
export NPM_CONFIG_FUND=false
export NPM_CONFIG_AUDIT=false
export PI_CODING_AGENT_DIR=/var/pi-home/agent
export PATH=/opt/pi-prefix/node/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
umask 022
mkdir -p "$HOME" "$PI_CODING_AGENT_DIR" /work /opt/pi-prefix /npm-cache
cd /work

phase() {
  echo "PHASE $1"
}

fail() {
  echo "$1" > "/evidence/$2.exit"
  echo "fail $2: $1" >&2
  exit "$1"
}

phase tools
for tool in curl git tar sha256sum openssl; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing $tool" >&2
    fail 2 tools
  fi
done
echo 0 > /evidence/tools.exit
uname -m | tee /evidence/uname-m.txt
if ! grep -qx aarch64 /evidence/uname-m.txt; then
  fail 2 arch
fi
echo 0 > /evidence/arch.exit

NODE_VERSION=22.23.3
NODE_TARBALL=node-v${NODE_VERSION}-linux-arm64.tar.gz
SUMS_SHA=4fe99a2ba9d552a6f51c13ed68fb11104cfa5df601aec616be689253a8139e7a
NODE_SHA=5ced2d48d1d7198739b7f86804de0171aefb6823b684b12341d3321afc3cb0b2
PI_COMMIT=7c10bd4337495ee613f2224843ecdf349b80d1df
PI_LICENSE_SHA=0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48
NPM_INTEGRITY='sha512-+956nfMFHr5lDUVY/2Q4k+YzojzBuCaBXFgj0eSlXVGr7QVliVddKdc1Pz6yVg1dOlJQmb67doOVrlMsIcIdaw=='

phase node-sums
curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -o /work/SHASUMS256.txt "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt"
echo "${SUMS_SHA}  /work/SHASUMS256.txt" > /work/sums.expected
if ! sha256sum -c /work/sums.expected > /evidence/node-sums.stdout 2> /evidence/node-sums.stderr; then
  fail 2 node-sums
fi
cp /work/SHASUMS256.txt /evidence/node-SHASUMS256.txt
echo 0 > /evidence/node-sums.exit

phase node-tarball
curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -o "/work/${NODE_TARBALL}" "https://nodejs.org/dist/v${NODE_VERSION}/${NODE_TARBALL}"
echo "${NODE_SHA}  /work/${NODE_TARBALL}" > /work/node.expected
if ! sha256sum -c /work/node.expected > /evidence/node-tarball.stdout 2> /evidence/node-tarball.stderr; then
  fail 2 node-tarball
fi
grep -F "${NODE_TARBALL}" /work/SHASUMS256.txt > /evidence/node-sums-line.txt
awk '{print $1}' /evidence/node-sums-line.txt > /evidence/node-sums-line.sha
if ! grep -qx "$NODE_SHA" /evidence/node-sums-line.sha; then
  fail 2 node-sums-line
fi
echo 0 > /evidence/node-sums-line.exit
rm -rf /opt/pi-prefix/node
tar --no-same-owner -xzf "/work/${NODE_TARBALL}" -C /opt/pi-prefix
mv "/opt/pi-prefix/node-v${NODE_VERSION}-linux-arm64" /opt/pi-prefix/node
/opt/pi-prefix/node/bin/node --version > /evidence/node-version.txt
echo 0 > /evidence/node-tarball.exit
cp /opt/pi-prefix/node/LICENSE /evidence/node-LICENSE
sha256sum /evidence/node-LICENSE > /evidence/node-LICENSE.sha256
echo "c738ae413cf561f174e34f6961f8ca458aae2369a73640dda6234c629b98bcc4  /evidence/node-LICENSE" > /work/node-license.expected
if ! sha256sum -c /work/node-license.expected > /evidence/node-license-pin.stdout 2> /evidence/node-license-pin.stderr; then
  fail 2 node-license-pin
fi
echo 0 > /evidence/node-license-pin.exit

phase pi-git
rm -rf /work/pi
git clone --depth 1 --branch v1.0.4 https://github.com/earendil-works/pi.git /work/pi
git -C /work/pi rev-parse HEAD > /evidence/pi-git-commit.txt
git -C /work/pi status --porcelain > /evidence/pi-git-status.txt
git -C /work/pi rev-parse --abbrev-ref HEAD > /evidence/pi-git-branch.txt
if ! grep -qx "$PI_COMMIT" /evidence/pi-git-commit.txt; then
  fail 2 pi-git
fi
if [ -s /evidence/pi-git-status.txt ]; then
  fail 2 pi-git-dirty
fi
echo 0 > /evidence/pi-git.exit
cp /work/pi/LICENSE /evidence/pi-LICENSE
sha256sum /evidence/pi-LICENSE > /evidence/pi-LICENSE.sha256
echo "${PI_LICENSE_SHA}  /evidence/pi-LICENSE" > /work/license.expected
if ! sha256sum -c /work/license.expected > /evidence/pi-license-check.stdout 2> /evidence/pi-license-check.stderr; then
  fail 2 pi-license
fi
echo 0 > /evidence/pi-license.exit
cp /work/pi/package.json /evidence/pi-root-package.json
cp /work/pi/packages/coding-agent/package.json /evidence/coding-agent-package.json
cp /work/pi/packages/coding-agent/install-lock/package.json /evidence/install-lock-package.json
cp /work/pi/packages/coding-agent/install-lock/package-lock.json /evidence/package-lock.json
sha256sum /evidence/package-lock.json > /evidence/package-lock.sha256
echo "35fd666029cb44d3cceb5a372cbacd2aecc478c69daa14898f006af291d4d869  /evidence/package-lock.json" > /work/lock.expected
if ! sha256sum -c /work/lock.expected > /evidence/package-lock-pin.stdout 2> /evidence/package-lock-pin.stderr; then
  fail 2 package-lock-pin
fi
echo 0 > /evidence/package-lock-pin.exit
cp /work/pi/packages/coding-agent/docs/rpc.md /evidence/upstream-rpc.md
cp /work/pi/packages/coding-agent/docs/rpc-commands.md /evidence/upstream-rpc-commands.md
cp /work/pi/packages/coding-agent/docs/json.md /evidence/upstream-json.md
sha256sum /evidence/upstream-rpc.md /evidence/upstream-rpc-commands.md /evidence/upstream-json.md > /evidence/upstream-docs.sha256

phase github-tarball
curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -o /work/pi-v1.0.4.tar.gz https://github.com/earendil-works/pi/archive/refs/tags/v1.0.4.tar.gz
echo "a95c2ded6a2d8b56247f4205c99e3bef6241b5b51044086d4185d75c2822c521  /work/pi-v1.0.4.tar.gz" > /work/github.expected
if ! sha256sum -c /work/github.expected > /evidence/github-tarball.stdout 2> /evidence/github-tarball.stderr; then
  fail 2 github-tarball
fi
sha256sum /work/pi-v1.0.4.tar.gz > /evidence/github-tarball.sha256
echo 0 > /evidence/github-tarball.exit

phase npm-metadata
curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -o /evidence/npm-registry.json https://registry.npmjs.org/@earendil-works/pi-coding-agent/1.0.4
node_meta=/opt/pi-prefix/node/bin/node
"$node_meta" -e '
const fs = require("fs");
const m = JSON.parse(fs.readFileSync("/evidence/npm-registry.json", "utf8"));
const want = {
  version: "1.0.4",
  license: "MIT",
  gitHead: "7c10bd4337495ee613f2224843ecdf349b80d1df",
  engine: ">=22.19.0",
  integrity: "sha512-+956nfMFHr5lDUVY/2Q4k+YzojzBuCaBXFgj0eSlXVGr7QVliVddKdc1Pz6yVg1dOlJQmb67doOVrlMsIcIdaw==",
  repo: "git+https://github.com/earendil-works/pi.git"
};
function bad(msg) { console.error(msg); process.exit(2); }
if (m.version !== want.version) bad("version " + m.version);
if (m.license !== want.license) bad("license " + m.license);
if (m.gitHead !== want.gitHead) bad("gitHead " + m.gitHead);
if (!m.engines || m.engines.node !== want.engine) bad("engine " + JSON.stringify(m.engines));
if (!m.dist || m.dist.integrity !== want.integrity) bad("integrity " + (m.dist && m.dist.integrity));
if (!m.repository || m.repository.url !== want.repo) bad("repo " + JSON.stringify(m.repository));
if (!m.dist.tarball) bad("tarball missing");
fs.writeFileSync("/evidence/npm-tarball.url", m.dist.tarball + "\n");
console.log("npm-metadata-ok");
' > /evidence/npm-metadata.stdout 2> /evidence/npm-metadata.stderr
meta_exit=$?
echo "$meta_exit" > /evidence/npm-metadata.exit
if [ "$meta_exit" -ne 0 ]; then
  exit "$meta_exit"
fi

phase npm-tarball
curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -o /work/pi-coding-agent-1.0.4.tgz "$(cat /evidence/npm-tarball.url)"
sha256sum /work/pi-coding-agent-1.0.4.tgz > /evidence/npm-tarball.sha256
got=$(openssl dgst -sha512 -binary /work/pi-coding-agent-1.0.4.tgz | openssl base64 -A)
printf 'sha512-%s\n' "$got" > /evidence/npm-tarball.integrity
if [ "sha512-${got}" != "$NPM_INTEGRITY" ]; then
  echo "npm integrity mismatch" >&2
  fail 2 npm-tarball
fi
echo "04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5  /work/pi-coding-agent-1.0.4.tgz" > /work/npm-sha.expected
if ! sha256sum -c /work/npm-sha.expected > /evidence/npm-tarball-sha.stdout 2> /evidence/npm-tarball-sha.stderr; then
  fail 2 npm-tarball-sha
fi
echo 0 > /evidence/npm-tarball-sha.exit
echo 0 > /evidence/npm-tarball.exit

phase npm-ci
rm -rf /opt/pi-prefix/install
mkdir -p /opt/pi-prefix/install
cp /evidence/install-lock-package.json /opt/pi-prefix/install/package.json
cp /evidence/package-lock.json /opt/pi-prefix/install/package-lock.json
cd /opt/pi-prefix/install
set +e
/opt/pi-prefix/node/bin/npm ci --ignore-scripts --no-audit --no-fund --cache /npm-cache \
  > /evidence/npm-ci.stdout 2> /evidence/npm-ci.stderr
ci_exit=$?
set -e
echo "$ci_exit" > /evidence/npm-ci.exit
if [ "$ci_exit" -ne 0 ]; then
  exit "$ci_exit"
fi
sha256sum /opt/pi-prefix/install/package-lock.json > /evidence/package-lock-after.sha256
if ! cmp -s /evidence/package-lock.json /opt/pi-prefix/install/package-lock.json; then
  fail 2 package-lock-drift
fi
echo 0 > /evidence/package-lock-drift.exit
installed=/opt/pi-prefix/install/node_modules/@earendil-works/pi-coding-agent/package.json
cp "$installed" /evidence/installed-package.json
license=/opt/pi-prefix/install/node_modules/@earendil-works/pi-coding-agent/LICENSE
if [ -f "$license" ]; then
  cp "$license" /evidence/installed-LICENSE
  sha256sum /evidence/installed-LICENSE > /evidence/installed-LICENSE.sha256
  echo "${PI_LICENSE_SHA}  /evidence/installed-LICENSE" > /work/installed-license.expected
  if ! sha256sum -c /work/installed-license.expected > /evidence/installed-license.stdout 2> /evidence/installed-license.stderr; then
    fail 2 installed-license
  fi
else
  echo "published npm package has no LICENSE file" > /evidence/installed-license-absent.txt
fi
echo 0 > /evidence/installed-license.exit

phase pi-help
export PATH=/opt/pi-prefix/node/bin:/opt/pi-prefix/install/node_modules/.bin:$PATH
set +e
pi --version > /evidence/pi-version.stdout 2> /evidence/pi-version.stderr
ver_exit=$?
pi --help > /evidence/pi-help.stdout 2> /evidence/pi-help.stderr
help_exit=$?
set -e
echo "$ver_exit" > /evidence/pi-version.exit
echo "$help_exit" > /evidence/pi-help.exit
echo "PHASE pi_version_exit=$ver_exit pi_help_exit=$help_exit"
if [ "$ver_exit" -ne 0 ] || [ "$help_exit" -ne 0 ]; then
  exit 1
fi
if ! grep -qx '1.0.4' /evidence/pi-version.stdout; then
  echo "pi version stdout is not 1.0.4" >&2
  exit 1
fi
echo 0 > /evidence/inside.exit
echo "pi-install-ok"

phase pin-oracle
export RUSTUP_TOOLCHAIN=1.94.0
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
set +e
rustc --edition 2024 -D warnings /tools/review_oracle.rs -o /opt/probe/review_oracle > /evidence/pin-oracle-rustc.stdout 2> /evidence/pin-oracle-rustc.stderr
orc=$?
echo "$orc" > /evidence/pin-oracle-rustc.exit
if [ "$orc" -eq 0 ]; then
  /opt/probe/review_oracle pin \
    /work/node-v22.23.3-linux-arm64.tar.gz \
    /evidence/node-SHASUMS256.txt \
    /work/pi-coding-agent-1.0.4.tgz \
    /work/pi-v1.0.4.tar.gz \
    /evidence/pi-LICENSE \
    /evidence/node-LICENSE \
    /evidence/package-lock.json \
    > /evidence/pin-oracle.stdout 2> /evidence/pin-oracle.stderr
  orc=$?
  echo "$orc" > /evidence/pin-oracle.exit
fi
set -e
echo "PHASE pin_oracle_exit=$orc"
if [ "$orc" -ne 0 ]; then
  exit "$orc"
fi

phase mock
rustc -vV > /evidence/rustc-version.txt
set +e
rustc --edition 2024 -D warnings /mocksrc/mock_openai.rs -o /opt/probe/mock_openai > /evidence/mock-rustc.stdout 2> /evidence/mock-rustc.stderr
mock_exit=$?
set -e
echo "$mock_exit" > /evidence/mock-rustc.exit
echo "PHASE mock_rustc_exit=$mock_exit"
if [ "$mock_exit" -ne 0 ]; then
  exit "$mock_exit"
fi
echo "mock-ok"
