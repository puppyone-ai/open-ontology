# a325 query-result export

Session `eef831aa-dba4-4af2-a78b-541bc9e0a325`. Fixed source `e8d297f95b32052ea587fe8ad67b6e8493a00300`. The proof compiled `git archive` of that commit plus an overlay of `tools/query-result-export`. It did not compile the moving worktree. Smoke recorded repo HEAD `e6ebf93b5ac261c097a24d145611e0300e4f88b5`. This report does not accept a parent plan item, does not close SP05-W06, AT-FILE-05, or AT-SEC-03, and does not release.

## Owned paths

- `tools/query-result-export/**` — standalone workspace, not a root workspace member
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a325/SP05/query-result-export/**`

Root `Cargo.toml` and root `Cargo.lock` were not edited. FileHost, retention, dialogue, and query-pg sources were not edited. No PostgreSQL was started. The compiled public fixture was sufficient.

## What the tool does

`query-result-export` reads one `oo.query-result-export/v1` request. The caller supplies `source_file`, `output_dir`, and `columns`. The source is one `query-pg` stdout as specified by `contracts/gateway/query-pg-cli-v1.md` at the fixed baseline: JSON lines, row cells of kind `null`, `bool`, `int`, or `text`, then one summary. `int` is canonical decimal text, including `-9223372036854775808`. PostgreSQL `numeric` (OID 1700) is not a cell kind. A `decimal` or `numeric` kind is refused.

The command writes three names under the caller directory, and only those names: `export.source.jsonl` (the exact source bytes), `export.csv`, and `export.sidecar.json`. Creation is `create_new`. An existing name is refused before any of the three is created. A later failure removes only names this call created. Each file is `sync_all`'d. Paths are checked one component at a time. A symlink component is refused. The command does not canonicalize through a symlink.

CSV is UTF-8, no BOM, CRLF, comma-separated, RFC 4180 quoting. The formula transformation id is `apostrophe-prefix-after-leading-whitespace-or-control`. It applies to caller column names and to `text` cells. After a leading run of Unicode White_Space or Cc controls, a next character `=`, `+`, `-`, or `@` gets one U+0027 prefix. Typed `int` values, including negatives, stay canonical digit text. `true` and `false` stay those letters. Null and empty text are both empty CSV fields. The sidecar lists `null_cells` and `empty_text_cells`. `complete` is true only for a clean succeeded summary whose row count and cell-byte sum match. A consistent truncated summary still writes the files, with `complete` false, and the process exits 1 with stderr `query-result-export: partial`. An inconsistent summary writes nothing.

The sidecar copies `original_sha256` and `executed_sha256`. `citation.in_source` is false and `citation.query_id` is null. Stdout has no `query_id`. `http_auth` and `authenticated` are false. `xlsx` is `unsupported`. The baseline has no xlsx writer, and this crate adds no registry dependency.

## Contract and fixture

`tools/query-result-export/CONTRACT.md` was written before the implementation. `tests/fixtures/query-clerk-order_doc-star.stdout` is the compiled query-pg stdout from `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a226/SP11/reviewed-pg-composition/live/steps/query-clerk-order_doc-star.stdout`. Host `fixture-cmp.exit` is 0. Inside `archive-fixture-cmp.exit` is 0. Length 1027. SHA-256 `54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb`.

`tests/fixtures/golden-clerk.csv` is hand-written CRLF bytes, length 171, SHA-256 `9c7c1562db84359e654e50f7f836465062759af12d84f36d52aa6f845c1607f1`. The live container wrote that CSV. `live-csv-cmp.exit` is 0. `live-source-cmp.exit` is 0. `linux/postedit/live-sha256.txt` records the same two digests for the files the container copied out.

`tests/fixtures/hand-formula.jsonl` is hand-authored in the same grammar. It is not a second compiled receipt. Its result byte sum is 128. The unit test checks int8 minimum and maximum, `0`, `-1`, Unicode, quotes, newlines, null, empty text, and the apostrophe prefix. Negative typed integers stay unprefixed. Negative text is prefixed.

`Cargo.lock` is version 4 and names only `query-result-export`. `cargo-update-invoked.txt` is `not-invoked` on smoke and on postedit.

## Toolchain

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. Both runs recorded `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Postedit recorded `cargo 1.94.0 (85eff7c80 2026-01-15)`.

Each builder was 6442450944 bytes, `NanoCpus` 1000000000, `CapDrop` `["ALL"]`, `Privileged` false, network `oo-a325-eef831aa-net`, `OOMKilled` false. `/tmp` was `rw,nosuid,nodev,noexec,size=1g`. `HOME` was `/tmp`. One builder at a time. PostgreSQL image was not started. Smoke capacity was `own=0 global=1 host_cargo=0`. Postedit capacity was `own=0 global=3 host_cargo=0`. Cap is 10.

Recipe origins are in `origin/recipes.txt`.

## Runs

| Run | Top exit | Docker exit | Container |
|---|---|---|---|
| `linux/smoke` | 0 | 0 | `9f52a8541f0b77c9aacef478a80f9ff03e9cad618beab86bad22ecb12b37b821` |
| `linux/postedit` | 0 | 0 | `08e3777ffd898580210affad0e662a19b2eae3fe2fa57686c549e10828f89c8c` |

There is no failed postedit attempt directory. Host `cargo test` and host Clippy on Rust 1.98 were run only while no container from this task existed, and the compiler errors from that pass were fixed before `linux/postedit` launched. The recorded proof is the 1.94 container.

Postedit phase exits are 0 for toolchain, sha256-tool, package-hash, source, component, fetch, test, clippy, build, and live. `test.stdout` shows 15 tests in `tests/api.rs` and 3 tests in `tests/cli.rs`, all passed. Clippy was `cargo clippy --locked --offline --all-targets -- -D warnings`. `clippy.exit` is 0. Fetch was `--locked --offline`. `fetch.exit` is 0. `rustup component add clippy` used the bridge network. That is a toolchain component install, not a crate-registry fetch.

The live binary is `/target/debug/query-result-export`. The complete export exited 0 with empty stdout and empty stderr. A second run on the same directory exited 4 with stderr `query-result-export: path`. `second.exit` is 4. `second-csv-kept.exit` is 0. A copy of the fixture with `truncated` set true exited 1 with stderr `query-result-export: partial`. `partial.exit` is 1. The partial CSV still matched the hand golden. The partial sidecar contains `"complete":false` and `"partial":true`.

The fixed archive stayed without `tools/query-result-export`. `overlay-count.txt` is `archive=63795 proof_without_overlay=63795 overlay=19`.

## Limits

The command does not open a database, does not read SQL, does not write xlsx, and does not authenticate a caller. Download authorization, retention, deletion, and the reaper remain parent work. A truncated export is written and labeled incomplete. It is not a complete result.
