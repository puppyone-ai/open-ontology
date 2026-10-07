# file-import-pg report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a308`. Child writer for a replace import of validated CSV, TSV, and JSONL into an unpublished LOGGED PostgreSQL shadow. The path is the reviewed file worker, canonical encoder, binary COPY, and the existing target-generation mapping, fence, and full-import receipts. This report records the private proof. It does not close SP05-W05, AT-FILE-06, or a release.

Fixed archive commit `72dc9e2db841de4de368216bbbe13d738f850ea9`, tree `bf6c8d28dcb47a8dcb1c6cc1e7b6bfc21b963e64`. Proof `repo-head.txt` is `e6ebf93b5ac261c097a24d145611e0300e4f88b5`. The archive was the smoke archive already on disk. It was not rebuilt. Root manifests, the root lock, and migrations were not edited. a313 remains the root writer.

## Final proof

Directory `linux/proof`. Container `c73cdfd60ea2d31bfbeda184754a47eade2207df2fb4a8fec9479215285d1b14`, removed after the run. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0` host `aarch64-unknown-linux-gnu`. PostgreSQL was the existing `oo-a308-pg` on `oo-a308-eef831aa-net`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

`hostconfig.txt`: ExitCode 0, OOMKilled false, Privileged false, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000, NetworkMode `oo-a308-eef831aa-net`. Capacity line: `own=0 global=1 host_cargo=0 cap_global=10 cap_own=1 postgres=1`.

| step | exit |
| --- | --- |
| version | 0 |
| source | 0 |
| sha256-tool | 0 |
| worker-build | 0 |
| worker-hash | 0 |
| refuse | 0 |
| lock-present | 0 |
| lock-subset-rustc | 0 |
| lock-subset | 0 (`lock_subset_ok tool_checksums=250`) |
| clippy-add | 0 |
| clippy | 0 |
| live | 0 |
| lock-stable | 0 |
| lock-installed | 0 |
| docker-run | 0 |

Worker build: `cargo build --locked --offline --bin oo-file-parser-worker -p oo-connectors`. Binary SHA-256 `48b6ab508a044a0419ae476f0aacfae41683392abad2002e65dbf8fa56d58054`. Refuse, which wrote the tool lock: `cargo test --offline --test refuse -- --test-threads=1`. Clippy: `cargo clippy --locked --offline --all-targets -- -D warnings`. Live: `cargo test --locked --offline --test live -- --test-threads=1 --nocapture` with `OO_A308_LIVE=1`. Tool lock SHA-256 from `lock-sha256.txt` is `9874a7ff5c09b333ab1392829334d648f42340a3ba40d7db167c4690c081e89d`. Launch copied that lock into `tools/file-import-pg/Cargo.lock` only because docker exited 0. No cargo update and no generate-lockfile.

`refuse.stdout`: 6 passed in 0.01s (`closed_requests`, `copy_budgets_are_fixed`, `decimal_typmod_and_projection`, `int8_minmax_hash_and_presence`, `path_admission_does_not_open_a_database`, `token_null_and_config_digest`). `live.stdout`: `live_replace_import` 1 passed in 4.66s. The vendor crate `sqlx-postgres` printed two unfulfilled `dead_code` expectations during Clippy and the live compile. The `file-import-pg` Clippy exit is 0.

The pre-overlay smoke in `linux/smoke` was not relaunched. Its `smoke.exit` is 0. Its `fetch-offline` exit is 101, followed by an online fetch exit 0. That 101 is preserved and is not a product failure. The earlier HTTP 402 stop was a quota block, not a product failure.

## What the live test exercised

Fresh database `oo_a308_file` on the already-running private TLS server. `tests/live.rs` created roles `oo_a308_runtime` and `oo_a308_agent`, created the database, and ran `install` plus `install_full_import`. Admin review, role publication, and a live pointer stay outside this child. A successful import returns caller state `awaiting_review`, stored run state `awaiting_publish`, `published` false, and `role_grant` false.

Covered on that database, in one test:

- CSV replace of int8 min and max, numeric `1.50` and `0.00`, bool text `true` and `false`, quoted `say, "hi"`, JSON text `{"a":1}`, an empty string, and SQL NULL. Shadow `relpersistence` is `p`. No PUBLIC grant on the shadow (`grantee = 0` count 0). `pg_roles` count is unchanged. Runtime and agent `SELECT` both return SQLSTATE `42501`.
- Exact replay keeps the same physical table and generation, writes no extra rows, and returns `replay` with no new `run_id`.
- A changed object hash on the same shard returns `hash_conflict` / `receipt`, exit 3. Row count and generation stay put.
- TSV quoted field `hello\tworld` round-trips as that text.
- Token null policy: unquoted `\N` is SQL NULL, quoted `\N` is the two-character text, unquoted empty and quoted empty are empty strings.
- JSONL object `{"a":1}` is stored as JSON text. JSON `null` is SQL NULL.
- Missing JSONL field `payload` returns `missing_field` before a shadow or a new `full_import_run`.
- Rewriting the file after the request hash returns `hash_drift` / `object` with `child_started` false.
- CSV row `a,b,c` against two columns returns `malformed` / `protocol`.
- JSONL `{"id":"t1"` (object cut off before `}`) returns `truncated` / `truncated`, exit 4, with no shadow and no new run.
- CSV int8 `9223372036854775808` returns `malformed` / `protocol`. The worker allowlist collapses that protocol fact to the token `protocol`.
- `pg_terminate_backend` on the open COPY connection removes the backend. The shadow relation is absent afterward. A fresh import of the same file writes one row, `k,kept`.

After each success and refusal, `/proc` has no `oo-file-parser-worker` command line and no `oo-file-parser` comm prefix. That is a direct observation in the builder mount namespace. It is not a sandbox certificate.

## Preserved failures

| directory | result |
| --- | --- |
| `linux/smoke` | pre-overlay smoke, docker path exit 0, fetch-offline 101 |
| `linux/proof-failed-1` | refuse compile 101, subset 0, 250 checksums. OOMKilled false |
| `linux/proof-failed-2` | refuse 101 because `expect_err` needs `Debug` on `ImportRequest`. Subset 0 |
| `linux/proof-failed-3` | refuse 0, clippy 101, live 101 `run_closed` at the deferred shadow trigger |
| `linux/proof-failed-4` | clippy 0, live 101. Import committed. Bool text is `true`/`false`. Database dropped afterward |
| `linux/proof-failed-5` | clippy 0, live 101 at `install failed` because `DROP DATABASE` left `oo_target_owner` and `oo_pipeline_runtime` |
| `linux/proof-failed-6` | clippy 0, refuse 0, subset 0, live 101. Complete JSONL object `{"id":"t1"}` with no newline imported as `src_trunc` run `46362c08-d51d-4bf6-a643-f273d4f4fc86` |

Before the passing run, this session dropped only `oo_a308_file`, revoked `oo_pipeline_runtime` from `oo_a308_runtime`, and dropped `oo_pipeline_runtime` and `oo_target_owner`. `oo_a308_runtime` and `oo_a308_agent` stayed. The passing test created the database and the installer roles again.

## Recipe origin

`linux/proof/recipe-origin.txt` records the a301 launch, inside, lock subset, and PostgreSQL scripts, the smoke scripts, and the sha256 tool, with path and SHA-256. Deltas are in that file: cargo mounted at `/cargo-home`, 6GiB builder, client secrets only, roles created in the test, lock copied back only on docker exit 0.
