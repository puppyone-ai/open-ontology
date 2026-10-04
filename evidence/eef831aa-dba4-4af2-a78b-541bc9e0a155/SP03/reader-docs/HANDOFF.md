# Handoff — a155 reader documentation alignment

Docs commit `e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6`, parent `fd83c00c4a3aa42b22e9425bb2919ffb24fd813b`. Fixed archive `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`.

The nine stale sentences now match the limited acceptance a153 signed on the a149 reader API. `scan`, `bounded_read`, and `bounded_reader` are supported for the PostgreSQL full-mode snapshot, the bounded pull, and `RunComplete` admission. `finish` returns `SnapshotFinished` as lifecycle close only. `finish_complete` returns `RunComplete` as full-selection admission only after `StreamItem::Exhausted` for every selected table.

Do not check SP03-W03. Do not edit the ledger. Do not treat the wording as full-type certification, CDC, incremental mode, registry support, source DDL, or source write. Do not mark a production loader as admitting `RunComplete`. a154 owns that import. The reader token is the prerequisite only.

## Read these

1. `REPORT.md` — smoke exits, blob table, containment, and the requirement map.
2. `FINDINGS.md` — the nine lines, what stays unsupported, and the pre-existing missing supply-chain link.
3. `commands.md` — the smoke invocation and the cleanup that follows the evidence commit.
4. `diff/docs.diff` — the five-file diff in `e6bd4c7a`. `diff/stat.txt` is `5 files changed, 11 insertions(+), 11 deletions(-)`.
5. `checks/production-blobs.txt` — connector, lock, lifecycle, and bounded-stream blobs still equal `bedd6a2c`. The ledger differs from `bedd6a2c` and matches HEAD. This commit did not touch it.
6. `checks/links.txt` — new links resolve. One pre-existing link target is absent.
7. `checks/stale-phrases.txt` — `old_phrases_present=0`.
8. `linux/smoke/console.log` — archive of `bedd6a2c` before the edit. Lib `63 passed`, clippy exit 0, elapsed 41s.
9. `linux/smoke/fixed-blobs.txt` — pre-edit documentation blobs and the production blobs the smoke hashed.

## API to hold

`connector_capabilities` returns `scan` and `bounded_read` supported, reason `PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission`. `cdc`, `type_conversion`, and `registry_api` stay `not_in_this_slice`. `source_ddl` and `source_write` stay `forbidden`. Type capability `bounded_reader` uses the same reason. `TypeSupport.converted` stays false. Catalog `type_conversion` stays unsupported.

Key chunks do not record full-run admission. Loss of the snapshot restarts the whole unpublished run. A new generation does not mix rows from the old cut.

## Cleanup

After the evidence commit, remove only `oo-a155-smoke`, volumes `oo-a155-eef831aa-cargo` and `oo-a155-eef831aa-target`, network `oo-a155-eef831aa-net`, and `/tmp/oo-a155-eef831aa-dba4-4af2-a78b-541bc9e0a155`. Commands are in `commands.md`. Do not prune. Do not remove another task's containers.
