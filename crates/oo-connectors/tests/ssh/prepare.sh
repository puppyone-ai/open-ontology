#!/bin/sh
set -eu
cd "$(dirname "$0")/../../../.."
cargo build -p oo-cli --locked
cargo build --manifest-path tools/oo-load/Cargo.toml --target-dir target --locked
cargo build --manifest-path deploy/lite/mysql-doctor/Cargo.toml --target-dir target --locked
cargo test -p oo-connectors --test ssh_transport_live --locked --no-run --message-format=json |
    python3 -c '
import json, shutil, sys
paths = [v["executable"] for line in sys.stdin if (v := json.loads(line)).get("executable")
         and v.get("target", {}).get("name") == "ssh_transport_live"]
assert len(paths) == 1, "missing transport test binary"
shutil.copy2(paths[0], "target/debug/ssh-transport-test")
'
