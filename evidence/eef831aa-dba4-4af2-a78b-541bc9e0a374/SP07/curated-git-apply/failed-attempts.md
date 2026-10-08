# Failed attempts

All of these were inside the one 6g / 1 CPU Rust 1.94 container. The source checkout ref was not moved. The green run is test exit 0 and clippy exit 0.

1. Compile exit 101. `env!("CARGO_BIN_EXE_curated_git_apply")` is unset at compile time. `std::env::set_var` is unsafe on Rust 1.94. Unused imports in the library.
2. Process exit 1. `setpriv --init-groups` requires a passwd entry for uid 501. Replaced with `--clear-groups`.
3. Compile exit 101. `PathBuf` moved into the first `Command` and used again.
4. Test exit 101. Five tests failed, six passed. The wrapper called `env` with `PATH` limited to the wrapper directory, so `GIT_DIR` was missing from the log. The binary env name keeps a hyphen. The gitlink fixture commit ran `reference-transaction` before the product started. `git fsck` text was not used as the unreachable-object oracle.
5. Test exit 101. One test failed, ten passed. `worktree_dirty` was true on a clean tree because `diff-files --quiet` treats an empty read-tree stat as a change. Fixed with a private `update-index --refresh`.
6. Clippy exit 101. `collapsible_if` in `src/git.rs`. Library only. Tests had already passed.
7. Clippy exit 101. Test lints: argument count, needless borrow, needless lifetime, `collapsible_if`, `unnecessary_get_then_check`. Tests had already passed.

Pre-edit smoke of the same image, from the session before implementation, printed rustc 1.94.0, cargo 1.94.0, git 2.47.3, and exited 0. The green run printed the same versions again.
