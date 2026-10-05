#!/bin/bash
# Pre-overlay control smoke of the immutable archive. No source writes.
set -eu
echo "oo_task=sp05-local-file-hosting-review-control"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "overlay=none"
echo "airgap_claim=no"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test file_hosting --lib"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings"
echo "arch_argv=cargo run --manifest-path Cargo.toml --locked --offline -p oo-arch-check -- --manifest /src/Cargo.toml"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f crates/oo-control/src/file_hosting.rs ] || [ ! -f crates/oo-control/tests/file_hosting.rs ]; then
  echo "archive is missing the file hosting sources" >&2
  exit 2
fi
cp Cargo.lock /tmp/Cargo.lock.before
mkdir -p /evidence/phases /evidence/dependency-meta

ensure_component() {
  local component="$1"
  local phase="$2"
  if rustup component list --toolchain 1.94.0 --installed | grep -q "^${component}"; then
    echo "${component}_component=already_installed"
    printf '%s\n' 0 > "/evidence/phases/${phase}.exit"
    return 0
  fi
  echo "=== rustup component add ${component} --toolchain 1.94.0 ==="
  set +e
  rustup component add "$component" --toolchain 1.94.0
  local add_exit=$?
  set -e
  echo "phase_exit=${add_exit} phase=rustup_component_add_${component}"
  printf '%s\n' "$add_exit" > "/evidence/phases/${phase}.exit"
  if [ "$add_exit" -ne 0 ]; then
    exit "$add_exit"
  fi
}

ensure_component rustfmt rustfmt-component
ensure_component clippy clippy-component

echo "=== cargo fmt --check -p oo-control ==="
set +e
cargo fmt --check -p oo-control
fmt_exit=$?
set -e
echo "phase_exit=$fmt_exit phase=cargo_fmt_check"
printf '%s\n' "$fmt_exit" > /evidence/phases/fmt-check.exit
if [ "$fmt_exit" -ne 0 ]; then
  echo "fmt check failed; archive left unchanged" >&2
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch"
printf '%s\n' "$fetch_exit" > /evidence/phases/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
if ! cmp -s /tmp/Cargo.lock.before /src/Cargo.lock; then
  echo "Cargo.lock changed during fetch" >&2
  printf '%s\n' 4 > /evidence/phases/lock-unchanged.exit
  exit 4
fi
echo "phase_exit=0 phase=lock_unchanged"
printf '%s\n' 0 > /evidence/phases/lock-unchanged.exit

echo "=== dependency crate files ==="
: > /evidence/dependency-meta/checksums.txt
: > /evidence/dependency-meta/manifest-fields.txt
while IFS="$(printf '\t')" read -r crate_name crate_version crate_checksum; do
  [ -n "$crate_name" ] || continue
  crate_file=$(find /cargo-home/registry/src /cargo-home/registry/cache -name "${crate_name}-${crate_version}.crate" -type f 2>/dev/null | head -n 1)
  if [ -z "$crate_file" ]; then
    echo "missing ${crate_name}-${crate_version}" >> /evidence/dependency-meta/missing.txt
    continue
  fi
  actual=$(sha256sum "$crate_file" | awk '{print $1}')
  printf '%s %s lock=%s file=%s\n' "$crate_name" "$crate_version" "$crate_checksum" "$actual" >> /evidence/dependency-meta/checksums.txt
  if [ "$actual" != "$crate_checksum" ]; then
    echo "checksum mismatch ${crate_name} ${crate_version}" >> /evidence/dependency-meta/missing.txt
  fi
  tar -xOf "$crate_file" "${crate_name}-${crate_version}/Cargo.toml" > "/evidence/dependency-meta/${crate_name}-${crate_version}.Cargo.toml"
  {
    echo "crate=${crate_name} version=${crate_version}"
    awk '
      /^\[package\]/ { in_pkg = 1; next }
      /^\[/ { in_pkg = 0 }
      in_pkg && ($1 == "license" || $1 == "rust-version" || $1 == "version" || $1 == "name" || $1 == "description") { print }
    ' "/evidence/dependency-meta/${crate_name}-${crate_version}.Cargo.toml"
    echo
  } >> /evidence/dependency-meta/manifest-fields.txt
  if command -v curl >/dev/null 2>&1; then
    curl -fsS --max-time 30 -H 'user-agent: oo-a183-review (open-ontology)' \
      "https://crates.io/api/v1/crates/${crate_name}/${crate_version}" \
      -o "/evidence/dependency-meta/${crate_name}-${crate_version}.api.json" \
      || echo "api_fetch_failed ${crate_name} ${crate_version}" >> /evidence/dependency-meta/missing.txt
  fi
done <<'EOF'
object_store	0.14.2	f1796bc93603f78c5760a69f2d58badc9618d22adade0a95385bb2adbae4eb94
nix	0.31.3	cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3d
humantime	2.4.0	15cdd26707701c53297e2fa6afb323d55fbc1d0810c3aec078ae3ef0424c3c15
futures-macro	0.3.34	9fb9654ba8355388abeb8dcb4fc62f511300867002afc858860463bdd9fe0c44
itertools	0.15.0	8b4baf93f58d4425749ca49a51c50ebab072c5df6994d08fed93541c331481dc
EOF

obj_crate=$(find /cargo-home/registry/cache -name 'object_store-0.14.2.crate' -type f | head -n 1)
if [ -n "$obj_crate" ]; then
  tar -tzf "$obj_crate" > /evidence/dependency-meta/object_store-0.14.2.crate-list.txt
  mkdir -p /evidence/dependency-meta/object_store-src
  tar -xzf "$obj_crate" -C /evidence/dependency-meta/object_store-src \
    --strip-components=1 \
    object_store-0.14.2/Cargo.toml \
    object_store-0.14.2/src/local.rs \
    object_store-0.14.2/src/lib.rs \
    || tar -tzf "$obj_crate" | awk '/local|multipart|lib.rs|Cargo.toml/' > /evidence/dependency-meta/object_store-interesting-paths.txt
fi

export CARGO_NET_OFFLINE=true
echo "=== cargo tree -p oo-control ==="
set +e
cargo tree --manifest-path Cargo.toml --locked --offline -p oo-control > /evidence/cargo-tree.txt
tree_exit=$?
set -e
echo "phase_exit=$tree_exit phase=cargo_tree"
printf '%s\n' "$tree_exit" > /evidence/phases/tree.exit
if [ "$tree_exit" -ne 0 ]; then
  exit "$tree_exit"
fi

echo "=== cargo test file_hosting ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test file_hosting --lib
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test"
printf '%s\n' "$test_exit" > /evidence/phases/test.exit

echo "=== cargo clippy oo-control ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy"
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit

echo "=== oo-arch-check ==="
set +e
cargo run --manifest-path Cargo.toml --locked --offline -p oo-arch-check -- --manifest /src/Cargo.toml
arch_exit=$?
set -e
echo "phase_exit=$arch_exit phase=arch"
printf '%s\n' "$arch_exit" > /evidence/phases/arch.exit

if ! cmp -s /tmp/Cargo.lock.before /src/Cargo.lock; then
  echo "Cargo.lock changed during the smoke" >&2
  printf '%s\n' 4 > /evidence/phases/lock-after.exit
  lock_after=4
else
  printf '%s\n' 0 > /evidence/phases/lock-after.exit
  lock_after=0
fi

worst=0
for code in "$fmt_exit" "$test_exit" "$clippy_exit" "$arch_exit" "$lock_after"; do
  if [ "$code" -gt "$worst" ]; then
    worst=$code
  fi
done
echo "phase_exit=$worst phase=worst"
printf '%s\n' "$worst" > /evidence/phases/worst.exit
exit "$worst"
