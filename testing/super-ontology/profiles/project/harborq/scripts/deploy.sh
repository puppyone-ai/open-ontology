#!/bin/bash
set -euo pipefail
# Roll the workers one at a time.
for w in $(cat workers.txt); do
  ssh "$w" systemctl restart harborq-worker
  sleep 5
done
