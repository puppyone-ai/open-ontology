#!/bin/bash
# Chain replay, then the product benchmark phase in the same builder.
set -u
bash /inside.sh
chain=$?
printf '%s\n' "$chain" > /evidence/linux/chain/entry-chain.exit
if [ "$chain" -ne 0 ]; then
  echo "chain_failed=$chain" >&2
  exit "$chain"
fi
bash /bench-inside.sh
exit $?
