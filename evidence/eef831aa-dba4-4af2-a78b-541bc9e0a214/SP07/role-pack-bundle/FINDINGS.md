# Findings

Self-check against `contracts/builder/role-pack-bundle-v1.md` at blob `ed645185db5f27802c5f84eb88ed911da7a3fbc1`. This is not an independent review and it does not mark the ledger.

## What the green run established

`bundle_document` slices `artifact` and `manifest` from the compact command result with a string-aware scanner. It re-parses those slices and requires them to equal the parsed fields. The process test requires the bundle to contain the exact `RolePack.artifact` and `manifest.json` bytes from `compile_role_pack`, with `pack_length` and `manifest_length` equal to those byte lengths. `pack_digest` is copied from the command result. The test requires it to differ from `pack_sha256` on these two packs. `manifest_sha256` equals both `hash_bytes` of the spliced manifest and `RolePack.manifest_sha256`.

Snapshot key order is `catalog_version`, `knowledge_id`, `knowledge_version`, `ontology_version`, `policy_id`, `policy_version`, `project`, `role`. The values are read from the manifest object. Status key order and the eleven required values match the contract table, including `row_security_reproven` false and `tokens.status` / `tokens.estimator`. Token budgets stay inside the spliced manifest. The top-level key list is the thirteen contract fields in sorted order. There is no trailing newline.

Publish order matches the contract. A staging path that already exists returns `io.collision` before `create_new`. `create_new`, write, flush, and `sync_all` happen before the destination check. A destination that exists then causes `remove_file` on the staging file this attempt created. `remove_file` on a directory returns `io.cleanup` and does not call `remove_dir_all`. The 2 second directory-mtime test is the process evidence that the staging file was created and then removed. The collision test is the process evidence that a staging path this attempt did not create is left in place.

Request handling stops at `DOCUMENT_MAX_BYTES + 1`. The oversize code is `oo.builder.role-pack-bundle/v1/json.budget.bytes`, not the compiler's stdin budget code. Non-UTF-8 is this command's `json.syntax` line `the request file is not UTF-8`. Compiler rejections keep `RolePackCommandError.code` and the static message. Diagnostics do not include the request body, the path, or the rejected argument. Help names `compile_role_pack_command`, caller-supplied public data, the refusal to authenticate, grant, mount, tokenize, trust a cache, or publish, and the parent as a trusted path rather than a jail.

The clerk pack still contains the compiler sentence `has no fixed tokenizer`. That sentence is inside the spliced artifact. The bundle does not add a tokenizer claim, a mount claim, or an authentication claim.

## Defects fixed before the green run

1. `linux/proof/attempt1`. `File` implements `Read` and `Write`, so `by_ref` did not resolve. The reads now call `Read::take`. The unused `std::io` import was removed.
2. `linux/proof/attempt2`. The integration test called `expect` on `RolePackCommandError`, which has no `Debug` and lives in the library this task does not edit. The test now matches the error and panics with `code` and `message`. The hash compare uses iterator equality instead of `GenericArray::as_slice`.
3. `linux/proof/attempt3`. The binary unit test called `expect` on `Fail`. `Fail` now derives `Debug`. Behavior of the published file is unchanged.

## Bounds a reviewer should treat as specified

- The output parent is a caller-supplied path. The command does not canonicalize, chroot, or rewrite `..`. It does not fsync the parent directory. A crash before `rename` can leave the staging file. A crash after `rename` can still lose the directory entry until the parent is durable.
- The destination check and `rename` are not a lock. A destination that appears in that gap can be replaced by `rename` on this platform. The contract assigns that race to the trusted parent.
- Symlink refusal is `symlink_metadata` and then no follow. There is no `O_NOFOLLOW` open. The file mode follows the process umask.
- If `remove_file` fails after a publish error, the returned code is `io.cleanup` and the earlier publish code is dropped.
- `pack_length` is the artifact JSON length. It is not `total_bytes`.
- `clippy --bin role-pack-bundle --test role_pack_bundle` exited 0. That command lints the binary without `cfg(test)` and lints the integration test. The seven binary unit tests ran under `cargo test`, which does not pass `-D warnings`.
- Authentication, authorization, publication, mount, samples, domains, file splitting, search, a database re-proof of row security, cache trust, and a tokenizer stay outside this command. A written bundle is not a grant.
