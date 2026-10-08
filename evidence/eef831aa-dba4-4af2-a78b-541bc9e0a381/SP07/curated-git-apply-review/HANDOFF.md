# Handoff

Verdict: APPROVE_LIMITED for original E57-D1/1 at `576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6`.

Trust `linux/proof-2/test.stdout` for the four independent tests and `linux/proof-2/clippy.exit` for Clippy. Trust `linux/proof/` as the failed oracle compile, not as a product result. Trust `linux/smoke/` for the 1.94.0 identity check that ran before the proof compile.

The adapter entry remains `apply_curated_git(registry, request, deadline)`. The CLI is `curated-git-apply apply --root <registered-checkout>`. `OperatorRegistry` is chosen by the operator. The request cannot select another root, ref, author, or hook path. The only ref is `refs/heads/curated`. Dataset paths are `ontology/curated/datasets/<schema>/<table>.yaml`. Receipts keep `authenticated` false, `parent_auth` required, and `identity` declared_only.

A later B04 service can call this library after it has a real identity check. This package does not become that service and does not publish semantics.

Do not add the package to the root workspace. Do not cargo-update the lock. The product commit was not modified by this review.

Author evidence `351adf350b0901312e6e2d77e10c8debfd272e49` has no `SHA256SUMS`. Use `source-inventory.txt` in this directory for the fixed-commit blob ids, and `MANIFEST.sha256` for SHA-256 of this review's own files.

This review does not accept parent SP02, parent SP07, or the goal. It does not change acceptance counts.
