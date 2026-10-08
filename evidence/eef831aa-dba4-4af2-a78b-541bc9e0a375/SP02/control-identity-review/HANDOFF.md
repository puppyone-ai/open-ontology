# Handoff

Verdict for the coordinator: **WITHHOLD original E54**.

Exact clause: OIDC Core 1.0 §3.1.3.7. ID Token `aud` must contain `client_id`. Measured result with `audience=configured-aud` and `client_id=rp-client`: configured `aud` was accepted (HTTP 302 and a current session); `aud` equal to `client_id` was `audience.rejected` (HTTP 401); `other-aud` was `audience.rejected` (HTTP 401). `binding_holds=false`.

Do not treat this packet as acceptance of a customer IdP, of the SP02 parent, or of the full goal.

## Scope

- UUID `eef831aa-dba4-4af2-a78b-541bc9e0a375`.
- Product commit `35bd6b8157cecd89f2ae085b012b4b0137562ddb`. Archive matches that tree.
- Contract commit `089b00d7f70da1bc42f7e201dfa070748c651d5b`. Contract, OpenAPI, and schemas match. The contract's own ID Token row says `aud` contains the configured audience. The withhold is the OIDC `client_id` clause the review was required to measure.
- Writes stayed under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a375/SP02/control-identity-review/`.

## Evidence map

- `REPORT.md`, `FINDINGS.md`, this file.
- `linux/raw/independent-proof.json` and `independent-proof.verdict` (`WITHHOLD`).
- Raw exits: `smoke` 0, `cargo-test-author` 0, `cargo-clippy-author` 0, `cargo-fetch` 0, `cargo-test-independent` 0, `cargo-clippy-independent` 0, `archive-immutable` 0, `archive-immutable-rerun` 0, `rerun-independent` 0. `run-proof.exit` is 1 from the first oracle compile failure and is preserved on purpose.
- `linux/raw/FAILED-ATTEMPTS.md` and `linux/raw/failed-attempt-{1,2,3}/`.
- `linux/raw/truncate-note.txt`.
- `linux/raw/recipe-origin.txt`.
- `responses/schema/` and `responses/actual/`.
- `SHA256SUMS` and `SOURCE-INVENTORY.txt`.

## Failed attempts

1. Independent crate did not compile (name clash on `authorize`, missing `SecureRandom`, shadowed `text`). `run-proof.exit` 1. Author tests and author Clippy had already passed.
2. Compile succeeded and the audience probe already returned `binding_holds=false`, but the ambiguous-JWKS case still had the IdP error mint set, so it saw `token.rejected`. The test process exited 101. Clippy was also missing inside that fresh container.
3. Test exited 0 with `WITHHOLD` and 0 failed cases. Clippy `-D warnings` failed on an unread `cookie_header` field and two `collapsible_if` sites.

The fourth run copied the fixed oracle, reused the cargo volume and the same PostgreSQL, and exited 0 for the test and for Clippy. No second database and no overlapping builder.

## Cleanup

Owned `oo-a375-proof`, `oo-a375-pg`, volumes `oo-a375-cargo`, `oo-a375-work`, and `oo-a375-eef831aa-pgdata`, network `oo-a375-eef831aa-net`, and `/tmp/oo-a375-eef831aa-dba4-4af2-a78b-541bc9e0a375` are removed. No foreign container was removed. Details: `linux/raw/owned-cleanup.txt`.

## What remains outside this packet

A change that rejects an ID Token unless `aud` contains `client_id` would touch product code and the frozen contract sentence. That change is not in this commit. The table-owner `TRUNCATE` limitation stays a retained note.
