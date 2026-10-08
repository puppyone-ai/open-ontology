# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a401`. Plan item SP09-W05.

## Verdict

This review ran the local path from a fresh private PostgreSQL export through Parquet, Arrow IPC, the pinned DuckDB CLI, formula-safe CSV, an Excel workbook of text cells, a Chinese SVG with the pinned font, a source manifest, early cancel, and a missing-CLI failure. Every published manifest has `closes_sp09_w05` false and `audited` false. `reviewers_assigned` is empty. The dependency proposal was reviewed and was not treated as an owner audit or a baseline edit. This commit does not accept SP09, AT-API-04, or the parent goal. The ledger was not edited.

The product tree, including `tools/local-analysis`, was not modified. The overlay tests lived only in this evidence directory and in a private archive copy.

## Fixed inputs

Local-analysis commit `e2732e323915a332a85f008c605b50c1f4d5e948`. Its `tools/local-analysis` subtree is `21d95d0f8f16aa8bc464af8f00763b907e650869`. The commit root tree recorded at launch is `a45f977401154ba535d6a1ffc483edbde5b8fa2f`. Producer `21cc96241a841b47dbbcb116549030e38a643ba0`. Exporter `62b4afecdd377fa94310f46467c48a01e61d14db`. Root lock before and after the chain is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. `cargo update` was not invoked.

Direct lock checksums match the proposal: arrow `234ce58e76c04e8162c2e4bc3fe53fa7c2418ef5fba7bb2838b8531b7d4fbf14` (ipc, MSRV 1.88, Apache-2.0), parquet `8af83d2940bc0510f9aef86d865f56fdc6095f87ab115ac885a80b7c5226d3ba` (arrow+snap, MSRV 1.88, Apache-2.0), rust_xlsxwriter `34f2d2cda7223168a14fb86bc7411dbc3379bd7d49749ecc53b91ed98585b6b0` (default features off, MSRV 1.88.0, MIT OR Apache-2.0, chosen Apache-2.0), zip `4a243cfad17427fc077f529da5a95abe4e94fd2bfdb601611870a6557cc67657` (dev-only, deflate, MSRV 1.88, MIT). No brotli, zstd, lz4, or C zlib feature is enabled. License texts were not re-fetched, so those license names are the proposal’s statements.

DuckDB is the upstream CLI 1.5.6 linux-arm64 asset, not a Rust crate. Archive SHA-256 `1af0541e649a3ae34eb20eec2ca564308bf0e8993e5d6dba6be77bfb289a2718` checked inside the builder. Unpacked SHA-256 `c0ac0b79e243ee312b3af29c34ac0b94e518aa44c4345036d5d96c1c042c9d48`. Version line `v1.5.6 (Variegata) 069cc9f9b5`. MIT. The font is Noto Sans CJK SC Regular from Sans2.004. Archive SHA-256 `a927e56f53bd6c3b920bc139c0b94aa36c7d9ad0cf009b159437a1a003581140`. OTF SHA-256 `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`. OFL-1.1. The 16 MiB OTF copy under `linux/proof/out/analysis/` is gitignored. Ages at adoption `2026-10-08T05:14:59Z` are in `source/dependency-age.txt`. None are under seven days. `scan_recorded` is false.

## Smoke and PostgreSQL

`linux/smoke/smoke.exit` is 0, recorded before the overlay and before the proof container. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode none, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000. The smoke container was removed.

`pg/up.exit` is 0. Server version `17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a401-pg`, id `fabf791cfe6badfbc946922d224eef5694ba84d0f51d2d85c217e443a8c567b1`. Memory 1073741824, NanoCpus 1000000000, privileged false, network `oo-a401-eef831aa-net`, `ports.json` is `{}`. CapDrop ALL, with CHOWN, DAC_OVERRIDE, FOWNER, SETGID, and SETUID added for the database entrypoint. SAN `DNS:oo-a401-pg`. Secrets stayed under `/tmp/oo-a401-eef831aa-dba4-4af2-a78b-541bc9e0a401/secrets` until cleanup.

## Chain

`linux/proof/analysis-proof.txt` is `ok`. `analysis-exits.txt` is `amount=0 wide=0 partial=1 cancel=1 engine=3 formula=0`. `phases/test-live.exit` is 0 and `test-live.log` records `test a327_live::live_numeric_proof ... ok`. Gold checks are `gold_match=1` for complete, partial, and formula. The live harness also recorded the expected negative command exits: `import-pg-not-ready` 1, `before-analyze` 5, `hidden` 6, `provision-loose` 7, `provision-over` 7.

This run’s stdout is 1527 bytes, SHA-256 `f1f1c1964c646475ff6a1a97363dc99c59a68abd4fb26d5dd2961b7ed3705b55`. Export CSV is 208 bytes, SHA-256 `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`. Audit `audits/amounts.jsonl` is 1044 bytes, SHA-256 `ed4819d6c664163bb1aa68cb61355ee5e5c0efcca41603886787aeb789d544c5`. Query id `q_f714069af89b93f6`. Role `oo_r_clerk`. Project `a327num`. The manifest `source_sha256` is this stdout. The analysis binary SHA-256 is `9ed228df101022b1db8407ad8a9bece31310ccaa804727b726614f0456ee9ffc`.

Amount request: engine `duckdb`, parquet and xlsx true, CLI `/native/duckdb`, font file set. Stdout and stderr are empty. CSV SHA-256 `2edb40e84bc45e0391474c745933839d410282614c337b164ce66f53476a9a8a` (95 bytes):

```
entry_id,amount
1,12345678901234.5678
2,-12345678901234.5678
3,0.0001
4,-0.0001
5,0.0000
```

SVG SHA-256 `14713a18002882b1dceda25478003f94a3a28ea08ad63063e0aaca679136f17f` (2044 bytes). It contains `数值已核对`, `DuckDB合计与精确十进制一致`, and `字体已校验`. It does not contain `不可引用`, `script`, `foreignObject`, or `href`. The only `http` URL is the SVG namespace. Parquet starts with `PAR1`, SHA-256 `6aa8df17972af47011a70b3740d54b29b59b0a6744ccb4ae1ea9b2f7d62bbdd6`. Arrow contains `ARROW1`, SHA-256 `710459e4473d5b282d841a2f01b1652903b17d1b57f9f70b553ce4269efbe462`. Workbook starts with `PK`, SHA-256 `4b9f32937a679e5a7f46d235c54997d8c1921d925d0b5ff54412d778fc5bb1a2`. `xlsx-scan.exit` is 0 and `xlsx-scan.txt` is `xlsx_scan_ok`. Manifest SHA-256 `bd9db7310ee2803c34b7529797cb5b42e61d650875f75a485ea48797c643e0fd`. Engine `duckdb`. Statuses `pinned_unaudited`. `null_values` 1, `null_groups` 0, `group_count` 5. Warnings include `admission_not_audited` and the pinned-unaudited names. DuckDB binary SHA-256 in the manifest is the unpacked CLI digest. `citable` is true. `truncated` is false. `authorization` is `not_claimed`.

Wide stays `rust_exact` with parquet true and DuckDB `not_invoked`. CSV SHA-256 `813f62ae845d4acf058403b60dcdb70e4e9af32ede60d850db045e555afbbb3f` keeps both 38-digit values, and the typed minus has no apostrophe. Partial stderr is `local-analysis: uncited`. Its SVG `<desc>` starts `不可引用` and does not contain `数值已核对`. `citable` is false and `truncated` is true. Cancel stderr is `local-analysis: canceled` and `out/analysis-cancel` has no files. A DuckDB request without a CLI exits 3 with `local-analysis: engine` and leaves `keep.txt`. Formula CSV SHA-256 `635fe3e16e0690eb0ee43096c793cc9f767c33b293fc574efdda12c5e41c2488` starts `entry_id,'-wide` and does not prefix the typed minus. `xlsx-formula.exit` is 0 and `xlsx-formula.txt` is `xlsx_scan_ok` for the needle `'-wide`.

## Focused test and Clippy

`linux/proof/focused-test-final.exit` is 0. Seven tests passed in `tests/a401_bounds.rs`: formula apostrophe versus typed minus, native cancel with no leftover temp directory, null group and null value, the real export, a tampered query id, a 39-digit coefficient refused as `engine` with parquet true, and an uncited truncated synthetic. The 39-digit case and the text key `=总额` are boundary oracles. The citable path is the fresh PostgreSQL export.

`linux/proof/clippy.exit` is 0. The command was `cargo clippy --locked --offline --all-targets -D warnings` on this package only. `rustup component add clippy` ran inside the final builder. The final log is the incremental check after the overlay fix: `Finished dev profile in 0.54s` with no warnings.

The analysis container’s own HostConfig, preserved in `failed-attempts/04-clippy-needless-borrow/hostconfig.txt`, is ExitCode 101, OOMKilled false, Privileged false, NetworkMode `oo-a401-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000. That 101 is the first Clippy miss. The final Clippy container is `linux/proof/hostconfig-clippy.txt`: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a401-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000. Final `chain.exit` and `docker-run.exit` are 0. Capacity at the analysis launch was host cargo 0 and rust builders 0.

## Failed attempts

- `linux/proof/failed-attempts/01-offline-before-fetch`: `cargo build --offline` for local-analysis exited 101 because `parquet` was not in the producer registry. The query and gold checks had already passed. The retry added `cargo fetch --locked` for this package. No lockfile change.
- `linux/proof/failed-attempts/02-docker-bind-invisible`: Docker exited 125. The recreated `/tmp` archive path was not visible to the daemon.
- `linux/proof/failed-attempts/03-cached-bind-path`: five more 125s on that same path. A new directory name was visible. Later launches stage the archives onto a fresh path.
- `linux/proof/failed-attempts/04-clippy-needless-borrow`: analysis proof and the seven tests had passed. Clippy `-D warnings` exited 101 at `tests/a401_bounds.rs:436` (`needless_borrows_for_generic_args`). The overlay dropped the extra borrow.
- `linux/proof/failed-attempts/05-resume-not-executable`: the host got exit 126. No container started. The next invocation was `bash clippy-resume.sh`.
- `linux/proof/failed-attempts/06-clippy-component-missing`: the seven tests passed again. Clippy was absent from the fresh image and the network was `none`, so the component could not be installed. Exit 1. The final resume installs Clippy on the private bridge and does not mount secrets.

## Dependency review

Pins, checksums, features, MSRV, and ages match `tools/local-analysis/DEPENDENCY-PROPOSAL.md` and `source/dependency-age.txt`. Tool `rust-version` is 1.94, which is above the crate MSRV values 1.88 and 1.88.0. DuckDB and the font are not crates. `pinned_unaudited` is the manifest status, not an audit.

Separate gap: `scan_recorded` is false. This review did not run `cargo audit` or `cargo deny`. `reviewers_assigned` is empty. `audited` is false. `exception` is null. `baseline_edited` is no. The proposal is not a signed release and not an owner audit.

## Cleanup

Removed `oo-a401-pg`, volumes `oo-a401-eef831aa-cargo`, `oo-a401-eef831aa-target`, `oo-a401-eef831aa-native`, and `oo-a401-eef831aa-pgdata`, network `oo-a401-eef831aa-net`, and `/tmp/oo-a401-eef831aa-dba4-4af2-a78b-541bc9e0a401`. The proof containers were already removed by their launch scripts. No prune. Other owners' containers were left running.

Charts to open: `linux/proof/out/analysis/analysis.svg` and `linux/proof/out/analysis-wide/analysis.svg`. The font file beside the amount chart is `NotoSansCJKsc-Regular.otf`.
