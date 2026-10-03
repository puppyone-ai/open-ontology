# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a135`. Fixed source `bd77b2520bcc3d63517bc49309002f25b58544b1`. Author evidence `3a5478ac`.

## Driver

**APPROVE_LIMITED** for the init_mut receive-tail and the short-frame cap path.

512 short frames, dropped and retained, cap 32, bytes 1.12.1 (`fc652a48…`), product blob `b56fe942…`: view stays 8192 through frame 511. Cancel and reset reserve once and peak at 16384. Fuse 24 KiB. `OOMKilled=false`.

TLS verify-full, 12000 rows, host oracle `ec867603…`: view 8192, RSS 8924 KiB, caller-retained 48000, reader SQL lines 2, reader writes 0, reader did not `SET`. Admin fixture was a separate connection.

This removes the driver-specific growth blocker only.

## Still unsigned

Full reader: unsigned. R3: unsigned. R4: unsigned.

`pg_stream_live` exit 0 and `pg_stream_bounds` exit 0 once on this combined tree (Arrow `stream.rs` blob `e7b5a6cc…` plus the tail fix). Finite-path compatibility only. `fetch_all` is not the row-rate proof and not the bounded Arrow proof. No matrix. No license tests. Author wire-bounds, accounting, and portal exits were cited from a133 (`proof/*.exit` = 0), not re-run. No workspace `sqlx-core` test. No embedded stripped-copy test.

## Repo

Production source, manifests, lock, spec, and ledger were not modified. Evidence is this directory only. `hash-manifest.sha256` is the complete file list of this directory except itself. a133's 21-line manifest is the author's selective list.

Own containers, volumes, network, and `/tmp/oo-a135-eef831aa` are removed in `cleanup.txt`. No prune and no foreign cleanup. Local evidence commit only. No push.

The coordinator's 15-minute wait and handle check are not this review.
