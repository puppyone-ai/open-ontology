# B01 identity producer

Session `eef831aa-dba4-4af2-a78b-541bc9e0a370`. Product commit `35bd6b8157cecd89f2ae085b012b4b0137562ddb`. Schema freeze remains `089b00d7f70da1bc42f7e201dfa070748c651d5b`.

The service implements the five frozen operations: login, callback, current, logout, and bootstrap consume. It is a management cookie session. It does not issue an agent session, a harness session, a public Gateway token, a data grant, or an owner certification.

## Proof

Linux arm64, Rust 1.94.0, private bridge `oo-a370-eef831aa-net`. One builder at 6 GiB and 1 CPU. One PostgreSQL 17.10 at 1 GiB and 1 CPU, TLS SAN `DNS:oo-a370-pg`, login role `oo_identity_admin` NOSUPERUSER and member of NOLOGIN `oo_identity_runtime`. The test issuer ran in the same builder process and signed real RS256 tokens. `OO_A370_LIVE=1` was required.

`cargo test --locked --offline` passed: 11 library tests, the silent usage CLI test, and `live_identity_boundaries`. `cargo clippy --locked --offline --all-targets -- -D warnings` passed after the 1.94 Clippy component was added inside the builder. The child `Cargo.lock` has 225 registry packages, all present with the same checksums in the root lock.

The live proof records status and error codes only. The database password is not in `linux/raw/proof.json`. The configured bootstrap subject is an explicit test fixture, not customer-owner evidence. The test issuer is not a customer IdP signoff.

## G1 contract

Frozen files were not rewritten. SHA-256:

- `CONTRACT.md` `35f9c4a1fccf08b695848c6e6291f191a392cf8ecd079490f80855a773035732`
- `openapi/control-identity.openapi.yaml` `d49bec8cd3ef59bce78e8f8123fd8ea58be7bedac6d84108a13a2c197bd0f040`
- `schema/error.schema.json` `1a6725385580d9d1ec668f84f71d6c441a05fcbe54d07b88046bfdaf98d26238`
- `schema/current.schema.json` `ffd32c86bd4f5188fb08adcbba49d46231c9cadaaf8530031b5055922e7823cb`
- `schema/bootstrap-consume.schema.json` `dcbbe3c79f86b14e621c17c340a470011763d4c8ee1698c5fc59f21759831697`
- `schema/bootstrap-result.schema.json` `42da8ab161dbf673d748b9e9c519c2af2ad75892c2bdc41f89f7bda407748e0d`
- `schema/logout.schema.json` `3c66428d1c325d45d8d96fcde5e8d1ac9cd53531eecf7c4932f2e6744cfc2064`
- `schema/capability-matrix.json` `c2122752ad5138f134527c88a761a03484fefe9b30364f28b60c30298ea118fd`
- `schema/config.schema.json` `1c593466dd680c58684f7ddc5bd40134a1b1152ca383f354d21d14fb70415681`

No frontend file was written. Frontend binding stays with G1 at schema freeze.

## Limits

SP02-W05 parent acceptance is not claimed. Customer IdP integration is not claimed. Independent review is still required. The audit trigger blocks UPDATE and DELETE. The table owner can still TRUNCATE; the service does not issue TRUNCATE.
