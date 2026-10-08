#!/bin/bash
# Runs inside the single Rust 1.94 builder. No login shell.
set -uo pipefail
mkdir -p /evidence/raw /evidence/linux/proof/phases /evidence/http /evidence/pg /work
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
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:/usr/bin:/bin"
export RUSTUP_HOME=/usr/local/rustup
export RUSTUP_TOOLCHAIN=1.94.0

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

if ! (cd /archive && sha256sum -c /evidence/archive/product.sha256) >/evidence/linux/proof/phases/archive-hash.log 2>&1; then
  echo "archive product hash mismatch"
  printf '%s\n' 1 > /evidence/linux/proof/phases/archive-hash.exit
  printf '%s\n' 1 > /evidence/raw/inside.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/linux/proof/phases/archive-hash.exit

rm -rf /work
mkdir -p /work
cp -a /archive/. /work/
if [ -f /work/rust-toolchain.toml ]; then
  mv /work/rust-toolchain.toml /work/rust-toolchain.toml.aside
fi
cd /work

echo "phase=lock"
if ! cargo metadata --locked --format-version 1 --manifest-path tools/feedback-receiver/Cargo.toml >/tmp/oo-a393-metadata.txt 2>/evidence/linux/proof/phases/metadata.err; then
  echo "seeded lock rejected; not generating"
  printf '%s\n' 1 > /evidence/linux/proof/phases/lock.exit
  printf '%s\n' rejected > /evidence/linux/proof/phases/lock-mode.txt
  printf '%s\n' 1 > /evidence/raw/inside.exit
  exit 1
fi
printf '%s\n' seeded > /evidence/linux/proof/phases/lock-mode.txt
printf '%s\n' 0 > /evidence/linux/proof/phases/lock.exit

python3 - << 'PY'
import pathlib, sys
root = pathlib.Path("/work/Cargo.lock").read_text()
child = pathlib.Path("/work/tools/feedback-receiver/Cargo.lock").read_text()
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
count = 0
for key, checksum in packages(child).items():
    expected = root_packages.get(key)
    if expected != checksum:
        raise SystemExit(f"checksum not in root lock: {key[0]} {key[1]}")
    count += 1
print(f"lock_subset=ok registry={count}")
PY
subset_status=$?
printf '%s\n' "$subset_status" > /evidence/linux/proof/phases/lock-subset-fast.exit
if [ "$subset_status" -ne 0 ]; then
  echo "lock subset failed before compile"
  printf '%s\n' 1 > /evidence/raw/inside.exit
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

if [ ! -f /evidence/raw/author-live-result.txt ] || ! grep -qx 'live=ok' /evidence/raw/author-live-result.txt; then
  echo "author live result is not ok"
  fail=1
fi

cp /evidence/probe/a393_http.rs /work/tools/feedback-receiver/tests/a393_http.rs
phase a393 cargo test --locked --manifest-path tools/feedback-receiver/Cargo.toml --test a393_http -- --test-threads=1
phase a393-clippy cargo clippy --locked --all-targets --manifest-path tools/feedback-receiver/Cargo.toml -- -D warnings

if [ ! -f /evidence/raw/a393-result.txt ] || ! grep -qx 'http=ok' /evidence/raw/a393-result.txt; then
  echo "independent http result is not ok"
  fail=1
fi

if ! (cd /archive && sha256sum -c /evidence/archive/product.sha256) >/evidence/linux/proof/phases/archive-hash-after.log 2>&1; then
  echo "archive changed"
  fail=1
fi
if ! (cd /work && sha256sum -c /evidence/archive/product.sha256) >/evidence/linux/proof/phases/work-hash-after.log 2>&1; then
  echo "work product bytes changed"
  fail=1
fi
drift=0
while IFS= read -r path; do
  rel=${path#/archive/}
  if ! cmp -s "$path" "/work/$rel"; then
    echo "work copy drifted $rel"
    drift=1
  fi
done < <(find /archive/tools/feedback-receiver -type f -print | sort)
if [ "$drift" -ne 0 ]; then
  fail=1
  printf '%s\n' 1 > /evidence/linux/proof/phases/product-unchanged.exit
else
  printf '%s\n' 0 > /evidence/linux/proof/phases/product-unchanged.exit
fi

secret_scan() {
  local needle line
  for needle in /run/secrets/feedback.pass /run/secrets/audit.pass; do
    if grep -R -F -q -f "$needle" /evidence; then
      echo "secret scan failed"
      return 1
    fi
  done
  for needle in /run/secrets/mtls/server.key /run/secrets/mtls/client.key /run/secrets/mtls/other.key /run/secrets/mtls/stranger.key /run/secrets/mtls/foreign.key; do
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
