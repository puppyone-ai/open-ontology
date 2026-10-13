#!/bin/bash
# Remove only this task's StarRocks runtime. Cargo volumes stay for the builder.
set -u
PREFIX=oo-dl01w1-45
docker rm -f "${PREFIX}-starrocks" >/dev/null 2>&1 || true
python3 "$(dirname "$0")/admit.py" release starrocks
echo "released ${PREFIX}-starrocks"
