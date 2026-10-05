# Findings

Self-check against `contracts/builder/role-pack-bundle-v1.md` at blob `8aba171993c3c1f6fe3c139d7f02c19907579558`. This is the author check. It is not an independent review and it does not mark the ledger. SP07-W06 and O-D02 stay unaccepted. A written bundle is not a grant.

## What was already wrong

a214 `FINDINGS.md` records that the destination check and `rename` are not a lock, and that a destination appearing in that gap can be replaced on this platform. The contract at blob `ed645185db5f27802c5f84eb88ed911da7a3fbc1` assigned that race to the trusted parent. That assignment is removed. The parent path stays caller-supplied. It is still not a jail. That trust is the confinement statement. It is not a reason to replace an existing output name.

## Red result before the edit

`linux/preoverlay/race_red.rs` is Safe Rust and carries `#![forbid(unsafe_code)]`. It writes staging bytes `BUNDLE-a221-gap` and neighbor bytes `KEEP-NEIGHBOR`. It calls `symlink_metadata` on the destination. On `NotFound` it writes `SENTINEL-a221-gap` and then calls `std::fs::rename` of the staging path onto the destination. That is the frozen tail of `readback_and_rename`, with the gap written out. The compiled `role-pack-bundle` process is not stopped or patched. The frozen binary has no yield between those two calls, so the oracle runs the same two operations.

On the pinned image the destination bytes became `BUNDLE-a221-gap`. `race-result.txt` says `result=overwritten`, `staging_exists=false`, `neighbor_bytes=KEEP-NEIGHBOR`. `race.exit` is 1. `race-compile.exit` is 0. The smoke container exits 0 only when tests and clippy exit 0 and this race is that red result. A preserved sentinel would have failed the smoke.

The oracle ran on the container tmpfs. It was not run on the macOS evidence mount.

## What the green run established

Publication is `std::fs::hard_link(staging, dest)` at `link_no_replace`. The binary contains `#![forbid(unsafe_code)]`. `git grep` of the product commit finds no `rename(` in `role-pack-bundle.rs`. The proof script's `no-rename.exit` is 0 for the same two greps. There is no `libc` use and no new dependency. `Cargo.toml` and `Cargo.lock` are absent from the product commit.

`finish_owned` calls `readback_and_link`. On a link error it returns `prefer_cleanup`, which removes the staging file this attempt created and leaves the destination. On success it calls `cleanup_owned` on the staging name and returns that result. `cleanup_owned` treats `NotFound` as success. It does not remove the destination. A directory staging path returns `io.cleanup` and is not removed with `remove_dir_all`.

`link_failure` maps `ErrorKind::AlreadyExists` to `io.destination` / `the output path already exists`. It maps `ErrorKind::Unsupported` or raw code 1 (`LINUX_LINK_EPERM`) to `io.link` / `the filesystem does not support a hard link`. Every other link error, including the missing-parent case, goes to `io.link` / `the staging file could not be linked`. The messages are static. The function does not copy the operating-system text or the path.

`hard_link_shares_the_inode_and_then_drops_the_staging_name` passed in the container. After `hard_link`, the destination and the staging file share `ino()` and `nlink() >= 2`. After `cleanup_owned`, the staging name is gone, the destination bytes are `BUNDLE`, and the destination inode is unchanged. A `rename` publication would not leave both names.

`existing_name_is_not_replaced` writes `SENTINEL-a221-gap` before `hard_link`. The call returns `io.destination`. The sentinel, the staging bytes `BUNDLE`, and the foreign file `KEEP` stay. `symlink_destination_is_not_replaced` leaves the symlink in place and leaves `KEEP-REAL` on the target. `link_failure_removes_only_the_staging_file` uses a missing parent, returns `io.link` / `the staging file could not be linked`, removes the staging file, does not create the missing directory, and leaves `KEEP`.

`link_errors_use_static_sentences` passed. It feeds `Error::from_raw_os_error` for raw codes 1, 95, 17, 13, 2, and 18. Code 1 and code 95 take the unsupported sentence. Code 17 takes `io.destination`. Codes 13, 2, and 18 take `the staging file could not be linked`. The product source compares raw code 1 and `ErrorKind::Unsupported`. It does not compare the number 95. On Rust 1.94 the raw-95 error took the unsupported sentence, so that error's kind was `Unsupported` on this toolchain. No filesystem that refuses hard links was mounted. The container `/tmp` is tmpfs, and the inode test passed on `std::env::temp_dir()`.

The two compiled processes share the staging name `shared.json.role-pack-bundle-staging`. `create_new` on that name serializes them. `competing_processes_one_publishes_and_the_other_refuses` therefore also passes if publication is still `rename`. The cargo log says the test passed. It does not say which role won, and it does not say whether the loser printed `io.collision` or `io.destination`. Both of those lines are refusals. The shared file equals exactly one of the two reference bundles written earlier. The neighbor and both reference files stay, and the staging name is absent. The gap replacement is the pre-edit oracle. The gap refusal of a name that is already present is the unit tests above. `second_publish_keeps_the_first_bundle` is sequential: the second process exits `io.destination` and the first bundle bytes stay.

A staging path this attempt did not create still returns `io.collision` before `create_new` and is left in place. `publish_leaves_a_preexisting_staging_path` and `preexisting_staging_and_long_name_are_left_untouched` passed. `publish_removes_staging_when_destination_exists` passed, so an output that already exists still causes the owned staging file to be created and then removed. The command does not skip staging with a separate existence check. The contract's last sentence says an existing output name is `io.destination` from `hard_link`.

Help still says the command does not authenticate, grant, mount, tokenize, trust a cache, or publish. It still says the parent is a trusted path, not a jail, and that the parent directory is not fsynced. It now says publication is a same-directory hard link, an existing output path is not replaced, and a filesystem that does not support hard links fails publication. The help test requires those phrases. The `HELP` constant in the committed binary does not contain `rename`.

`field_span_keeps_braces_inside_strings`, `hash_bytes_matches_sha256_vectors`, and `two_roles_splice_compiler_bytes_and_independent_hashes` passed. The diff from `d209` to `0c6e6f1e` in the binary touches the help text, `finish_owned`, the rename tail, and the new link helpers and tests. The bundle document splice is outside that diff.

The contract drops the `io.rename` row and adds `io.link` with the two static messages. It says `hard_link` creates a second name for the same inode, does not copy bytes, and does not replace an existing name. It says there is no fallback to `rename`. It says a crash before the link can leave the staging file, and a crash after the link and before the staging name is removed can leave both names. It says the parent is not fsynced. It says this is not a publication barrier and not a power-fail guarantee.

## Bounds a reviewer should treat as specified

- Same-directory `hard_link` is the same-filesystem strategy. A cross-device path is `io.link` / `the staging file could not be linked`. There is no byte-copy fallback.
- The unsupported-filesystem sentence is the `ErrorKind::Unsupported` or raw-code-1 mapping. This session did not mount a filesystem that returns that error from a real `link(2)`.
- The parent directory is still a caller-supplied path. The command does not canonicalize, chroot, or rewrite `..`. It does not fsync the parent. A crash can leave the staging name, both names, or an undurable directory entry.
- If `remove_file` fails after a publish error, the returned code is `io.cleanup` and the earlier publish code is dropped. After a successful link, a cleanup failure returns `io.cleanup` and does not remove the destination.
- Two `role-pack-bundle` processes that share one output path also share one staging name. The two-process test shows one success and one refusal. It does not open the old check-then-rename window.
- `clippy --bin role-pack-bundle --test role_pack_bundle` exited 0. That command lints the binary without `cfg(test)` and lints the integration test. The eleven binary unit tests ran under `cargo test`, which does not pass `-D warnings`.
- Authentication, authorization, publication, mount, samples, domains, file splitting, search, a database re-proof of row security, cache trust, and a tokenizer stay outside this command.
