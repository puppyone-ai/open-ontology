# Structure draft review (a071)

Decision: **request_changes** for the pure structural compiler only.

| | |
|---|---|
| Commit | `4c989bce58407cf19d3d93dc600162d7322d92f8` |
| Tree | `691a56de066b08585ab7ef2f192caca8879e41f1` |
| Parent | `b14c7821031990aafd713f23068658d41042ee2d` |
| Subject | builder: compile catalog evidence into a reviewable structure draft |
| Archive | `/tmp/oo-a071-eef831aa-dba4-4af2-a78b-541bc9e0a071/archive` |
| Archive check | 817 blobs, missing 0, content mismatch 0, extra 0, mode mismatch 0 |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a071` |
| Product source | read-only git archive. No source edit, no commit, no push. |

The compiler under review is `compile_structure_draft` in `crates/oo-builder`, protocol `oo.builder.structure-draft/v1`, compiler id `oo-builder/structure-draft/v1`. The catalog adapter parses a captured `oo.source.pgsql/v1` document. `provenance.pg_catalog_integration` is the constant `not_accepted`. The contract commit recorded in every probed draft is `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`.

This decision does not accept a PostgreSQL import, and it does not complete SP07. Semantic classification, profile selection, owner RBAC, publication, role packs, and query execution stay unsigned. Docs 06, 10, and 17, the SP07 plan, and ledger item V01-OOS-01A describe a larger builder than this slice. The slice hardcodes `llm` `not_used`, `publication` `unavailable`, `certification` `none`, and `source_connection` `not_used`.

## Required changes

1. NFC composition drops a second combining mark of the same class. Unicode 16 NormalizationTest line 60 fails, and `unicode-normalization` 0.1.25 disagrees. The composition pair is present in the table.
2. `nfc_tables.rs` matches the Unicode 16.0.0 UCD and carries no Unicode copyright or permission notice. `NOTICE` records no third-party notice.
3. A fresh column rejection removes that column from the effective dataset and leaves the effective foreign key naming it.
4. A primary key may repeat one column. `["id","id"]` is emitted into generated and effective output.
5. The hand-written NFC also disagrees with `unicode-normalization` 0.1.25 on Unicode 17 sequences whose combining classes are absent from the Unicode 16 table. Concrete sequence: `U+0065 U+1ACF U+0323`.

Findings 1 and 2 are enough for `request_changes`. Findings 3 and 4 are structural defects in the emitted draft. Finding 5 is the remaining gap against the normalizer b2 actually pins.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`, `--pull never`. Toolchain override `RUSTUP_TOOLCHAIN=1.94.0`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0 (85eff7c80 2026-01-15)`.

Run `4ea98272-980a-44a2-8c89-8a90af4740df`, container `oo-a071-msrv-4ea98272-980a-44a2-8c89-8a90af4740df`. Own bridge, `--cap-drop ALL`, `no-new-privileges`, memory 8g, cpus 4, pids 4096, `HOME=/tmp`, private cargo and target volumes. No host network, no docker socket, no host home, not privileged. Exit 0. `OOMKilled=false`.

| Phase | Exit |
|---|---|
| `cargo fetch --manifest-path Cargo.toml --locked` | 0 |
| `cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder` | 0 |
| `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings` | 0 |

Test counts: library 18 passed, binary unit tests 0, integration `structure_draft_cli` 2 passed. The library golden test passed on the unmodified archive. Logs: `msrv/console.log`.

An earlier fetch that passed `-p` to `cargo fetch` exited 101 and is not the recorded smoke. Cargo 1.94 rejects that flag. The recorded command omits it.

## Independent probe

Run `bbc4cb3e-b57c-4002-ac34-83783490f6a0`, container exit 0, `OOMKilled=false`, same image and the same security flags, own bridge `oo-a071-probe-bbc4cb3e-b57c-4002-ac34-83783490f6a0-net`. The writable input was a copy of the archive plus one probe test, `crates/oo-builder/tests/independent_review.rs`. The archive itself was not mounted. At start, one foreign Rust container was running and this probe was the only owned builder.

`independent_exit=0`. Sixty-nine catalog envelopes plus four CLI cases. Library repeat equality held on all sixty-nine. `cli_equal` held on all sixty-nine: success stdout matched the library bytes, and draft errors produced empty stdout. Analyzer: `cases=69 results=73 failures=1`. The single failure is the probe expectation for `44-depth-32.json`. Nesting of 32 empty objects stays inside the depth budget and then fails `field.unknown` because the root key `k` is not an envelope field. Depth 33 returns `json.budget.depth`. That expected-code miss is a probe error.

Hashes were recomputed in Python from the documented preimages (`probe/hashing.py` logic, `hashlib.sha256`). The schema preimage is the hand-built `pg-catalog-hash/v1` string. Config, type, evidence, and manifest use sorted-key JSON with `separators=(",", ":")` and `ensure_ascii=False`. Manifest is the draft with `provenance.manifest_hash` removed, hashed, then inserted. Compiler hash functions were not called. Every successful draft matched those digests. No probe draft or error contained `DO_NOT_SURFACE_INSTRUCTION_9f3a`, `owner_role_do_not_leak`, or `publish_all_rows`.

Checked digests:

| Draft | schema_hash | manifest_hash |
|---|---|---|
| Empty `{"tables":[]}` | `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc` | `afccf1a27d3a55d4c3e33a4c35be811c69903e78cbb0a9b3afd0d9c3eb8dd892` |
| Customer | `60f7939195cc539644d11132ae4a604c3b025167f1fe657d121167feb8463499` | `3ce302dde9df01663e4de5a77708789ce02c0002d02a1f96be2c73750a219920` |
| Shop | `2871541a741986d678281238d59a5cadac3a3fb0f34536df65821fc69681c603` | `e98269fc9fcd0c45af52f93e5a986bdc2fce75eff7e733e0c7e3256a7dd8d1cc` |
| FK parent outside selection | `858e8ce3fa02bb685755f882a2933fd5ec5b1170949d8c01ffaadd02a14979b7` | `632fc58965a6f678a611159fb86a9720848118cd9a92cf35302a7e499eab83db` |

Customer bindings: config `fcaf671132ab4dd63d56d0ea590d755b72a9010d946c40905caf632cfa31e2f2`, evidence `d555fd4f24cb3c59ff33181c9703629b68c4daa88570af667456c28795ef91ff`, type `00599f24a5a34e6e93725a14c5847a4460c0ec6cc8ac8d80266390c0e7f861ca`. Source `synthetic` / `app`. `captured_at` `2026-10-02T00:00:00.000000Z` is outside the schema preimage and inside the manifest.

Shop reorder bytes match the shop draft. Primary-key member order changes the draft. Composite foreign-key column order changes the draft. Composite `(tenant_id, account_id) -> (tenant_id, id)` stays `many_to_one`. Ambiguous pairs stay separate. A parent outside the selection emits no relationship. An unverified referenced column list is emitted and queued `fk_target_key_unverified`, which is what the contract describes.

CLI, from the probe binary:

| Case | Exit | Stderr |
|---|---|---|
| bare command | 2 | `oo-builder: process entry is not implemented (SP00-W01 skeleton)` |
| unknown command | 2 | `oo-builder: unknown command` |
| extra argument | 2 | `oo-builder: structure-draft reads JSON from stdin and takes no arguments` |
| invalid UTF-8 | 1 | `oo-builder: oo.builder.catalog-evidence/v1/json.syntax at /: stdin is not UTF-8` |

Review controls that held on the probe set: fresh accept marks the queue item accepted, leaves `publishable` false, and does not copy curated semantic fields into effective. Fresh reject of `column/public.customer/id` removes the column and strips the dangling primary key, with queue kind `primary_key_column_not_effective`. A stale reject keeps the column and records `stale_decision`. Duplicate decisions return `duplicate.decision`. Unknown fresh items return `review.unknown_item`. PII and whole-dataset decisions return `review.not_in_scope`. A publish action returns `review.action`. Curated items and fields that are not in the generated draft return `curated.unknown_item` or `schema.invalid`. Altered schema, config, type, and evidence hashes return static `hash.*` messages. An altered version returns `version.unsupported` with the static text `schema, config, type, or evidence version is not supported`. Altered source and selection return `source.mismatch` and `selection.mismatch`. Raw comment, default, and policy bodies return `schema.invalid` and the probe token does not appear. Shop mood keeps label count 1 and `enum_labels_sha256` `615fe7ad91dfb0f976c05c5705c33d2fafbb63b4b8d01274c67a455ad2c62373`. The label text is absent from the draft. Owner and privilege text from the discard case are absent. `publishable` stays false.

Direct `oo-builder` dependencies in `Cargo.lock` are `oo-core`, `oo-jobs`, `serde` 1.0.229, and `serde_json` 1.0.151. The builder sources do not name `oo_jobs`, `sqlx`, `reqwest`, `postgres`, `tokio`, or a model client. `llm` is the literal `not_used` in `draft.rs`. `oo-jobs` depends only on `oo-core` and is a skeleton. `oo-core` pulls non-network crates (serde, jsonschema, sha2, chrono with `std` only, and the pins listed in its manifest). Those transitive crates are real. The builder is not dependency-free.

## Finding 1. NFC composition blocks an equal-class mark

`crates/oo-builder/src/nfc.rs` `compose` (lines 80-103) sets `previous_ccc = class` after a successful compose. UAX #15 blocks a mark when an earlier character has a combining class greater than or equal to that mark. A consumed mark must not become that blocker. The crate `unicode-normalization` 0.1.25 leaves the previous class unchanged when the mark is absorbed. This file updates it anyway.

`nfc_tables.rs` line 12005 contains `(274, 768, 7700)`, which is `U+0112 + U+0300 -> U+1E14`. The pair is not missing.

`U+1E14` decomposes to `U+0045 U+0304 U+0300`. The first mark composes with E to `U+0112` and the class update then blocks the grave accent. The result is `U+0112 U+0300`.

Linux probe against the published crate and the Unicode 16 test file (`probe/raw/nfc-run.txt`, `nfc_exit=1`):

```
unicode_normalization_version=17.0.0
U16_TEST rows=19965 bad_slots=3830
U17_TEST rows=20034 bad_slots=3898
scalar_scan seen=1112064 mismatches=191
MISMATCH NormalizationTest.txt:60 slot=2 src=0045 0304 0300 expected=1E14 builder=0112 0300 reference=1E14
MISMATCH NormalizationTest.txt:61 slot=0 src=0112 0300 expected=1E14 builder=0112 0300 reference=1E14
```

The same line-60 failure is present for the source scalar `1E14`. Line 62 yields builder `0112 0300 0304` where both references yield `1E14 0304`.

These inputs match the reference and are not counterexamples: `customer`, `U+00E9`, `e` + `U+0301`, `U+00C5`, `A` + ring, `U+212B` to Å, Hangul L+V and L+V+T, `e` + acute + dot-below to `U+1EB9` + acute, `U+0344` to `U+0308 U+0301`, `고객`, `客户`.

The in-tree unit test covers that matching set and does not cover `U+1E14`.

Repair: stop updating `previous_ccc` when a composition is consumed, then rerun Unicode 16 `NormalizationTest.txt` and the 0.1.25 crate on the same vectors. A version-constant comment is not the defect.

## Finding 2. Copied Unicode tables have no redistribution notice

`nfc_tables.rs` is 214143 bytes, sha256 `794de9ec4ad9048a188b6174d467b044ac71d50a9e8f099c66ecd41bf272b338`. The header says the tables were generated from Unicode 16.0.0. There is no generator script in the tree, no Unicode copyright, and no permission notice.

An independent reconstruction from the official UCD, after splitting the decomposition pool on the array literal rather than on every integer in the file, matched Unicode 16.0.0 exactly:

| UCD file | sha256 | bytes |
|---|---|---|
| UnicodeData.txt | `ff58e5823bd095166564a006e47d111130813dcf8bf234ef79fa51a870edb48f` | 2175362 |
| CompositionExclusions.txt | `89e83cf9cc8bef6c1f8bf77e42cf6f0341dfa42e66261f4dbe9b492e7a23c8ee` | 9007 |
| DerivedNormalizationProps.txt | `4d4c03892dea9146d674b686e495df2d55a28d071ac474041d73518f887abddc` | 1372269 |
| NormalizationTest.txt | `d811971453e7075e1ad56fb1b301eece5aa80757b81f6156e74a1bfb3ae5ceb1` | 2798671 |

Decomposition, combining class, and primary composition pairs: equal, missing 0, extra 0. Hangul syllables are algorithmic and absent from the decomposition table. `U+0344` and `U+212B` decompose and are excluded from composition, which matches the exclusion rule. The bytes are reproducible from that UCD. The missing in-tree generator is a provenance gap, and it is not a table mismatch.

`NOTICE` says third-party notices are none recorded, and it defers a per-artifact notice to an M1 artifact that does not exist. The Unicode data license requires its copyright and permission notice with copies or substantial portions of the data files, or in documentation that accompanies them. A "Unicode 16.0.0" sentence is not that notice. `nfc.rs` itself is sha256 `7527bd98b33ad4776c5a11d8619d9074fad88091d079b6cf828a1f530458d6a2`. `LICENSE` is Apache-2.0, sha256 `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`. `NOTICE` sha256 `463cbfaba4a8bc4e494e2daf307334b7cee314f10a137e06b6bbe43c502026fc`.

## Finding 3. Effective foreign key keeps a rejected column

Contract `contracts/builder/structure-draft/README.md` says a reject removes that item from `effective` only, and that a dangling effective primary-key name is removed and queued `primary_key_column_not_effective`. `strip_dangling_keys` in `draft.rs` (lines 1979-2020) implements that primary-key sentence. `validate_relationships` in `oo-core` `oos/model.rs` (lines 1057-1067) checks only that the two ends have the same non-empty length.

Probe `27-fk-endpoint-rejected.json`, library exit 0, CLI bytes equal, repeat equal. Effective `public.orders` columns are `id`, `ship_to`, `amount`, `zip`, `mood`. `bill_to` is absent. Queue item `column/public.orders/bill_to` is `rejected`. Effective relationship `bill_to_fkey` remains `status` `draft` with `from.columns` `["bill_to"]` and `to.columns` `["id"]`. The relationship queue items stay `pending` (`candidate_relationship` and `fk_ambiguous`).

The effective document names a column the effective dataset does not contain. The contract sentence does not require this closure. The repair is to drop or reject a relationship end whose column is not effective, and to say so in that contract paragraph. Primary-key stripping on the customer fresh-reject case did work.

## Finding 4. Repeated key members are accepted

`validate_key_columns` in `pgsql.rs` (lines 1062-1091) checks budget, attnum, and name. It does not reject a repeated member. Duplicate column attnums outside a key are rejected (`duplicate.attnum`).

Probe `55-duplicate-pk-member.json` accepts `primary_key` `["id","id"]` into both generated and effective output. Exit 0. The contract does not name a within-key error. A key that lists one column twice is not a usable structural key. Reject it with a static error that does not echo the name.

## Finding 5. Unicode 17 combining classes are absent

The builder tables match Unicode 16.0.0. Compared with Unicode 17.0.0, decomposition and composition pairs still match, and 34 combining classes are missing from the builder (sample code points 6863 and the following scalars; full list in `probe/raw/nfc-tables-report.txt`). `combining` returns 0 for an absent class, so those marks behave as starters.

`unicode-normalization` 0.1.25 prints `UNICODE_VERSION` `(17, 0, 0)`. Crate file sha256 `5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8`. License `MIT OR Apache-2.0`. Direct dependency `tinyvec` 1.13.3, crate sha256 `fd3ca314f692efd6c868f8408f53fe444634a845f96c028b97d35f6a1f79f0ee`, no further dependencies. The crate's own tables carry a Rust project copyright and no separate Unicode data notice.

Concrete mismatch, from the same NFC run:

```
extra_sequences rows=238 mismatches=36
MISMATCH extra src=0065 1ACF 0323 builder=0065 1ACF 0323 reference=1EB9 1ACF
```

The same shape continues through `U+1ADA`. The reference composes `e` + `U+0323` to `U+1EB9` and leaves `U+1ACF` because that mark's Unicode 17 class sorts around the dot-below. The builder never sees the class.

Unicode 17.0.0 file hashes used for the table diff: UnicodeData.txt `2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c` (2198209 bytes), CompositionExclusions.txt `2f239196ef3b5b61db5cc476e9bd80f534d15aa1b74e1be1dea5d042a344c85f` (9007), DerivedNormalizationProps.txt `71fd6a206a2c0cdd41feb6b7f656aa31091db45e9cedc926985d718397f9e488` (1377582), NormalizationTest.txt `5019ffd530751a741900c849c0e010332f142a3612234639bd200b82138a87db` (2827429).

Repair for identifier compatibility with b2: use the same normalizer, or carry these classes and fix finding 1. Declaring Unicode 16 does not make `U+1E14` pass.

## Boundaries that are not defects

Numeric typmod follows the contract and b2 `numeric_parts`: values below 0 have no parts; otherwise precision is `(typmod - 4) >> 16` and scale is `(typmod - 4) & 0xFFFF`. PostgreSQL's `make_numeric_typmod` stores a signed 11-bit scale, so `numeric(10,-2)` is typmod 657410 and the unsigned decode is precision 10 scale 2046. Probe `10-numeric-unsigned-scale.json` accepts typmod 657410 with claimed scale 2046 as `known_builtin_not_converted`, `data_type` `numeric`. Probe `56-signed-scale-claim.json` rejects claimed scale -2 with static `field.invalid` `numeric precision or scale does not match typmod`. Typmod 0 is accepted as precision 65535 scale 65532 with `data_type` null (`unbounded_or_wide_numeric`). This slice does not implement the signed PostgreSQL scale. Integration remains `not_accepted`.

Identifier length is explicit. Physical names are not truncated. A 63-byte non-safe column (`A` repeated) maps to a 128-byte `q_` ident and enters the OOS column because `TableName` allows 128 bytes. A 31-byte uppercase table maps to a 64-byte `name_ident`. A 31-byte uppercase schema maps to a 64-byte `schema_ident`, the dataset is not emitted, and the queue kind is `identifier_not_in_oos` because `SourceName` stops at 63 bytes. PostgreSQL `NAMEDATALEN-1` is 63. This slice emits no DDL, so those longer idents are not target DDL.

Author goldens are the compiler's own checked-in bytes. The smoke run's full library suite passed `draft::tests::checked_in_goldens_match_compiler_bytes` on the clean archive. Independent truth for this review is the Python preimage, which matched those drafts. `finish_goldens` (`draft_tests.rs` lines 881-909) writes when `OO_WRITE_DRAFT_GOLDENS` is set to any value, including an empty one, and then compares. Only that test calls it.

The negative check that ran inside the first probe used a filter without `--lib` and reported 0 passed with the test filtered out, exit 0. A later container with `--lib` and the short name plus `--exact` also filtered all 18 library tests and exited 0. Neither run executed the assertion. The run that did is `d8431bf6-f1bb-4fb1-8377-763d8ad410d7`:

- `OO_WRITE_DRAFT_GOLDENS` unset (`PHASE write_var=unset`)
- one trailing space on the writable copy of `examples/ontology-draft/goldens/empty.draft.json`
- corrupted sha256 `0838f791b4dab730ba77fe8d21a355a51cff43799ba4049ada29d44d7a16b623` before and after the test
- `draft::tests::checked_in_goldens_match_compiler_bytes` panicked at `draft_tests.rs:896` (`goldens/empty.draft.json drifted from the compiler`)
- cargo exit 101, container exit 101, `OOMKilled=false`
- file restored to sha256 `47e31be7ac19a1fa2cdd862be4dfa21de3520de23e7693ad779ccd175f69add1`, identical to the archive

Normal validation failed closed and did not bless the altered golden. A set variable would write. That path was not executed.

`oo-jobs` is an unused direct dependency of this slice. It is not a functional defect.

## Evidence

| Path | sha256 |
|---|---|
| `probe/raw/phase1.jsonl` | `ea306a65eb3e73f7ac73656aa7e9fafcc7e7acdebee1e40876b26773ed521429` |
| `probe/raw/analyze.txt` | `5ef029960157f854f4103f9da7b0ce350d7301540da59fdccdceb4b024a954ee` |
| `probe/raw/nfc-run.txt` | `792db41d070a383aa82f5d468cde73bb54476757c38918201a9370b0fce57b5d` |
| `probe/raw/nfc-tables-report.txt` | `146938d1c6031ea02241faa21b8ce37cdaaf4719fc95c75a2530c14dfe3df639` |
| `msrv/console.log` | recorded with the smoke exits above |
| `probe/golden-exact/console.log` | the exit 101 golden run |

Raw exits, inspect, and host config are under `msrv/`, `probe/`, `probe/golden/`, `probe/golden-lib/`, and `probe/golden-exact/`.

## Cleanup

After the reports were written, this session removed only resources labeled `oo.owner=a071` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a071` whose names start with `oo-a071-`. Five containers, five networks, and two volumes (`cargo`, `target`) were removed. Every `docker rm`, `docker network rm`, and `docker volume rm` exited 0. No prune. The owned label filter was empty afterward. Foreign containers `oo-a074-pg-54714c6c` and `oo-a072-pg1710-7c1a0e2b-4f63-4b0a-9c2d-11a072fence01` were still running. The log is `cleanup.txt`.
