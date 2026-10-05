# Findings — a178 role-pack command

Verdict: **APPROVE LIMITED** the `oo-builder role-pack` command on `af3308c32fa9e90bb29fab970b691d57dbc35279`, tree `ef9b913640afd14cb87c9e79baaa321ebb205560`.

No product line is required to change for this command. The deciding artifacts are `linux/probe-2/out/outcome.json` and the pack files under `linux/probe-2/out/{clerk,ada2,audit}/`. `linux/probe/` is a preserved harness miss. It is not a second product result. `linux/smoke/` is the pre-overlay archive.

The measure is contract `contracts/builder/role-pack-cli-v1.md` on this commit, against the library contract `contracts/builder/role-pack-v1.md` accepted by a176. REQ-OOS-05 and `docs/06-ontology-layer.md` section 7.1 remain the pack measure. This review did not reopen that campaign. `role_pack.rs` is blob `1327bad464fe8f510cc7baf8566ad71e3d00df2f`.

## What the command does

`compile_role_pack_command` (`crates/oo-builder/src/role_pack_cli.rs` lines 52–77) scans with `oo_core::parse_json_value`, rejects an unknown top-level field, requires `effective_oos` and `visibility` to be objects, canonicalizes those two objects, and calls `compile_role_pack`. It does not contain a second closure, SQL scan, or pack hash.

`map_scan` (lines 80–97) keeps the scanner code and replaces the message. Byte, depth, node, and duplicate-key codes get fixed sentences. Every other scanner failure, including trailing data, becomes `role-pack command JSON is not valid`. `map_document` (lines 100–112) classifies serde's English `unknown field` and `missing field` phrases and does not print serde's text.

`From<RolePackError>` (lines 30–36) copies `code` and `message` and drops `path`. `command_document` (lines 115–146) splices `RolePack.artifact`, `manifest.json`, and the pack's digest fields in sorted key order. `require_compiler_status` (lines 149–174) refuses to print a manifest whose six statuses moved. `require_pack_files` (lines 177–205) checks artifact digests and each file's text and byte length against `pack.file`. It does not compute a second digest.

The binary (`crates/oo-builder/src/bin/oo-builder.rs` lines 75–94) rejects an argument before reading stdin, then prints `oo-builder: role-pack: <code> <message>`. An unknown command (lines 96–99) prints `oo-builder: unknown command` and does not print the name. Usage (lines 103–131) lists `structure-draft`, `captured-pg-draft`, `review-pg-draft`, and `role-pack`, and says the visibility object is caller-supplied public data and is not a live permission proof. `read_stdin` (lines 250–261) stops at `DOCUMENT_MAX_BYTES + 1` and uses the existing catalog-evidence codes for a read failure and for non-UTF-8. `lib.rs` line 12 is `#![forbid(unsafe_code)]`. The new module's only `unsafe` match in the smoke scan is that attribute.

## Fresh binary

`linux/probe-2/out/outcome.json` records empty `hash_violations`, `kept_misses`, `hidden_hits`, and `status_mismatches`, and `temp_files` 0. Clerk and ada2 share digest `0adc296da6c6c882d4886792862b7eece542870e53d3673f3804f64a0a153e76`. Audit digest `409726ad03a0768bcd1097c115ffc69bb996c921ee610c2d05e490ef06301fe2`. `principal_artifact_shared` is true. `cache_keys_differ` is true.

Clerk cache key (`out/clerk/cache_key.txt`):

```text
oo.builder.role-pack-cache/v1|project=yard|principal=ada178|role=oo_r_clerk|policy=pol-yard@3|knowledge=know-yard@1|ontology=ontology-v1|catalog=catalog-v1|compiler=oo-builder/role-pack/v1|pack=0adc296da6c6c882d4886792862b7eece542870e53d3673f3804f64a0a153e76
```

Ada2's key is the same line with `principal=ada178b` and the same pack digest. Neither artifact contains `ada178`, `ada178b`, or `aud178`. Stdout has no trailing newline. Canonical bytes of the parsed stdout equal the stdout string, and the embedded artifact bytes equal `RolePack.artifact`.

The independent oracle hashed each file with `sha2` 0.10.9, built the path-sorted `{bytes, path, sha256}` array, and matched `pack_digest`. It hashed `manifest.json` and the cache-key string the same way. Manifest SHA-256 is `48553b60415c64f16d130b41bd5fb3eab5d3c766eb10d9d97c9c9df05cdec1ec`.

## Visible text, hidden text, PII, row security

Clerk `tables/yard.bale.md` line 1 is `# yard.bale — 货包`. Line 4 is `grain: one row per bale`. Line 5 is `` primary key: `bale_id` ``. Line 7 is `Row security: rls. Cross-row samples and most_common values are omitted. Global statistics are omitted. This compiler did not re-prove row security.` Line 8 is `Estimated row count is omitted.` Lines 12–14 are the allowlisted rows: `bale_id` / `包号` / `本季到港重量`, `mass_kg` / `净重` / `unit kg`, and `badge` / `工牌` / `Samples are omitted because pii is personal. badge note stays`. Line 18 is `None.`

`metrics.md` line 5 is `## kept_mass — 净重合计`. Line 8 is `` default anchor: `month_start` ``. Line 9 says reconciliation is not in the pack and `control_total` was not executed. Line 16 is:

```sql
SELECT mass_kg AS total FROM yard.bale
```

The metrics file says reconciliation is not executed and `control_total` is not claimed. `glossary.md` lines 5–9 keep `净重口径` mapped to `yard.bale`. `calendar.md` line 14 keeps `本季: month_start .. today`. `examples/verified.sql` is the compiler's empty sentence: `No verified example is visible for this role.`

Clerk stdout has no `vault_bin`, `舱记`, `秤台`, `yard.scale`, `pier.slip`, `泊位单`, `717171`, `292929`, `hidden_mass`, `owner-a178-do-not-copy`, `slip-hidden-a178`, `bale_scale`, `reading_kg`, `scale_id`, `most_common_vals`, or `pii-sample-do-not-echo-a178`. The row-security sentence still contains the token `most_common`. The absence check uses `most_common_vals`.

Audit `tables/yard.scale.md` lines 1–8 keep `# yard.scale — 秤台`, `Row security: none. This compiler did not query the database.`, and `estimated rows: 41`. Audit stdout has no `yard.bale`, `包号`, `vault_bin`, `舱记`, `badge`, `工牌`, `717171`, `292929`, `泊位单`, `pier.slip`, `kept_mass`, `hidden_mass`, the owner marker, `本季到港重量`, or `personal`.

Clerk `manifest.json` is one canonical line. `authentication`, `authorization`, and `publication` are `not_claimed`. `mount` is `not_performed`. `samples` is `omitted`. `domains`, `file_splitting`, and `search` are `unsupported`. `row_security_reproven` is false. `tokens.estimator` is `none` and `tokens.status` is `unsupported`. `role` is `oo_r_clerk`. The manifest has no principal. The file list is `INDEX.md`, `README.md`, `calendar.md`, `examples/verified.sql`, `glossary.md`, `metrics.md`, `relationships.md`, and `tables/yard.bale.md`. `tables/yard.scale.md` and `tables/pier.slip.md` are absent from this role.

The process was started with its working directory empty. After both successful commands the directory was still empty.

## Forwarding and bounded input

These probe-2 stderr lines are the whole diagnostic. Each command exited 1 with empty stdout. The marker, the stale version, and the compiler path are absent.

- `out/stale.stderr`: `oo-builder: role-pack: oo.builder.role-pack/v1/binding.policy the snapshot policy version does not match the effective document`
- `out/compiler.stderr`: `oo-builder: role-pack: oo.builder.role-pack/v1/binding.compiler the visibility snapshot is not bound to this compiler version`
- `out/effective-version.stderr`: `oo-builder: role-pack: oo.builder.role-pack/v1/protocol.mismatch the document protocol is not the role-pack input this compiler accepts`
- `out/sample.stderr`: `oo-builder: role-pack: oo.builder.role-pack/v1/unsupported.field that field is not a role-pack input and was not interpreted as authority`
- `out/nested.stderr`: `oo-builder: role-pack: oo.builder.role-pack/v1/schema.rejected the effective document does not match the role-pack contract`
- `out/version.stderr`: `oo-builder: role-pack: oo.builder.role-pack-command/v1/protocol.mismatch the command protocol is not the role-pack command this entry accepts`
- `out/duplicate.stderr`: `oo-builder: role-pack: oo.builder.role-pack-command/v1/json.duplicate_key duplicate JSON object key`
- `out/unknown.stderr`: `oo-builder: role-pack: oo.builder.role-pack-command/v1/field.unknown role-pack command has an unknown field`
- `out/trailing.stderr`: `oo-builder: role-pack: oo.builder.role-pack-command/v1/json.syntax role-pack command JSON is not valid`
- `out/depth.stderr`: `oo-builder: role-pack: oo.builder.role-pack-command/v1/json.budget.depth JSON nesting exceeds the depth budget`
- `out/nodes.stderr`: `oo-builder: role-pack: oo.builder.role-pack-command/v1/json.budget.nodes JSON node count exceeds the budget`
- `out/oversize.stderr`: `oo-builder: role-pack: oo.builder.role-pack-command/v1/json.budget.bytes document exceeds the byte budget`

The oversize diagnostic does not contain `1048576` or `1048577`. The scanner's own byte message includes those counts (`crates/oo-core/src/json_scan.rs` lines 15–24). The command replaced it. Trailing data does not contain the scanner phrase `trailing data`. Non-UTF-8 stdin is `oo.builder.catalog-evidence/v1/json.syntax` with `stdin is not UTF-8`, which is the shared reader the contract names.

No arguments exits 2. `out/usage.exit` is `2`. Usage contains the four command names, `compile_role_pack`, `caller-supplied public data`, `not a live permission proof`, and `does not write a file, mount a pack, authenticate, grant, or publish`. An extra `role-pack` argument exits 2 with `out/extra.stderr`: `oo-builder: role-pack reads one oo.builder.role-pack-command/v1 JSON document from stdin and takes no arguments`. The argument is absent. An unknown command whose name is the marker exits 2 with `oo-builder: unknown command`.

`structure-draft`, `captured-pg-draft`, and `review-pg-draft` each exit 2 on an extra argument and do not print it. A bad document exits 1 with empty stdout and without the marker. Captured stderr starts `oo-builder: captured-pg-draft:`. Review stderr starts `oo-builder: review-pg-draft:`. Structure-draft stderr is the existing `DraftError` display, `oo-builder: oo.builder.catalog-evidence/v1/field.unknown at /: catalog evidence envelope is not oo.builder.catalog-evidence/v1`, and it does not mention `role-pack`. The author suites for those three commands passed beside the oracle: 3, 4, and 4 tests.

## Harness miss, preserved

`linux/probe/clippy.stderr` is one overlay diagnostic: `assertions_on_constants` at `role_pack_a178_review.rs:866`, `assert!(JSON_MAX_DEPTH < 40)`. Author tests exited 0. The oracle exited 0. Clerk digest on that run matches probe-2. The product bytes did not change. Probe-2 removes that assert. Its clippy exit is 0. The failing overlay is `linux/probe/role_pack_a178_review.rs`.

## Still open

The visibility object is trusted public data. This command does not prove it against a live permission system. Serde classification matches the English phrases `unknown field` and `missing field`. A change in that wording falls through to `schema.invalid` with a static sentence. The review command uses the same style.

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, row-security re-proof, and publication gates G1–G13 stay open. This review does not approve SP07-W06. No ledger checkbox was marked. No release was cut. No product file was edited.

A visibility snapshot is an input. It is not authentication and it is not a grant. A successful command is not SQL execution. This command does not query PostgreSQL and does not re-prove row security.
