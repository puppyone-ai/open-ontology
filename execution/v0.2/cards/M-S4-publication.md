## Card M-S4 — lane M: publication follow and Gateway pointer switch

TODO: S4-M-01. Lane label: `m`. Spec: REQ-TOK-05, AT-TOK-03. Docs: docs/product/04 §4.5, docs/07-gateway.md, docs/04-pipeline.md
(publication), tools/release-coordinator/, crates/oo-gateway/src/{pack_reload.rs, publication_binding.rs, public_surface.rs}.
Rebase on v0.2/product-loops first. Work rule: smallest local workaround and keep going; commit as you go.
Deliver: each sync cycle that loads new data through oo-load publishes a new data generation; the Gateway switches its
publication pointer atomically to the latest verified generation without restart; in-flight queries finish on the
generation they started on (REPEATABLE READ snapshot or generation-pinned views), new queries see the new one;
`/v1/freshness` reports the data-as-of time of the active generation; old generations are retained per policy and
released only when no query holds them. Publication binding must survive the switch (no PUBLICATION_DRIFT for the
supported path). Exit test: three consecutive cycles on a PostgreSQL fixture with a client running queries continuously
across the switches — zero query errors, results consistent with the generation each query started on, freshness
advancing each cycle; plus one MySQL and one Oracle cycle. Record evidence.
Write scope: tools/release-coordinator/**, crates/oo-gateway/src/{pack_reload.rs,publication_binding.rs,query_executor.rs}
and new files for generation pinning, crates/oo-pipeline publication glue, tools/oo-load publication step, tests.
Lane B owns the auth files in oo-gateway — do not edit auth_lifecycle.rs.
Added by E191: in deploy/lite, the scheduler/worker cycle must run `oo-load sync` for the configured stack instead of the
legacy `job-worker native-fire` path (its parent admission lives in `jw_native`, which oo-load does not write), then the
data-only Builder cycle and the publication step. Builder stays a one-shot job; `oo status` reports it as such. Exit adds:
after one scheduled cycle, `oo status` reports all seven services/jobs healthy (deploy/lite/README.md "Status").
