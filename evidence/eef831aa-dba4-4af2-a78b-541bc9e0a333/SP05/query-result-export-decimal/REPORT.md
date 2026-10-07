# a333 query-result export decimal repair

Session `eef831aa-dba4-4af2-a78b-541bc9e0a333`. Development repair of FILE-D04 / E44. Fixed commit `63566b60aabdf29a29bfe55c4866edf3c4081cfd`. No new plan child.

Exporter unit and API work is done. The live chain is **WITHHELD**. a327 has not delivered a compiled `query-pg` stdout. This task did not start PostgreSQL and did not poll a327 again. Parent SP05-W06, AT-FILE-05, AT-SEC-03, and release stay open. This report does not accept E44 and does not release.

## What a331 withheld

a331, commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`, withheld E44. `SHA256SUMS` in that review is 163 lines. The withhold is the frozen exact-decimal clause. The exporter had defined exact decimal as the text of an `int` cell and refused kind `decimal`. The int, bool, text, null, formula, sidecar, overwrite, and partial checks held and did not accept the child. U+FEFF was recorded as `unprefixed` and was not the withhold.

## Freeze

`freeze/AMENDMENT.md` was written before the smoke and before product edits. Its SHA-256 is `4154e28f750f2f36d62b4684e4382e0e19fd1e46b1361c504a1f0b49fd86ecd2`. The post-edit container checked that digest again and it matched. The amendment adds kind `decimal` with `negative`, `coefficient`, and `scale`. It does not rename `int`. It does not treat integer text as the exact-decimal cell.

The expected JSON is the cell already frozen by a327:

```json
{"kind":"decimal","negative":false,"coefficient":"123456789012345678","scale":4}
```

Plain text is the `ExactDecimal` display. The CSV copies that text with no apostrophe. The sidecar records `decimal_count` and `decimal_cells` after `int_cells`, each with `row`, `column`, `negative`, `coefficient`, `scale`, and `text`. Null, `source_sha256`, partial completion, formula counts, citation, `http_auth`, and `xlsx` stay. Kind `numeric` stays refused. A non-canonical coefficient or scale is a source failure. The command does not strip zeros and does not clear a negative zero.

U+FEFF is one extra character in the leading formula skip. No other format character was added. One gold text cell is U+FEFF then `=1+1`.

## Producer

One observation, in `producer-observation.txt`. a327's post-edit exit is 2. `seam-query-executor-pg-live.exit` is 101. There is no `REPORT.md` and no query-pg stdout file under the a327 evidence tree. Code commit `c671a3ca` is an ancestor of the fixed commit, and the fixed archive contains the decimal variant in `query-pg.rs`. That is not a delivered compiled stdout. This task did not compile `query-pg` and did not start a database.

## Runs

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each builder was 6442450944 bytes, `NanoCpus` 1000000000, `CapDrop` `["ALL"]`, `Privileged` false. `cargo update` was not invoked. `Cargo.toml` and `Cargo.lock` are unchanged from the fixed commit. No registry dependency was added.

| Run | Top exit | Docker exit | Container |
| --- | --- | --- | --- |
| `linux/smoke` | 0 | 0 | `4128876ef63b859f2372d1663f5b4c0794c7861249be34545cdf9eaa1affeb87` |
| `linux/postedit-failed-1` | 1 | 1 | Clippy component absent. Tests had already exited 0. |
| `linux/postedit-failed-2` | 101 | 101 | `format_in_format_args` in `tests/api.rs`. Tests 22 + 3 exited 0. |
| `linux/postedit` | 0 | 0 | `4c277756ae1b1d8cfe77d46b7da8b0dc910e468e598c6d85f9427767ad90fee8` |

Smoke archive `listed=68428 extracted=68428`. Pre-edit `cargo test --locked --offline` exited 0: 15 tests in `tests/api.rs` and 3 in `tests/cli.rs`. Post-edit tests exited 0: 22 and 3. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0 on the third run.

The decimal tests build the frozen JSON in the test process. They are not compiled `query-pg` receipts. The clerk fixture is still the compiled int/bool/text/null stdout, SHA-256 `54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb`, and its CSV is unchanged.

## Limits

No parent acceptance. No new unit count. No PostgreSQL proof. xlsx, download authorization, retention, deletion, and the reaper remain parent work.
