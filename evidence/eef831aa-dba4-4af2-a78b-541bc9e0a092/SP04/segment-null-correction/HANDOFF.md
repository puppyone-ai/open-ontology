# HANDOFF — a092 segment NULL correction

Stop here for fixed review. No push, pull request, release, or customer access.

## Read first

- `crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql`
- `contracts/pipeline/target-fence-receipts/target-fence-receipts-v2.json`
- `crates/oo-pipeline/tests/segment_null_correction.rs`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a092/SP04/segment-null-correction/REPORT.md`

Assigned baseline `fc877c482a61729ea46e779a674a46f8f72c80c5`, tree `d40755c31abd82d932b56886aa048ab4d7aa5e4b`, parent `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`. It is an ancestor of the parent below.

Parent at commit: `82d4c85492fc1b76fa1048bbd970de2909594295`, tree `95cc13b92b7d57833c268b15550e8c45c7f3f17f`. The delivery commit is the one whose tree contains this file. Confirm with `git show --name-only --format=fuller -1`.

Migration SHA-256: `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512`.

v1 file hashes that must stay:

- `0000` `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208`
- `0001` `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9`

## What the reviewer is checking

`commit_apply_receipt` and `apply_receipts_segment_check` accept only an explicit absent segment or a non-null legal present segment. Unknown index/count combinations raise `23514` from the segment rule, including when the generation is also stale. A legal segment with a stale generation on the holding backend is still `OO013`. Preexisting illegal receipts abort `0002` with static `OO021` and no receipt mutation. Lawful receipts, including `target_commit_at` and `fence_generation`, stay byte-equal across the upgrade. `_oo.schema_ready` for v1 does not authorize a v2 driver. `segment_contract_ready` trusts the caller argument only as a comparison to the stored row.

## Evidence map

- `linux/baseline/` — fc877c4 smoke, all phase exits 0, cleanup exits 0.
- `linux/attempt-51b9fcf0-eb99-4b82-a8f8-f914415571cf/` — first overlay smoke, fetch 0, test 101, clippy 101, cleanup exits 0.
- `linux/fixed/` — second overlay smoke, run `2ad2946a-e63e-478f-9397-f2539d27ea56`, all phase exits 0, cleanup exits 0.
- `pg/` — one PostgreSQL 17.10 fixture. `summary.log` includes three earlier harness failures and then `live=pass`. Case files match the passing run. Cleanup exits 0.

The live fixture binary was host rustc 1.98.1, before the manifest-dir and two Clippy edits. Rust 1.94 compiled the final test and skipped it (`1 ignored`). No second PostgreSQL fixture was run.

## Left alone

`0000`, `0001`, `target-fence-receipts-v1.json`, root manifest and lock, jobs, connector, SDK, builder, xtask, and older evidence. No `GRANT oo_target_owner TO oo_pipeline_runtime`.

## Not claimed

SP04-W05, AT-PL-05, Loader, CDC, control mirror, ACK, physical mapping, 100 workers, dual arch, and release. A085-F2, F3, and F5 remain open. F4 is closed only for the new contract's external conjunction.
