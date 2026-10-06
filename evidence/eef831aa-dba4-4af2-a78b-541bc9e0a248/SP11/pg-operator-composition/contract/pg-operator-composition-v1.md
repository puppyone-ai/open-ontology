# PG operator composition v1

Evidence-only composition on fixed commit `e0704257a1509b968a80a9ccd2f200319d5483ad`.
This file does not accept a plan item, a development unit, or a component review.

## Chain

One private TLS PostgreSQL 17.10. Two tables, `shop.customer` and `shop.order_doc`. Two roles, `clerk` and `peek`, on one login. Source generation 48.

Order: `init-pg`, `import-pg` (import before init must refuse), `catalog-pg`, `captured-pg-draft`, `review-pg-draft`, `reviewed-effective-oos`, `reviewed-policy`, paste that policy array into `install-column-acl`, `provision_mapping_reader`, `analyze-pg`, then `query-pg`. Two `role-pack-bundle` outputs are produced from the effective document.

`reviewed-policy` receives the current mapping `config_digest` and `run_generation` on the binding. The install request pastes the policies array, including `"row_filter": null`. The query request omits that key when the value is null. It does not write a claims object.

`provision_mapping_reader` is the public library function. The harness does not execute a fixture `GRANT` for `_oo.physical_mappings`. Business columns are not hand-granted.

Reviewer `ada.owner` at `2026-10-06` is the classification record in the effective document. It is not authentication. Source `oo_a248_reader` and `oo_a248_writer` audit counts stay 0. Seed row counts stay 3.

## Not claimed

Semantic publication, natural language, authentication, authorization, a WAL, and a harness mount stay unclaimed. `published` on the analyze receipt stays false. M1-I01 and M1-I02 stay `not_accepted`. `parent_acceptance`, `development_unit`, and `component_approval` stay false. This run is not a component-review substitute.
