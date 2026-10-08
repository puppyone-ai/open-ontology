# Findings

No product defect in the E57 path exercised by `tests/a381_review.rs`.

## Author evidence has no SHA256SUMS

Commit `351adf350b0901312e6e2d77e10c8debfd272e49` lists nine files under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a374/SP07/curated-git-apply/`. None is named `SHA256SUMS`. The author's `source-inventory.txt` is a Git blob listing for `tools/curated-git-apply`. This review generated its own listing with `git ls-tree -r 576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6 -- tools/curated-git-apply`. The blob ids are the same. No SHA-256 manifest was invented for the author tree.

## The immutable archive has no source Git directory

`tests/apply.rs` resolves the repo from `CARGO_MANIFEST_DIR` and asserts `.git` is a directory. The proof archive is `git archive` of the fixed commit plus the review oracle. It has no `.git`. Nine author tests panic with `source git dir`, and the hostile-env re-exec of the accepted-review test exits 101 for that reason. `cli_usage_and_unknown_field` does not need that directory and passed. Host `git rev-parse` already checked the pinned trees. The independent probe builds a private fixture repository and does not read the source checkout. The author-suite exit 101 is a harness limit of this archive.

## First proof compile

`oo-a381-proof` exited 101 before any product test ran. `tests/a381_review.rs:262` passed `&PathBuf` to `shell_single`, which takes `&str`. The oracle was corrected after that container was removed. The failed logs stay in `linux/proof/`.

## What the passing probe showed

The product cleared the Git child environment, used a private index, refused a mismatched blob, wrote only the approved invoice label, description, and grain, left the other semantic fields and the unrelated paths byte-identical, and published with compare-and-swap. Replay, conflict, lost reply, and two writers behaved as the contract states. The losing commit object remained. A dirty worktree and the shared index were not rewritten. Hooks did not run.

## Out of scope

Public B04 authentication, semantic publication, parent SP02, parent SP07, and goal acceptance are not claimed. Column v2 label behavior was not independently repeated. `description-pack` was not called. Postgres was not started.
