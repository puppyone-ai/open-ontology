# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a240`. Fixed commit `df0bce2e235babc0944b68350921c13038741ab9`.

`parent_acceptance` is false. This is not a completed parent goal and not a release. SP06-W01 stays open.

## Left in the tree

`oo_gateway::mapping_reader_access::provision_mapping_reader` takes an administrator TLS `Client`, an explicit login, and one full `&[ColumnAclPolicy]`. `compile_column_acl_set` still validates the set. The new function grants only the three mapping privileges the catalog loader needs, or it replays when those privileges are already exact. Failures roll the transaction back and do not revoke a privilege the login already held.

`crates/oo-gateway/src/lib.rs` exports the module and nothing else. `policy.rs`, SQL, manifests, and the lock are unchanged.

The contract is `contracts/gateway/mapping-reader-access-v1.md`, protocol `oo.gateway.mapping-reader-access/v1`. It states that the grant is administrator-selected provisioning, that authentication and tenancy are not decided here, and that raw mapping rows are visible to the login.

## Proof

Final chain exit 0 on Rust 1.94.0 and PostgreSQL 17.10. Oracle: `live/oracle.txt`. Outcome: `live/outcome.json`. Failed attempts: `linux/chain/attempt-1` (no `Debug` on `Prepared`), `attempt-2` (mapping-table column privilege count), `attempt-3` (`init-pg` exit 6 on a second database). Smoke of the fixed archive, before overlay, exit 0.

## Still open

Authentication of the administrator and of the login. Audit WAL. Row-level security. Publication. A non-superuser grantor; the fixture used `oo_fixture_admin`. A login with this grant can read every mapping row in the database. That exposure is named, not closed.

Query-pg still has its own fixture grant in the reviewed composition harness. This library call is the product replacement for that seam. The query-pg binary was not edited and was not the subject of this proof. The live test called `load_policy_catalog` and `DedicatedSession::execute_trusted_select`.

## Cleanup

`cleanup/run.sh` removes only `oo-a240-smoke`, `oo-a240-chain`, `oo-a240-pg`, the a240 cargo, target, and pgdata volumes, network `oo-a240-eef831aa-net`, and `/tmp/oo-a240-eef831aa-dba4-4af2-a78b-541bc9e0a240`. Label checks use `index .Config.Labels "oo.owner"`. It does not prune and does not touch another owner. `cleanup/CLEANUP.txt` records that run: the a240 container, three volumes, network, and private tmp directory were removed. Pinned images stayed.
