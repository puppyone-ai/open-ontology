# a245 parse-file command

Development UUID `eef831aa-dba4-4af2-a78b-541bc9e0a245`. This is the Linux proof for the parse-file operator command. It does not accept SP05-W02, AT-FILE-02, or the phase ledger, and it is not a release.

## Pins

| Item | Value |
| --- | --- |
| Fixed archive | `1ddbd865fc77c7a1a6209949336f592107d77683` |
| Fixed tree | `8c7153d196ae1533c1fd568603a2befd66ae95dc` |
| Parent of the contract commit | `cf72579e9bcf37778ff0924fa8656dfffe54df95` |
| Contract commit | `9907d40bc0d2fd684a786cd8f98473d52403fc3e` |
| Command commit | `0a30fcda7fb586bd8d6955e03ae306888374b915` |
| Image | `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` |
| Toolchain | Rust 1.94.0, `aarch64-unknown-linux-gnu`, `RUSTUP_TOOLCHAIN=1.94.0` |
| Builder | one container, 6 GiB, 1 CPU, `CARGO_BUILD_JOBS=1`, `--cap-drop ALL`, no PostgreSQL |

Recipe origin is `recipes/ORIGIN.md`. The a220 build06 and a204 launch hashes are recorded there. This run uses one container, 6 GiB, and 1 CPU. The git object of the fixed archive was not modified. The private extract received the overlay only after the pre-overlay smoke exited 0. `linux/build07/repo-head.txt` records `f6431f5e71853d347ccd7c9d4b71db2bce7e58e2`, which was HEAD when that launch archived the fixed commit. Later commits on master are outside the extract.

## Decisive run

`linux/build07` exited 0. Container id `b34cd5ef639a48821a0decf7c4f2f80b3cc4770a415a715d629f307de6a3fcf8`.

| Phase | Exit |
| --- | --- |
| `docker-run` | 0 |
| offline fetch | 0 |
| archive smoke `file_parser_worker` | 0, 12 passed, 0.96s |
| archive smoke `file_worker_parent` | 0, 8 passed, 1.01s |
| overlay absent, then copy | 0 |
| `parse_file_cli` | 0, 12 passed, 0 failed, 221.04s |
| Clippy component | 0 |
| Clippy `-D warnings` on `--bin parse-file --test parse_file_cli` | 0 |

Host config: `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a245-eef831aa-net`, `OOMKilled=false`. The launch script removed container `oo-a245-build07`. At start, own rust builders were 0, global rust builders were 0, and host cargo was 0.

Overlay SHA-256 from that run matches the source blobs:

| File | SHA-256 |
| --- | --- |
| `parse-file.rs` | `98e56bec8a5b5da33233444cd24e2e7aba48b89040475ec104be8dd72e3ee246` |
| `parse_file_cli.rs` | `ddb3908ac5038cad467dcd73b1954b247d505f9e58a0e4badcca40af223e952a` |
| `parse-file-cli-v1.md` | `73cb1a070998c01edf6a87887678389f4ec6bad74cca07e120af165a02f657aa` |

## Preserved attempts

| Build | docker | test | clippy | What failed |
| --- | --- | --- | --- | --- |
| build01 | 101 | 101 | 101 | `E0277` on `hasher.finalize().into()`, then Clippy `while_let_loop`, `collapsible_if`, `type_complexity`. Offline fetch was 101; the locked online fetch was 0. Smoke was 0. |
| build02 | 101 | 101 | 0 | 11 tests passed. The 1 ms deadline left `term_signal` null where the test required 9. |
| build03 | 101 | 101 | 101 | `E0618`: a local `budget` value shadowed the `budget` function. |
| build04 | 101 | 101 | 0 | `lineage_ceiling`, child exit 8, command exit 5, on a 65536-row batch. |
| build05 | 101 | 101 | 0 | Kill wall 61719 ms against a full parse of 61623 ms. Empty was 9073 ms. |
| build06 | 101 | 101 | 0 | Half-gap kill fired (`kill_elapsed_ms=35861`, `group_signal` `sent`, child exit 7, `term_signal` null). The test still required signal 9. |
| build07 | 0 | 0 | 0 | Decisive run. |

Smoke exited 0 on every build. The archive refused the overlay paths before the copy on every build.

## What the command does

`parse-file` reads one `oo.connectors.parse-file/v1` document from stdin. It converts the profile through `DelimitedDialect::try_from_separator`, `DelimitedColumn::try_new`, `DelimitedProfile::try_new`, `JsonlColumn::try_new`, `JsonlProfile::try_new`, and `PullBudget::jsonl` or `PullBudget::delimited`. It hashes the absolute worker on a separate 120000 ms / 2147483648 byte clock, then calls `ChildRequest::try_new` and `ParentConfig::start`. It pulls with `next_batch` until `Ok(None)`, then `finish`. `ParentParseComplete` is the only success source. The stdout document has lineage counts, byte sums, and object, profile, schema, and config identities. It has no cell text and no input path. `admits_import`, `published`, and `pg_run_complete` are copied from the receipt and stay false. `kernel_restriction` is `unsupported`.

The command does not call `DelimitedReader` or `JsonlReader`. An input SHA that differs from the file still starts the worker, and that call ends as parent `hash_mismatch` with child exit 6. An executable SHA mismatch does not call `start`.

## Cleanup

See `CLEANUP.txt`. The cargo volume, the target volume, the network, and the private extract were removed after build07. No prune.

## Inventory

`tools/sha256_manifest.rs` is the a199 std-only FIPS SHA-256 tool, SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. After the container, the volumes, the network, and the extract were gone, host `rustc 1.98.1` compiled that file and wrote `MANIFEST.sha256`. The tool printed `files=331` and passed the empty and `abc` self-checks. It skips the manifest file itself. The product tests ran in the pinned 1.94 container. This inventory step hashes the evidence tree.
