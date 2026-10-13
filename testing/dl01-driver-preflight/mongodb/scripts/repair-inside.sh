#!/bin/bash
# Linux Rust 1.94 tests, clippy, and build. Does not run cargo generate-lockfile.
set -euo pipefail

cd /src
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
grep -q '^rustc 1.94.0 ' /evidence/rustc-version.txt
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.txt 2>&1
fi

echo "probe-fetch" | tee /evidence/phase.txt
if ! cargo metadata --locked --offline --format-version 1 >/evidence/probe-metadata.txt 2>/evidence/probe-metadata.err; then
  cargo fetch --locked > /evidence/probe-fetch.txt 2>&1
  printf 'fetched\n' > /evidence/probe-fetch.result
else
  printf 'cached\n' > /evidence/probe-fetch.result
fi
echo "probe-test" | tee -a /evidence/phase.txt
cargo test --locked --offline --jobs 1 --workspace -- --test-threads=1 > /evidence/probe-test.txt 2>&1
echo "probe-clippy" | tee -a /evidence/phase.txt
cargo clippy --locked --offline --jobs 1 --all-targets --workspace -- -D warnings > /evidence/probe-clippy.txt 2>&1
echo "probe-build" | tee -a /evidence/phase.txt
cargo build --locked --offline --jobs 1 --bins > /evidence/probe-build.txt 2>&1

mkdir -p /evidence/bin
cp /target/debug/oo-mongodb-driver-probe /evidence/bin/oo-mongodb-driver-probe
cp /target/debug/oo-mongodb-arrow-decode /evidence/bin/oo-mongodb-arrow-decode
chmod 755 /evidence/bin/oo-mongodb-driver-probe /evidence/bin/oo-mongodb-arrow-decode

echo "opmsg" | tee -a /evidence/phase.txt
gcc -D OO_OPMSG_TEST -Wall -Wextra -Werror -o /tmp/opmsg-self-check /src/scripts/opmsg-name-log.c
/tmp/opmsg-self-check | tee /evidence/opmsg-self-check.txt
gcc -shared -fPIC -O2 -Wall -Wextra -Werror -o /evidence/bin/liboo-opmsg-names.so /src/scripts/opmsg-name-log.c -ldl
chmod 755 /evidence/bin/liboo-opmsg-names.so

echo "preview-test" | tee -a /evidence/phase.txt
cd /work/tools/mongodb-preview
export CARGO_TARGET_DIR=/target/mongodb-preview-194
if ! cargo metadata --locked --offline --format-version 1 >/evidence/preview-metadata.txt 2>/evidence/preview-metadata.err; then
  cargo fetch --locked > /evidence/preview-fetch.txt 2>&1
  printf 'fetched\n' > /evidence/preview-fetch.result
else
  printf 'cached\n' > /evidence/preview-fetch.result
fi
cargo test --locked --offline --jobs 1 -- --test-threads=1 > /evidence/preview-test.txt 2>&1
echo "preview-clippy" | tee -a /evidence/phase.txt
cargo clippy --locked --offline --jobs 1 --all-targets -- -D warnings > /evidence/preview-clippy.txt 2>&1
echo "preview-build" | tee -a /evidence/phase.txt
cargo build --locked --offline --jobs 1 > /evidence/preview-build.txt 2>&1

echo "proxy-test" | tee -a /evidence/phase.txt
rustc --test /work/tools/mongodb-preview/scripts/loopback-proxy.rs -O -o /tmp/loopback-proxy-test > /evidence/proxy-rustc.txt 2>&1
/tmp/loopback-proxy-test > /evidence/proxy-test.txt 2>&1
rustc /work/tools/mongodb-preview/scripts/loopback-proxy.rs -O -o /evidence/bin/loopback-proxy > /evidence/proxy-build.txt 2>&1
chmod 755 /evidence/bin/loopback-proxy

if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence || true
fi
printf 'repair-build-ok\n' | tee /evidence/build-result.txt
