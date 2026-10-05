# Reviewed PostgreSQL composition

## Verdict

This directory records one composition on the fixed archive `ed661809cf6c1e8df37af5842838dd1eb839c98a`. The harness wrote `live/outcome.txt` as `reviewed_pg_composition_recorded`. `parent_acceptance`, `development_unit`, and `component_approval` are false. M1-I01 is `not_accepted`. M1-I02 is `not_accepted`. The run does not add a development unit and does not approve a component.

Semantic publication, authentication, a WAL, natural language, and a harness mount stay missing. The fixture login mapping grant stays a library seam.

## Fixed source

The builders compiled a `git archive` of `ed661809cf6c1e8df37af5842838dd1eb839c98a`. The archive file count was 34560. The overlay added the evidence harness as workspace member `a226-harness` and did not edit the product crates, the root manifest, or the committed lock. `cargo-update-invoked.txt` is `no`. `lock-registry-added.txt` and `lock-registry-removed.txt` are empty.

The archive already contains the compilers this run called. Those commits are ancestors of the fixed archive. Calling them here does not approve the units:

| Commit | Subject |
| --- | --- |
| `9daeaa05e34c` | builder: compile a reviewed captured draft into effective OOS |
| `543580768dec` | gateway: install one column ACL set across mapped tables |
| `613aa981dc2c` | pipeline: add explicit mapped-shadow statistics maintenance |
| `6e1e6e576d1a` | pipeline: add the init-pg operator command |
| `0c6e6f1e0efe` | builder: publish the role-pack bundle with a non-replacing hard link |

`query-pg` is the gateway binary from the same archive. This run did not exercise a benchmark query adapter.

Recipe origins and the blobs they were copied from are in `origin/recipes.txt`.

## Resources

Pinned images, already local, `--pull never`, `linux/arm64`:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` (`rustc 1.94.0`)
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` (PostgreSQL 17.10)

The new builder was 6 GiB and 1 CPU (`Memory=6442450944`, `NanoCpus=1000000000`), `CapDrop=ALL`, `Privileged=false`, private network `oo-a226-eef831aa-net`. The successful chain container was `8feb038f06a7066e8954ec9bdde6221f855a158c28e1e05eef12a498eadabfab`. `OOMKilled=false`. `linux/chain/docker-run.exit` is 0. `linux/chain/chain.exit` is 0. `linux/chain/phases/secret-scan.exit` is 0.

PostgreSQL listened only on that network. The certificate SAN is `DNS:oo-a226-pg`. `live/server.txt` records `ssl=true` and `server_version=17.10 (Debian 17.10-1.pgdg12+1)`. One source database `oo_a226_src` and one target database `oo_a226_dst`. Tables `shop.customer` and `shop.order_doc`.

## Pre-overlay smoke

`linux/smoke/smoke.exit` is 0 and `linux/smoke/docker-run.exit` is 0. That run finished before the overlay existed. Focused tests, all `--locked --offline --test-threads=1`, exited 0: `oo-builder` lib `reviewed_effective`, the builder integration tests named in `linux/chain/inside.sh`, pipeline `init_pg_cli`, `catalog_pg_cli_live`, and `import_pg_cli_live`, gateway lib `policy::tests`, and `query_pg_cli`. Live tests stayed ignored. `OO_A198_LIVE` was unset. The chain repeated those tests and they exited 0 again. Harness clippy and the product bin clippy groups (`clippy-pipeline`, `clippy-builder`, `clippy-query`) exited 0.

## Decisive raw results

| Step | Exit | Raw result |
| --- | --- | --- |
| `import-pg-not-ready` | 1 | stdout empty; stderr `oo-pipeline: target schema is not ready` |
| `init-pg` | 0 | `install=bootstrapped`, `full_import=installed`, `historical_v2=false` |
| `import-pg` | 0 | protocol `oo.pipeline.import-pg-result/v1`, `state=awaiting_publish`, stderr empty |
| `catalog-pg` | 0 | comment marker absent from stdout |
| `captured-pg-draft` | 0 | draft hash `e01fc8ed6fe99e8bb509e99fe3491973d9286776b41341f56de84ab61bb7ef3a` |
| `review-pg-draft` | 0 | draft hash `7c0036687e438aeeb723273f7742362986fbb35a3d2c80fabcacfa81f5bd1cf2` |
| `reviewed-effective-oos` | 0 | schema `da877f8bc78b7a9a14b8648004bfd904d8bc89b774bbca1253c1758927a74a2f` |
| `role-pack-review-stdout` | 1 | `field.unknown` |
| `role-pack-wrapped-draft` | 1 | `schema.rejected` |
| `role-pack-private-note` | 1 | `classification.unknown_pii` |
| `role-pack-clerk` | 0 | pack digest `3471223bb6a5ce12fbf60dc557691f1a07f43b2ca9032bf6dd7b99f7438871b2` |
| `role-pack-peek` | 0 | pack digest `65f2e272d6fd57714963b12926120740e306ee1724541690b4f1499af6fc2d90` |
| `role-pack-bundle-clerk` | 0 | empty stdout and stderr |
| `role-pack-bundle-peek` | 0 | empty stdout and stderr |
| `role-pack-bundle-clerk-again` | 1 | `io.destination` `the output path already exists` |
| `query-before-analyze` | 5 | stderr `query-pg: policy`; stdout empty |
| eight successful `query-*` reads | 0 | both tables, projected columns, `count(*)`, and star |
| `query-hidden`, `query-insert`, `query-pos` | 6 | stderr `query-pg: guard` |

Import left `publication_count` `0` and `runtime_owns_target` `false`. Source generation 26. Target generation 1. Run id `b8078736-c12e-446f-ad56-2baa616f3a1e`.

The effective document confirmed nine fixture decisions. `private_note` stayed unclassified (`pii` null, `semantic_type` null). The hidden reason recorded for it is `classification.absent`. Provenance `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` are `not_claimed`. Role-pack manifest statuses keep authentication and publication `not_claimed` and mount `not_performed`. Bundle statuses do the same. Clerk and peek pack text both contain the declared foreign key. Clerk pack digest and bundle `pack_sha256` differ (`3471223b…` and `a7986e71…`).

`install_column_acl_set` installed both tables for each role. Grantor for both is `oo_fixture_admin`. Mapping privileges on the login were `false,false,false` after that install and `true,true,true` after the fixture `GRANT` of `_oo` `USAGE`, `_oo.physical_mappings` `SELECT`, and `lookup_physical_mapping` `EXECUTE`. `live/mapping-grant.txt` labels that grant `fixture_configuration_not_library` and the role/login binding `configuration_not_auth`.

Shadow `reltuples` were `-1,-1` before `analyze_mapped_shadows` and `3,3` after. The receipt has `published=false` and `estimate_is_exact=false`. Estimated rows were 3 and 3. The loaded catalog visible columns were `customer_id,display_name,active` and `order_id,customer_id,memo,posted,amount,clerk_note`. `private_note` is absent there. `pending_unclassified` was 2 on customer and 1 on `order_doc`.

Direct SQL as the login, after `SET ROLE`, returned SQLSTATE 42501 for peek `private_note`, peek `clerk_note`, peek `SELECT *`, clerk `private_note`, and clerk `INSERT`. Peek `display_name` matched the independent gold (`丙号`, null, `丁铺`). Eight local audit files contain admit and complete records. Those files do not contain the byte string `SELECT`.

The source reader audit count stayed 0. One writer insert was audited. After that insert the source customer count was 4 and source `reltuples` stayed `-1,-1`. The imported shadow estimates stayed 3.

## Preserved failures

| Attempt | Exit | Cause |
| --- | --- | --- |
| `linux/chain/attempt-1` | build-overlay 101 | one `json!` object exceeded the macro recursion limit |
| `linux/chain/attempt-2` | build-overlay 101 | visibility slices and `Call::one` types |
| `linux/chain/attempt-3` | clippy-harness 101 | dead fields and `let_unit_value` |
| `linux/chain/attempt-4` | run 1 | principal `fixture.operator` failed `check_slug` |

Each attempt kept its console log, phase exits, and `why.txt`. Product crates were not edited. Attempt 4 also keeps the raw `role-pack-private-note.stderr` from that run. The final probe, with principal `fixture_operator`, returned `classification.unknown_pii`. Standalone lock resolution was not executed. `linux/chain/attempt-standalone/why.txt` cites the a211 failure.

## Missing seams

- Semantic publication is missing. Import state is `awaiting_publish` and `pg_publication` count is 0.
- Authentication and authorization are `not_claimed`.
- The login mapping `USAGE` / `SELECT` / `EXECUTE` grant is fixture configuration. The library did not grant it.
- Role and login binding is configuration. There is no column-ACL CLI.
- Business columns were not hand-granted. The policy JSON was built from the emitted effective document.
- Local query audits are JSONL files. A WAL is missing.
- Natural language is missing. Harness mount was not performed.
- M1-I01 and M1-I02 are not accepted.
- Component reviews still queued: a215, a224, a217, a218, a219, a223.

## Cleanup

`cleanup/run.sh` removes only `oo-a226-smoke`, `oo-a226-chain`, `oo-a226-pg`, the three `oo-a226-eef831aa-*` volumes, network `oo-a226-eef831aa-net`, and `/tmp/oo-a226-eef831aa-dba4-4af2-a78b-541bc9e0a226`. It does not prune, does not delete the pinned images, and does not touch another owner. The first cleanup exited 2 because `{{.Config.Labels.oo.owner}}` does not read the label key `oo.owner`. That log is `cleanup/attempt-1-label-template.txt`. The script now uses `index`. `cleanup/CLEANUP.txt` records the removal. The hash inventory is produced after that file exists.
