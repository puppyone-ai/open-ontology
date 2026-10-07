# trusted-demo contract

Status: identity delta frozen for OPS-D01 / SP11-W06 before the script change. This is not a new unit or component.
Fixed product archive: `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.
Owned paths: `deploy/trusted-demo/**` and one evidence directory `evidence/<run_id>/SP11/trusted-demo-identity/**`.

This document describes one development demonstration. It does not accept the parent unit, M1, I01, I02, a release, authentication, authorization, semantic publication, natural language, a model provider, or a harness. It does not stand in for Compose, the ops console, or a full deployment.

## Deployment manifest

`start.sh` does not choose a run id. Every `--public-fixture`, `--smoke`, `--stop`, and `--clean` requires the same `--manifest` file. Omitting it exits 2. There is no default UUID and no default owner.

The file is one canonical JSON object, optionally ending in a single newline. Key order, names, and spacing are fixed:

```json
{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"<owner>","run_id":"<uuid>","source_id":"public_shop"}
```

Rules:

- `protocol` is `oo.deploy.trusted-demo-deployment/v1`.
- `project` is `open-ontology`. This is the Docker label `oo.project`. It is not the ontology project token.
- `fixed_source` is the archive commit above. A different commit is refused.
- `source_id` is `public_shop`. The fixed import CLI accepts `^[a-z][a-z0-9_]{0,62}$`. This fixture does not take another dataset.
- `run_id` is a lowercase UUID, `8-4-4-4-12` hex. Uppercase hex is refused.
- `owner` is the last four characters of `run_id`, also lowercase hex. The shell and the Rust driver both check that equality.
- Unknown keys, a second line, a symlink, or a file larger than 512 bytes is refused.
- The manifest has no password, CA, or filesystem path. Resource names and the secret directory are derived only after those checks. A value that contains `/`, a space, or `..` cannot satisfy the grammar.

The first `--public-fixture` writes that canonical line to `evidence/<run_id>/SP11/trusted-demo-identity/deployment.json`. Later commands refuse when the file they were given does not match that durable copy. Docker labels on every object this command creates are `oo.project`, `oo.owner`, `oo.session` (the run id), and `oo.fixed` (the archive commit). A label set that is not all four is foreign. Foreign objects are not started, removed, or overwritten.

Derived names, after validation:

| Use | Value |
| --- | --- |
| Docker owner label | `owner` |
| Inspect config `owner` | `run_id` (the tool requires a UUID) |
| Host and container | `oo-<owner>-pg` |
| Network | `oo-<owner>-<first eight of run_id>-net` |
| Volumes | `oo-<owner>-<first eight>-cargo`, `target`, `pgdata` |
| Private tmp | `/tmp/oo-<owner>-<run_id>` |
| Databases | `oo_<owner>_src`, `oo_<owner>_dst` |
| Logins | `oo_<owner>_reader`, `_writer`, `_runtime`, `_login` |
| Secret ref | `secret://pg/<owner>/reader` |
| Shard | `<owner>main` |
| Versions | `ont-<owner>`, `cat-<owner>`, `pol-<owner>`, `know-<owner>` |
| Evidence | `evidence/<run_id>/SP11/trusted-demo-identity/` |

`--public-fixture` exits 2 when any of those Docker objects, the private tmp directory, or `live/run` already exists. Matching labels are a collision and are not overwritten. Foreign labels are a refusal. `--smoke` reuses the volume and network only when the labels match and the receipt is already `ready_for_trusted_query`. `--stop` removes only containers whose four labels match. It leaves volumes, secrets, and evidence. `--clean` removes the matching containers, volumes, network, and private tmp. It does not prune and does not remove a foreign name.

## Historical demonstration

The original demonstration is selected only by an explicit manifest. It is not implied by a bare `start.sh`. Owner `a305` and run id `eef831aa-dba4-4af2-a78b-541bc9e0a305` derive the old names: host `oo-a305-pg`, databases `oo_a305_src` and `oo_a305_dst`, reader `oo_a305_reader`, runtime `oo_a305_runtime`, login `oo_a305_login`, secret ref `secret://pg/a305/reader`, shard `a305main`, network `oo-a305-eef831aa-net`, and tmp `/tmp/oo-a305-eef831aa-dba4-4af2-a78b-541bc9e0a305`. Versions are `ont-a305`, `cat-a305`, `pol-a305`, and `know-a305`. A fresh clone passes its own manifest. It must not reuse that run.

## What the command is

```bash
./deploy/trusted-demo/start.sh --manifest <file> --public-fixture
```

Authorized automation may run it. It does not claim a human clicked approve.

The command:

1. Builds the reviewed CLI binaries from a `git archive` of the fixed commit. The archive is mounted read-only. The script does not copy product sources into the archive and does not run `cargo update`.
2. Starts one private PostgreSQL 17.10. Source and target are two databases and separate roles on that one server.
3. Loads the public synthetic fixture with a restricted source login (`SELECT` on the two shop tables).
4. Runs `pg-onboarding inspect`. Success of this phase is `awaiting_review`.
5. Binds the fixture's declared review decisions to fingerprints taken from the captured draft. The fixture does not contain those fingerprints. The driver does not accept the whole queue and does not invent a reviewer identity.
6. Proves a deterministic bad decision stops before column ACL, mapping-reader provision, analyze, role pack, and trusted query. The bad decision names `shop.customer.not_a_column`.
7. Runs `pg-onboarding apply-reviewed` on the untouched inspect directory.
8. Checks the north-role positive rows and the hidden-column and write guards that `query-pg` actually emitted.
9. Writes a receipt with locations, protocol versions, digests, and the query command that was executed.
10. Restarts the same PostgreSQL volume and repeats the north-role query. The role-pack digest must match.

`--smoke` repeats step 10 when a receipt already says `ready_for_trusted_query`. The same manifest is required. Audit files are not reused.

## Resources

- Rust image: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `RUSTUP_TOOLCHAIN=1.94.0`.
- PostgreSQL image: `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.
- One new builder, 6 GiB, 1 CPU. One PostgreSQL, 1 GiB, 1 CPU.
- Private network derived above. No host network, no published host port, no Docker socket, no host home, no privileged mode, `--cap-drop ALL`.
- PostgreSQL adds only `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, and `SETGID`, the same set as the reviewed a274 server wrapper.
- Secrets, CA, and passwords live under `/tmp/oo-<owner>-<run_id>/secrets`. They are not written into the repo. The Safe Rust scan report must not contain those password bytes. Passwords are not accepted from argv or from the manifest.

## Identity

Source id `public_shop`. Administrator `oo_fixture_admin`. Generation `48`. Tables `shop.customer`, `shop.order_doc`, in that order. Host, databases, logins, secret ref, and shard come from the manifest table above.

The ontology project token is the same string as the source id. Apply copies `source_id` into the project field. The fixed import CLI accepts a source id only as `^[a-z][a-z0-9_]{0,62}$`. Attempt 2 of the historical demonstration sent `public-shop` and stopped at `import-not-ready` with stderr `oo-pipeline: source config source_id`, before any target connection. `public_shop` is that correction. It is not a new dataset, role, or acceptance claim.

The config identity is `oo-pg-config-identity/v1` over those non-secret fields and the import CLI default budgets. Password and CA bytes are not inputs.

## Declared review

`fixture/declared-review.json` lists the accept kinds, the skipped prefixes, and the column decisions. Kinds are `structural_column`, `structural_primary_key`, and `candidate_relationship`. Prefixes `pii/`, `semantic/`, `type/`, and `classification/` are not accepted. `private_note` may receive a structural accept and must not receive a classification decision. Reviewer `by` is `public-fixture`. That string is a fixture label, not a human approval.

North role slug `north` (database role `oo_r_north`) sees the declared customer and order columns, including `clerk_note`, and does not see `private_note`. Narrow role slug `narrow` sees only `customer_id`, `display_name`, `order_id`, `customer_id`, and `posted`.

The positive statement is:

`SELECT customer_id, display_name, active FROM shop.customer ORDER BY customer_id`

The hidden guard is `SELECT private_note FROM shop.customer`. The write guard is an `INSERT` into `shop.customer`. Both are expected to fail inside `query-pg` before a trusted result. The chain also keeps the reviewed `before_analyze` policy refusal and the `_oo_pos` guard.

Rows compared by the oracle are the rows in `fixture/source.sql`. The target is filled only by `init-pg` and `import-pg`. The driver does not insert business rows into the target.

## Non-claims

Inspect and apply outcomes already carry the tool's non-claims. The receipt repeats them and adds:

- `human_approval = not_claimed`
- `authentication = not_claimed`
- `authorization = not_claimed`
- `semantic_publication = missing`
- `natural_language = unsupported`
- `model_provider = unsupported`
- `harness = not_performed`
- `m1_i01 = not_accepted`
- `m1_i02 = not_accepted`
- `parent_acceptance = false`

No model call, provider key, or natural-language loop is part of this command.

## Exits

- `0`: the phase finished as this contract requires.
- `1`: image missing, PostgreSQL did not become ready, a compiler failed, or an oracle refused. No receipt may say ready in that case.
- `2`: usage, manifest refused, foreign labels, container name already present, or an owned resource or run directory already exists and will not be overwritten.
- `3`: this task already has a builder, the host already has cargo/rustc, or ten labeled Rust builders are already running.

`pg_isready` success is the only PostgreSQL readiness signal. A container start is not readiness.

## Smoke before the chain

Before the workspace binaries are built, the same container runs, against the immutable archive and with `live_chain` ignored:

`cargo test --manifest-path tools/pg-onboarding/Cargo.toml --locked --offline`

then Clippy `--all-targets -- -D warnings` for that manifest. The driver tests and Clippy run after that smoke and before `apply-reviewed`. A smoke failure stops the chain. The full arm64/amd64 matrix is out of scope.

## Receipt

`evidence/<run_id>/SP11/trusted-demo-identity/live/receipt.json` uses `protocol = oo.deploy.trusted-demo-receipt/v1`. It names the fixed commit, the run id, the owner, the project, the run directory, the outcome files, the role-pack SHA-256 values, and the executed `query-pg --request` path. Repeating that request as-is is not the restart command, because the audit file already exists. Restart uses `start.sh --manifest <file> --smoke`, which keeps the SQL, role, and policy id, and writes a new audit file.
