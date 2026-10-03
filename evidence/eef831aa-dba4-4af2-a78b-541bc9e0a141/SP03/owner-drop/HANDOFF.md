# HANDOFF — a141 blocked owner Drop

Source commit: `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`.

Parent: `6cdb559b45ea9d0fed709ca74d80eecf264d8016`.

Fixed input: `58afa97ded85a7dd89c72532bfc8973c45255d7c`, tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`.

Review the four files in that commit. The proof snapshot was the `58afa97` archive plus that overlay. `crates/oo-connectors/src/stream.rs` is unchanged from the fixed input and was not part of the commit.

## Read first

1. `FINDINGS.md` — shutdown cause, stat-snapshot cause, and the 54 ms absence.
2. `REPORT.md` — images, exits, and attempt layout.
3. `linux/green-3/drop-outcome.txt` — both original `(pid, backend_start)` rows absent at 54 ms, `snapshot_finished=0`, reader SQL audit.
4. `linux/green-3/postgres-window.log` — `ROLLBACK` then `Connection reset by peer` for pids 2386 and 2387 at `22:22:46.437`.
5. `linux/green-3/cancel-outcome.txt` and `linux/green-3/keys-outcome.txt` — the affected tests, once each, exit 0.
6. `linux/clippy-1/console.log` — focused `cargo clippy -D warnings` exit 0 on the same overlay.
7. `linux/red-1` — the same regression on unmodified `58afa97`, exit 101, preserved.
8. `linux/diag-2/stacks.txt` — autocommit sampler showing the rows gone while the lock transaction's cached `pg_stat_activity` still listed them.

## Still open

R4, the full reader, and the SP03 parent stay unsigned. a139 keys remain approved. a139 cancel and unpublished-run remain REQUEST_CHANGES against `58afa97`. Independent review of `6b95d6f2` is next.

A drain past 5000 ms and SQLSTATE `25006` were not provoked. The column-revoke matrix was not re-run.

No ledger checkboxes were marked. No push and no release.

## Cleanup and manifest

`cleanup.txt` lists the owned containers, volumes, network, and temporary paths removed after this evidence commit. `hashes.sha256` lists every other file in this directory. The evidence commit is path-limited to `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a141/SP03/owner-drop`.
