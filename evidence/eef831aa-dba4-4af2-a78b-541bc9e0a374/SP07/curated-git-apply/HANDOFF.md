# Handoff

Package: `tools/curated-git-apply`. Library entry: `apply_curated_git(registry, request, deadline)`. CLI: `curated-git-apply apply --root <registered-checkout>`. Deadline is 30 seconds and is not a request field.

`OperatorRegistry` is trusted and is not parsed from the request. The request `project_root` must strict-walk to that same directory. The only ref is `refs/heads/curated`. Dataset paths are `ontology/curated/datasets/<schema>/<table>.yaml`. The caller cannot set the Git author. Reviewer text on the receipt is a label, with `authenticated` false and `identity` `declared_only`.

A later authenticated B04 service can call this library after it has a real identity. This adapter does not become that service. It keeps `parent_auth` required.

Replay of the same idempotency key and the same request bytes returns the same receipt and does not create a second commit. The outcome string stays `committed`. The same key with a different request returns `idempotency.conflict` and does not move the ref. A dirty worktree is left as it is. The caller needs a fresh checkout to see the new commit. There is no reset, clean, or checkout update.

Do not add this package to the root workspace. Do not cargo-update the lock. Path dependencies are the pinned oo-core, oo-builder, and description-review trees.

Green Linux proof: implementation `576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6`, test exit 0, clippy exit 0, builder notes in `raw/`. Failed attempts are in `failed-attempts.md`. Those tests were not rerun for this supplement.

`SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the sidecar, the walker digest of that list. `source/SHA256SUMS` and `source/SHA256SUMS.sha256` are the same walker over `tools/curated-git-apply` at the implementation commit. `source/pin.txt` is the commit and tree pin. Git blob ids stay in `source-inventory.txt`. The walker is `linux/hash-oracle/main.rs`, byte-identical to the a365 file named in `recipe-origin.txt`. It checks empty and `abc`, skips only the evidence-root `SHA256SUMS` and `SHA256SUMS.sha256`, and writes the sidecar. Host `shasum` is not this inventory.

`linux/hash/` holds the source-hash container log and its exit. A second container rewrites `SHA256SUMS` after those logs exist. Its console is not stored in this tree. `CLEANUP.txt` names the containers and the temp directory removed with this run.
