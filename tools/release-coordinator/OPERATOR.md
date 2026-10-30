# Whole source publication operator contract

Source owner supplies actual reviewed effective OOS JSON, complete role visibility JSON and native target tables. Publication never substitutes source evidence or approves customer ownership. Production, standby and G10 remain separate pending requirements.

One project has one control database. The same role visibility contains every approved dataset; do not split a whole source into one-table projects. Every target dataset must exist, use its actual qualified name and have original imported PostgreSQL types. The published data role `oo_r_<source>` is NOLOGIN; the separate reader LOGIN is a member of that role and the publisher validates `SET ROLE` before observing access. Target administrator is `oo_target_admin`, control administrator `oo_release_admin`. Fixture DBA creates only owned synthetic namespaces. Source credentials are never publication credentials.

For an already serving target, the operator must also allow `oo_target_admin`
to terminate sessions of each dedicated reader LOGIN. Grant only the managed
login role, with `INHERIT TRUE, SET FALSE, ADMIN FALSE`; do not give the reader
an administrator membership or grant cluster-wide `pg_signal_backend` merely
to fix this prerequisite. A missing privilege refuses publication with SQLSTATE
42501. Closing a fence waits up to five seconds per backend for termination;
an unconfirmed termination refuses publication instead of counting a signal as
a stopped session.

Recommended namespace contract (the source owner chooses actual existing names): `native_mysql_20261012`, `native_oracle_20261012`, `native_postgres_20261012`, `native_mariadb_20261012`, `native_clickhouse_20261012`, `native_doris_20261012`, `native_starrocks_20261012`, `native_mongodb_20261012`, `native_opensearch_20261012`, `native_sqlserver_20261012`. Project, schema, role and relation are explicit input; no implicit shop/public.customer default. Do not rename original identifiers silently. SQLServer native endpoint prerequisite remains pending.

Control config JSON exact keys: `host,port,database,user,password_file,ca_file`. Maintain config adds all exact keys `target_host,target_port,target_database,target_user,target_password_file,target_ca_file,target_reader_user,target_reader_password_file`. Password/CA references are private files; never put passwords in argv or logs. Unknown keys are rejected.

Stage stdin JSON exact keys:

```json
{"protocol":"oo.release-coordinator/stage/v1","project_id":"SOURCE_PROJECT","candidate_id":"CANDIDATE","version":"VERSION","base_version":null,"effective_oos":{},"visibility":{},"policy_id":"POLICY","policy_version":"POLICY_VERSION","knowledge_id":"KNOWLEDGE","knowledge_version":"KNOWLEDGE_VERSION","catalog_version":"CATALOG_VERSION"}
```

Replace empty objects with actual producer output. All version/role/project fields must match the actual compiler manifest. `stage` recompiles the entire role Pack through the fixed builder and returns actual `source_hash`, `pack_digest`, file hashes and candidate identity. Do not fabricate these fields.

```bash
"$PUBLISH_BIN" maintain-install --config "$CONTROL_CONFIG" < maintain-install.json
"$PUBLISH_BIN" stage --config "$CONTROL_CONFIG" < stage.json > stage.receipt.json
"$PUBLISH_BIN" maintain-activate --config "$MAINTAIN_CONFIG" < activate.json > activate.receipt.json
```

Maintain-install stdin: `protocol=oo.release-coordinator/maintain-install/v1`, `project_id`. Activate stdin exact keys: `protocol=oo.release-coordinator/maintain-activate/v1`, `project_id,candidate_id,source_hash,expected_version,schema_name,relation_name,role_name`. The schema/relation are an anchor that must occur in the complete visibility. The publisher derives all table bindings from the sealed full visibility, closes all affected tables before grants and pointer CAS, then opens every table with the same candidate/Pack/version. Same column names in different tables have separate irreversible revocation histories. `maintain-recover` uses protocol `oo.release-coordinator/maintain-recover/v1` with the same keys and repairs all candidate/pointer datasets. Recovery cannot restore previously revoked columns.

Actual Gateway must load the producer artifact whose accessible digest equals stage receipt, and the matching complete approved policy/visibility. Record knowledge/describe/validate/query/freshness responses and audit query_id for every original table, with independent expected typed values. `publication-read.sh` invokes fixed OO CLI with private token file. Publication receipt alone does not prove public knowledge consumption. For complete multi-table sources use `reviewed/<role-slug>/command.json`, `artifact.json`, `decisions.json`, `binding.json`: fixed Gateway `load_reviewed_role` verifies actual reviewed-effective command and captured source seals, then compiles every policy. The legacy operator-preview `policy.json` branch is single-table and cannot close a complete multi-table source. Binding exact keys: `project,ontology_version,catalog_version,policy_id,policy_version,knowledge_id,knowledge_version,source_id,schema,schema_hash,artifact_sha256,config_digest,generation,role,login,allow`; allow entries carry `dataset,column,explicit_allow`. Values come from actual reviewed/import outputs. Role is slug without `oo_r_`, login is actual mapped data login; generation/config_digest must match actual import physical mappings. Do not invent binding seals.

The existing original Linux ELF proof is retained. Current multi-table source requires its new Linux1.94 smoke and real multi-table wide/narrow/rollback outputs before it is usable. No production certificate is inferred from `knowledge_published` or `test_scope_not_production`.

Native shadow-table maintenance (implementation under verification): use protocol `oo.release-coordinator/maintain-native-activate/v1` or `oo.release-coordinator/maintain-native-recover/v1` and add exact `native_mapping` object with `source`, `config_digest`, `generation`. These values come from the actual sealed import/catalog; do not invent stored physical names or generation. Publisher calls the real target `_oo.lookup_physical_mapping` for every logical table and column, validates its hash-name shape and measures actual PostgreSQL types, then performs full-role fence/grants on those resolved shadows. API target config may carry the same native_mapping object. Logical Pack names remain original names. The target admin needs authorized lookup/ALTER/GRANT rights; Gateway runtime needs authorized lookup EXEC and mapped read privileges, without pipeline membership. This paragraph is a contract, not proof that actual source integration already ran.

Native Gateway compatibility boundary: the fixed reviewed loader validates captured PostgreSQL artifacts; native reviewed-sealed-import artifacts use a different compiler/protocol. They must not be retagged as PostgreSQL. An actual native-reviewed validation adapter is required before native source public consumption. Root coordinates its sole writer and fixed native builder dependency. Current documented four-file reviewed path is proven for captured-PG, not proof of native artifact acceptance.

Native reviewed Gateway file contract (author-tested adapter): native-input.json is actual {catalog,settings,decisions,protocol} input; command.json is its actual core reviewed output; binding.json binds accepted source/schema/catalog/config/generation and role identities. The actual annotated effective/validated Knowledge Pack remains authoritative for source_type/caveats. Full source integration awaits genuine S9/S10 and rolePack artifacts; this adapter test is not that approval. Native maintenance may use datasets:[{dataset,source,config_digest,generation}] inside native_mapping when actual imported entries have distinct contexts; each dataset is resolved independently and the whole vector is persisted immutably per candidate.

## Installed deployment mode

`deployment_mode` is an optional field in the installed coordinator `--config`
file or release-control-api configuration, and in the Gateway operator directory's
`config.json`. It is not a command/request field. Omission preserves
`primary_replica`; the only other value is `all_in_one`. Unknown values fail startup.
`all_in_one` with `replica_pg` (or another replica binding) fails with
`all_in_one_replica_conflict`, rather than silently ignoring the standby.

For a single PostgreSQL instance, set `deployment_mode` to `all_in_one` in both
installed configurations. The coordinator's `target_reader_user` and password
must identify the same login as Gateway `data_user` and `data_password_file`.
The login must not be a superuser, database/role creator, replication role or
BYPASSRLS role. Grant only mapping/quality metadata reads and membership in the
reviewed NOLOGIN SELECT role; do not grant DML, object ownership or schema CREATE.
Gateway opens these data sessions with `default_transaction_read_only=on`, and
the existing executor continues to use a read-only transaction for every query.

The Gateway's installed `data_query_concurrency` and `data_query_timeout_ms`
default to 4 and 5000 in single-instance mode, versus 256 and 30000 in the
primary/replica data plane. Single-instance overrides must be positive and strictly
below those primary/replica defaults. HTTP concurrency and timeout are capped by
these limits too; request timeout hints cannot raise the installed query limit.

After target commit, the coordinator opens a new TLS session with the reader
login. It reads the role-scoped `oo_publish_target.visible_vector()` installed by
the additive `oo_publish_visibility_v1.sql`, compares version/Pack/columns, reads
mapping generations and native publication table generations, and probes the
reviewed columns under the query role and RLS. The physical standby barrier is
not used or described as a replica observation in this mode.

A failed post-commit observation returns exit code 2, reason
`post_commit_visibility_failed`, sanitized SQLSTATE detail, `fail_closed=true`,
`half_published=true` and `knowledge_published=false`. The new fences are closed
using `close_target`, which revokes their grants; an activation retry rechecks
visibility and cannot turn that closed state into a successful publication.
There is no automatic restoration of the previous generation. This explicitly
replaces the earlier P2 requirement that the old generation remain readable;
recovery uses the existing rollback/new-publication operations.

Readiness files are written only after the existing Gateway CLI preflight. They
contain `ready`, `deployment_mode`, `readonly_replica`, `role`,
`source_generation`, `pack_digest`, `gateway_container` and `private_root`.
Single-instance records say `all_in_one` / `false`; primary/replica records say
`primary_replica` / `true`. A record cannot select the runtime deployment mode.
Legacy primary/replica readiness records without the mode remain accepted.

Run the owned PostgreSQL 17 proof from the worktree root:

```sh
bash testing/all-in-one/run.sh
```

It creates only `oo-wf-allinone-proof-pg`, exercises publication, a Gateway CLI
query, mapping visibility, injected post-commit failure and database write
refusals, then removes its container/anonymous volume. Fixture credentials and
certificates stay in a task-prefixed temporary directory, never in source.
The internal `oo-core` dependency is reused for the shared closed mode/limit and
readiness contract; no external package or registry version was added.
