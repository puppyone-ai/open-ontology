# SP07 structure draft

Task a063, session `eef831aa-dba4-4af2-a78b-541bc9e0a063`. This commit adds a pure catalog-to-reviewable-structure compiler in `oo-builder`. It does not finish SP07, and it does not finish the vertical flow.

The parent of this commit is `b14c7821031990aafd713f23068658d41042ee2d`, also written in `parent.txt` immediately before `git commit`. The prompt named baseline `275d886896de1e011a4ca4de49035adbdfe15aa9`. That commit is an ancestor of the parent. This report is inside the task commit and does not name the task commit.

## What landed

`compile_structure_draft` reads one `oo.builder.catalog-evidence/v1` envelope and returns canonical `oo.builder.structure-draft/v1` bytes with no trailing newline. The `structure-draft` subcommand does the same from stdin to stdout. No arguments. Exit 0 on success, exit 1 on a draft error, exit 2 when the command is missing or unknown. A bare `oo-builder` still prints the SP00-W01 skeleton line and exits 2.

The only adapter in this slice is `pgsql`. It parses an already captured `oo.source.pgsql/v1` document. The field set and `pg-catalog-hash/v1` preimage follow frozen commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. Provenance records `pg_catalog_integration: not_accepted`. There is no `oo-connectors` dependency, no source connection, no model call, no publication, no grant, and no certification.

Generated, curated, and effective layers stay separate. Known primary keys and foreign keys are structural facts and tentative `declared_fk` candidates. PII and semantic type stay unknown and unpublished. Review decisions bind to the schema hash and the fact fingerprint. A changed schema hash returns the item to pending and records `stale_decision`. Curated `pii` and `entity` stay in the curated echo.

`Cargo.lock` adds `serde` 1.0.229 and `serde_json` 1.0.151 to `oo-builder` only. Those versions were already in the lock. `sha2` is not a direct dependency. Digests go through `oo_core::api::payload_sha256`. Crate edges stay `oo-core` and `oo-jobs`. `oo-core` was not edited.

The contract is `contracts/builder/structure-draft/README.md`. Synthetic fixtures and their digests are `examples/ontology-draft/`.

## Checks

Host, after the final sources, captured in `host/console.log`:

```
rustc 1.98.1 (48a229cea 2026-09-01) (Homebrew)
commit-hash: 48a229ceaefd4985c50990b14116b6d856af0985
host: aarch64-apple-darwin
cargo test -p oo-builder --locked --offline --all-targets -- --test-threads=8
test_exit=0
cargo clippy -p oo-builder --all-targets --locked --offline -- -D warnings
clippy_exit=0
```

Lib tests: 18 passed, 0 failed. Binary unit tests: 0. CLI tests: 2 passed, 0 failed. An earlier Clippy run on the same tree failed with `clippy::too_many_arguments` on a test helper (`CLIPPY_EXIT:101`). That helper now takes one column struct. The captured Clippy command is the run after that change.

Linux arm64 MSRV, run `136df3eb-c925-48f6-ab21-ab1c9bfe1a56`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, bridge network, no privilege, no host network, no home mount, no socket mount, private cargo and target volumes. `RUSTUP_TOOLCHAIN=1.94.0`. Log: `msrv/console.log`.

```
rustc 1.94.0 (4a4ef493e 2026-03-02)
commit-hash: 4a4ef493e3a1488c6e321570238084b38948f6db
host: aarch64-unknown-linux-gnu
clippy_component_exit=0
fetch_exit=0
test_exit=0
clippy_exit=0
worst_phase_exit=0
```

`msrv/docker-run.exit` is `0`. `msrv/inspect.txt` is `ExitCode=0 Status=exited OOMKilled=false Error=`. The container commands were `cargo fetch --locked`, `cargo test --locked --offline -p oo-builder --all-targets -- --test-threads=8`, and `cargo clippy --locked --offline -p oo-builder --all-targets -- -D warnings`. Same counts: 18, 0, and 2 passed.

The pre-implementation smoke is `pre-smoke/`, run `ce854398-0ec1-4c91-bcdc-1c57421f7627`. Attempt 1 exited 101 because cargo 1.94 `fetch` rejects `-p`. Attempt 2 used `cargo fetch --locked` and the container exited 0. The host wrapper then hit zsh `read-only variable: status` and exited 1. `attempt-2.docker-run.exit` is `0` and `attempt-2.inspect.exit` records the container exit 0. That wrapper failure is not a test failure. The post-implementation script does not assign `status`.

After `msrv/inspect.txt` was saved, the a063 container, network, and both volumes for `136df3eb-c925-48f6-ab21-ab1c9bfe1a56` were removed, as were the pre-smoke container, network, and volumes for `ce854398-0ec1-4c91-bcdc-1c57421f7627`. Labels were checked before each removal: owner `a063`, session `eef831aa-dba4-4af2-a78b-541bc9e0a063`. No other project's containers were removed.

## Golden digests

Empty schema preimage `{"tables":[]}` SHA-256 `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc`. One-column `public.customer.id` `int4` schema hash `60f7939195cc539644d11132ae4a604c3b025167f1fe657d121167feb8463499`. Both are asserted on the compiled drafts.

| Draft | SHA-256 |
|---|---|
| `examples/ontology-draft/goldens/empty.draft.json` | `47e31be7ac19a1fa2cdd862be4dfa21de3520de23e7693ad779ccd175f69add1` |
| `examples/ontology-draft/goldens/customer.draft.json` | `486a22960ec2c759455bf26c1d970e44dc486a5e7225a38ec4d6c08b876cb106` |
| `examples/ontology-draft/goldens/shop.draft.json` | `a7afbcc681086049687571a0a46f4592c227e30fdf837ffd8177e5a87505fa72` |
| `examples/ontology-draft/goldens/reject.draft.json` | `b78970ba6760c7b9322db13836cd6968a23f22e6a4c958877917e8065dd22889` |
| `examples/ontology-draft/goldens/stale-reject.draft.json` | `165e8bbf51172424f31a4a2b0915fb247b1d5e0a12bdca96c725e3ceb9d0f207` |
| `examples/ontology-draft/goldens/accept.draft.json` | `847f05eebe459b37cd11e6357b538a7af387d4a9f88e2b42b4ae580dbdd97324` |
| `examples/ontology-draft/goldens/curated.draft.json` | `255407e0a8ce9af92315594615e174482f3296263d6e330a3240031e9427a4ea` |

`examples/ontology-draft/MANIFEST.sha256` lists every fixture. Customer manifest hash inside the draft is `3ce302dde9df01663e4de5a77708789ce02c0002d02a1f96be2c73750a219920`.

## Still open

SP07-W01 overlay and semantic diff, including AT-OOS-01, beyond the core merge used here. SP07-W02 profile, cache, sample, time window, and statistics. SP07-W03 rules, PII, enums, expert cards, and auto-accept. SP07-W04 model proposals. SP07-W05 review API, console, owner RBAC, and classification publication. SP07-W06 role packs. Real PostgreSQL import, approved-draft integration, and query integration are separate. The pinned catalog contract has not been accepted as a driver integration. No publication, grant, or certification is performed here.
