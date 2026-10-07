#!/bin/bash
# Hash this review directory. Does not rewrite any other owner's manifest.
# Origin: a333 chain inventory inside.sh
# SHA-256 0fc9c1d3d1e9d758aafb26581d4be453df03d6eccb155490bd79abf3c4deee61
set -u
echo "oo_task=sp05-query-result-export-chain-review-inventory"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a344"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export HOME=/tmp
mkdir -p /cargo-home/bin /evidence/linux/inventory
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/inventory/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/linux/inventory/inventory.exit; exit 2 ;;
esac
rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
printf '%s\n' "$?" > /evidence/linux/inventory/sha256-tool.exit
printf 'abc' > /cargo-home/sha256-abc.txt
/cargo-home/bin/sha256 file /cargo-home/sha256-abc.txt > /evidence/linux/inventory/sha256-self.txt
if ! grep -q 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' /evidence/linux/inventory/sha256-self.txt; then
  echo "sha256 abc vector mismatch" >&2
  printf '%s\n' 1 > /evidence/linux/inventory/inventory.exit
  exit 1
fi
/cargo-home/bin/sha256 dir /evidence/SHA256SUMS /evidence
hash_exit=$?
printf '%s\n' "$hash_exit" > /evidence/linux/inventory/inventory.exit
echo "inventory_exit=$hash_exit"
exit "$hash_exit"
