# Handoff

## This unit

Fixed archive `182e2137ee261ed889b1aff5ada6e505a4a17723`.

Contract commit `56710cc73cfe1663272ecaa503984e1f663428ad`.
Implementation commit `aba067efe4b19bae868e15e23157ebbcf0a4969f`.

Contract: `tools/description-provider/CONTRACT.md`.

Workspace: `tools/description-provider`. Binary: `description-provider`.
Entry: `description-provider run --config PATH`, with the description-request command on stdin.

The library calls `prepare_description_request`, then one reqwest 0.13.5 HTTPS chat completion, then `complete_description_candidates`.

Passing proof: `linux/proof-7`. Pre-edit smoke: `linux/preoverlay`. Both are Linux arm64 Rust 1.94, 6 GiB, 1 CPU, no PostgreSQL. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Provider lock content SHA-256 `d6b28e14bceb66512cc44acbbdce9af84754c860bef199384eb0b1662f9258cb`.

Inventory: `SHA256SUMS`, written by `tools/sha256_manifest.rs` after the reports and cleanup record were closed. That tool is content-identical to the a199 blob `f21989651570187fc286462349d59174a6a6b8ef`. `tools/sha256.rs` is content-identical to the a253 blob `b69a2af13099ce4baf2d04e79c5692d9988ab67d`.

## Next child

A later gate owns a real provider, the residency owner, and a cost certificate. This child only reports the caller's explicit approval and a declared price times reported tokens. Both certificate flags stay false.

Do not treat `confidence` as approval or as a PII classification. `effect` remains `review_only`. The compiler statements stay `not_used`, `not_approved`, and `not_metered`. A transport failure must keep the structural draft and emit no candidate.

The fixed producer's `selected_datasets` order was observed, including one passing `description_selection_order` test on the archive. Full S5 acceptance of that sort is a different child. a262 was not edited here.

## Do not widen this unit

Cache, retry, streaming, a second endpoint, raw samples, customer rows, and edits to the root lock or `oo-builder` are outside this child. A new expansion needs its own frozen contract before code.

This child does not claim parent acceptance, full S5, publication, or a billing certificate.
