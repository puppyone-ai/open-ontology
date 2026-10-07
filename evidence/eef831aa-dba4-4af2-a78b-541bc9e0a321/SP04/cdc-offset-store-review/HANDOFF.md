# Handoff

Verdict: **APPROVE_LIMITED** for the frozen development child CDC-D05, `tools/cdc-offset-store`, at fixed archive `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`. Accepted scope is the fenced target receipt, the immutable control mirror, and the contiguous compare-and-swap watermark. ACK stays false. Caller `source_gap` and `epoch_barrier` are not a source certificate. Large segments, source acknowledgement, Valkey `XACK` and `XTRIM`, and the reject ledger stay outside this child. Parent SP04 and those items stay open. This packet does not verdict CDC-D04 and does not record coordinator acceptance. No builder, PostgreSQL, or test was started for this wording change.

Read `REPORT.md` and `FINDINGS.md` first. Product bytes copied from the fixed archive are under `source/blobs/cdc-offset-store/`. Raw exits are the `*.exit` files under `linux/` and `pg/`. `MANIFEST.sha256` is the Rust inventory of 255 files and does not include itself. Excluded after that walk: `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt`. The clarification replaces only the digest lines for `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`. The other 252 lines stay the Rust walk.

The passing proof is `linux/preoverlay`, then `linux/overlay`, then `linux/restart`. Do not treat `linux/overlay-attempt-1`, `linux/restart-attempt-1`, or `linux/restart-attempt-2` as the result. Their logs stay so the failed exits remain visible.

The crate is safe Rust (`unsafe_code` forbidden). It has no path dependency. The next parent slice is source acknowledgement and stream trim, fed by a real normalizer, not by this checkpoint. Use the fixed target receipt scripts. Do not point that work at the moving `cdc-apply-pg` tree.

Owned path for the commit:

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a321/SP04/cdc-offset-store-review/**`

No product path, root lock, or root manifest is part of this change.
