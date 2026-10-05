# Oracle

The deciding oracle is `overlay/role_pack_a178_review.rs`, copied onto a second extract of `git archive af3308c32fa9e90bb29fab970b691d57dbc35279` and executed as `cargo test -p oo-builder --test role_pack_a178_review` inside `linux/probe-2`. The pre-overlay smoke did not contain this file. `linux/smoke/overlay-absent.txt` is `absent`.

The subject is the `oo-builder` binary from that archive (`CARGO_BIN_EXE_oo-builder`). Library expectation is `compile_role_pack` on canonical JSON and again on `serde_json::to_string` of the same values. The test requires equal `artifact`, `pack_digest`, and `cache_key` across those two library calls. Stdin to the binary is one unsorted command document: `visibility`, then `effective_oos`, then `protocol`.

Stdout is parsed and compared with that `RolePack`:

- `protocol` is `oo.builder.role-pack-command/v1`
- canonical bytes of the whole stdout object equal the stdout string, with no trailing newline
- canonical bytes of `artifact` equal `RolePack.artifact`, and that string occurs in stdout
- canonical bytes of `manifest` equal `pack.file("manifest.json")`
- `pack_digest`, `manifest_sha256`, `cache_key`, and `cache_key_sha256` match the pack
- the command object and the artifact object have no `principal` field

Independent digest, using dev-dependency `sha2` 0.10.9 and a local canonical JSON writer. It does not call `oo_builder` hash helpers or `canonical::sha256_hex`:

1. SHA-256 of `manifest.json` bytes equals `manifest_sha256`.
2. SHA-256 of the cache-key string equals `cache_key_sha256`.
3. For each `artifact.files[]` entry, `pack.file(path)` equals `text`, SHA-256 of `text` equals `sha256`, and `bytes` equals the text length.
4. The entries become a path-sorted array of `{bytes, path, sha256}`. SHA-256 of that canonical JSON equals `pack_digest` and stdout `pack_digest`.

Two principals with the same role snapshot, `ada178` and `ada178b`, share the artifact and the pack digest and do not share the cache key. Audit role `oo_r_audit` has a different digest. The binary's working directory is an empty temp directory. The test counts the directory after both successful commands.

Observed on `linux/probe-2/out/outcome.json`, and already on the preserved `linux/probe/out/outcome.json`:

- clerk and ada2 `0adc296da6c6c882d4886792862b7eece542870e53d3673f3804f64a0a153e76`
- audit `409726ad03a0768bcd1097c115ffc69bb996c921ee610c2d05e490ef06301fe2`
- manifest `48553b60415c64f16d130b41bd5fb3eab5d3c766eb10d9d97c9c9df05cdec1ec`
- `hash_violations`, `kept_misses`, `hidden_hits`, and `status_mismatches` empty
- `temp_files` 0

Refusal cases assert the entire stderr line, exit 1 or 2, empty stdout, and the absence of the fixture marker. The row-security sentence `Cross-row samples and most_common values are omitted.` is legal. The forbidden token is `most_common_vals`.

The fixture is a yard pack with one visible metric and one hidden column. It is not the a176 closure suite. `role_pack.rs` was not edited. No dependency was added.
