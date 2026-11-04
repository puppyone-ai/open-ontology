## Card G-S3b — lane G: publisher integration and the remaining Self-build gaps

TODO: S3-G-02 (and S2-G-01 follow-ups). Rebase on v0.2/product-loops first; your S2/S3 commits stay on your branch.
1. Apply your own proposal `tools/pg-builder/PUBLISHER-INTEGRATION.patch` to crates/oo-gateway/src/library_policy.rs
   (granted for this card), with tests, and remove the patch file. The reviewed-community envelope must be required
   whenever a producer marks community review, and must match the effective policy and visibility.
2. Hierarchical summaries (column → table card → community → domain) feeding the Pack README/INDEX entry point.
3. Price/time preflight: from the budgeter's dry-run split, print batches, estimated input/output tokens, cost range
   (price table from config; unknown → stated as unknown) and duration at the current concurrency, before any call.
Tests for each; one small live run through model-egress (bounded). Work rule: smallest local workaround and keep going;
commit as you go.
