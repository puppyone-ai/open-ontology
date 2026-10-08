# Findings

No product defect in the E57 path exercised by `tests/a381_review.rs` or by the nine `tests/apply.rs` cases in `linux/author-9`.

## Author evidence has no SHA256SUMS

Commit `351adf350b0901312e6e2d77e10c8debfd272e49` lists nine files under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a374/SP07/curated-git-apply/`. None is named `SHA256SUMS`. The author's `source-inventory.txt` is a Git blob listing for `tools/curated-git-apply`. This review generated its own listing with `git ls-tree -r 576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6 -- tools/curated-git-apply`. The blob ids are the same. No SHA-256 manifest was invented for the author tree.

## Source Git metadata for the nine author cases

`linux/proof-2` had no `.git`. Nine author tests panicked at `source git dir` there. `linux/author-9` initialized an unborn Git directory only in `/tmp/a381-eef831aa-archive-author9`. The workspace head and the fixed tree were the same before and after that init. All 843 archived product files kept their blob ids (`bytes-match.txt`). The nine cases then passed, including the hostile-env re-exec. `cli_usage_and_unknown_field` was already green and was not repeated. `pinned_trees_match_the_reviewed_commit` stayed filtered. The four independent tests were not on this archive and were not repeated. Clippy stayed at the proof-2 exit 0.

## First proof compile

`oo-a381-proof` exited 101 before any product test ran. `tests/a381_review.rs:262` passed `&PathBuf` to `shell_single`, which takes `&str`. The oracle was corrected after that container was removed. The failed logs stay in `linux/proof/`.

## What the passing probe showed

The product cleared the Git child environment, used a private index, refused a mismatched blob, wrote only the approved invoice label, description, and grain, left the other semantic fields and the unrelated paths byte-identical, and published with compare-and-swap. Replay, conflict, lost reply, and two writers behaved as the contract states. The losing commit object remained. A dirty worktree and the shared index were not rewritten. Hooks did not run.

## Out of scope

Public B04 authentication, semantic publication, parent SP02, parent SP07, and goal acceptance are not claimed. `column_label_stays_inside_classification` passed in the author-9 rerun. `description-pack` was not called. Postgres was not started.
