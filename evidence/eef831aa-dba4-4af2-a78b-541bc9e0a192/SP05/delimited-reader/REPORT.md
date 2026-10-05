# a192 bounded CSV and TSV source reader

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a192`. This slice adds `oo.source.delimited/v1` to `oo-connectors`. The reader pulls an explicit comma or tab dialect from any `std::io::Read`, projects a typed profile into series-60 `RecordBatch` values, and mints `DelimitedComplete` only after a real EOF, full profile coverage, and a matching whole-object SHA-256.

Source commit `671507b229ad7d1bc6a2e948b5b74d6d7252393f` on parent `65169f374c665d8efdd62a8bcd47317c38a1359e`. Baseline `15d23fe9f2cbc74e3f3278d70d0f56390568126e` is an ancestor of that parent. The commit contains only:

- `crates/oo-connectors/src/delimited.rs`
- `crates/oo-connectors/src/lib.rs` (module and re-export)
- `crates/oo-connectors/tests/delimited.rs`
- `contracts/connectors/delimited-reader-v1.md`

`source/blobs/` matches that commit. `source/blob-sha256.txt` records the SHA-256 of each blob. No manifest, lock, dependency, JSONL reader, PostgreSQL driver, snapshot, core, control, gateway, builder, pipeline, arch-gate, spec, or ledger file was edited. No parent checkbox was changed.

## Behavior

The dialect is an argument: comma or tab. Any other separator byte is `ambiguous_dialect`. The parser is a field state machine over one fixed window of `max_record_bytes`. It accepts quoted fields, doubled quotes, quoted newlines, CRLF, and a last record that ends at EOF. It refuses malformed quotes, a bare CR, ASCII controls other than TAB, LF, and CR, invalid UTF-8, and a width that differs from the profile. Errors carry a kind and a static fact. Display does not include cell bytes.

The profile fixes column order, names, nullability, and the logical type (`utf8`, `bool`, `int64`, `decimal`). BOM policy is `refuse`, `strip`, or `require`. Header policy is `absent` or `exact`. Null policy is empty-field or an explicit token such as `\N`. Supported Arrow values are `StringArray`, `BooleanArray`, `Int64Array`, and `Decimal128Array`. Decimal text is checked lexically, then `oo_core::ExactDecimal::parse_sql`, `rescale_exact`, and `fits_decimal128`. `1.2` into scale 2 becomes coefficient `120`. `1.251` into scale 2 is `inexact_decimal`. There is no `f64` and no half-up rounding. `i64::MIN` is converted through `i128`.

The object hash is updated from raw bytes before parse and compared only at EOF on a record boundary. `DelimitedItem::Exhausted` has no boolean EOF field. `finish` is the only constructor of `DelimitedComplete`. `admits_import`, `published`, `arrow_ipc`, and `executes_native` are false. The receipt is a parse proof. It is not a PostgreSQL `RunComplete`, and it does not admit a loader.

Cancel and the deadline are checked before each `read`. A `Read` that blocks inside one call is not interrupted. The hold test in `cancel_before_a_read_and_a_blocked_read_refuse_completion` requests cancel while that call is inside `read`, waits until the call returns, and then observes `cancelled` with `reads == 1`. `finish` does not mint `DelimitedComplete`.

## Linux Rust 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, Rust `1.94.0` (`4a4ef493e 2026-03-02`). `RUSTUP_TOOLCHAIN=1.94.0` overrides the workspace toolchain file channel `1.98.1`. The runs used private network `oo-a192-eef831aa-net`, cargo volume `oo-a192-eef831aa-cargo`, and target volume `oo-a192-eef831aa-target`. Cap drop `ALL`, no new privileges, memory 8g, no host network, no published ports, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`. No PostgreSQL. `CLEANUP.txt` records removal of that network, those volumes, and the task `/tmp` archives after the logs were stored. The pinned image remains. `docker prune` was not run.

Focused-4 capacity before launch: `own=0 global=0 host_cargo=0 cap_global=6`.

| Run | Result |
|---|---|
| `linux/preedit` | Exit 101. Container `249e9f0ce2b5`. Archive omitted `contracts/`, so `empty-catalog.json` was missing. `OOMKilled=false`, `Privileged=false`. Log preserved. |
| `linux/preedit-2` | Exit 0. Container `4ee2c075067a`. `cargo test -p oo-connectors --lib` 64 passed. `absence.exit` 0. `docker-run.exit` 0. |
| `linux/focused` | Exit 101. Container `29250354ee0a`. Four borrow errors in `delimited.rs`. Log preserved. |
| `linux/focused-2` | Lib 64 passed (`test-lib.exit` 0). Delimited test and clippy exit 101. Container `434d1df70433`. SHA-256 state array was an untyped integer. Log preserved. |
| `linux/focused-3` | Lib 64 passed. Delimited 25 passed. Clippy exit 101 on three test-only lints. Container `addbb024faa8`. Log preserved. |
| `linux/focused-4` | Exit 0. Container `96e2fe6bd993`. `--lib` 64 passed. `--test delimited` 25 passed in 0.06s. `cargo clippy -p oo-connectors --all-targets -- -D warnings` exit 0. `Finished dev profile` in 1.24s. `worst_phase_exit=0`. |

Dependency compile printed two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. They are outside this slice. The oo-connectors clippy command still exited 0.

## Hashes

The product object hash in the reader is `sha2`. The integration test compares outputs, lineage, and the digest to a separate safe-Rust SHA-256 (FIPS 180-4) in `tests/delimited.rs`. That oracle self-checks empty (`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`) and `abc` (`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`) on every call. Focused-4 wrote the agreed dumps under `linux/focused-4/oracle/` and the same bytes are in `oracle/`:

| Object | SHA-256 |
|---|---|
| positive CSV | `ece01fdd4319d0aa63a1375818971b24921d27cd1476a5cd3c61f11a08aa8b42` |
| positive TSV | `e8fb7bdbfed915c308432bf31a2d697822dab6080d32107dcb7622de89c9ebc6` |
| `abc` self-check | `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` |

The CSV fixture is 132 bytes, BOM stripped, exact header, null token `\N`. Recorded lineage: row 0 line 2 offset 28, row 1 line 4 offset 78 with an empty note, row 2 line 5 offset 116 with a null name and an empty note at offset 132. The TSV fixture is 29 bytes, BOM refused, header absent. Recorded lineage: row 0 offset 0, row 1 line 2 offset 12.

Launch and `inside.sh` still use `sha256sum` to check the readonly archive identity, the same way the preedit runs did. That check is archive identity. It is not the product object-hash oracle. `MANIFEST.sha256` is the SHA-256 of every file in this evidence directory except itself, computed with Python `hashlib` on the host after the tree was complete. No second Rust builder was started for that manifest.

## What this slice does not do

It does not import, publish, write Arrow IPC, sniff a dialect, decode GB18030, or execute a native payload. `DelimitedComplete` is a parse receipt. Full CSV/TSV format checks, GB18030, DuckDB sniff, Arrow IPC workers, the loader, and publication stay open. Independent review of this commit has not run.
