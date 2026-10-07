# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a334`. Review of the owned relay crate and this evidence tree only. No parent acceptance.

## Withhold

The required `grok-4.7` completion happened once, in `linux/chain-2`. The required private query did not. `linux/execute/phases/execute.exit` is 6. The execution object has no rows and no citation. Treating the file inventory, the models HTTP 200, the TLS listener, or the live fixture's `count(*)` receipt as success for this proposal would be wrong. Those are separate observations.

The live fixture did prepare `depot.counter` and did run its own `SELECT count(*) FROM depot.counter` to a clean receipt. That statement was supplied by the reviewed test, not by the model. The model statement remained `SELECT COUNT(headcount) AS visible_headcount_values FROM depot.counter;`. It was handed to `query-pg` once and refused.

## Guard

`crates/oo-gateway/src/policy_catalog.rs` in the fixed archive documents the loader as resolving only `pg_catalog.count(*)`, with `aggregate_star: true`. `sql_guard.rs` resolves a function only when the aggregate-star flag and the argument type OIDs match a loaded function. `COUNT(headcount)` has an argument and is not an aggregate star, so it does not match the loaded `count(*)`. `query-pg` maps that refusal to exit 6 and does not write the audit file. The observed stderr token is `guard`, and `audit-absent.txt` matches that contract. This finding is from that source and from the one execute output. The statement was not submitted again to obtain a longer reason string.

## Relay

The client-token boundary matches the frozen contract. Missing, wrong, and duplicate `Authorization` values do not dial. Proof-1 and proof-2 each ran 27 tests and Clippy `-D warnings` at exit 0 on Linux Rust `1.94.0`. The lock subset against the fixed root lock and `tools/description-provider/Cargo.lock` had no extra package versions. `cargo update` and `cargo generate-lockfile` were not run.

Proof-2 exists because the public client-token fixture contained the 6-byte local key as a substring of its digit run. The literal was changed to `synthetic-client-token-abcdefghijklmn`. Proof-1 remains the test of the earlier literal. Proof-2 is the test of the tree that can be committed. The committed tree does not contain the key bytes.

The one 426 diagnosis is unchanged and was not repeated. The relay tests still assert that an upstream 426 is not reported as success. The successful propose did not fall back to `grok-4.3` and did not change `model`.

## Preserved failures

Keep these exits. They are part of the record.

- `host/offline-build.exit` 101
- `linux/chain/docker-run.exit` 3 and `phases/propose.exit` 3
- `linux/chain-2/defer-1/docker-run.exit` 3
- `linux/execute/phases/execute.exit` 6

`linux/chain/propose.stderr` is `ontology-dialogue: config` because the pack helper never ran. That is not a provider error.

## Scope

No edit under `tools/ontology-dialogue`, `crates/oo-gateway`, the a328 builder, or the root manifests. The dialogue and `query-pg` binaries were built from a git archive of `63566b60aabdf29a29bfe55c4866edf3c4081cfd`. The archive already contains `tools/ontology-dialogue`, so no overlay was applied. The execute script refuses to continue when that tree is missing.

Passwords, the client token, and the upstream key were not copied into evidence. `execute.stdout` and `request.json` were scanned for the password files before copy. The upstream key was not mounted.

## Cleanup check

`CLEANUP.exit` is 0. After cleanup, a334 containers and volumes were 0, port 18317 was clear, the private tmp directory was gone, and `/tmp/oo-customer-demo-20261007/llm-token` remained mode `0600`, 6 bytes. The two local auth files were restored from the private backup before that directory was removed.

## Follow-up

The public-header path does not satisfy the running proxy. At commit `acdace936fa7df2905500c7f5e0a97d683138dea`, `applyXAIChatHeaders` sets `x-grok-client-version` to `0.2.120` and then applies auth-attribute custom headers. A client header is copied only when an attribute value is a `$` placeholder. Login writes an empty metadata map. Issue 6249 documents the per-auth `headers` object as the override and says there is no env surface. That object is credential-bearing configuration. This follow-up did not touch it.

The relay still sent the two fixed public headers. The one new grok-4.7 propose returned HTTP 426 from the CLI chat-proxy route. Dialogue recorded `provider` and wrote no proposal. No approval was written and `query-pg` was not spawned. The historical guard refusal stays a separate failed query. The fixture `count(*)` receipt is still not a model receipt.

`linux/proof-3` passed 28 tests, including `public_client_version_is_fixed`, and Clippy at exit 0. Preserved follow-up failures: `linux/proof-3/attempt-1/docker-run.exit` 125, `host-2/attempt-1/fetch.exit` 101, `host-2/attempt-tls/smoke.exit` 1, `linux/chain-3/phases/propose.exit` 1. `pg-2/attempt-1` is the LibreSSL `-ext` stop before any database container existed.
