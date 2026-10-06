# Handoff

## This unit

Fixed archive `8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e`.

Contract commit `7f500ff0a61a42ea606a281dd931099091a52ef6`.
Implementation commit `8f4e792c4eca6eabb204de4c8bc09caa854165ec`.

Contract: `tools/description-review/CONTRACT.md`.

Workspace: `tools/description-review`. Binary: `description-review`.
Entry: `description-review apply`, with one `oo.builder.description-review-command/v1` document on stdin.

The library calls `oo_builder::complete_description_candidates` on the original command and may write `label`, `description`, and `grain` on an approved current `DatasetFile`.

Passing proof: `linux/proof-4`. Pre-edit smoke: `linux/preoverlay`. Both are Linux arm64 Rust 1.94, 6 GiB, 1 CPU, no PostgreSQL. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Review lock content SHA-256 `eed8dd34b188b9fa6a22d907373cab8242b48014d6dc21878c24828c01f011dc`.

Inventory: `SHA256SUMS`, written by `tools/sha256_manifest.rs` after the reports and cleanup record were closed. That tool is content-identical to the a199 blob `f21989651570187fc286462349d59174a6a6b8ef`. `tools/sha256.rs` is content-identical to the a253 blob `b69a2af13099ce4baf2d04e79c5692d9988ab67d`.

## Next child

A later gate owns publication, the customer-owner proof, a live provider, and full S5. This child records declared reviewer labels and dates. It does not authenticate them.

Do not treat `confidence` as approval or as a PII classification. `effect` on a candidate remains `review_only`. Caveats and open questions stay in the review queue. They are not new `DatasetFile` fields and this unit does not append to `DatasetFile.caveats`.

The fixed producer's `selected_datasets` order was observed, including one passing `description_selection_order` test on the archive. Full S5 acceptance of that sort is a different child.

## Do not widen this unit

Edits to the root lock, `oo-builder`, `oo-core`, column policy, semantic classification, or automatic acceptance are outside this child. A new expansion needs its own frozen contract before code.

This child does not claim parent acceptance, full S5, publication, or an owner authentication proof.
