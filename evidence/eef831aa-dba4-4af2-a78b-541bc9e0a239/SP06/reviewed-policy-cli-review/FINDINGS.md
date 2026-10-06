# Findings

Independent review of commit `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d` against `contracts/gateway/reviewed-policy-cli-v1.md`. Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a234/SP06/reviewed-policy-cli/`. That author session wrote the command. This directory is a separate session. Verdict `APPROVE_LIMITED`. M1-D14C stays unaccepted.

## Command output

`reviewed-policy` was executed on stdout from the fixed archive's `reviewed-effective-oos` and on the sealed captured-draft artifact that produced it. The clinic fixture is independent of the author shop fixture. Two administrative bindings, `nurse` and `desk`, compile two policy sets from one command, one artifact, and one decision array. Nurse stdout is byte-identical across two runs. The harness deserializes its own private document into `ColumnAclPolicy` values and passes them to `oo_gateway::policy::compile_column_acl_set`. Those values equal `compile_reviewed_policies` on the same command, artifact, decisions, and binding. `compiled.role()` is `oo_r_` plus the binding slug. The JSON document carries the slug. `row_filter` is null on every policy in that output. Confirmed email is `declared` with `explicit_allow` true. Confirmed open is `boolean_flag` with class `none`, and the document omits `explicit_allow` false.

Role, login, config digest, generation, and the allowlist are mapping input. The command's non-claims stay `not_claimed`. Wrapper `publication` is `unavailable` and wrapper `grants` is `not_authorized`. The reviewer label `mina.chart` is a decision field the producer requires. It is absent from command stdout. The compiled policy login is the binding login. The bin source does not name `tokio_postgres` or `install_column`. This review does not install a policy.

## Boundary cases

These refusals were produced by editing the real compiler inputs or the operator document, then calling the command. Stdout was empty. Stderr was the single redacted line. The owner string and the sample string were absent.

| Case | Exit | Stderr |
|---|---|---|
| `{` | 4 | `reviewed-policy: scan` |
| Duplicate `protocol` key | 4 | `reviewed-policy: scan` |
| 1048577-byte body | 3 | `reviewed-policy: budget` |
| Extra field carrying the owner string | 5 | `reviewed-policy: request` |
| Decision schema hash on `email` replaced | 10 | `reviewed-policy: decision stale_schema` |
| `personal` email with `explicit_allow` false | 12 | `reviewed-policy: pii_allow` |
| `none` on `id` with `explicit_allow` true | 12 | `reviewed-policy: pii_allow` |
| Last hex digit of `binding.seal.artifact_sha256` flipped | 8 | `reviewed-policy: seal` |
| `samples` key recanonicalized into the command | 14 | `reviewed-policy: forbidden` |

The stale line does not contain the column name. The oversize marker does not appear. A static claims row filter on the local DTO is `PolicyError::RowFilterRefused`, and that display text does not contain the claim word.

Hidden-after-bind stays the adapter seal mapping reviewed at a233. This command review proved seal by flipping `binding.seal.artifact_sha256`.

## Local DTO and the 1 MiB envelope

`ColumnAclPolicy` has no `Serialize` or `Deserialize`. The command's JSON types are private to the bin and use `deny_unknown_fields`. `compile_column_acl_set` is not re-exported from the crate root. `lib.rs` re-exports `compile_column_acl`. The harness reaches the set function through `oo_gateway::policy`. The operator document is one JSON value on stdin. A body of 1048577 bytes exits 3 and is not parsed. The contract states that three adapter documents near 1048576 bytes do not fit in this one envelope. Both limits are in the frozen contract text that the overlay test read from the archive.

## draft_hash

The clinic artifact's `draft.provenance` has no `draft_hash` key. a233 already judged that placement against the producer protocol. This review observed the same key absence on a different catalog. It is not a defect in the command at this commit. The contract file was not amended.

## Still open

Authentication, grants, publication, an audit WAL, live row security, and a non-superuser grantor stay outside this command. Physical install was not run. No PostgreSQL was started. M1-D14C and SP06-W01 stay open. No dependency was added. `Cargo.lock` and the gateway manifest were not edited. a207 remains the connector-manifest and root-lock writer.
