#!/bin/bash
# Pre-edit package/lib smoke. Archive of HEAD before the import command. No PostgreSQL.
set -u
echo "oo_task=import-pg-cli-smoke-before"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=head_archive_before_edit"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a162-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a162-gitsystem
unset RUST_LOG || true
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a162-smoke
git config --file "$GIT_CONFIG_GLOBAL" user.email a162-smoke@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line"
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit
rustc -Vv > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt

cd /src
fail=0
note() {
  name=$1
  code=$2
  printf '%s\n' "$code" > "/evidence/${name}.exit"
  echo "phase_exit=$code phase=$name"
  if [ "$code" -ne 0 ] && [ "$fail" -eq 0 ]; then
    fail=$code
  fi
}

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
  note rustup-clippy $?
  if [ "$(cat /evidence/rustup-clippy.exit)" -ne 0 ]; then
    printf '%s\n' "$(cat /evidence/rustup-clippy.exit)" > /evidence/inside.exit
    exit "$(cat /evidence/rustup-clippy.exit)"
  fi
else
  echo "clippy_component=already_installed"
  note rustup-clippy 0
fi

python3 - <<'PY'
import os
root = "crates"
needles = ("unsafe",)
hits = []
for crate in ("oo-pipeline", "oo-connectors"):
    base = os.path.join(root, crate, "src")
    for dirpath, _, files in os.walk(base):
        for name in files:
            if not name.endswith(".rs"):
                continue
            path = os.path.join(dirpath, name)
            with open(path, "r", encoding="utf-8") as handle:
                for lineno, line in enumerate(handle, 1):
                    if "unsafe" in line:
                        hits.append(f"{path}:{lineno}:{line.strip()}")
print("unsafe_token_count", len(hits))
for hit in hits:
    print(hit)
open("/evidence/unsafe-tokens.txt", "w", encoding="utf-8").write("\n".join(hits) + ("\n" if hits else ""))
# The only allowed token in these two crates is the forbid attribute.
bad = [hit for hit in hits if "forbid(unsafe_code)" not in hit]
open("/evidence/unsafe-classifier.exit", "w").write("0\n" if not bad else "1\n")
if bad:
    raise SystemExit(1)
PY
note unsafe-classifier $?

cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
note fetch $?
if [ "$fail" -ne 0 ]; then
  printf '%s\n' "$fail" > /evidence/inside.exit
  exit "$fail"
fi

cargo test -p oo-pipeline --lib --locked --offline > /evidence/pipeline-lib.stdout 2> /evidence/pipeline-lib.stderr
note pipeline-lib $?
cargo test -p oo-connectors --lib --locked --offline > /evidence/connectors-lib.stdout 2> /evidence/connectors-lib.stderr
note connectors-lib $?
cargo clippy -p oo-pipeline --lib --locked --offline -- -D warnings > /evidence/pipeline-clippy.stdout 2> /evidence/pipeline-clippy.stderr
note pipeline-clippy $?
cargo clippy -p oo-connectors --lib --locked --offline -- -D warnings > /evidence/connectors-clippy.stdout 2> /evidence/connectors-clippy.stderr
note connectors-clippy $?
printf '%s\n' "$fail" > /evidence/inside.exit
exit "$fail"
