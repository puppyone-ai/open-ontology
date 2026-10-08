#!/bin/bash
# Runs inside the single Rust 1.94 builder. No login shell.
set -uo pipefail
cd /repo
mkdir -p /evidence/raw /evidence/linux/proof/phases
exec > >(tee -a /evidence/raw/console.log) 2>&1

echo "phase=toolchain"
rustc --version
cargo --version
rustc -vV
if ! cargo clippy --version >/evidence/linux/proof/phases/clippy-version.txt 2>&1; then
  rustup component add clippy
  cargo clippy --version
fi

export HOME=/tmp
git config --global --add safe.directory /repo
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:/usr/bin:/bin"
export RUSTUP_HOME=/usr/local/rustup
export RUSTUP_TOOLCHAIN=1.94.0
export OO_A385_LIVE=1
export OO_A385_RESULT=/evidence/raw/live-result.txt
export OO_A385_STDERR=/evidence/raw/server-stderr.txt

fail=0
phase() {
  local name="$1"
  shift
  echo "phase=${name}"
  if "$@" >"/evidence/linux/proof/phases/${name}.log" 2>&1; then
    printf '%s\n' 0 >"/evidence/linux/proof/phases/${name}.exit"
  else
    local status=$?
    printf '%s\n' "$status" >"/evidence/linux/proof/phases/${name}.exit"
    echo "phase=${name} exit=${status}"
    fail=1
  fi
}

immutable_hash() {
  sha256sum \
    /repo/Cargo.toml \
    /repo/Cargo.lock \
    /repo/rust-toolchain.toml \
    /repo/deploy/trusted-demo/pg/up.sh \
    /repo/deploy/trusted-demo/pg/pg-wrap.sh
  find /repo/tools/audit-ingest /repo/tools/selfbuild-suggestions -type f -print | sort | xargs sha256sum
}

if ! immutable_hash > /evidence/raw/immutable-before.txt; then
  echo "immutable hash failed"
  exit 1
fi

echo "phase=lock"
if ! cargo metadata --locked --format-version 1 --manifest-path tools/feedback-receiver/Cargo.toml >/tmp/oo-a385-metadata.txt 2>/evidence/linux/proof/phases/metadata.err; then
  echo "seeded lock rejected; generating once"
  rm -f tools/feedback-receiver/Cargo.lock
  if ! cargo generate-lockfile --manifest-path tools/feedback-receiver/Cargo.toml > /evidence/linux/proof/phases/lock-generate.log 2>&1; then
    printf '%s\n' 1 > /evidence/linux/proof/phases/lock.exit
    echo "phase=lock generate failed"
    exit 1
  fi
  printf '%s\n' generated > /evidence/linux/proof/phases/lock-mode.txt
else
  printf '%s\n' seeded > /evidence/linux/proof/phases/lock-mode.txt
fi
printf '%s\n' 0 > /evidence/linux/proof/phases/lock.exit

python3 - << 'PY'
import pathlib, sys
root = pathlib.Path("/repo/Cargo.lock").read_text()
child = pathlib.Path("/repo/tools/feedback-receiver/Cargo.lock").read_text()
if "git+" in child:
    raise SystemExit("git source")

def packages(text):
    out = {}
    name = version = source = checksum = ""
    open_ = False
    def flush():
        nonlocal open_, name, version, source, checksum
        if open_ and source.startswith("registry+"):
            if not checksum:
                raise SystemExit(f"missing checksum {name} {version}")
            out[(name, version)] = checksum
        open_ = False
        name = version = source = checksum = ""
    for line in text.splitlines():
        if line == "[[package]]":
            flush()
            open_ = True
            continue
        if not open_:
            continue
        if line.startswith("name = "):
            name = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("version = "):
            version = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("source = "):
            source = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("checksum = "):
            checksum = line.split("=", 1)[1].strip().strip('"')
    flush()
    return out

root_packages = packages(root)
for key, checksum in packages(child).items():
    expected = root_packages.get(key)
    if expected != checksum:
        raise SystemExit(f"checksum not in root lock: {key[0]} {key[1]}")
print("lock_subset=ok")
PY
subset_status=$?
printf '%s\n' "$subset_status" > /evidence/linux/proof/phases/lock-subset-fast.exit
if [ "$subset_status" -ne 0 ]; then
  echo "lock subset failed before compile"
  exit 1
fi

phase fetch cargo fetch --locked --manifest-path tools/feedback-receiver/Cargo.toml
if [ "$fail" -ne 0 ]; then
  echo "fetch failed"
  printf '%s\n' "$fail" > /evidence/raw/inside.exit
  exit "$fail"
fi
export CARGO_NET_OFFLINE=true

phase test cargo test --locked --manifest-path tools/feedback-receiver/Cargo.toml -- --test-threads=1
phase clippy cargo clippy --locked --all-targets --manifest-path tools/feedback-receiver/Cargo.toml -- -D warnings

if ! immutable_hash > /evidence/raw/immutable-after.txt; then
  echo "immutable hash failed"
  fail=1
fi
if ! cmp -s /evidence/raw/immutable-before.txt /evidence/raw/immutable-after.txt; then
  echo "immutable files changed"
  fail=1
fi

if ! grep -qx 'live=ok' /evidence/raw/live-result.txt; then
  echo "live result is not ok"
  fail=1
fi

secret_scan() {
  local needle line
  for needle in /run/secrets/feedback.pass /run/secrets/audit.pass; do
    if grep -R -F -q -f "$needle" /evidence; then
      echo "secret scan failed"
      return 1
    fi
  done
  for needle in /run/secrets/mtls/server.key /run/secrets/mtls/client.key /run/secrets/mtls/other.key /run/secrets/mtls/stranger.key; do
    while IFS= read -r line || [ -n "$line" ]; do
      case "$line" in
        ""|-----BEGIN*|-----END*) continue ;;
      esac
      if [ "${#line}" -lt 40 ]; then
        continue
      fi
      if grep -R -F -q -- "$line" /evidence; then
        echo "secret scan failed"
        return 1
      fi
    done < "$needle"
  done
  echo "secret_scan=ok"
}
secret_scan
scan_status=$?
printf '%s\n' "$scan_status" > /evidence/linux/proof/phases/secret-scan.exit
if [ "$scan_status" -ne 0 ]; then
  fail=1
fi

printf '%s\n' "$fail" > /evidence/raw/inside.exit
echo "inside_exit=${fail}"
exit "$fail"
