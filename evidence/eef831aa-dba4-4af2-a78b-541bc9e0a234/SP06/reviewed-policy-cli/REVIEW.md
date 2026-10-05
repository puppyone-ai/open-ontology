# Review

Same session as the implementation. Frozen scope only: `crates/oo-gateway/src/bin/reviewed-policy.rs`, `crates/oo-gateway/tests/reviewed_policy_cli.rs`, and `contracts/gateway/reviewed-policy-cli-v1.md`. This is not an independent review and it does not accept M1-D14C.

## Checked against the green run

The contract was committed as `e21f20d2894d0ffc584fb68709c0e355d94201a6` before the command commit `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d`. The Linux overlay used those blobs: bin `34f7824250c39a58f177e70f21dc6db02b5527bd`, test `de88e2dba6648fadf61d5f92884732317db13a1c`, contract `6ae87b2c56468793151ccceccbf2fc34e80125fe`. `linux/retry2` exit 0. Six tests passed in 0.44s. Clippy `-D warnings` on `--bin reviewed-policy --test reviewed_policy_cli` exited 0. `Cargo.lock` content hash `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` was unchanged. `lib.rs`, `reviewed_policy.rs`, `policy.rs`, `Cargo.toml`, and `Cargo.lock` blobs match the fixed commit.

The request is one scanner document, protocol `oo.gateway.reviewed-policy-cli/v1`, with `command`, `artifact`, and `decisions` as strings and one typed binding. Extra fields exit 5. A non-canonical command string exits 6. The output object is `protocol` then `policies`. Policy field order matches the contract. `explicit_allow` is serialized only for class `declared`. The test's independent deserializer rejects unknown fields, builds `ColumnAclPolicy`, and calls `compile_column_acl_set`. Clerk and peek both matched the adapter's policies. `compiled.role()` is not in the JSON; the test reads it from `compile_column_acl_set`.

Refusal lines are fixed tokens. The stale producer path prints `reviewed-policy: decision stale_schema` and exits 10. PII mismatches exit 12. A forbidden `samples` key exits 14 and the marker is absent from stdout and stderr. Oversize input exits 3 without parsing. An argument exits 2 with the fixed usage sentence and is not copied into it. An empty allowlist exits 16.

`#![forbid(unsafe_code)]` is the only `unsafe` token in the command. The retry1 harness matched that line and exited 2. The product bytes did not change for retry2. The retry2 check ignores that attribute. `unsafe-lines.txt` is empty and the phase exit is 0.

## Left as specified

The DTO seam, the 1 MiB operator envelope, the private `query-pg` policy type, and hidden-after-bind as `seal` are recorded in `FINDINGS.md`. The contract already states them. No shared type was given serde to close the seam.

Clippy on this command does not lint the rest of `oo-gateway`. The pre-edit smoke ran the existing 21 `reviewed_policy` tests on the fixed archive. No PostgreSQL was used. Authentication, install, grants, publication, and an audit WAL stay outside this command. M1-D14C and SP06-W01 stay open.
