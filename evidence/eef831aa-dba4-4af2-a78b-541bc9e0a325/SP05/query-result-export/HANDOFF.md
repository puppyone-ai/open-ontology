# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a325`. Slice E44 / FILE-D04. Fixed commit `e8d297f95b32052ea587fe8ad67b6e8493a00300`. Parent acceptance is false. SP05-W06, AT-FILE-05, and AT-SEC-03 stay open. This handoff does not release.

## Result

`tools/query-result-export` is a standalone crate with an empty `[workspace]`, edition 2024, rust-version 1.94, and no dependencies. It writes `export.source.jsonl`, `export.csv`, and `export.sidecar.json` from one query-pg stdout plus caller column names. The contract is `tools/query-result-export/CONTRACT.md`.

Proof:

- `linux/smoke/` exit 0 before the overlay existed. Container `9f52a8541f0b77c9aacef478a80f9ff03e9cad618beab86bad22ecb12b37b821`.
- `linux/postedit/` exit 0. Container `08e3777ffd898580210affad0e662a19b2eae3fe2fa57686c549e10828f89c8c`. Test 18 passed. Clippy `-D warnings` exit 0. Live CSV SHA-256 `9c7c1562db84359e654e50f7f836465062759af12d84f36d52aa6f845c1607f1`. Live source SHA-256 `54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb`.

`origin/recipes.txt` records the a204, a200, a199, a254, and a266 blobs and the hashes of this task's scripts. `REPORT.md` and `FINDINGS.md` are the run record and the review.

## Left open

xlsx, numeric OID 1700 as a cell, download authorization, retention, deletion, and the reaper. The hand formula file is not a compiled query-pg receipt. A column kind conflict, a U+FEFF prefix, and a failed `sync_all` are implemented and not separately proved. See FINDINGS.

## Resources

This task's builder, network `oo-a325-eef831aa-net`, volumes `oo-a325-eef831aa-cargo` and `oo-a325-eef831aa-target`, and `/tmp/oo-a325-eef831aa-dba4-4af2-a78b-541bc9e0a325` are removed after the proof files were copied into this directory. Cleanup output is `cleanup.txt`. No prune. Other owners' containers were not removed.

`tools/query-result-export/target` is host build output. It is not part of the crate commit. Do not add it.

Root `Cargo.toml` and root `Cargo.lock` stay unchanged. Do not mark the ledger or the spec closed from this slice.
