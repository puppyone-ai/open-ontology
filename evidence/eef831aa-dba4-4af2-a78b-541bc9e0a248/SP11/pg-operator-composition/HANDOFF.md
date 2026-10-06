# Handoff

a248 recorded a private TLS composition through role-pack bundles and then stopped. `reviewed-policy` exit 11 is the blocker. Do not treat this directory as acceptance of M1-I01 or M1-I02.

## Left on the table

The clerk allow list includes `shop.order_doc.amount` with confirmed semantic type `quantity` and physical type `int8`. `oo_gateway::reviewed_policy::compatible` rejects that pair, so the command writes no policies array. Without that array the harness cannot paste a DTO into `install-column-acl`, call `provision_mapping_reader`, run `analyze-pg`, or run `query-pg`.

A later change to `compatible`, or a different frozen classification of `amount`, is outside this evidence directory. This run did not edit product crates. Reclassifying `amount` inside the harness would disagree with the effective document already emitted.

The null `row_filter` seam is implemented in `src/policies.rs` and was not executed. There is no successful DTO to show `"row_filter": null` or the omitted query key.

## Preserved runs

- `linux/smoke/` pre-overlay smoke, exit 0, Rust 1.94.0.
- `linux/chain/attempt-1/` overlay build exit 101.
- `linux/chain/` second run, host exit 1, stop at `reviewed-policy-clerk`.
- `live/steps/` holds the compiler stdout, stderr, and exit files that were produced.
- `cleanup/CLEANUP.txt` records removal of `oo-a248-pg`, the cargo, target, and pgdata volumes, network `oo-a248-eef831aa-net`, and the private tmp directory.

Recipe origins and the current script blobs are in `origin/recipes.txt`. The frozen scope text is `contract/pg-operator-composition-v1.md`. `SHA256SUMS` is the inventory of this directory, produced by `tools/sha256.rs` after its empty-string and `abc` self-test. The inventory compiler is host `rustc`, not the 1.94 proof image.

## Next observation

Any retry needs a new chain attempt directory. `linux/chain/launch.sh` refuses to overwrite `console.log`. One builder and one private PostgreSQL. The PostgreSQL from this run has been removed, so a retry also needs a new `pg/up.sh` execution and new secret files.
