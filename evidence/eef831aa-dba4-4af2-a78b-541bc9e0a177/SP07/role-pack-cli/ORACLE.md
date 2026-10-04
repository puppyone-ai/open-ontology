# Oracle

The deciding oracle is the Rust test `two_roles_match_the_library_pack_and_drop_hidden_metadata` in `crates/oo-builder/tests/role_pack_cli.rs`, executed inside `linux/commit` against `git archive af3308c32fa9e90bb29fab970b691d57dbc35279`.

Library side: `compile_role_pack` on canonical JSON, then again on `serde_json::to_string` of the same values. The test requires equal `artifact`, `pack_digest`, and `cache_key`.

Binary side: `oo-builder role-pack` with the unsorted command document on stdin. Stdout is parsed and compared with that `RolePack`:

- `protocol` is `oo.builder.role-pack-command/v1`
- `pack_digest`, `manifest_sha256`, `cache_key`, and `cache_key_sha256` match the pack
- canonical bytes of `artifact` equal `RolePack.artifact`
- canonical bytes of `manifest` equal `pack.file("manifest.json")`
- the artifact object has no `principal` field

Independent digest, using dev-dependency `sha2` 0.10.9 and a local canonical JSON writer. It does not call `oo_builder` hash helpers or `canonical::sha256_hex`:

1. SHA-256 of `manifest.json` bytes equals `manifest_sha256`.
2. SHA-256 of the cache-key string equals `cache_key_sha256`.
3. For each `artifact.files[]` entry, `pack.file(path)` equals `text`, SHA-256 of `text` equals `sha256`, and `bytes` equals the text length.
4. The entries become a path-sorted array of `{bytes, path, sha256}`. SHA-256 of that canonical JSON equals `pack_digest` and stdout `pack_digest`.

Observed on `linux/commit/out/outcome.json`:

- clerk `1ab9c3bc971f87aab7d71b400432de2aacee3804579b4870fa93e5cd19854522`
- audit `7add1ff8fa967b47e6cab4ac3a187ed885e68b86902e5d1b05a59201f14242be`

Clerk files in the artifact: `INDEX.md`, `README.md`, `calendar.md`, `examples/verified.sql`, `glossary.md`, `manifest.json`, `metrics.md`, `relationships.md`, `tables/wharf.crate.md`. There is no `tables/wharf.scale.md` in the clerk artifact.

The same test process is the absence oracle. Hidden markers and the stale policy version are required to be missing from stdout or stderr. The row-security sentence `Cross-row samples and most_common values are omitted.` is legal. The forbidden token is `most_common_vals`, which keeps the a176 probe-1 false positive from being repeated.
