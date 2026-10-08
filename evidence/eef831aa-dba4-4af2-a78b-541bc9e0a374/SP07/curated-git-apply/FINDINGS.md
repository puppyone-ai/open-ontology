# Findings

## Git 2.47 and an empty index stat

`git read-tree <commit>` into a private index leaves stat fields empty. With `GIT_DIR` set, `git diff-files --quiet` then exits 1 for every tracked path and the raw diff shows a zero worktree SHA. Content can still match `git hash-object`. `git update-index --refresh -q` on that private index records the stat when the bytes match, and `diff-files --quiet` then exits 0. A real content change still exits 1. The main `.git/index` and the worktree are not updated. The receipt still says `require_fresh_checkout` and `worktree: not_updated`.

## Rust 1.94

`std::env::set_var` and `remove_var` are unsafe. The crate forbids unsafe. The accepted-review test re-execs that one test with `SSH_AUTH_SOCK`, `GIT_AUTHOR_NAME`, and `GIT_SSH_COMMAND` set on the child process. The Git wrapper log must not contain those names. Cargo exposes the binary as `CARGO_BIN_EXE_curated-git-apply` at runtime. `env!("CARGO_BIN_EXE_curated_git_apply")` is unset at compile time.

## Image

The Rust image has Git 2.47.3 and does not have the clippy component installed. Clippy was added inside the same container before `cargo clippy --locked`. `setpriv --init-groups` failed because uid 501 is not in `/etc/passwd`. `setpriv --reuid=501 --regid=20 --clear-groups` dropped the cargo process to the host uid. `Cargo.lock` is owned by that uid.

## CAS loser

Two requests with the same expected head and different idempotency keys: one `update-ref` wins, the ref moves once, and the other commit object stays unreachable. The test counts commit objects with `git cat-file --batch-check --batch-all-objects` and does not prune.

## Fixture hooks

The harness Git sets `core.hooksPath` to an empty directory so the fixture commit that plants a gitlink does not run `reference-transaction`. The product still points `core.hooksPath` at its own empty directory, clears the child environment, and disables signing, fsmonitor, and protocol. The hook marker stayed absent.

## Scope that this run does not claim

No model call, no Postgres, no customer, no publication, no grant, and no authenticated reviewer. `description-pack` is pinned and not called. `tools/selfbuild-control-api` was not modified. The root manifest and root lock were not modified.
