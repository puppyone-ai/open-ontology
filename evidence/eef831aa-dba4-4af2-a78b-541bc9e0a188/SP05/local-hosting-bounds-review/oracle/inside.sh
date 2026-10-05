#!/bin/bash
# Independent LocalFileSystem overlay on the immutable 3a251dea archive.
set -eu
echo "oo_task=sp05-local-hosting-bounds-review-oracle"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "overlay=review_bounds.rs"
echo "fixed=3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6"
echo "fetch_argv=none_offline"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test review_bounds -- --nocapture --test-threads=1"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export OO_A188_NOTES=/evidence/notes
mkdir -p /evidence/phases /evidence/notes
rustc -Vv
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f crates/oo-control/tests/review_bounds.rs ]; then
  echo "review overlay is missing" >&2
  exit 2
fi
cp Cargo.lock /tmp/Cargo.lock.before
mkdir -p /tmp/product-before
while IFS= read -r path; do
  [ -n "$path" ] || continue
  mkdir -p "/tmp/product-before/$(dirname "$path")"
  cp "$path" "/tmp/product-before/$path"
done < /evidence/product-paths.txt

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
  echo "fmt check failed" >&2
  exit "$fmt_exit"
fi

echo "=== cargo test review_bounds ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test review_bounds -- --nocapture --test-threads=1
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_review_bounds"
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== cargo clippy oo-control ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy"
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

if ! cmp -s /tmp/Cargo.lock.before /src/Cargo.lock; then
  echo "Cargo.lock changed during the oracle" >&2
  printf '%s\n' 4 > /evidence/phases/lock-after.exit
  exit 4
fi
echo "phase_exit=0 phase=lock_after"
printf '%s\n' 0 > /evidence/phases/lock-after.exit

python3 - <<'PY'
import hashlib, pathlib, sys
root = pathlib.Path("/src")
before = pathlib.Path("/tmp/product-before")
paths = pathlib.Path("/evidence/product-paths.txt").read_text().splitlines()
bad = []
lines = []
for path in paths:
    if not path:
        continue
    data = (root / path).read_bytes()
    prior = (before / path).read_bytes()
    sha = hashlib.sha256(data).hexdigest()
    match = data == prior
    if not match:
        bad.append(path)
    lines.append(f"path={path} sha256={sha} match={int(match)}")
lines.append(f"product_files_match={int(not bad)}")
pathlib.Path("/evidence/product-files-match.txt").write_text("\n".join(lines) + "\n")
if bad:
    sys.exit("product files changed: " + ", ".join(bad))
PY
echo "phase_exit=0 phase=product_files"
printf '%s\n' 0 > /evidence/phases/product-files.exit
echo "phase_exit=0 phase=worst"
printf '%s\n' 0 > /evidence/phases/worst.exit
echo "review_oracle=pass"
