# Handoff

Owned paths:

- `tools/metric-reconcile/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a316/SP08/metric-reconcile/**`

Contract: `tools/metric-reconcile/CONTRACT.md`. Operator summary: `tools/metric-reconcile/README.md`.

The crate is its own workspace. Build it against the fixed archive, or against a tree whose `oo-core` and `oo-gateway` still match that archive, with the child `Cargo.lock` and `--locked --offline`. Do not `cargo update`. Do not edit the root `Cargo.toml` or `Cargo.lock` from this component. a313 is the root-lock writer.

Commands:

```text
metric-reconcile compile
metric-reconcile evaluate --config CONFIG
```

Stdin is the request JSON, max 2_097_152 bytes. `CONFIG` points at `password_file` and `ca_file`. Do not put the password on argv or in the environment.

The live fixture is `src/live.rs`, gated by `OO_A316_LIVE=1`. It no-ops otherwise. It expects host `oo-a316-pg`, database `oo_a316`, admin `oo_fixture_admin`, login `oo_a316_login`, and file paths for the two passwords and the CA. `OO_A316_EVIDENCE`, when set, receives `pass.json`, `tolerance.json`, and `scale.json`.

Final raw proof is `linux/overlay-4`. Earlier exits stay in `linux/preoverlay`, `linux/overlay`, `linux/overlay-2`, and `linux/overlay-3`. PostgreSQL startup evidence is `pg/`.

Do not read `metric_certified: true` in `pass.json` or `scale.json` as full SP08 certification, publication, or owner authentication. Those three flags are false. Omitted gates are `not_run`.

After this evidence is committed, the task removes its own container `oo-a316-pg`, volumes `oo-a316-eef831aa-cargo`, `oo-a316-eef831aa-target`, and `oo-a316-eef831aa-pgdata`, network `oo-a316-eef831aa-net`, and `/tmp/oo-a316-eef831aa-dba4-4af2-a78b-541bc9e0a316`. It does not prune Docker or stop another owner's builder.
