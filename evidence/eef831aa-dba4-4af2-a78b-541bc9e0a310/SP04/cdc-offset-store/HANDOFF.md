# Handoff

Read `REPORT.md` and `FINDINGS.md` first. Product sources are in `source/blobs/tools/cdc-offset-store/`. Raw exits are the `*.exit` files under `linux/` and `pg/`. `MANIFEST.sha256` is the Rust inventory of this directory and does not include itself. `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` are written after that inventory.

The passing proof is `linux/impl-5` plus `linux/restart-2`. Do not treat impl-1 through impl-4 or `linux/restart` as the result. Their logs stay so the failed exits remain visible.

The crate is safe Rust (`unsafe_code` forbidden). It has no path dependency. The next parent slice is source acknowledgement and stream trim, fed by a real normalizer, not by this checkpoint. Do not point that work at a moving CDC writer tree. Use the fixed target receipt contract.

Owned paths for the commit:

- `tools/cdc-offset-store/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a310/SP04/cdc-offset-store/**`

No other path is part of this change. The root lock and the root manifest stay as they were at `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc` for this crate's pins.
