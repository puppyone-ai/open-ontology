# Gateway compiled RolePack input

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a341`. Frozen before the loader edit. This is the GW-D02/E45 completion for the pack the HTTP process loads. It does not accept E45, and it does not add a unit.

The a337 review at `51d08e59ddb3c92fc0b9f2958966b56e19032b7f` approved the loopback HTTP child for an operator catalog only. That catalog treated `pack_digest` as any 64 lowercase hex characters. Replacing it with `ab` repeated 32 times still loaded. The fixture had no `compiler` field and `oo-builder` was not run. That path is not this contract.

## What the process loads

`pack_dir` contains one regular file per pack. The file name is the existing `artifact_name` rule. The bytes are the sealed `RolePack.artifact` string from `oo-builder` `compile_role_pack` (`crates/oo-builder/src/role_pack.rs`, `seal`). The gateway does not call the compiler, does not link `oo-builder`, and does not invent a second digest.

The artifact object is exactly these keys:

| Key | Required value |
|---|---|
| `protocol` | `oo.builder.role-pack/v1` |
| `compiler` | `oo-builder/role-pack/v1` |
| `pack_digest` | 64 lowercase hex |
| `manifest_sha256` | 64 lowercase hex |
| `entry_count` | integer count of `files` |
| `total_bytes` | integer sum of file UTF-8 lengths |
| `files` | array of `{bytes, path, sha256, text}` |

`text` is the file body. `bytes` is the UTF-8 length of `text`. `sha256` is SHA-256 of those bytes, lowercase hex. This is `payload_sha256` (`crates/oo-core/src/api.rs`): SHA-256, no other encoding.

One file is `manifest.json`. Its `protocol` is `oo.builder.role-pack-manifest/v1`. Its `compiler` equals the artifact compiler. The manifest also carries `project`, `role`, `policy.id`, `policy.version`, `knowledge.id`, `knowledge.version`, `ontology_version`, `catalog_version`, and `files`. Identity tokens use the compiler patterns: a project, policy id, and knowledge id are slugs; a role is `oo_r_` plus a slug tail; versions are the compiler version token. The manifest `files` array is `{bytes, path, sha256}` for every file except `manifest.json`, in path order. That is the array `seal` captured before it inserted the manifest.

The manifest status stays the compiler status: `authentication`, `authorization`, and `publication` are `not_claimed`; `mount` is `not_performed`; `samples` is `omitted`; `row_security_reproven` is false. The loader does not treat those strings as authentication.

`README.md` is required. These lines occur once and match the manifest:

- `compiler: \`oo-builder/role-pack/v1\``
- `project: \`<project>\``
- `ontology_version: \`<ontology_version>\``
- `role: \`<role>\``

The README contains the manifest role token once. Another `oo_r_` token is a role mismatch. A `tables/*.md` row whose pii cell is `unknown` is the same failure: the compiler refuses unknown PII instead of emitting it.

## Pack digest

`pack_digest` is the SHA-256 of the canonical JSON array of `{bytes, path, sha256}` for every file, including `manifest.json`, sorted by path.

Canonical JSON is the byte copy of `crates/oo-builder/src/canonical.rs` `canonical_json` at fixed commit `cd8f754c1b4db22fc962370c4b29bb846b0fccc3`: object keys are sorted, arrays keep the order already built, strings use the short escapes in that function. The loader does not define a new preimage. `serde_json::to_string` is not the digest.

## Refusals before listen

`load_operator_dir` calls `PackCatalog::load_dir` before `serve`. Any error below becomes the process error `packs`. The process does not write `listen.addr`.

| Condition | `PackError.reason` |
|---|---|
| `compiler` key absent | `legacy_unsupported` |
| `compiler` present and not `oo-builder/role-pack/v1` | `compiler` |
| file `sha256` differs from the text, `manifest_sha256` differs from `manifest.json`, or the manifest `files` list differs from the other files | `source_digest` |
| two files use one path | `path_duplicate` |
| a file is larger than 262144 bytes, the total is larger than 1048576, there are more than 128 files, or `entry_count` / `total_bytes` disagree with the files | `budget` |
| declared `pack_digest` differs from the recomputed digest, including a 64-hex substitute | `pack_digest` |
| README role differs from the manifest role, another `oo_r_` token appears, or a table pii cell is `unknown` | `role_mismatch` |
| project, ontology version, catalog version, policy, knowledge, or compiler line disagrees | `binding` |

The ceilings are the compiler constants `MAX_FILE_BYTES`, `MAX_TOTAL_BYTES`, and `MAX_ENTRIES`. The visibility snapshot can set a lower limit. That lower limit is not stored in the artifact, so the loader enforces the compiler ceiling and the declared totals. Paths follow the compiler structural rules (relative, no `\\`, `:`, empty segment, `.`, or `..`, at most 256 bytes). The loader does not renormalize NFC and does not add `unicode-normalization`. The compiler already stored NFC paths.

`fixture_artifact` returns `legacy_unsupported`. It does not emit a digest. An operator JSON file without `compiler` is the same refusal. It is not rewritten into a compiler artifact.

## HTTP tuple

The request path is unchanged. The bearer is hashed and resolved by the accepted token store. Project, principal, role, policy version, and knowledge version come from that tuple. Caller headers that disagree are rejected. The pack is selected by project, role, policy version, and knowledge version. The metadata response returns the recomputed `pack_digest` and the verified `{path, bytes, sha256}` list. It does not return file text. Column names are the backtick names on `tables/*.md` rows, so a role sees only the columns the compiler wrote.

No new grant, session kind, production SQL, or auth header is added. `POST /v1/sql` stays unsupported. The 5 second positive cache, revocation after that deadline, outage after that deadline, body limit, request timeout, and global concurrency stay as they are.

## Proof fixture

`fixtures/effective.json` is a public synthetic `oo.builder.effective-oos/v1` document. The visible columns carry a confirmed classification and an approved review. `unclassified_pii` has no PII level. `fixtures/visibility-picker.json` and `fixtures/visibility-auditor.json` are `oo.builder.role-visibility/v1` snapshots. Neither allowlist names `unclassified_pii`. `fixtures/visibility-hidden.json` allowlists that column so the compiler refuses it.

`src/fixture.rs` is a std-only driver. It writes an `oo.builder.role-pack-command/v1` document around those inputs and runs the fixed `role-pack-bundle` binary. The gateway file is the bundle's raw `pack` span. The driver does not recompute `pack_digest` and does not write a file when the compiler refuses. The live test then issues opaque tokens on the private PostgreSQL, reads `/v1/pack/metadata`, and checks the compiler digest, the role's columns, the absent unclassified name, and a tampered artifact that exits before listen.
