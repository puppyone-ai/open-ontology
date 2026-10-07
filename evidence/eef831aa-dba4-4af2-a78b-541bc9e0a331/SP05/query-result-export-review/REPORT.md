# a331 query-result export review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a331`. Independent review of FILE-D04 / E44. Fixed commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`. The proof compiled `git archive` of that commit. It did not compile the moving worktree and it did not overlay product source. Smoke recorded repo HEAD in `linux/smoke/repo-head.txt`.

Child verdict: **WITHHELD**.

The withhold is the frozen exact-decimal requirement. The int, bool, text, null, formula, sidecar, overwrite, and partial checks held. Those checks do not accept E44. Parent SP05-W06, AT-FILE-05, AT-SEC-03, and release stay open. This report does not accept a parent plan item and does not release.

## Frozen requirement

`specs/open-ontology-v0.1-plan.md` line 1319 at the fixed commit, copied in `source/blobs/e44-plan-line.txt`:

`E44 / FILE-D04` requires a source-query hash, types, null, exact decimal, and truncation sidecar, plus a spreadsheet formula-safe bounded CSV, an actual query fixture, hand gold, and limits. Author `a325`. The development prompt `execution/prompts/v01-query-result-export-active.md` requires the same sidecar contents: exact source hash, types, null, decimal, truncation, and row counts, from an actual compiled query-pg fixture.

The fixed producer does not emit that decimal. `crates/oo-gateway/src/query_executor.rs` `oid_allowed` matches bool, int8, int2, int4, and text. OID 1700 is absent from that allow-list. `decode_cell` returns `TypeRefused` for every other OID. `OutCell` in `crates/oo-gateway/src/bin/query-pg.rs` is `null`, `bool`, `int`, and `text`. The producer contract says `int` is the executor's decimal text for an integer, including the minimum int8 value. Smoke recorded both sites in `linux/smoke/producer-kinds.txt`.

The exporter contract then defines "exact decimal" as the canonical base-10 text of a typed `int` cell, and it refuses a `decimal` or `numeric` kind. That sentence describes the baseline integer text. It does not fulfill the frozen exact-decimal clause. This review does not adopt that narrower reading as a pass.

## What the independent run proved

The compiled public fixture is `tools/query-result-export/tests/fixtures/query-clerk-order_doc-star.stdout`. Inside the archive it matches `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a226/SP11/reviewed-pg-composition/live/steps/query-clerk-order_doc-star.stdout`. `linux/smoke/fixture-cmp.exit` is 0. Length 1027. SHA-256 `54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb`, from `tools/sha256.rs` in `linux/smoke/fixture-sha256.txt`. The bytes contain no `.` and no `"kind":"decimal"` or `"kind":"numeric"`. The amount cells are int text: `-9223372036854775808`, `9223372036854775807`, and `0`.

The reviewer hand gold is `oracle/hand-clerk.csv`, 171 bytes, written from those cells as UTF-8 CSV with CR LF and no BOM. The review oracle compared the compiled binary's `export.csv` to that file. `clerk_csv_eq_hand_gold` is ok. The digest of those bytes is `9c7c1562db84359e654e50f7f836465062759af12d84f36d52aa6f845c1607f1`. The clerk CSV contains no apostrophe. `true,-9223372036854775808,` is present, so the typed int8 minimum stays canonical.

`export.source.jsonl` matches the fixture bytes. The sidecar `source_sha256` is the fixture digest and `csv_sha256` is the CSV digest. `original_sha256` is `2a3ab40de0b44170f24ad916604478c76c2e3b67bb59b280105d56e9893f7750`. `executed_sha256` is `30de27abc8df667f41d05241c326f3f4cd09eefd5961caa81d431d3a2bc4e762`. `null_cells` is `[[1,2],[1,5]]`, `null_count` is 2, and `empty_text_cells` is empty. `citation.in_source` is false and `citation.query_id` is null. `xlsx` is `unsupported`. `http_auth` and `authenticated` are false. `complete` is true and `partial` is false. Stdout and stderr were empty and the process exited 0.

A second run on the same directory exited 4 with stderr `query-result-export: path`. The CSV bytes stayed the hand gold. A directory that already held `export.csv` containing `keep` also exited 4. That file stayed `keep`, and the sidecar and source copy were not created.

A copy of the fixture with `"truncated":true` exited 1 with stderr `query-result-export: partial`. The CSV still matched the hand gold. The sidecar has `"complete":false`, `"partial":true`, and `"truncated":true`.

The committed `hand-formula.jsonl` is hand grammar in the query-pg shape. It is not a second compiled receipt. The compiled binary prefixed text after space, tab, CR, LF, SOH (U+0001), NBSP (U+00A0), and U+3000. Typed `-9223372036854775808` and typed `-1` stayed unprefixed (`0,-1,'=1+1`). Text `-2+3` was prefixed. Quoted quotes and an embedded newline stayed RFC 4180. The sidecar `null_cells` is `[[0,19]]`, `empty_text_cells` is `[[0,18]]`, and `formula_prefixed` is 11.

A hand-grammar `decimal` cell `"1.50"` and a hand-grammar `numeric` cell `"1.50"` each exited 5 with stderr `query-result-export: source` and left no artifact. Those inputs are not compiled receipts. They show the deliberate refusal. The compiled fixture never supplied a decimal cell to export.

## U+FEFF boundary

One hand-grammar text cell is U+FEFF followed by `=1+1`. It is not a compiled receipt. The CSV bytes are `note` CR LF, then `EF BB BF 3D 31 2B 31`, then CR LF. `linux/proof/oracle-out/feff-class.txt` is `unprefixed`. The clerk CSV does not start with a file-level BOM.

`REQ-SEC-02` requires a prefix for `= + - @` and for leading whitespace or a control character. U+FEFF is category Cf. `formula_prone` skips Unicode White_Space and `char::is_control` (Cc). It does not skip U+FEFF. The observed bytes match that predicate. This review records the boundary and does not open a further character campaign.

The smallest change that would prefix this cell is to treat U+FEFF as one more leading character in `formula_prone` in `tools/query-result-export/src/csv.rs`. That edit was not made. Product files stayed read-only. The FEFF result is not the reason for WITHHELD.

## Author tests and toolchain

`cargo test --locked --offline --no-fail-fast -- --test-threads=1` exited 0. `tests/api.rs` ran 15 tests, all passed. `tests/cli.rs` ran 3 tests, all passed. Empty lib and bin targets ran 0 tests. `test.exit` is 0. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. `clippy.exit` is 0. `cargo fetch --locked --offline` exited 0. `cargo-update-invoked.txt` is `not-invoked` on smoke and on proof. `Cargo.lock` in the archive names only `query-result-export`.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Both runs recorded `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each builder was 6442450944 bytes, `NanoCpus` 1000000000, `CapDrop` `["ALL"]`, `Privileged` false, network `oo-a331-eef831aa-net`. `/tmp` was `rw,nosuid,nodev,noexec,size=1g`. `HOME` was `/tmp`. `/src` was read-only. One builder at a time. PostgreSQL was not started. Smoke and proof capacity were `own=0 global=0 host_cargo=0`. Cap is 10. Archive file count was `listed=66095 extracted=66095`.

| Run | Top exit | Docker exit | Container |
|---|---|---|---|
| `linux/smoke` | 0 | 0 | `a807d442e09beb94e50554b9ba3f9ab78fbe32ef70ab8b19a4783e36d9b3a2ae` |
| `linux/proof` | 0 | 0 | `b7245b71806f170795993ef24090f383110f4dfa4b08ec76e5664972807b0b88` |

The package manifest of `tools/query-result-export` at the fixed commit is `linux/proof/package-sha256.txt`. The Rust tool's stamp of that manifest is `c53ecce730033363051149ef6605af457e2c03f1910f82b3cb9b718c0d228f87`. Source digests for the producer contract, `query-pg.rs`, `docs/10-security.md`, the exporter contract, `csv.rs`, and `lib.rs` are in `linux/smoke/source-sha256.txt`. Blobs are in `source/blobs/`.

Recipe origins and the hashes of this review's scripts are in `origin/recipes.txt`.

## Smallest fix for the withhold

Do not rename i64 text to exact decimal. An exporter-only `decimal` kind would describe bytes `query-pg` does not emit, and this review did not fabricate a producer.

The change that meets the frozen clause is a compiled `query-pg` stdout cell that carries an exact decimal, coefficient and scale, not a float and not an i64, plus an exporter arm that copies that canonical text into the CSV without formula-prefixing a typed number and records it in the sidecar. Producer OID 1700 is outside this review. Until that compiled stdout exists, E44's exact-decimal clause is unfulfilled. No edit was made to product source.

## Limits

The command does not open a database, does not read SQL, does not write xlsx, and does not authenticate a caller. Download authorization, retention, deletion, and the reaper remain parent work. A truncated export is written and labeled incomplete. This review adds no unit count and no rework count.
