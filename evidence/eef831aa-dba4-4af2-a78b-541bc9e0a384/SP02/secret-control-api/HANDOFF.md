# Handoff

Author delivery for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a384`. SP02-W04 is not accepted. The parent goal is not accepted. A customer KMS, an issued mTLS CA, and a customer IdP are not claimed.

## Where the proof is

Packet: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a384/SP02/secret-control-api/`.

- `REPORT.md`, `FINDINGS.md`, this file, `SOURCE-INVENTORY.txt`.
- `linux/raw/author-proof.json` is the 23 HTTP cases.
- `linux/raw/run.json` is the restarted reference at version 2. It has no secret value.
- `linux/raw/phase.txt`, `pg-restart.txt`, `witness-disposition.txt`, and the `*.exit` files are the raw results.
- `linux/raw/failed-attempts/01-compile` through `12-jsonb-parameter` are the failed resumes. The green exits stay in `linux/raw/` and are not copied into a failure archive.
- Product tree: `tools/secret-control-api/`. Contract, OpenAPI, schemas, migration, and the `secret-control` binary live there.

## Hash

`tools/sha256_manifest.rs` is the copied std-only hasher. `linux/raw/source-sha256.txt` is that program over `tools/secret-control-api`. `SHA256SUMS` is that program over this packet. `SHA256SUMS.sha256` is the host SHA-256 of the `SHA256SUMS` file and is not a line inside `SHA256SUMS`. The inventory container id is `linux/raw/hash-container.id`. The container is removed after the second manifest pass. Do not rerun the manifest after `SHA256SUMS.sha256` exists.

## Cleanup

`linux/cleanup.sh` removes only `oo-a384-proof`, `oo-a384-pg`, volumes `oo-a384-eef831aa-pgdata`, `oo-a384-cargo`, and `oo-a384-work`, network `oo-a384-eef831aa-net`, and `/tmp/oo-a384-eef831aa-dba4-4af2-a78b-541bc9e0a384`. `linux/raw/cleanup.txt` records `foreign_cleanup=no` and `prune=no`. The hash container is a separate short inventory run and is removed by name after it exits.

## Reviewer boundary

The next independent step is a review of the committed product tree and this packet. That review is not this handoff. Codex updates the ledger. This session does not.
