# DL01-W1-36 handoff

Approved extension: sealed-import compiler on DL01-W1-05 at `b1ffb079d7beec4297dbd3deec97219584c6775d`, seal `b0e64601a24d4a824fe30ed32cba038deb811ee7`. Reviewer session `025e07bb-e26b-4c0e-8b43-a80ac0f48dda`. Packet: `evidence/025e07bb-e26b-4c0e-8b43-a80ac0f48dda/DL01/semantics-review`.

`cb5b71e37` approved source `5bd6e414bf0b3ca008dc413fb39f2c6d0aa8bae5` only. The files in this packet replace that review text. Coordinator acceptance of parent DBX-W05 is still open. The acceptance count stays 0/8. This review does not mark the parent, the support matrix, a source grade, a 14-night run, or a release.

## What the next reader can trust

The compile input was archive `577160c22de63adb9b38e0147f3bc2da893c4d9d` plus `crates/oo-core`, `crates/oo-builder`, `tools/source-semantics`, and `contracts/schema/oos-v1.json` from `b1ffb079`. Ninety-six overlay blobs matched. Root cargo files matched the archive. The worktree was not compiled.

The oos-v1 overlay is required. Archive content sha256 is `48432a164cba1e42c28790ad499850a8f3b76abc0b3fb863d9538b6dbe279462`. The `b1ffb079` file, which is byte-identical to `5bd6e414`, is `ee134c232f0d1f1e5ac3cdd435462ee15ed0f66a801dad7d7cdd32daf244f331`. Leaving the archive schema in place makes the schema contract compare the wrong document. The author handoff sentence that says to leave `contracts/schema/oos-v1.json` on the archive disagrees with the author's own attempt 4 mount of `overlay-schema` and with these hashes. Attempt 4 also set `OO_WRITE_SCHEMAS=1` on a writable schema mount. This review left that variable unset and mounted the tree read-only. `phases/schema-stable.exit` is 0.

Product gates on decisive container `oo-dl01w1-36-b1b`: schema 0, source-semantics contract 0, core `sealed_import` 0, reviewed 0, role pack 0, builder `sealed_import` 0, source-semantics tests 0, three clippy gates 0, lock subset 0. The first container `oo-dl01w1-36-b1` compiled the tree and passed the same tests; its clippy-tool exit 101 was the review probe only. Logs are in `rerun-1/`.

Producer catalog sha256 `d9af109ea2f8cd63497771d960a8c396b959501b575b07aa9668fb04a61db844`. Positive identity `33bb4e8b9385da219db0fd784ab06589bda0bbe9c08518fc38fd68fbefe4677f`. Altered-input identity `24563cf2bd58ac70592c7cdb2b5c9883abb6852691f1c14479c8b5b284bebe8b`.

Debug binary sha256 `8399e4d6e5124417f8f015eb504d54776f7ebd9931603747d1ebc528310c9e06`, size 106118216. It was deleted with the private target.

## What stays unfinished

Gateway freshness, live database connection, and DBP8 live binary sort are parent and source gates. The child reports them as `not_called`, `not_connected`, and `not_run`. The mysql engine on the producer catalog is a recorded field. `source_connection` is `not_used`. This packet is not a MySQL source acceptance.

`tools/source-conformance` belongs to W37. It was not overlaid here.

Author sealed-import attempts 1 through 3 stay failed in the author packet. Attempt 4 stays green. This review did not edit them.

## Cleanup already done

`docker rm` exit 0 removed `oo-dl01w1-36-b1` and `oo-dl01w1-36-b1b`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` remains. `/tmp/oo-dl01w1-36` and `/tmp/w36-sealed-prep` were removed. `/tmp/oo-dl01w1-05` was not touched.

## Reproduce the compile

Use a private target and a read-only tree. Overlay the four paths from `b1ffb079`, including `contracts/schema/oos-v1.json`. Do not set `OO_WRITE_SCHEMAS`. Copy the producer catalog with `git show c6f628a3a8c34c731704e2008d31bf00ec6a7f9a:evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/mysql/import-proof/phases/pg-import-catalog.json` and require sha256 `d9af109ea2f8cd63497771d960a8c396b959501b575b07aa9668fb04a61db844`.

```
cargo test --locked --offline -p oo-core --test schema_contract schemas_match_the_rust_types_and_the_meta_schema -- --exact --test-threads=1
cargo test --locked --offline -p oo-core --lib source_semantics_contract -- --test-threads=1
cargo test --locked --offline -p oo-core --lib sealed_import -- --test-threads=1
cargo test --locked --offline -p oo-builder --test reviewed_effective -- --test-threads=1
cargo test --locked --offline -p oo-builder --test role_pack -- --test-threads=1
cargo test --locked --offline -p oo-builder --test sealed_import -- --test-threads=1
cargo test --locked --offline --manifest-path tools/source-semantics/Cargo.toml -- --test-threads=1
cargo clippy --locked --offline -p oo-core --lib --tests -- -D warnings
cargo clippy --locked --offline -p oo-builder --lib --test sealed_import --test reviewed_effective --test role_pack -- -D warnings
cargo clippy --locked --offline --manifest-path tools/source-semantics/Cargo.toml --all-targets -- -D warnings
```

The review probes are `harness/w36_independent.rs` and `harness/w36_sealed.rs`. They are not in commit `b1ffb079`. The CLI probe falls back to `/target/debug/source-semantics` when `CARGO_BIN_EXE_source_semantics` is empty. `CARGO_BUILD_JOBS=1`. One linux/arm64 builder, 6g memory, 1 cpu, pids-limit 512.
