# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a386`. Plan item SP09-W05.

## Verdict

This card ran the local path from a fresh private PostgreSQL export through Parquet, Arrow IPC, the pinned DuckDB CLI, formula-safe CSV, Excel written as text, a Chinese SVG with the pinned font, a source manifest, early cancel, and a missing-CLI failure. `closes_sp09_w05` is false in every manifest. `audited` is false. `reviewers_assigned` is empty. This card does not sign itself, and it does not accept SP09, AT-API-04, or the parent goal. Codex updates the ledger. The ledger was not edited.

`tools/local-analysis/DEPENDENCY-PROPOSAL.md` records the pinned child dependencies. It is a proposal. It is not an owner audit and it does not edit `supply-chain/dependency-baseline.json`.

`linux/chain/` keeps the earlier `rust_exact` run, whose `analysis-exits.txt` ends `parquet=3 xlsx=3 engine=3`. That record is unchanged. `linux/chain-full/` is the later run that writes the real files.

## Producers

Producer commit `21cc96241a841b47dbbcb116549030e38a643ba0`. Exporter commit `62b4afecdd377fa94310f46467c48a01e61d14db`. Both were `git archive` inputs. Root lock before and after both chains is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.

The child lock is `tools/local-analysis/Cargo.lock`. `linux/full-test/generate-lockfile.exit` is 0. It was not regenerated afterward. Direct checksums match the proposal: arrow `234ce58e76c04e8162c2e4bc3fe53fa7c2418ef5fba7bb2838b8531b7d4fbf14`, parquet `8af83d2940bc0510f9aef86d865f56fdc6095f87ab115ac885a80b7c5226d3ba`, rust_xlsxwriter `34f2d2cda7223168a14fb86bc7411dbc3379bd7d49749ecc53b91ed98585b6b0`, zip `4a243cfad17427fc077f529da5a95abe4e94fd2bfdb601611870a6557cc67657`. Features stay `arrow` ipc, `parquet` arrow+snap, `rust_xlsxwriter` with default features off, and dev `zip` 8.3.0 deflate. No brotli, zstd, lz4, or C zlib feature is enabled.

DuckDB is the upstream CLI 1.5.6 linux-arm64 glibc asset, archive SHA-256 `1af0541e649a3ae34eb20eec2ca564308bf0e8993e5d6dba6be77bfb289a2718`, unpacked SHA-256 `c0ac0b79e243ee312b3af29c34ac0b94e518aa44c4345036d5d96c1c042c9d48`. The font is Noto Sans CJK SC Regular from Sans2.004, OTF SHA-256 `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`. Those binaries live on the private volume and are not in git. The chart copy’s digest is in this report and in `SHA256SUMS`. The 16 MiB file is gitignored.

## Private PostgreSQL

The second `pg/up.exit` is 0. Server version `17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a386-pg`, id `29fe08709fa2a61f249265a4a4b817b4eba58243d69067bd0942b474c257ef1d`. Memory 1073741824, NanoCpus 1000000000, privileged false, network `oo-a386-eef831aa-net`, no published port. CapDrop ALL, with CHOWN, DAC_OVERRIDE, FOWNER, SETGID, and SETUID added for the database entrypoint. SAN `DNS:oo-a386-pg`. The first container id `42b2fa236b839fa3d95d39cc9f1ed5394947d40a8f933e215483280496a2720e` is in `pg/first-run/`. Secrets stayed under `/tmp/oo-a386-eef831aa-dba4-4af2-a78b-541bc9e0a386/secrets` until cleanup. A scan of this evidence tree found no password or private-key bytes. The public SAN template in `pg/up.sh` is the text that script writes to `san.cnf`.

## Focused Rust run

`linux/full-test/test.exit` is 0. `linux/full-test/clippy.exit` is 0. `linux/full-test/fetch.exit` is 0. `components.txt` lists `clippy-aarch64-unknown-linux-gnu`. The suite is 8 library tests, 8 API tests, and 3 CLI tests, including the pinned DuckDB sum and `prove_native_cancel`. HostConfig for `oo-a386-full-test`: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a386-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000. The native volume mount was read-only.

Earlier attempts stay in `linux/full-test/failed-attempt-1` (offline test before fetch, exit 101) and `failed-attempt-2` (`Cursor` is not a Parquet `ChunkReader`, plus Clippy `-D warnings`). `linux/test/clippy.exit` remains 1 from the image that had no Clippy component.

## Live chain

`linux/chain-full/docker-run.exit` is 0. `linux/chain-full/chain.exit` is 0. `linux/chain-full/analysis-proof.txt` is `ok`. `linux/chain-full/clippy.exit` is 0. The successful launch took about 24 seconds on the warm cargo and target volumes. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a386-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000. Capacity before launch: host cargo 0, rust builders 0. `test-live.log` records `test a327_live::live_numeric_proof ... ok`. Phase exits under `linux/chain-full/phases/` are 0.

`analysis-exits.txt`:

```
amount=0 wide=0 partial=1 cancel=1 engine=3
```

Amount is the full request: `engine=duckdb`, `parquet=true`, `xlsx=true`, pinned CLI `/native/duckdb`, pinned font. Stdout and stderr for that command are empty. CSV SHA-256 `2edb40e84bc45e0391474c745933839d410282614c337b164ce66f53476a9a8a` (95 bytes), the same bytes as the earlier exact sum. SVG SHA-256 `c30c555c31a51c5c2eef7eb1aa9faf7e9f085a21a1d2d218ea1796fb3a42ffb1`. It contains `数值已核对`, `DuckDB合计与精确十进制一致`, `字体已校验`, and one `http` URL, the SVG namespace. Parquet starts with `PAR1`, SHA-256 `6aa8df17972af47011a70b3740d54b29b59b0a6744ccb4ae1ea9b2f7d62bbdd6`. Arrow contains `ARROW1`, SHA-256 `710459e4473d5b282d841a2f01b1652903b17d1b57f9f70b553ce4269efbe462`. Workbook SHA-256 `004ea50935522fd618546e3b7d70eaf61202464046c407d2559777e8885361c0`. `xlsx-scan.exit` is 0 and `xlsx-scan.txt` is `xlsx_scan_ok`: the inflated XML contains `12345678901234.5678` and contains no `<f>` formula element. Manifest SHA-256 `91301d61c94bc872558bc18feeb592683b6bf26fe49b4b6adf641555cc8f0c80`. Query id `q_49b26b99a4752eca`. Engine `duckdb`. Statuses `pinned_unaudited`. `audited` false. `closes_sp09_w05` false. DuckDB binary SHA-256 in the manifest is the unpacked CLI digest above.

Wide stays `rust_exact` and keeps the 38-digit values. Partial stderr is `local-analysis: uncited`. Cancel stderr is `local-analysis: canceled` and the cancel directory has no files. A DuckDB request without a CLI exits 3 with `local-analysis: engine` and leaves `keep.txt`.

The first `linux/chain-full` launch exited 1 after about 358 seconds. Every producer phase was 0 and the five analysis commands had already returned `amount=0 wide=0 partial=1 cancel=1 engine=3`. The proof stopped at `xlsx-amount` because `grep` cannot see DEFLATE XML. That console and the small outputs are in `linux/chain-full/failed-attempt-1/`.

This run’s complete stdout is 1527 bytes, SHA-256 `15ba2ac4a9ac5e86218741a119cb944e0098f88137c4292eb6aceeed2f5b4001`. Export CSV is 208 bytes, SHA-256 `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`. `gold-complete.txt` says `gold_match=1 kind=complete`. Audit `audits/amounts.jsonl` is 1044 bytes, SHA-256 `b0579351099153df2a5076c4744018f6fe3d71bbfb7dfe590aee3289e3307bb5`. The binary is `/target/la/debug/local-analysis`, SHA-256 `0ddd38f7bb1f0cf73eaa75abab77ccc07f0b09b2689d3948186ce773e587f5ea`.

Charts to open: `linux/chain-full/out/analysis/analysis.svg` and `linux/chain-full/out/analysis-wide/analysis.svg`. The font file beside the first chart is `NotoSansCJKsc-Regular.otf`.
