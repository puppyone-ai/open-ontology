# S5-D04 description review

## Scope and verdict

This child adds a standalone Rust workspace at `tools/description-review`. The library and the `description-review apply` stdin command take one `oo.builder.description-review-command/v1` document. The command carries the original `complete_description_candidates` command, including its captured response, plus the expected canonical outcome SHA-256, the current schema and evidence hashes, a curated `DatasetFile` map, and per-dataset approve, reject, or defer decisions. Each decision carries a declared reviewer label and date and the request, card, schema, evidence, and candidate hashes.

`apply_description_review` re-executes `oo_builder::complete_description_candidates` on that immutable command string and requires the outcome bytes to hash to the expected SHA-256. A successful apply requires `effect=review_only` and outcome `candidates`. The supplied candidate JSON is not the trusted source. An approval may write `label`, `description`, and `grain` onto the matching typed `DatasetFile`. Core type and schema checks refuse an invalid write. Text is not truncated. Caveats and open questions stay on the decision and the pending queue. Existing curated descriptive text stays unless that field is an explicit approve overwrite with the current value hash. A stale anchor leaves that dataset unchanged and records a stale queue item. Confidence does not approve a write. PII, semantic type, column and table ACL, status, entity, and metric fields are not inputs and are not rewritten.

The output is canonical JSON: merged `DatasetFile` documents, decision provenance, the review queue, and `review_sha256`. The source artifact is not mutated. The library does not open a database or write a file. Reviewer labels are declared. Public UI and owner authentication are not claimed.

This report does not claim parent acceptance, full S5, publication, residency, or a customer-owner proof.

Contract commit `7f500ff0a61a42ea606a281dd931099091a52ef6`. Implementation commit `8f4e792c4eca6eabb204de4c8bc09caa854165ec`. The contract commit is an ancestor. The implementation parent is `862521bd949225137601161d96224b2f5f3ffd34`.

## Fixed source

Fixed commit `8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e`. Fixed tree `06de4166bf04332543ad0ffc0076692b950f58dd`. The product proof builds a selective git archive of that commit and overlays only `tools/description-review`. The archive blob of `crates/oo-builder/src/description_candidate.rs` is `f783ae95f1f7398da18b6c47bcc87941db49f7a3`, matching the fixed commit. The root `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. No existing source, library, bin, manifest, or root lock was edited. a207 remains the connector manifest and root lock writer.

`repo-head.txt` at the proof-4 launch records `f57e430f51b9f44175ffab5ae876cd64dd558c78`. Later commits on the shared branch moved HEAD. The proof compiled the working-tree overlay, which the implementation commit records. The child lock in that commit is byte-identical to `linux/proof-4/review-Cargo.lock`.

The review contract was committed before the implementation.

## Recipes

Launch and report shape follow reviewed recipes. The UUID, fixed commit, owned paths, 6 GiB limit, and 1 CPU limit are this child's values. a200 commit `a587031c719a484830d78e493605f48e2056cae4` is the private PostgreSQL recipe. This unit starts no database, so that recipe was not applied.

| Origin | Commit | Path | Git blob |
|---|---|---|---|
| a204 overlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh` | `03c8e15c60a376ef0a757c062ebd110063a0ffe6` |
| a258 proof inside | `1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a258/SP07/description-candidate-cli/linux/proof/inside.sh` | `c967e03f8afa3082d4c342c11e58f09ff023ec0b` |
| a199 manifest tool | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `f21989651570187fc286462349d59174a6a6b8ef` |
| a253 file hasher | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/tools/sha256.rs` | `b69a2af13099ce4baf2d04e79c5692d9988ab67d` |
| a263 lock subset | `efdffed8` tree file | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a263/SP07/description-provider/tools/lock_subset.rs` | `6bae8ee573146f576a15cad6712892bca797a243` |

`tools/sha256.rs` and `tools/sha256_manifest.rs` are byte-identical to those blobs. Content SHA-256 `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` and `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `tools/lock_subset.rs` changes only the path allow-list to `description-review|oo-builder|oo-core|oo-jobs`. Its git blob is `52a4c40fc1a329a8b4eb6176691e825ea81725aa`. Content SHA-256 `6d561716d69aeec55739dbf976007b2066ac6674e7caf414c277a301d6aeec2b`.

Proof-2, proof-3, and proof-4 are the same scripts with the evidence directory and container name advanced so a failed `console.log` is not overwritten.

## Pre-edit smoke

`linux/preoverlay` on the pinned Linux arm64 Rust 1.94 image, 6 GiB, 1 CPU, private network, dropped capabilities, no pull. The archive contained `description_candidate.rs` and did not contain `tools/description-review`.

The first offline fetch exited 101 on a cold cargo volume. An online fetch then exited 0, and the following offline fetch exited 0. `cargo update` was not invoked. The root lock hash was unchanged.

- `oo_builder` lib tests named for the candidate constants: 2 passed, 33 filtered out
- `description_candidate` tests: 10 passed
- `description_candidate_cli` tests: 5 passed
- `description_selection_order`: 1 passed
- `cargo clippy -D warnings` on those targets: exit 0
- `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false

`rustc` reported `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Memory was 6442450944 and NanoCpus was 1000000000. Container id `c7b25878d1c8a30c0f43977ee5161a0f087490d4052180fb27e50d7338ec0450`.

## Decisive verification

`linux/proof-4` is the passing run. The container copied the fixed archive, overlaid the review tree, and generated the child lock with `cargo fetch` from a copy of the root lock. It did not run `cargo update`. `tools/lock_subset.rs` reported `registry=117 path=4 missing=0 mismatch=0`. A later offline fetch used `--locked`. The root lock hash after the run was still `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

- library and binary unit tests: 0 tests, exit 0
- `tests/review.rs`: 16 passed, 0 failed, 0.52s
- doc-tests: 0 tests, exit 0
- `cargo clippy --locked --offline --all-targets -- -D warnings`: exit 0, no warning or error lines, finished in 16.03s
- `proof.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false

Same image, 6 GiB, 1 CPU, `CapDrop` ALL, tmpfs `/tmp` with `nosuid,nodev,noexec`. Container id `8149518d444e41c7d230b12188fd29337bd598d384891455722fd82083b8513f`. Network `oo-a277-eef831aa-net`. `cargo-update-invoked.txt` is `no`.

The 16 integration tests build a fresh sealed physical `public.A` / `public.b` capture and call `complete_description_candidates`. They approve one dataset and reject the other, defer, leave a missing decision unchanged, require a current-value hash before overwrite, keep a second dataset when one anchor is stale, stale the set when the current schema or evidence hash moves, leave a provider failure body out of the review document, reject a wrong outcome hash and a compiler error with no document, refuse an injected classification key without echoing the token, apply a 256 / 4000 / 256 candidate whole, and refuse a backtick description without a partial write. The CLI test compares stdout with the library document. Merged documents parse and serialize as `DatasetFile`, and `validate_dataset` is empty. Field preservation compares the whole typed document after the three descriptive fields are set to the expected values.

Content SHA-256 of the committed tree, from `tools/sha256.rs` on the pinned image, matches `linux/proof-4/review-Cargo.lock` for the lock (`eed8dd34b188b9fa6a22d907373cab8242b48014d6dc21878c24828c01f011dc`).

| File | Content SHA-256 | Git blob |
|---|---|---|
| `.gitignore` | `44c92e3a70ad3307b7056871c2bdb096d8bfa9373f5bf06a79bb6324a20ff2fb` | `ea8c4bf7f35f6f77f75d92ad8ce8349f6e81ddba` |
| `CONTRACT.md` | `8c12d2a46e0c37c197d1d2c4959774b1f5fb57f20ae815a06d2e6261de6d1a12` | `117922f16914f5594435ceb574960ac0753f1ab1` |
| `Cargo.lock` | `eed8dd34b188b9fa6a22d907373cab8242b48014d6dc21878c24828c01f011dc` | `c9a7448b4878e48d3d2191ff552647a8af255b44` |
| `Cargo.toml` | `75408b1c0d01a7b0c923d560b1de5b1594b86692da0eb32d6d541e73093fb4d1` | `433f2952aa01016bcd6186a9b8be2296b1dce0f7` |
| `src/apply.rs` | `24137358a89c6db87ffbcdcc02abd4f86a218945170136fa5e91a0a6bad05255` | `4c1af85fb1579083a4e261c4a8b8a3318e281ac1` |
| `src/canonical.rs` | `0552989bac3a25d0a1c0cda64b0f8125f9d26068690b609bf8ed45846030086f` | `d4f4e1f7ec997019038a0ff30fc066c049c6eaa1` |
| `src/error.rs` | `6e60ce376d9ce83d0a792ee26b2afe3aa2b9df91626c7f4835e41f58315161c8` | `23eab5e1e1423dbeef439f3d9b3f83f4d11f0792` |
| `src/lib.rs` | `c1b0a352e31adb96f316fe956fedd765ac98419b0043fc36d4ceb6de88e042f3` | `12202a5f4136b37f67786ec1ee42598bd22482b7` |
| `src/main.rs` | `e481be10ed8b070bef56f963855072bfbb32852ff12d68efdfad8573e568ffb6` | `20ff8a22f66e9d609a2d34714c9f56af85a5b6d4` |
| `tests/review.rs` | `db02fb04e873f7f5d027f07af6b6ae26a5cba25bcd257bf725be688cc0c79f28` | `bf7695081db75d12607c9fc191c268d1b07a3b39` |

Copies of those bytes are under `source/blobs/tools/description-review/`. `diff -rq` against `git archive 8f4e792c tools/description-review` reported no difference.

## Retained failures

| Directory | What failed | Tests | Docker |
|---|---|---|---|
| `linux/proof` | `E0277` `char: LowerHex` and `E0596` cloning a slice reference | not run | 101 |
| `linux/proof-2` | `CARGO_BIN_EXE_description_review` unset; `Value` compared with `&Value` | not run | 101 |
| `linux/proof-3` | `reviewer_length_is_a_command_error_and_256_is_kept` expected an empty queue while `public.q_41` had no curated file | 15 passed, 1 failed | 101 |
| `linux/proof-4` | passing run | 16 passed | 0 |

These directories were not overwritten. The proof-3 failure is the contract rule: a candidate with no curated `DatasetFile` queues `pending` / `curated_missing`. The implementation already did that. The test expectation was corrected before proof-4. No review container was started. This session's `REVIEW.md` is the author review, not an independent acceptance.

## Limits

Compiler statements copied from the candidate outcome stay as the compiler wrote them, including `curated=not_written`. The review document adds its own statements: authentication is not claimed, auto-accept is not authorized, confidence is recorded only, and database, files, grants, publication, PII, semantic classification, entity, metric, status, and column ACL are unchanged. `DatasetFile.caveats` is not a write target of this unit. `open_questions` is not a `DatasetFile` field. A non-candidate compiler outcome returns `outcome=not_candidates` and leaves every curated document unchanged. Owned containers, volumes, the private network, and the private tmp directory are removed after this report. `CLEANUP.txt` records that removal.
