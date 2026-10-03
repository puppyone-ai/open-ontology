# a127 report — named portal

Session `eef831aa-dba4-4af2-a78b-541bc9e0a127`. The pull-driven named portal is in source commit `182702e6e67d16de10f280eaf422fce4e2f5a5f2` (tree `444c52b8fd431396c77d2e3bf7144696aa9de78a`, parent `aa192ce7c588408d109228de2d27ca7a8f7b3213`). The commit was made from the overlay that `oo-a127-msrv-final` compiled and tested. Slot A then committed `ccd93e1adc34b24ab998d90972549f8cffb59f0d` and its evidence `c05de23d024df902679959e164a8e7cd211165d7`. The ten portal blobs are unchanged through that evidence commit. R2-P stays unchecked pending independent review.

## Identity

| Field | Value |
| --- | --- |
| Fixed input | `b8e7bf1b1c25fbc0f8213898eb7de84b248f8211` |
| Fixed tree | `e8f408d6f66ef5361dc7c6d6185ca7942eaf1819` |
| Fixed parent | `578a55231e4b4373db014ff1450f8e1bb17ba639` |
| Accepted driver | `ff783ac933b86e0ff068181afe0258764c5d2e5d` |
| Source commit | `182702e6e67d16de10f280eaf422fce4e2f5a5f2` |
| Source tree | `444c52b8fd431396c77d2e3bf7144696aa9de78a` |
| Source parent | `aa192ce7c588408d109228de2d27ca7a8f7b3213` |
| Source parent tree | `c61619755547199a828e1ce45ec3c1aba5db7141` |
| Rust image | `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` |
| Rust platform | linux/arm64, rustc 1.94.0 (`4a4ef493e` 2026-03-02), host `aarch64-unknown-linux-gnu` |
| PostgreSQL image | `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` |
| Airgap on the green run | `cargo_offline` against the owned cargo volume filled by the smoke fetch |

`linux/smoke/vendor-match.txt` records the six pre-edit vendor paths as equal across the worktree, `b8e7bf1`, and the accepted driver. After the source commit, `git show b8e7bf1` for those six paths still hashed to the `pre` lines in `linux/final/overlay-sha256.txt`. The ten committed paths hashed equal to the tested snapshot and to the worktree at commit time. Digests and git blobs are in `linkage/source-commit.txt`.

## Commands

Frozen baseline, smoke attempt 2, inside the rust image, online fetch into the owned volume:

```
cargo test -p oo-connectors --lib --locked
cargo clippy -p oo-connectors --all-targets --locked -- -D warnings
```

Green final run, `linux/final/inside.sh`, offline:

```
cargo fetch --manifest-path Cargo.toml --locked --offline
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors \
  --test pg_wire_bounds --test pg_wire_accounting --test pg_portal_bounds --test pg_portal_live \
  -- --test-threads=1
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

The final snapshot was a complete `git archive` of `b8e7bf1` plus the ten overlay files. The workspace was not the compile root.

## Green exits

`linux/final/*.exit` are all 0: toolchain, fetch, snapshot, lib, focused, clippy, clippy-component, docker-run. `console.log` records `elapsed_seconds=44`.

| Suite | Result |
| --- | --- |
| `oo-connectors` lib | 49 passed, 0.00s |
| `pg_portal_bounds` | 6 passed, 26.48s |
| `pg_portal_live` | 1 passed, 0.58s |
| `pg_wire_accounting` | 12 passed, 4.58s |
| `pg_wire_bounds` | 13 passed, 6.07s |
| clippy `-D warnings` | exit 0 |

Portal bounds tests: `allowance_zero_rejected`, `cancelled_pull_poisons_connection`, `default_executor_keeps_unnamed_limit_zero`, `execute_error_resyncs_aborted_transaction`, `fragmented_row_does_not_send_next_execute`, `named_portal_pages_suspend_and_complete`.

Live test: `live_named_portal_repeatable_read`. It covers zero rows, one row (`PortalSuspended`, then a demanded completion with no row), three ordered rows with a concurrent writer and a snapshot oracle, a Bind-time 22012 that rolls back without poison, and a cancelled `pg_sleep` (57014) that does not report success.

`linux/final/sql-log-class.txt`:

```
reader_statements=85
reader_verbs=BEGIN,ROLLBACK,SELECT,SHOW
writer_verbs=BEGIN,COMMIT,INSERT,UPDATE
admin_verbs=DELETE,SELECT,UPDATE
oracle_verbs=BEGIN,ROLLBACK,SELECT,SET
reader_forbidden=0
```

The reader count includes earlier failed attempts in the same PostgreSQL log. The raw log is not in this evidence.

## Preserved non-zero exits

Smoke attempt 1, `linux/smoke/attempt-1`: lib 101, clippy 101, docker-run 101. Cause: the archive omitted `contracts/source/pgsql/empty-catalog.json`. Fetch and toolchain were 0.

Smoke top level (attempt 2): toolchain, fetch, inspect, snapshot, lib, clippy, clippy-component, and docker-run are 0. Library 49 passed. Clippy 0. `elapsed_seconds=6`. `airgap_claim=no`. `portal.rs` was absent. The baseline was not fetched again.

Final attempts, each docker-run 101, kept under `linux/final/attempt-N`:

| Attempt | lib | focused | clippy | Cause |
| --- | --- | --- | --- | --- |
| 1 | 101 | 101 | 101 | E0106 `prefix(self)`; E0308 flush `io::Error` |
| 2 | 0 | 101 | 101 | E0277 `expect_err` needs `Debug` |
| 3 | 0 | 101 | 101 | E0502/E0505 cancel future borrowed the portal |
| 4 | 0 | 101 | 101 | one-row Execute returned `PortalSuspended`; clippy `redundant_closure` |
| 5 | 0 | 101 | 0 | `pg_stat_activity.state` was `active` |
| 6 | 0 | 101 | 0 | `SELECT (1/0)::int4` raised 22012 at open |
| 7 | 0 | 101 | 0 | writer insert id 4 hit 23505 from a prior commit |
| 8 | 0 | 101 | 0 | snapshot id `0000001C-00000002-1` is hexadecimal |
| 9 | 0 | 101 | 0 | `SELECT (1 / $1)::int4` raised 22012 at Bind |
| 10 | 0 | 101 | 101 | E0502 reader borrowed while the transaction lived |

A host launch between attempt 9 and attempt 10 exited before `docker run` on an unbound variable in an unquoted heredoc. It has no attempt directory. The green invocation records `prior_attempt=10`.

## Scope left open

Arrow on the same cut, a source-wide row budget, backend teardown within 5 seconds, the full reader, production catalog probe, and R1 are open. This run does not check R2-P. The ledger, plan, and checklist were not edited. Nothing was pushed.

Own cleanup is in `cleanup.txt`. After it, a127 containers, the a127 network, and the four a127 volumes were gone. a128 and a064 containers were still present. a125 had no containers in that listing.
