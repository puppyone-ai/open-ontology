# Reviewed PostgreSQL composition

Evidence contract for session `eef831aa-dba4-4af2-a78b-541bc9e0a226`. This file freezes what the harness may claim. It does not accept M1-I01, M1-I02, or any parent goal.

The fixed product archive is `ed661809cf6c1e8df37af5842838dd1eb839c98a`. The harness calls the binaries and public functions already in that archive. It does not edit a product crate, a root manifest, or a root lock.

## Chain

One private TLS PostgreSQL 17.10 holds source database `oo_a226_src` and target database `oo_a226_dst`. The fixture is `fixture/source.sql`: `shop.customer` and `shop.order_doc`. Typed gold lives in the Rust harness, separate from that SQL text.

Required compiled order:

1. `catalog-pg` on the source reader.
2. `captured-pg-draft`, then `review-pg-draft` with structural accepts only.
3. `reviewed-effective-oos` with explicit decisions bound to that artifact's schema hash and structural-column fact fingerprints.
4. `role-pack` for `oo_r_clerk` and `oo_r_peek`, then `role-pack-bundle` once per role. A second link to the same output path must fail and leave the first file in place.
5. `init-pg`, then `import-pg`. The import state is `awaiting_publish`. `import-pg` before `init-pg` is the not-ready refusal.
6. `install_column_acl_set` once per role, both tables, columns and classification copied from the emitted effective document.
7. `analyze_mapped_shadows` after the import, on the target connection only.
8. `load_policy_catalog`, then `query-pg` for both tables, including `count(*)`, star, and a two-turn typed read.

`private_note` has no decision. It stays unclassified. Direct `SELECT` of its physical column and a direct `INSERT` return SQLSTATE 42501. `query-pg` guard refusals exit 6 and create no audit file. Successful reads have a local admit record and then a complete record.

## Labels that are not acceptance

The fixture login, the `GRANT role TO login`, and `explicit_allow` on a visibility allowlist are configuration. They are not authentication and not authorization.

`USAGE` on `_oo`, `SELECT` on `_oo.physical_mappings`, and `EXECUTE` on `_oo.lookup_physical_mapping` are a known library seam. `install_column_acl_set` does not grant them. The harness may add them only after recording their absence, and only as fixture configuration.

Semantic publication, authentication, the audit WAL, natural language, and a harness mount stay missing. A local JSONL audit file is not that WAL. `pg_publication` stays empty. No business column is granted by hand. No successful pack or confirmed label is written except by the compilers above.
