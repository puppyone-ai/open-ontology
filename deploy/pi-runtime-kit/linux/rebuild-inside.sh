#!/bin/bash
# Rebuild only the kit verifier. The Pi prefix stays. Tests and Clippy run first.
set -euo pipefail
export HOME=/var/pi-home
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/kit
export CARGO_INCREMENTAL=0
export TMPDIR=/cargo-home/tmp
mkdir -p "$HOME" "$TMPDIR"
PREFIX=/opt/pi-runtime-kit
FACTS=/install6/facts.env
trap 'code=$?; if [ ! -s /evidence/inside.exit ]; then echo "$code" > /evidence/inside.exit; fi' EXIT
rustc --version | tee /evidence/rustc-version.txt
grep -q '^rustc 1.94.0' /evidence/rustc-version.txt
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
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
cp /target/kit/debug/kit "$PREFIX/bin/kit"
chmod 755 "$PREFIX/bin/kit"
new=$(sha256sum "$PREFIX/bin/kit" | awk '{print $1}')
awk -v sha="$new" '$2=="bin/kit"{printf "%s  bin/kit\n", sha; next} {print}' "$PREFIX/manifest.sha256" > /cargo-home/tmp/manifest.sha256
mv /cargo-home/tmp/manifest.sha256 "$PREFIX/manifest.sha256"
cp "$PREFIX/manifest.sha256" /evidence/manifest.sha256
cp "$PREFIX/manifest.sha256" /install6/manifest.sha256
awk -v sha="$new" -F= 'BEGIN{OFS="="} $1=="kit_bin_sha256" {$2=sha} {print}' "$FACTS" > /cargo-home/tmp/facts.env
mv /cargo-home/tmp/facts.env "$FACTS"
"$PREFIX/bin/kit" verify "$PREFIX" "$PREFIX/manifest.sha256" > /evidence/verify.stdout 2> /evidence/verify.stderr
echo 0 > /evidence/verify.exit
printf 'kit_bin_sha256=%s\n' "$new" > /evidence/kit-bin.sha256
echo 0 > /evidence/inside.exit
echo "rebuild-ok"
