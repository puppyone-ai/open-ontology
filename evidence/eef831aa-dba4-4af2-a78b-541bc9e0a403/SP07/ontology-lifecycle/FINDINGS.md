# Findings

## Product gaps, still open

1. OC-01, owner a373. `tools/selfbuild-control-api` and `pg-builder` emit JSON. There is no `operationId: exportGeneratedOos` line in the pinned OpenAPI. Schema-valid `ontology.yaml` and `generated/` YAML are not on disk.
2. OC-02, owner a373. `decideReviewItem` can store approve, reject, and defer, and the result keeps `ledger_authenticated` false, `published` false, and `grants` `not_authorized`. There is no `operationId: exportReviewDecisions` and no `operationId: compileEffectiveYaml`. `pg-builder approve` sets `authenticated` false. That is not the curated overlay, not stable-identity merge, and not a reseal.
3. OC-03, owner a374. Curated Git apply commits description fields for one review command. There is no `operationId: applyReviewedTree`. Receipts stay `authenticated: false`. A description commit is not `reviews/decisions.yaml` plus the overlay.
4. OC-04, owner a392. `createRelease` reads an operator pack file and a gates file. There is no `operationId: evaluateGatesFromGit`. `production_ready` is false. `certificate_class` is `not_issued` or `test_scope_not_production`. This card did not write a gates file and does not call a private fixture a production release.
5. OC-05, owner a397. Agent entry compares a local directory with `published_path`. The contract says pack bytes are `private_fixed` and release-control-api is not edited. There is no `operationId: bindActiveReleasePack`. A compiled pack with `published` false does not pass OC-05.
6. OC-06 stays `blocked`. Model failure, CAS conflict, restart, stale override, and revoke were not executed, so this card does not claim those side-effect results.

`chain_closed` becomes true only when every gap status is `executed`. A marker line in a file whose SHA-256 is not the pin is `unpinned` and is not executed. A marker in the pinned bytes would be `present_unexecuted` and would still leave the chain open. This build never emits `executed`.

## Compile failures, fixed before the green run

Attempt 1: Cargo treated the new package as a member of the root workspace, and the 1.94 image had no Clippy component. Exit 1. Fixed by an empty `[workspace]` table in the tool manifest, which does not edit the root manifest, and by `rustup component add clippy` inside the disposable container.

Attempt 2: `loopback_port` received a `PathBuf`, and Clippy rejected a range loop in SHA-256. Exit 1.

Attempt 3: tests passed, 10 of 10. Clippy rejected two nested `if` blocks. Exit 1.

Attempt 4: tests 10 passed, Clippy exit 0, container exit 0. Logs are under `linux/focused-4/` and `linux/failed-attempts/`.

## Not claimed

No full parent acceptance. No SP07 acceptance. No SP08 acceptance. No customer release. The 100000-row fixture was not opened.
