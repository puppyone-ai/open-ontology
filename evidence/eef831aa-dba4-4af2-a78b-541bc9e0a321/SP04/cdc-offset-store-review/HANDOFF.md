# Handoff

Read `REPORT.md` and `FINDINGS.md` first. Product bytes copied from the fixed archive are under `source/blobs/cdc-offset-store/`. Raw exits are the `*.exit` files under `linux/` and `pg/`. `MANIFEST.sha256` is the Rust inventory of this directory and does not include itself. `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` are written after that inventory.

The passing proof is `linux/preoverlay`, then `linux/overlay`, then `linux/restart`. Do not treat `linux/overlay-attempt-1`, `linux/restart-attempt-1`, or `linux/restart-attempt-2` as the result. Their logs stay so the failed exits remain visible.

The crate is safe Rust (`unsafe_code` forbidden). It has no path dependency. The next parent slice is source acknowledgement and stream trim, fed by a real normalizer, not by this checkpoint. Use the fixed target receipt scripts. Do not point that work at the moving `cdc-apply-pg` tree.

Owned path for the commit:

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a321/SP04/cdc-offset-store-review/**`

No product path, root lock, or root manifest is part of this change.
