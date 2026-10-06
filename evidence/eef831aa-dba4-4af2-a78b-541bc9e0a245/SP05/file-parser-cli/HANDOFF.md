# Handoff

Contract commit `9907d40bc0d2fd684a786cd8f98473d52403fc3e` on parent `cf72579e9bcf37778ff0924fa8656dfffe54df95`. Command commit `0a30fcda7fb586bd8d6955e03ae306888374b915`. Fixed archive `1ddbd865fc77c7a1a6209949336f592107d77683`. Decisive proof `linux/build07` exited 0: worker smoke 12, parent smoke 8, `parse_file_cli` 12, Clippy 0. One 6 GiB / 1 CPU builder. No PostgreSQL. Containers, the cargo volume, the target volume, the network, and the private extract were removed after that exit. See `CLEANUP.txt`.

Owned paths:

- `crates/oo-connectors/src/bin/parse-file.rs`
- `crates/oo-connectors/tests/parse_file_cli.rs`
- `contracts/connectors/parse-file-cli-v1.md`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a245/SP05/file-parser-cli/**`

The connector manifest, the root lock, `file_worker_parent`, and `oo-file-parser-worker` were not edited. Cargo discovers `src/bin/parse-file.rs` without a `[[bin]]` entry.

SP05-W02, AT-FILE-02, and the phase ledger stay open. Nothing here accepts them. `KernelRestriction::Unsupported` stays in force. A later independent review can use build07 and the source blobs; this note is not that review.

The next call still has to supply the absolute compiled worker, its SHA-256, the caller-admitted file and SHA-256, a profile, a budget, and a parent deadline in 1..=120000 ms. The summary is not an import.
