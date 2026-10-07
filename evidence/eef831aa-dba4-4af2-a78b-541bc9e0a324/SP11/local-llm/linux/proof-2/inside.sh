#!/bin/bash
# Second proof. The first proof kept its lock-extra list and exited before fetch.
# This run downgrades the drifted transitive crates to versions present in the fixed archive lock.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export TMPDIR=/target/tmp
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy NO_PROXY no_proxy
unset OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE
mkdir -p /target/tmp /evidence/phases
cd /crate

phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/${name}.stdout" 2> "/evidence/${name}.stderr"
  local code=$?
  printf '%s\n' "$code" > "/evidence/phases/${name}.exit"
  echo "${name}_exit=${code}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

rustc -vV | tee /evidence/rustc-version.txt
if ! grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "msrv mismatch" >&2
  exit 1
fi
test -f /crate/Cargo.lock
test -f /archive/Cargo.lock

if ! rustup component list --installed | grep -q '^clippy'; then
  phase rustup-clippy rustup component add clippy
else
  echo "clippy already installed" > /evidence/rustup-clippy.stdout
  printf '%s\n' 0 > /evidence/phases/rustup-clippy.exit
fi

# name from-version to-version. Skip a pin when an earlier update already landed on `to`.
has_pair() {
  awk -v want_name="$1" -v want_version="$2" '
    $1=="name" && $2=="=" { gsub(/"/,"",$3); name=$3; next }
    $1=="version" && $2=="=" && name!="" {
      gsub(/"/,"",$3);
      if (name==want_name && $3==want_version) found=1;
      name="";
    }
    END { exit found ? 0 : 1 }
  ' /crate/Cargo.lock
}
while read -r name from to; do
  [ -n "$name" ] || continue
  if has_pair "$name" "$to" && ! has_pair "$name" "$from"; then
    echo "already ${name}@${to}" > "/evidence/update-${name}.stdout"
    printf '%s\n' 0 > "/evidence/phases/update-${name}.exit"
    echo "update-${name}_exit=0"
    continue
  fi
  if ! has_pair "$name" "$from"; then
    echo "missing ${name}@${from}" > "/evidence/update-${name}.stderr"
    printf '%s\n' 1 > "/evidence/phases/update-${name}.exit"
    echo "update-${name}_exit=1"
    exit 1
  fi
  phase "update-${name}" cargo update --manifest-path /crate/Cargo.toml -p "${name}@${from}" --precise "$to"
done <<'PINS'
crypto-common 0.1.7 0.1.6
generic-array 0.14.7 0.14.9
h2 0.4.20 0.4.19
hyper 1.12.0 1.11.1
js-sys 0.3.106 0.3.105
libc 0.2.190 0.2.189
mio 1.2.4 1.2.3
quinn-proto 0.11.19 0.11.18
quinn-udp 0.5.16 0.5.15
smallvec 1.16.2 1.16.1
want 0.3.2 0.3.1
wasm-bindgen 0.2.129 0.2.128
wasm-bindgen-futures 0.4.79 0.4.78
wasm-bindgen-macro 0.2.129 0.2.128
wasm-bindgen-macro-support 0.2.129 0.2.128
wasm-bindgen-shared 0.2.129 0.2.128
web-sys 0.3.106 0.3.105
zeroize 1.9.1 1.9.0
PINS

pairs() {
  awk '$1=="name" && $2=="=" { gsub(/"/,"",$3); name=$3; next }
       $1=="version" && $2=="=" && name!="" { gsub(/"/,"",$3); print name, $3; name="" }' "$1"
}
pairs /archive/Cargo.lock > /tmp/allowed.pairs
pairs /archive/tools/description-provider/Cargo.lock >> /tmp/allowed.pairs
pairs /crate/Cargo.lock | awk '$1 != "oo-local-llm"' > /tmp/new.pairs
sort -u /tmp/allowed.pairs -o /tmp/allowed.pairs
sort -u /tmp/new.pairs -o /tmp/new.pairs
comm -13 /tmp/allowed.pairs /tmp/new.pairs > /evidence/lock-extra.txt
if [ -s /evidence/lock-extra.txt ]; then
  echo "lock still has versions outside the fixed archive" >&2
  wc -l /evidence/lock-extra.txt
  exit 1
fi
echo "lock_versions=subset" | tee /evidence/lock-versions.txt
cp /crate/Cargo.lock /evidence/Cargo.lock
phase fetch cargo fetch --manifest-path /crate/Cargo.toml --locked
phase test cargo test --manifest-path /crate/Cargo.toml --locked --offline --all-targets
phase clippy cargo clippy --manifest-path /crate/Cargo.toml --locked --offline --all-targets -- -D warnings
find src Cargo.toml Cargo.lock CONTRACT.md .gitignore -type f | sort | xargs sha256sum > /evidence/source-after.sha256
echo "proof_ok"
