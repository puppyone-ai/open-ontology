# Source inventory

Product base `20f065096fa9e20414eed8951d830333160e4af6` (`pg-backup-restore: prove the cut with writer locks`). Working parent at report time `d63c388a61f6249cd85b5fc171dbcb0155659d07`. `gold.rs` blob `2564210b226be3d6980265a1dc429cfadf2a613f` matches that parent. `Cargo.toml` and `Cargo.lock` are unchanged. No root manifest edit.

## Product tree

Edited:

- `tools/pg-backup-restore/CONTRACT.md`
- `tools/pg-backup-restore/src/cut.rs`
- `tools/pg-backup-restore/src/fixture.rs` (two fixture sequences)
- `tools/pg-backup-restore/src/lib.rs`
- `tools/pg-backup-restore/src/manifest.rs`
- `tools/pg-backup-restore/src/ops.rs`
- `tools/pg-backup-restore/src/pg.rs`
- `tools/pg-backup-restore/src/snapshot.rs` (new)
- `tools/pg-backup-restore/tests/oracle.rs`

Unchanged product files that this cut still uses: `decide.rs`, `gold.rs`, `json.rs`, `main.rs`, `protocol.rs`, `sha256.rs`, `Cargo.toml`, `Cargo.lock`.

The diff stat against the parent for the edited tracked files is 705 insertions and 89 deletions, plus the new `snapshot.rs`.

## Evidence

All of `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a377/SP14/pg-backup-catalog-cut/`.

- `CONTRACT-FREEZE.md` was written before the product edit.
- `linux/pre-edit-smoke/` is the Rust 1.94 smoke, inside exit 0.
- `linux/build/` is the focused test and clippy build, inside exit 0. Phase exits: build, check-build, check-clippy, check-test, clippy-component, clippy, test, all 0.
- `linux/run-live.sh`, `live-phases.sh`, `psql-exec.sh`, `private-glibc.sh`, `inside.sh` are the harness.
- `pg/pg-wrap.sh` is the a274 file, SHA-256 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`.
- `oracle/` is the independent checker. It does not link the product crate.
- `live/` is the recorded run, including `live/check-input/` (manifest, both dumps, gold, probes, readiness).
- `failed-attempts/` holds build 1–4 and live 1–2.
- `SHA256SUMS` and `SHA256SUMS.sha256` are produced by this crate's `hash-tree` after this inventory. The sidecar is the hash of the sums file. Host `shasum` is not the authority.

No customer call, push, or release. Enum and calendar product SQL were not edited.
