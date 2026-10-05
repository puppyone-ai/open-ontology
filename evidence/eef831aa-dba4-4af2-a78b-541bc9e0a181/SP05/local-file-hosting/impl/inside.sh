#!/bin/bash
# Implementation smoke: lock the object_store closure, then test, clippy, and the architecture gate.
set -eu
echo "oo_task=sp05-local-file-hosting-impl"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml"
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
mkdir -p /evidence/phases /tmp/lockdiff
cp Cargo.lock /tmp/lockdiff/Cargo.lock.orig
cp Cargo.lock /evidence/Cargo.lock.orig

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

echo "=== cargo fmt -p oo-control ==="
set +e
cargo fmt -p oo-control
fmt_exit=$?
set -e
echo "phase_exit=$fmt_exit phase=cargo_fmt"
printf '%s\n' "$fmt_exit" > /evidence/phases/fmt.exit
if [ "$fmt_exit" -ne 0 ]; then
  exit "$fmt_exit"
fi

echo "=== cargo fetch --manifest-path Cargo.toml ==="
set +e
cargo fetch --manifest-path Cargo.toml
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch"
printf '%s\n' "$fetch_exit" > /evidence/phases/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

awk '
function flush() {
  if (name == "") {
    return
  }
  if (checksum == "") {
    checksum = "-"
  }
  print name "\t" version "\t" checksum
}
/^\[\[package\]\]/ { flush(); name = ""; version = ""; checksum = ""; next }
/^name = "/ && name == "" {
  name = substr($0, 9, length($0) - 9)
  next
}
/^version = "/ && name != "" && version == "" {
  version = substr($0, 12, length($0) - 12)
  next
}
/^checksum = "/ && name != "" && checksum == "" {
  checksum = substr($0, 13, length($0) - 13)
  next
}
END { flush() }
' /tmp/lockdiff/Cargo.lock.orig | sort > /tmp/lockdiff/old.tsv
awk '
function flush() {
  if (name == "") {
    return
  }
  if (checksum == "") {
    checksum = "-"
  }
  print name "\t" version "\t" checksum
}
/^\[\[package\]\]/ { flush(); name = ""; version = ""; checksum = ""; next }
/^name = "/ && name == "" {
  name = substr($0, 9, length($0) - 9)
  next
}
/^version = "/ && name != "" && version == "" {
  version = substr($0, 12, length($0) - 12)
  next
}
/^checksum = "/ && name != "" && checksum == "" {
  checksum = substr($0, 13, length($0) - 13)
  next
}
END { flush() }
' /src/Cargo.lock | sort > /tmp/lockdiff/new.tsv

: > /evidence/lock-added.txt
: > /evidence/lock-version-added.txt
: > /evidence/lock-delta-errors.txt
set +e
awk -F '\t' '
NR == FNR {
  old[$1] = old[$1] " " $2
  pair[$1 "\t" $2] = $3
  next
}
{
  key = $1 "\t" $2
  if (!(key in pair)) {
    if ($1 in old) {
      print $1 "\t" $2 "\t" $3 >> "/evidence/lock-added.txt"
      print "added version alongside existing: " $1 " " $2 >> "/evidence/lock-version-added.txt"
    } else {
      print $1 "\t" $2 "\t" $3 >> "/evidence/lock-added.txt"
    }
  } else if (pair[key] != $3) {
    print "checksum changed: " $1 " " $2 >> "/evidence/lock-delta-errors.txt"
    bad = 1
  }
}
END { if (bad) exit 1 }
' /tmp/lockdiff/old.tsv /tmp/lockdiff/new.tsv
delta_new=$?
awk -F '\t' '
NR == FNR { seen[$1 "\t" $2] = 1; next }
{
  key = $1 "\t" $2
  if (!(key in seen)) {
    print "existing package version removed: " $1 " " $2 >> "/evidence/lock-delta-errors.txt"
    bad = 1
  }
}
END { if (bad) exit 1 }
' /tmp/lockdiff/new.tsv /tmp/lockdiff/old.tsv
delta_old=$?
set -e
if [ "$delta_new" -ne 0 ] || [ "$delta_old" -ne 0 ]; then
  echo "phase_exit=4 phase=lock_delta"
  printf '%s\n' 4 > /evidence/phases/lock-delta.exit
  echo "lock delta refused" >&2
  cat /evidence/lock-delta-errors.txt >&2
  exit 4
fi
echo "phase_exit=0 phase=lock_delta"
printf '%s\n' 0 > /evidence/phases/lock-delta.exit
echo "=== lock additions ==="
cat /evidence/lock-added.txt
echo "=== versions added beside an existing package ==="
cat /evidence/lock-version-added.txt

cp /src/Cargo.lock /evidence/Cargo.lock
mkdir -p /evidence/dependency-meta
while IFS="$(printf '\t')" read -r crate_name crate_version crate_checksum; do
  [ -n "$crate_name" ] || continue
  crate_file=$(find /cargo-home/registry/cache -name "${crate_name}-${crate_version}.crate" -type f | head -n 1)
  if [ -z "$crate_file" ]; then
    echo "missing ${crate_name}-${crate_version}" >> /evidence/dependency-meta/missing.txt
    continue
  fi
  actual=$(sha256sum "$crate_file" | awk '{print $1}')
  printf '%s %s lock=%s file=%s\n' "$crate_name" "$crate_version" "$crate_checksum" "$actual" >> /evidence/dependency-meta/checksums.txt
  tar -xOf "$crate_file" "${crate_name}-${crate_version}/Cargo.toml" > "/evidence/dependency-meta/${crate_name}-${crate_version}.Cargo.toml"
  if command -v curl >/dev/null 2>&1; then
    curl -fsS --max-time 20 -H 'user-agent: oo-a181-dependency-record (open-ontology)' \
      "https://crates.io/api/v1/crates/${crate_name}/${crate_version}" \
      > "/evidence/dependency-meta/${crate_name}-${crate_version}.api.json" || \
      echo "api_fetch_failed ${crate_name} ${crate_version}" >> /evidence/dependency-meta/api-errors.txt
  else
    echo "curl_missing" >> /evidence/dependency-meta/api-errors.txt
  fi
done < /evidence/lock-added.txt

echo "=== cargo tree -p oo-control --locked --offline ==="
set +e
cargo tree -p oo-control --locked --offline --format '{p} {f}' > /evidence/cargo-tree.txt
tree_exit=$?
set -e
echo "phase_exit=$tree_exit phase=cargo_tree"
printf '%s\n' "$tree_exit" > /evidence/phases/tree.exit
if [ "$tree_exit" -ne 0 ]; then
  exit "$tree_exit"
fi

echo "=== cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test file_hosting --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test file_hosting --lib
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_oo_control"
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_control"
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

echo "=== cargo run --locked --offline -p oo-arch-check ==="
set +e
cargo run --manifest-path Cargo.toml --locked --offline -p oo-arch-check -- --manifest /src/Cargo.toml
arch_exit=$?
set -e
echo "phase_exit=$arch_exit phase=oo_arch_check"
printf '%s\n' "$arch_exit" > /evidence/phases/arch.exit
if [ "$arch_exit" -ne 0 ]; then
  exit "$arch_exit"
fi

mkdir -p /evidence/formatted
cp crates/oo-control/src/lib.rs /evidence/formatted/lib.rs
cp crates/oo-control/src/file_hosting.rs /evidence/formatted/file_hosting.rs
cp crates/oo-control/tests/file_hosting.rs /evidence/formatted/file_hosting_test.rs
cp crates/oo-control/Cargo.toml /evidence/formatted/Cargo.toml

echo "elapsed_smoke_seconds=$SECONDS"
echo "smoke_worst_phase_exit=0"
printf '%s\n' 0 > /evidence/phases/worst.exit
