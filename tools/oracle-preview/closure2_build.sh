#!/bin/bash
# One Linux rustc 1.94 slot. Archive a6825165, then the owned child overlay.
# Does not build the live workspace and does not retag oracledb.
set -eu
mkdir -p /out
exec > >(tee -a /out/build.log) 2>&1
echo "build_start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
export PATH="/usr/local/cargo/bin:${PATH}"
export RUSTUP_TOOLCHAIN=1.94.0
if [ -d /opt/oo-cargo/registry ]; then
  rm -rf /usr/local/cargo/registry
  ln -s /opt/oo-cargo/registry /usr/local/cargo/registry
fi
rustc --version
cargo --version
echo "jobs=1"
test -d /src/a682/tools/pg-builder
test -f /crate/oracledb-26.0.0-beta.4.crate
echo "d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397  /crate/oracledb-26.0.0-beta.4.crate" | sha256sum -c -
rm -rf /tmp/oracledb-src /tmp/child-ws
mkdir -p /tmp/oracledb-src
tar -xzf /crate/oracledb-26.0.0-beta.4.crate -C /tmp/oracledb-src
python3 /patch/apply_oci_cancel.py /crate/oracledb-26.0.0-beta.4.crate /tmp/oracledb-src/oracledb-26.0.0-beta.4
mkdir -p /tmp/child-ws
cp -a /child-src/. /tmp/child-ws/
python3 - <<'PY'
from pathlib import Path
path = Path("/tmp/child-ws/child/Cargo.toml")
text = path.read_text()
old = 'oracledb = { version = "=26.0.0-beta.4", features = ["arrow"] }'
new = 'oracledb = { path = "/tmp/oracledb-src/oracledb-26.0.0-beta.4", features = ["arrow"] }'
if old not in text:
    raise SystemExit("child Cargo.toml pin missing")
path.write_text(text.replace(old, new, 1))
PY
export CARGO_BUILD_JOBS=1
run_cargo() {
  local name="$1"
  shift
  echo "STEP $name offline"
  if env CARGO_NET_OFFLINE=true "$@"; then
    return 0
  fi
  echo "STEP $name online"
  env -u CARGO_NET_OFFLINE "$@"
}
cd /src/a682/tools/pg-builder
run_cargo pg-builder cargo build --locked --jobs 1 --bin pg-builder --target-dir /target/import-a682
cd /src/a682
run_cargo oo-pipeline cargo build --locked --jobs 1 -p oo-pipeline --bin oo-pipeline --target-dir /target/import-a682
cd /tmp/child-ws
export RUSTFLAGS="--cfg oci_cancel"
run_cargo child cargo build --jobs 1 --bin oo-oracle-p2-child --target-dir /target/oracle-p2-oci
test -x /target/import-a682/debug/pg-builder
test -x /target/import-a682/debug/oo-pipeline
test -x /target/oracle-p2-oci/debug/oo-oracle-p2-child
sha256sum /target/import-a682/debug/pg-builder /target/import-a682/debug/oo-pipeline /target/oracle-p2-oci/debug/oo-oracle-p2-child | tee /out/binaries.sha256
echo "build_ok $(date -u +%Y-%m-%dT%H:%M:%SZ)"
