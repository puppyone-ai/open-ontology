# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a334`. Owner a334. Fixed archive `63566b60aabdf29a29bfe55c4866edf3c4081cfd`.

## Owned paths

- `testing/real-db/local-llm/**` — contract `oo.local-llm-relay/v1`, client-token addition `oo.local-llm-relay.client-token/v1`, crate, lock, bins `oo-local-llm-relay` and `oo-local-llm-models`.
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/**` — diagnosis, host smoke, preoverlay, proof-1, proof-2, chain, chain-2, execute, Postgres launch scripts, this handoff.

No root `Cargo.toml` or `Cargo.lock` edit. No edit under `tools/ontology-dialogue`, `crates/`, or the a328 builder. The crate is its own workspace.

## What the next reader has

The green Linux evidence for the committed crate is `linux/proof-2`: Rust `1.94.0`, 27 tests passed, Clippy `-D warnings` exit 0, lock subset clean. `linux/proof-2/source-after.sha256` is the hash of that crate. `linux/proof-1` is the same checks before the test-literal redaction and must stay.

The completion evidence is `linux/chain-2/proposal.json`. The withhold evidence is `linux/execute/execute.stdout` and `phases/execute.exit` 6. Do not promote the live oracle's `count_receipt=clean` into a receipt for the model SQL.

The relay, Postgres, cargo volumes, target volumes, network, and `/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334` are gone. `CLEANUP.exit` is 0. The token file `/tmp/oo-customer-demo-20261007/llm-token` remains mode `0600`, 6 bytes. Do not delete it from this unit. Public certificates are `host/ca.pem` and `host/leaf.pem`. Private keys were only under the removed tmp directory.

## Rebuild

Use image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, one builder, 6 GiB, 1 CPU, `--cap-drop ALL`, private cargo and target volumes, and `cargo test --locked --offline --all-targets` plus `cargo clippy --locked --offline --all-targets -- -D warnings`. Do not run `cargo update` or `cargo generate-lockfile`.

A host listen needs a new keypair and a new client-token file from `oo-local-llm-relay --mint-client-token`. The process dials `127.0.0.1:8317` only. A container client uses `https://host.docker.internal:<port>` and the public CA. Dialogue's bearer file is the client-token file, not the 6-byte upstream key. Dialogue's 16-byte minimum is unchanged.

Recipe origins are in `linux/preoverlay/recipe-origins.txt`, `linux/proof-1/recipe-origins.txt`, `linux/chain/recipe-origins.txt`, `linux/chain-2/recipe-origins.txt`, and `linux/execute/recipe-origins.txt`. The a300 execute shape is proof-8 at `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`, with no dialogue overlay because `63566b60` already contains the tool. `linux/libclang-extract.sh` is a byte copy of the a300 script, sha256 `23d9cd6a4e7033f22a98afc9243c2c9dc2d7e60f5246e9ddcbc8aaa949c2c289`.

## Next query

A later attempt needs a new proposal or an explicit decision to keep this one. This proposal's SQL is still the `COUNT(headcount)` statement. The fixed guard admits `count(*)` and refuses `count(column)`. Rewriting the model SQL locally would no longer be the model statement. A new completion would be a new request, not a retry of this one.

## Follow-up

The crate now also carries `oo.local-llm-relay.public-client-version/v1`. `linux/proof-3` is the green post-edit Linux evidence: 28 tests, Clippy 0, Rust `1.94.0`. `linux/proof-2` remains the pre-edit hash.

The new question in `linux/chain-3` did not produce a proposal. `phases/propose.exit` is 1 and stderr is `ontology-dialogue: provider`. `upstream-426.txt` records proxy HTTP 426 for that single POST. No query receipt was created. `linux/execute-2` is the unused launch script for a later legal proposal; it was not run. Do not point it at `linux/chain-2/proposal.json`.

Auth files, backups, and credential-bearing config were not read or written in this follow-up. The historical restoration in `CLEANUP.txt` still stands. Docker on this host could not bind-mount `/tmp`; follow-up private files used `/var/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334` and were removed with the follow-up cleanup. The 6-byte token file remains.

## Readiness 2026-10-08

The model receipt is `linux/execute-3/execute.stdout`. The proposal is `linux/chain-4/proposal.json`. The operator approval is `linux/chain-4/approval.json`. The audit is `linux/execute-3/audit.jsonl`. Rows are the integers 1, 2, and 3. `query_receipt` is `clean`.

`linux/proof-3` remains the Linux Rust `1.94.0` proof for the unchanged crate. `readiness/source-note.txt` records the content-hash match. Do not treat `host-3/models.json` or the live oracle `count_receipt` as this receipt.

`CLEANUP-readiness.exit` is 0. The relay, `oo-a334-pg`, the a334 volumes, the task network, and `/var/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334` are gone. `/tmp/oo-customer-demo-20261007/llm-token` remains mode `0600`, 6 bytes. Auth files were not touched. Historical guard-6 and 426 records stay.

Parent acceptance, a full-goal close, and a release are out of scope.
