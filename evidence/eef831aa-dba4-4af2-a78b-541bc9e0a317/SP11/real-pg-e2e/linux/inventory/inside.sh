#!/bin/bash
# Hash the evidence tree with the walker already built on the cargo volume.
# No cargo update. No network use beyond the attached private network.
set -u
echo "oo_task=sp11-real-pg-e2e-inventory"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a317"
test -x /cargo-home/bin/sha256_manifest
/cargo-home/bin/sha256_manifest /crate /evidence/source/CRATE-SHA256SUMS
crate_exit=$?
echo "crate_walker_exit=$crate_exit"
if [ "$crate_exit" -ne 0 ]; then
  exit "$crate_exit"
fi
/cargo-home/bin/sha256_manifest /evidence /evidence/SHA256SUMS
walker_exit=$?
echo "walker_exit=$walker_exit"
exit "$walker_exit"
