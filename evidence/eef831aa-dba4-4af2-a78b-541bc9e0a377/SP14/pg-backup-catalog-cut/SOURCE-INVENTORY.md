# Source inventory

Product base `20f065096fa9e20414eed8951d830333160e4af6`. Prior sequence-refusal packet `d86fea93c4e537b77fd8bd4a47ca91f3a4746785`. Working parent at report time `068168f619019c3e9c9947fce232f8e30cbe3216`. `gold.rs` blob `2564210b226be3d6980265a1dc429cfadf2a613f`. `Cargo.toml` and `Cargo.lock` are unchanged. No root manifest edit.

## Product tree

Edited in this packet:

- `tools/pg-backup-restore/CONTRACT.md`
- `tools/pg-backup-restore/src/cut.rs`
- `tools/pg-backup-restore/src/fixture.rs` (roles `oo_a377_owner` and `oo_a377_operator`, owner assignment)
- `tools/pg-backup-restore/src/ops.rs`
- `tools/pg-backup-restore/src/snapshot.rs`
- `tools/pg-backup-restore/tests/oracle.rs`

Unchanged product files that this cut still uses: `decide.rs`, `gold.rs`, `json.rs`, `lib.rs`, `main.rs`, `manifest.rs`, `pg.rs`, `protocol.rs`, `sha256.rs`, `Cargo.toml`, `Cargo.lock`.

## Evidence

All of `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a377/SP14/pg-backup-catalog-cut/`.

- `CONTRACT-FREEZE.md` is the earlier freeze. `CONTRACT-FREEZE-SEQUENCE.md` is the sequence-lock amendment written before the product edit in this packet.
- `linux/pre-edit-smoke/` is the Rust 1.94 smoke, inside exit 0.
- `linux/build-prior-unproven/` is the builder that produced the `42809` refusal packet.
- `linux/build/` is the focused test and clippy build for the owned-by fence, inside exit 0.
- `prior-unproven-cut/` is the live tree of that refusal packet.
- `seq-lock-probe/` holds the PostgreSQL 17.10 measurements of `OWNED BY`, cache `nextval`, and the operator role.
- `live/` is this run: backup exit 0, corrupt-dump exit 4, restore exit 0, cut-check exit 0. `live/check-input/` is the manifest, both dumps, gold, probes, and readiness that `cut-check` read.
- `runtime/` is gitignored. It holds the password file and private keys used by the local server. It is not part of the hash tree.
- `pg/pg-wrap.sh` is the a274 file, SHA-256 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`.
- `oracle/` is the independent checker. It does not link the product crate.
- `failed-attempts/` holds the earlier build and live misses.
- `SHA256SUMS` and `SHA256SUMS.sha256` are produced by this crate's `hash-tree` inside the private PostgreSQL container, after this inventory. The sidecar is the hash of the sums file. Host `shasum` is not the authority.

No customer call, push, or release. Enum and calendar product SQL were not edited.
