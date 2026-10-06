# Independent review of repaired S5-D01 and S5-D02

## Verdicts

S5-D01, the public description library: **APPROVE_LIMITED**.

S5-D02, the compiled `description-candidate` CLI: **APPROVE_LIMITED**.

Both verdicts are for fixed commit `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`, tree `b3a0fd3155ebb985be8faeb75f51978acdc75aae`. The decisive run is `linux/proof-2`. `linux/proof` is a preserved review-oracle failure and stays in this directory.

These verdicts cover the frozen library outcomes and the compiled phase CLI proved below. They do not accept full S5. They do not accept the provider. They do not accept the parent goal. They are not a release. Acceptance counts, the ledger, the plan, and product files were not edited. a257 remains REQUEST_CHANGES on `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`. That failure was not rerun and was not rewritten.

## Scope

Contracts read: `contracts/builder/description-candidate-v1.md` and `contracts/builder/description-candidate-cli-v1.md`, as stored at the fixed commit. Prior records read: a253, a257, a258, and a262 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`.

S5-D01 is `prepare_description_request` and `complete_description_candidates`. Cards stay ordered by `(schema, name)`. `selected_datasets` is the admitted visible datasets, sorted. `candidates` are ordered by `dataset`. `request_sha256` is null when no request was issued.

S5-D02 is the `description-candidate` binary. Phases are `prepare_request` and `complete_candidates`. Exit 0 writes the library string and adds no newline. Exit 1 stderr is `description-candidate: <code> <message>`. Exit 2 is usage, an unknown phase, or an extra argument.

Producer commit `56b791368d7b5ec050aa36ac91d33bc2234bc4d2` and CLI commit `827e768b9f94af6134a5734bd16e066ab0d47528` are the bytes in the fixed tree. The identity gate compared git blobs before each launch. `linux/smoke/source-identity.txt` records the matches. `phases/source.exit` is 0 on smoke, proof, and proof-2.

| Path | Fixed blob | Matches |
|---|---|---|
| `crates/oo-builder/src/description_candidate.rs` | `f783ae95f1f7398da18b6c47bcc87941db49f7a3` | `56b79136` |
| `crates/oo-builder/src/lib.rs` | `190ee15aeaca3abca6362e397f089ec654c1ad53` | `56b79136` and `827e768b` |
| `crates/oo-builder/tests/description_candidate.rs` | `ca576b9b9bcb8e82984d33a98e5cd188f56ee83d` | `56b79136` |
| `crates/oo-builder/tests/description_selection_order.rs` | `57097859e7693aa5004db0ba0d0141880535b731` | `56b79136` |
| `contracts/builder/description-candidate-v1.md` | `6b9fef8b1662e23116f7dc9cc3c5c538246a88cb` | `56b79136` |
| `crates/oo-builder/src/bin/description-candidate.rs` | `273de1e7cba3835150766545ba320320f56e65eb` | `827e768b` and `56b79136` |
| `crates/oo-builder/tests/description_candidate_cli.rs` | `a4f1f7377ce2dcc47d3c9d882bebeefc47d2218a` | `827e768b` and `56b79136` |
| `contracts/builder/description-candidate-cli-v1.md` | `1d6f461677a32705a7463c153ae15fecb109b77a` | `827e768b` and `56b79136` |
| `crates/oo-builder/Cargo.toml` | `d3cf3f73ba7884e34fd503ccd13a7d010161b78a` | `56b79136` |
| `Cargo.lock` | `2990f901ea98a7b465f462cce8742bc5c51c13aa` | `56b79136` and `827e768b` |

Content SHA-256 inside the containers, from `linux/smoke/source-sha256.txt`:

- producer `c392dd9ef7dfff45ea9f6298b257e8fc4bdeca04784acbfbb696ef644f884954`
- CLI `c28297d86349507fff5bc67e5aa796a0f58836b8974cc9b1d7f08ade7d31798d`
- lib `c6b5f3204d2b4da278a7194cdda62ba4d2f2310c71bf89450bc71b8d0111fdbf`
- author test `0882a7c1a2782a16c3e78728c60cb959a4a29b40ca8f16bcbf4bf2a53b07a56e`
- CLI test `96e0ab7ddb35280ef6eb3deee8b8799bc386e87042070b3d493941e5bead0c9d`
- selection-order test `449b6db74c39e169fdd79b608538233ff2de9eab683b63217ac6a8b5c3d8b768`
- candidate contract `bdf41af5a4669103a8d67fde9935921f05bb58651a8c20c5bac0c3cc38cb5908`
- CLI contract `bd37f499bbba86dae2905bc45e1770a5342193f8facc7d62dcd34458f3156d64`
- lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`

The producer content hash is the a262 patched hash. Proof and proof-2 hashed the same producer and CLI again. `cargo update` was not invoked. The lock after each run matched the archive.

## How the bytes were built

Smoke, proof, and proof-2 used `git archive 567d621e` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts`. The archive mount was read-only. The container copied it and moved `rust-toolchain.toml` aside. `RUSTUP_TOOLCHAIN=1.94.0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `cargo 1.94.0 (85eff7c80 2026-01-15)`.

Each builder had 6 GiB (`Memory=6442450944`), 1 CPU (`NanoCpus=1000000000`), `--cap-drop ALL`, `--security-opt no-new-privileges`, `--pull never`, private bridge `oo-a264-eef831aa-net` (`internal=false`), `HOME=/tmp`. Mounts were the archive, the review blobs, the hasher, the inside script, and the evidence directory. Proof and proof-2 also mounted the overlay read-only. No docker socket. No host home. No PostgreSQL. No model call.

Workspace HEAD during smoke was `928db2949667bb3e52da465c7719d9ecba3a0c93`. During proof and proof-2 it was `56710cc73cfe1663272ecaa503984e1f663428ad`. The archive stayed `567d621e`. Recipe origins are in `ORIGIN.md`.

## Smoke

`linux/smoke` ran before the review overlay existed. `docker-run.exit` 0. `smoke.exit` 0. Container `e2ef6bb117536c9d36a854f57ddbf11a05b1309b7d84fe2f77af09d89733af3f` removed. OOMKilled false. Privileged false. NetworkMode `oo-a264-eef831aa-net`. Capacity `own=0 global=0 host_cargo=0`.

- offline fetch exit 101 (`no matching package named chrono-tz` on the cold volume), then locked fetch exit 0
- lib `description_candidate`: 2 passed, 33 filtered
- author `description_candidate`: 10 passed
- `description_selection_order`: 1 passed
- `description_candidate_cli`: 5 passed
- overlay absent
- tree diff is only `rust-toolchain.toml` moved aside
- lock unchanged

Smoke did not run Clippy. The cold-volume offline miss is the same shape a262 recorded. It is not a product defect.

## Proof, preserved oracle failure

`linux/proof` copied `overlay/description_candidate_a264.rs` as it stood before the request-tight correction. `docker-run.exit` 101. `proof.exit` 101. `phases/test-a264.exit` 101. Container `5764a1b000aeec08c6d88e2dff378f861bd28f88d42ebaf62e38b9ca9e91c4bc` removed. OOMKilled false. Privileged false. Same memory, CPU, capability, and network limits. Capacity `own=0 global=0 host_cargo=0`.

The panic is `description_candidate_a264.rs:547`:

```
assertion `left == right` failed
  left: String("4d0ad807d8869cc6cd93f70424510780e26453514aaf8ac07e2b6e8a0f640bb1")
 right: String("5801d0ba79262f00c00902f8d92ac91f3de67959647a7145ac45dd92bfbc52b4")
```

Left is the response-byte outcome `request_sha256`. Right is the full-budget request hash. Those commands use different `max_response_bytes`, so the hashes differ. The fixture compared them. Lib tests 2 passed, author tests 10 passed, selection-order 1 passed, CLI tests 5 passed, and `cargo clippy -D warnings` on the lib, the bin, the author tests, the selection-order test, the CLI tests, and this fixture exited 0. `clippy.log` has no `warning:` line. Fetch offline was 0 because the volume was warm. Lock unchanged. Tree diff is the review test plus the toolchain file moved aside.

The worktree fixture hash on that run is `b3fc1b89d4cf0d950131a0689eeff2038db351a9bc4bfc141aa109152a525c01`, recorded in `linux/proof/source-sha256.txt`. A later copy into `linux/proof/description_candidate_a264.rs` raced with the overlay edit and stored the corrected bytes. This review restored that path by reversing the three overlay edits and checking the content SHA-256 against `b3fc1b89…`. The restored file's line 547 is the failing `request_sha256` comparison. The corrected fixture stayed at `overlay/description_candidate_a264.rs`, content SHA-256 `f692589f6dbbbfd97b24f5475695981f317ee7c386dc675e8aa33ab18444f12e`.

## Proof-2

`linux/proof-2` copied the corrected fixture onto a fresh container of the same archive. `docker-run.exit` 0. `proof.exit` 0. Container `cf05571ca983496aa7726c45e57ead1be9c222d238ed4e67690586476af69b8f` removed. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000. NetworkMode `oo-a264-eef831aa-net`. Capacity at launch was `own=0 global=1 host_cargo=0`. `containers-before.txt` names `oo-a263-proof`. That container was not stopped.

`physical_a_and_b_library_and_cli` passed in 0.19s. The fixture compiles physical tables `A` and `b`, selects `b` then `A`, and checks ten documents through the public functions and through `CARGO_BIN_EXE_description-candidate`:

- early table budget: `budget_failure` / `tables`, `request_sha256` null, `candidates` empty, datasets `public.b` then `public.q_41`
- request-byte budget: `budget_failure` / `request_bytes`, `request_sha256` null, same dataset order
- full request: cards `A` / `public.q_41` then `b` / `public.b`
- success: candidates `public.b` then `public.q_41`, labels kept, `effect=review_only`, confidence `0` then `1`, `card_sha256` bound to the full request
- truncated, incomplete, and provider failure: matching outcome, empty candidates, body labels absent, full-request `request_sha256`
- malformed duplicate protocol key: `format_rejected` / `duplicate_key`, sentinel absent
- tight request at `max_response_bytes=1`: `request_sha256` differs from the full request
- response-byte budget: `budget_failure` / `response_bytes`, `request_sha256` equals that tight request, response body absent, datasets still `public.b` then `public.q_41`

CLI stdout bytes equal the library string for all ten. Stderr is empty. Exit is 0. The binary filename is `description-candidate`.

Boundaries in the same test: usage with no arguments, exit 2, six exact lines; unknown phase and both function names, exit 2, `description-candidate: unknown phase\n`, names not printed; extra argument on both phases, exit 2, `description-candidate: the phase takes no arguments\n`; stdin of 1048577 bytes; non-UTF-8 `ff fe`; a lone `{`; a duplicate key; `prepare_request` with `captured_response`; `complete_candidates` without it; an unknown table name in a command that also contains the owner. Closed exits have empty stdout and the contract stderr line. Owner and the planted secrets are absent from every document and every stderr.

Focused tests on the same container: lib 2 passed, author 10 passed, selection-order 1 passed, CLI 5 passed. Clippy component log is `info: downloading component clippy`, exit 0. Clippy command:

```
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --lib --bin description-candidate \
  --test description_candidate --test description_selection_order \
  --test description_candidate_cli --test description_candidate_a264 \
  -- -D warnings
```

`clippy.exit` 0. The log has no `warning:` line and ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.13s`. Lock unchanged. Tree diff is the review test plus the toolchain file moved aside. Producer and CLI content hashes match smoke.

## Cleanup

Own volumes `oo-a264-eef831aa-cargo` and `oo-a264-eef831aa-target`, network `oo-a264-eef831aa-net`, and `/tmp/oo-a264-eef831aa-dba4-4af2-a78b-541bc9e0a264` were removed after proof-2. No prune and no foreign cleanup. See `CLEANUP.txt`.

`SHA256SUMS` is rewritten by `linux/manifest/finalize.sh` after this report, `FINDINGS.md`, `HANDOFF.md`, and `CLEANUP.txt` exist. The tool is `tools/sha256_manifest.rs`.
