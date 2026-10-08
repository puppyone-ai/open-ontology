# Handoff

Independent review for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a400`. Verdict: APPROVE_LIMITED for frozen commit `bb831355662c1dffd76b86bf60417158f4513134`. SP02-W04 is not accepted. The parent goal is not accepted. A customer KMS, an issued mTLS CA, a customer IdP, and foreign pool refresh are not claimed.

## Where the proof is

Packet: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a400/SP02/secret-control-api-review/`.

- `REPORT.md`, `FINDINGS.md`, this file, `SOURCE-INVENTORY.txt`.
- `linux/raw/review-proof.json` is the 27 cases.
- `linux/raw/restart-proof.json` is the post-restart resolve.
- `linux/raw/run.json` is the restarted reference at version 2. It has no secret value.
- `linux/raw/pg-after-restart.txt` is catalog metadata from the same volume. It has no ciphertext bytes.
- `linux/raw/phase.txt`, `pg-restart.txt`, `witness-disposition.txt`, and the `*.exit` files are the raw results.
- `linux/raw/failed-attempts/01-typecheck/` is the failed resume. The green exits stay in `linux/raw/`.
- Frozen product: `tools/secret-control-api` and `tools/control-identity` at `bb831355662c1dffd76b86bf60417158f4513134`. This review did not edit them.

## Hash

`tools/sha256_manifest.rs` is the copied std-only hasher. `linux/raw/source-sha256.txt` is that program over the archived `tools/secret-control-api` tree, written while the archive still exists. Owned cleanup then removes the database, volumes, and archive. `SHA256SUMS` is that program over this packet after cleanup. `SHA256SUMS.sha256` is the host SHA-256 of the `SHA256SUMS` file and is not a line inside `SHA256SUMS`. `linux/raw/hash-container.id` is the packet-hash container. That container is removed by name after it exits. Do not rerun the manifest after `SHA256SUMS.sha256` exists.

## Cleanup

`linux/cleanup.sh` removes only `oo-a400-proof`, `oo-a400-pg`, `oo-a400-hash`, volumes `oo-a400-eef831aa-pgdata`, `oo-a400-cargo`, and `oo-a400-work`, network `oo-a400-eef831aa-net`, and `/tmp/oo-a400-eef831aa-dba4-4af2-a78b-541bc9e0a400`. `linux/raw/cleanup.txt` records `foreign_cleanup=no` and `prune=no`. Owned cleanup runs before the packet manifest so `cleanup.txt` is inside `SHA256SUMS`.

## Boundary

The next ledger update belongs to the coordinator. This session does not edit the ledger, does not push, and does not accept a parent goal.
