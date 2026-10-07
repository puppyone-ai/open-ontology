# ORIGIN

Recipes were read from this worktree. They were not re-fetched. Hashes below are `shasum -a 256` of the current files. Blob ids are `git rev-parse HEAD:path` at parent `d22187ea0fce8e420323660841f2da3b447d5de8`, which matches fixed base `a13b1cc80ed66c07199a41d47b31fd42530f2be9` for `sha256.rs`.

## SHA-256

| Path | Commit that last changed the file | SHA-256 | Use |
|---|---|---|---|
| `tools/cdc-offset-store/src/sha256.rs` | `b5ee3f4c7abad6a78341ac5b06ee21a19b7bea6e` | `eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037` | Source. Blob `1800a66e987cbfbd86eb83ba983cfc4a31e1bcc1` at the fixed base and at the parent. |
| `tools/review-ledger/src/sha256.rs` | this crate, `f862683945a6e5a8803e3f9450d0f51cde5a98ad` | `eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037` | Byte copy. `cmp` was identical. |

Empty digest `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` and `abc` digest `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` are the unit and contract oracles.

## TLS shape

`tools/cdc-offset-store/src/tls.rs` last changed in `b5ee3f4c7abad6a78341ac5b06ee21a19b7bea6e`, SHA-256 `53cf121fab7ebe702d8f1f04bf29fd4c7a7fe0f0556da83e2b405a562e769906`.

`tools/review-ledger/src/tls.rs` SHA-256 `55f2a832859edf0b8686bec38b9f1b0eea4a065d925cec719439176217b1fbd2`. The connect shape follows the cdc file at the fixed base: rustls, aws-lc-rs, empty root store, caller CA, hostname verification, no client certificate. The hide-commit wrapper is not copied. A lost commit reply is `commit.unknown`.

## PostgreSQL wrapper

`deploy/trusted-demo/pg/pg-wrap.sh` last changed in `b91b67bee0f603725b2cb7cc9c284588a2699392`. Current file SHA-256 `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`. The comment inside that file records an older origin hash `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` and says the script came from a274, which records a254. The proof mounted the current file read-only. It did not check out the comment hash.

`deploy/trusted-demo/pg/up.sh` last changed in `10b78993641c88154c75c22219c79c1566e2cecd`. SHA-256 `904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff`. The proof did not execute `up.sh`. It reused the same image digest and the wrapper's server flags inside `proof/launch.sh`, with UUID-private names.

## Compiler entry points

No description-review, description-pack, oo-builder, or oo-core source was edited. The live review receipt calls `description_review::apply_description_review`. The pack refusal calls `description_pack::apply_description_pack`. `tests/support/world.rs` builds one description-review command through those crates. It is not a byte copy of another file. Clippy removed one needless borrow at `serde_json::to_value`.

Amendment `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b` adds `pack_fixture` in the same test helper. It calls `apply_description_review` and then `apply_description_pack` on a command built from that review fixture. The live receipt stores the compiler document hash. Those compiler crates were not edited. `sha256.rs` is still the byte copy above.

Candidate protocols accepted by the ledger are `oo.builder.description-candidate/v1` and `v2`. Review command `oo.builder.description-review-command/v1`. Review outcome `oo.builder.description-review/v1` and `v2`. Compiler id `oo-builder/description-review/v1`. Pack command `oo.builder.description-pack-command/v1`. Pack protocol `oo.builder.description-pack/v1`. Compiler id `oo-builder/description-pack/v1`.
