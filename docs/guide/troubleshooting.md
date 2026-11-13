# Troubleshooting L1

Start with the failed command's JSON `message`/`code`, exit status and stage.
Keep raw output private: a successful build's share prompt contains a one-use
enrollment code. Never send model keys, source passwords, tokens or private
keys with a bug report. A failed query is not zero rows.

## Installation and runtime

Cause: the installed CLI, runtime image, platform or deployment state does not
match the requested operation.

| Code / symptom | Fix |
| --- | --- |
| npm `ETARGET` / `E404` for 0.2.0 | The release is unavailable from that registry. Stop; do not silently install 0.1.x and claim L1. Ask the release owner for a verified 0.2.0 package and runtime image. |
| `NATIVE_MANIFEST_INVALID`, `NATIVE_BINARY_MISSING`, `NATIVE_DIGEST_MISMATCH`, `NATIVE_NOT_EXECUTABLE`, `UNSUPPORTED_PLATFORM` | Reinstall the verified package for the host architecture. Never bypass the digest check or fall back to an unrelated global binary. |
| `LITE_IMAGE_DIGEST_REQUIRED` | The CLI needs its release's immutable runtime pin. Install the matching package/image, not an arbitrary tag. |
| `LITE_IMAGES_MISSING` | `oo up` pulls digest-pinned registry images itself; this error means the pull failed (network, registry access) or the pin is a local image ID. Follow the exact pull/load instruction in the error. A local image ID must be loaded, not pulled. No services were started. |
| `LITE_DOCKER_UNAVAILABLE` | Start Docker and check access using the same user/context. |
| `BUILD_LITE_NOT_PROVISIONED`, `BUILD_LITE_SERVICE_NOT_RUNNING` | Run `oo up --config stack.yaml` before build. |
| `BUILD_LITE_STACK_CHANGED` | Re-run up with the changed configuration, then resume build; do not edit checkpoints. |
| `LITE_LIFECYCLE_FAILED`, `BUILD_LITE_NO_PROGRESS` | Inspect the owned deployment's stage and private runtime logs; preserve the failure and escalate if no actionable cause is present. These aggregate codes are not a diagnosis. |
| `REMOTE_*`, `SSH_*` | Check pinned host key, SSH authentication, remote Docker/compose/systemd and permissions. Do not accept a changed host key automatically. SSH deployment is not proof that remote retained build works. |
| `BUNDLE_*` | Check the independently trusted manifest, checksums, exact platform and complete file set. Do not edit the manifest to bypass integrity checks. |

For an offline bundle, use its verified launcher: it supplies the validated
PostgreSQL content ID through `OO_LITE_POSTGRES_IMAGE`. Do not replace that
value with a tag or edit the manifest. If an older CLI still requests the
registry digest after loading the bundle, use the matching corrected CLI.

`retained_ready` means the build chain is provisioned, not that a publication
exists. `foundations_only` is also not full health. Seven healthy services,
measured scheduled-cycle timestamps and a successful data query are separate
checks. Missing health/source-time measurements remain unknown.

## Scheduled data status

Check `data_published_at` separately from `cycle.result`. `partial` means the
fresh tables were published while `tables.stale` retained older data; use each
table's reason and `since` to diagnose it. An all-stale sync or a global source
failure does not publish. Failed cycles do not reset the publication clock.
Health follows `data_verified_at`: the newest cycle that confirmed the served
publication is current, either by activating a new one or by an exact replay
when the source had no changes. `data_publication_overdue` means that time is
older than twice the interval plus 60 seconds; a source with no new rows stays
healthy while cycles keep confirming it.

Comment/FK changes and new columns mark only the affected table
`review_pending`. Scheduling continues with the last reviewed registry and
new columns remain unpublished. Inspect the private cycle `builder.json`
with `oo review --input <builder.json> --json` for pending tables and changes.
This is inspection, not approval; a new semantic draft still requires the
normal explicit owner review.

Changes that are not scoped to one table stop the whole cycle instead:
inferred cross-table relationship candidates, cross-table enum dictionary
bindings, report definitions and other library-wide semantic inputs. The
cycle result is `review_required` with reason `semantic_review_required`,
`tables.review_pending` has an entry `library:<library>` with `scope:
library` and the changed inputs, and `alert` is `library_review_required`.
Nothing is published until the owner reviews a new draft; the last reviewed
publication keeps serving. Agents are not told that newer data is held; only
`oo status` shows it, so act on `library_review_required` promptly. Unchanged sources keep the same library digest
across generations, because row counts and ratios are normalized.

Agents see a stale table's reason without column names (for example
`schema_drift:type`); the owner sees the full reason in `oo status --json`.

Each scheduled C5 cycle compares per-chunk digests computed by the source
and transfers only chunks whose digest changed; every 24th cycle (and the first
cycle after deployment) re-reads all chunks to catch a digest collision. The
schedule key `c5_full_read_every` (1–10000) changes that period; 1 restores a
full read every cycle. Digest computation itself still scans each table at the
source on every cycle.

### Upgrading an existing deployment

- C5 tables (the lite default) seal their column catalog on the first cycle
  after the upgrade when the live source still matches the previous lineage;
  any table that changed meanwhile stays `schema_unproven` until it is
  rebaselined. The legacy lineage did not record collation or charset, so a
  collation change made before the upgrade is not detected by that first seal.
- C1 (log CDC) tables on MySQL and Oracle never recorded a column shape: run
  one explicit rebaseline per source after upgrading. PostgreSQL C1 seals like C5.
- The logical publication identity (migrations 0135/0136) is installed by
  `oo up`. Records valid before 0136 are re-minted by the installer; an
  identity invalidated by a later change to a published view, relation or a
  routine it uses is never re-approved automatically and needs a new review
  and publication.
- Routines a published view calls dynamically (for example `EXECUTE` inside a
  PL/pgSQL body) cannot be followed through catalog dependencies. Publish views
  only over routines the publisher owns.

After upgrading the CLI/runtime, run `oo up` for the owned deployment to
install the additive lite cycle journal. Existing schedule configuration and
reviewed-registry hashes remain unchanged. The old library-wide data-only
digest is retained; table digests are derived from pinned facts. Without the
journal, cycle status remains unmeasured rather than inventing success from a
local receipt. The initial publication time can be read from the ready Agent
DB head before the first scheduled event.

## Credentials

Cause: a required private reference is missing, inaccessible, unsafe, expired
or belongs to a different audience.

`*_REQUIRED` names the missing input; provide that input through the documented
file/stdin mechanism. Source/model secrets are regular, owner-owned, mode 0600
files under the owner's private secret store. Do not use symlinks, shared files
or inline secrets in stack.yaml. An existing secret with different contents
requires explicit rotation, not overwrite.

For `TUI_SECRET_*`, `SECRET_*` or `MODEL_*` import errors, check the selected
file and its permissions, not just the reference name. A model key is not a
Gateway credential. For `AUTH_*`, `TOKEN_*`, expired/revoked access or a used
enrollment code, obtain authorized renewal/new enrollment from the owner and
repeat the share prompt's `oo login` command. Never retry issuance blindly
after a lost enrollment response; reconcile its audit first.

## Connectivity

Cause: the source or Gateway cannot be reached with the required authenticated
TLS/route. Check host, port, DNS, CA chain, certificate hostname and client
identity. `TUI_CA_FILE_REQUIRED` means PostgreSQL/MySQL need a CA file imported
in wizard step 1. `TLS_*`/`CERT_*` are not permission to disable verification.
Container-side localhost is not host localhost; use the deployment-supported
route. Diagnose the source and Gateway separately.

## Model

Cause: provider configuration, egress policy, upstream response or model budget
failed. `UPSTREAM_TIMEOUT` means no successful completion was received within
the bounded timeout. Check provider availability and endpoint compatibility;
do not treat it as empty model output. `UPSTREAM_*` and `MODEL_*` require
checking the private base-URL/key references, provider, model ID and bounded
probe result. Keep endpoint/key contents out of logs.

Budget/context errors require a measured context budget and valid persisted
usage. An incomplete retained Builder output needs reconciliation; deleting it
to reset the call budget is not supported recovery. `budget_unmeasured` is not
an unlimited budget. After an interrupted model call, preserve evidence and
resolve the call outcome before resuming.

## Review and publication

Cause: draft semantics, connectivity or publication proofs have not been
approved or do not match the current input.

`review_required` is an intentional gate. In a TTY, inspect the owner-review
items. Without a TTY, review `.oo/review-input.json`, supply an explicit
hash-bound decision file and resume with `--review-file`; never auto-approve
just to get a green build. Changed semantic digests require new review.
`REVIEW_*`/`REGISTRY_*` errors require correcting the named review constraint,
not weakening gold values or opening the fence manually.

`publication_unverified` can mean C5 left unchanged tables in an older run.
The retained build attempts bounded snapshot resume and one candidate retry.
If that fails, preserve the run and reconcile the missing cuts; do not
manufacture acknowledgements. A lost share-prompt response needs audit
reconciliation rather than automatic duplicate issuance.

## Database proof

Cause: an Agent DB invariant rejected a transition. SQLSTATEs can describe
several guards; use the associated stage/message to distinguish them.

| SQLSTATE | Cause / fix |
| --- | --- |
| `OO010` | Lease, barrier or compare-and-swap conflict. Reconcile the current run/lease and resume the same authorized run; do not start competing publishers. |
| `OO024` | Required physical mapping/table absent. Recheck the loaded generation and catalog binding before retrying the query/publication. |
| `OO030` | Closed or mismatched proof/identity/manifest/fence. Preserve the exact rejection and regenerate valid evidence through the normal loader/review/publisher chain. Never bypass the guard. |
| `OO031` | Expected open run absent. Check whether it already completed or was closed; do not reopen it by editing tables. |

Other `OO0..` codes in the index link to their migration guard. Ask the operator
to inspect that specific precondition on the Agent DB. Never run repair DDL
against the source or modify an existing migration.

## Query budget

Cause: admission, concurrency, deadline or cancellation policy stopped the
request. `RATE_LIMITED`/`OVERLOADED` permit only bounded admission retries.
`TIMEOUT`/`UPSTREAM_TIMEOUT` do not imply an empty result; query timeouts are
not automatically retried. Reduce scope/row count or inspect the query ID.
Use `oo operations`, `oo status <query_id>` and `oo cancel <query_id>` with
the same principal/project/role. Records are process-local, not durable history.

## Configuration and source

Cause: input shape, scope, source capability or a stage-specific invariant
was rejected. Use the named guard and `oo help <command>` to correct the
input. `oo help --help` itself is rejected by the npm wrapper; use `oo --help`.
`ADMIN_NOT_IMPLEMENTED` is an unsupported operation, not an invitation to
hand-edit state.

Explicit `schema.table` scope entries are required; wildcard discovery is not
implemented. Run doctor and give the DBA only its capability-specific
requirements from the [generated grant reference](../database-support/dba-grants.md).
Do not grant writer/admin rights to the reader. Loader checkpoint, digest,
epoch, schema and watermark mismatches require reconciliation or an explicit
new baseline, not deletion of evidence.

`FUNCTION_NOT_ALLOWED` and `STATEMENT_NOT_ALLOWED` can mean unsupported SQL
syntax. Use describe and validate, then rewrite within the supported dialect.
`CROSS_TABLE_SKEW`, `DELETES_LAGGED` and unmeasured source time remain caveats
even when the publication is live. Do not claim source-real-time freshness.

<!-- generated error index -->

## Error-code index

Generated from CLI, npm wrapper, Gateway, SDK, stack, loader, retained publisher, Builder, model-egress, lite and pipeline SQL sources. Includes conservative configuration/status matches as well as errors; appearance here does not mean an error was observed in a live run. Dynamic external-driver messages are not enumerable. Each remedy links to a cause/fix section; the source location identifies the precise guard.

| Code | Cause and fix | First source location |
| --- | --- | --- |
| `ADJUDICATION_DUPLICATE_ITEM` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:221`](../../crates/oo-cli/src/tui/adjudication.rs#L221) |
| `ADJUDICATION_FILE_IO` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:591`](../../crates/oo-cli/src/tui/adjudication.rs#L591) |
| `ADJUDICATION_GRAPH` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:232`](../../crates/oo-cli/src/tui/adjudication.rs#L232) |
| `ADJUDICATION_JSON` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:602`](../../crates/oo-cli/src/tui/adjudication.rs#L602) |
| `ADJUDICATION_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:217`](../../crates/oo-cli/src/tui/adjudication.rs#L217) |
| `ADJUDICATION_SHAPE` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:215`](../../crates/oo-cli/src/tui/adjudication.rs#L215) |
| `ADJUDICATION_SUMMARY_MISMATCH` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:235`](../../crates/oo-cli/src/tui/adjudication.rs#L235) |
| `ADMIN_ARGUMENT` | [configuration-and-source](#configuration-and-source) | [`build.rs:141`](../../crates/oo-cli/src/admin/build.rs#L141) |
| `ADMIN_CONFIG_IO` | [configuration-and-source](#configuration-and-source) | [`lite.rs:35`](../../crates/oo-cli/src/admin/lite.rs#L35) |
| `ADMIN_CONFIG_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`mod.rs:428`](../../crates/oo-cli/src/admin/mod.rs#L428) |
| `ADMIN_IDENTITY_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:47`](../../packages/cli/bin/auth.mjs#L47) |
| `ADMIN_NOT_IMPLEMENTED` | [configuration-and-source](#configuration-and-source) | [`mod.rs:420`](../../crates/oo-cli/src/admin/mod.rs#L420) |
| `ADMIN_STATUS_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`mod.rs:138`](../../crates/oo-cli/src/admin/mod.rs#L138) |
| `ALL_IN_ONE_CLI` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2680`](../../tools/release-coordinator/src/w73.rs#L2680) |
| `ALL_IN_ONE_FAILURE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2745`](../../tools/release-coordinator/src/w73.rs#L2745) |
| `ALL_IN_ONE_MAPPING` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2719`](../../tools/release-coordinator/src/w73.rs#L2719) |
| `ALL_IN_ONE_PUBLISH` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2531`](../../tools/release-coordinator/src/w73.rs#L2531) |
| `ALL_IN_ONE_READY` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2690`](../../tools/release-coordinator/src/w73.rs#L2690) |
| `ARGUMENT_ERROR` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:565`](../../tools/model-egress/model_egress.py#L565) |
| `AUDIT_UNAVAILABLE` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:409`](../../crates/oo-cli/src/outcome.rs#L409) |
| `AUTH_ARGUMENTS` | [credentials](#credentials) | [`auth.mjs:196`](../../packages/cli/bin/auth.mjs#L196) |
| `AUTH_FILE_INVALID` | [credentials](#credentials) | [`auth.mjs:26`](../../packages/cli/bin/auth.mjs#L26) |
| `AUTH_RENEW_REQUIRED` | [credentials](#credentials) | [`auth.mjs:96`](../../packages/cli/bin/auth.mjs#L96) |
| `AUTH_REQUIRED` | [credentials](#credentials) | [`auth.mjs:228`](../../packages/cli/bin/auth.mjs#L228) |
| `AUTO_INCREMENT` | [configuration-and-source](#configuration-and-source) | [`library_observation.py:117`](../../tools/pg-builder/scripts/library_observation.py#L117) |
| `A_B` | [configuration-and-source](#configuration-and-source) | [`policy_catalog_capabilities_pg_tests.rs:901`](../../crates/oo-gateway/src/policy_catalog_capabilities_pg_tests.rs#L901) |
| `BUILD_AGENT_TARGET_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`build.rs:151`](../../crates/oo-cli/src/admin/build.rs#L151) |
| `BUILD_ARTIFACT_PATH` | [configuration-and-source](#configuration-and-source) | [`build.rs:47`](../../crates/oo-cli/src/admin/build.rs#L47) |
| `BUILD_BUILDER_FAILED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:309`](../../crates/oo-cli/src/admin/build_retained.rs#L309) |
| `BUILD_BUILDER_INTERRUPTED_REQUIRES_RECONCILIATION` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:285`](../../crates/oo-cli/src/admin/build_retained.rs#L285) |
| `BUILD_BUILDER_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`build.rs:303`](../../crates/oo-cli/src/admin/build.rs#L303) |
| `BUILD_CANDIDATE_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:233`](../../crates/oo-cli/src/admin/build_retained.rs#L233) |
| `BUILD_CANDIDATE_REFUSED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:237`](../../crates/oo-cli/src/admin/build_retained.rs#L237) |
| `BUILD_CHAIN_EXISTS` | [configuration-and-source](#configuration-and-source) | [`mod.rs:357`](../../crates/oo-cli/src/admin/mod.rs#L357) |
| `BUILD_CLIENT_DIRECTORY` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:90`](../../crates/oo-cli/src/admin/build_tail.rs#L90) |
| `BUILD_COMMAND_OUTPUT_LIMIT` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:153`](../../crates/oo-cli/src/admin/build_retained.rs#L153) |
| `BUILD_COMMAND_PIPE` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:119`](../../crates/oo-cli/src/admin/build_retained.rs#L119) |
| `BUILD_COMMAND_START` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:114`](../../crates/oo-cli/src/admin/build_retained.rs#L114) |
| `BUILD_COMMAND_TIMEOUT_OR_IO` | [query-budget](#query-budget) | [`build_retained.rs:150`](../../crates/oo-cli/src/admin/build_retained.rs#L150) |
| `BUILD_COMPLETION_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`build_lite.rs:458`](../../crates/oo-cli/src/admin/build_lite.rs#L458) |
| `BUILD_CYCLE_REQUIRES_RESUME_AND_ID` | [configuration-and-source](#configuration-and-source) | [`build.rs:172`](../../crates/oo-cli/src/admin/build.rs#L172) |
| `BUILD_ERROR_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`build.rs:395`](../../crates/oo-cli/src/admin/build.rs#L395) |
| `BUILD_EXECUTION_FAILED` | [configuration-and-source](#configuration-and-source) | [`draft.py:287`](../../deploy/lite/draft.py#L287) |
| `BUILD_EXISTS_USE_RESUME` | [configuration-and-source](#configuration-and-source) | [`build.rs:229`](../../crates/oo-cli/src/admin/build.rs#L229) |
| `BUILD_GATEWAY_NOT_READY` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:209`](../../crates/oo-cli/src/admin/build_tail.rs#L209) |
| `BUILD_GATEWAY_PUBLICATION_BINDING` | [review-and-publication](#review-and-publication) | [`build_tail.rs:134`](../../crates/oo-cli/src/admin/build_tail.rs#L134) |
| `BUILD_GATEWAY_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:150`](../../crates/oo-cli/src/admin/build_tail.rs#L150) |
| `BUILD_GATEWAY_START` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:193`](../../crates/oo-cli/src/admin/build_tail.rs#L193) |
| `BUILD_GATEWAY_TLS` | [connectivity](#connectivity) | [`build_tail.rs:160`](../../crates/oo-cli/src/admin/build_tail.rs#L160) |
| `BUILD_GATEWAY_URL` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:137`](../../crates/oo-cli/src/admin/build_tail.rs#L137) |
| `BUILD_INSTALL` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:97`](../../crates/oo-cli/src/admin/build_retained.rs#L97) |
| `BUILD_INTERRUPTED` | [configuration-and-source](#configuration-and-source) | [`progress.rs:270`](../../crates/oo-cli/src/admin/progress.rs#L270) |
| `BUILD_ISSUANCE_UNCERTAIN_RECONCILE_AUDIT` | [connectivity](#connectivity) | [`build_tail.rs:33`](../../crates/oo-cli/src/admin/build_tail.rs#L33) |
| `BUILD_LIFECYCLE_FAILED_RECONCILE_AUDIT` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:120`](../../crates/oo-cli/src/admin/build_tail.rs#L120) |
| `BUILD_LIFECYCLE_START` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:118`](../../crates/oo-cli/src/admin/build_tail.rs#L118) |
| `BUILD_LITE_GATEWAY_SERVICE` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:436`](../../crates/oo-cli/src/admin/build_lite.rs#L436) |
| `BUILD_LITE_NOT_PROVISIONED` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:38`](../../crates/oo-cli/src/admin/build_lite.rs#L38) |
| `BUILD_LITE_NO_PROGRESS` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:480`](../../crates/oo-cli/src/admin/build_lite.rs#L480) |
| `BUILD_LITE_OWNER` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:48`](../../crates/oo-cli/src/admin/build_lite.rs#L48) |
| `BUILD_LITE_PATH` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:69`](../../crates/oo-cli/src/admin/build_lite.rs#L69) |
| `BUILD_LITE_RETAINED_GRANT` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:372`](../../crates/oo-cli/src/admin/build_lite.rs#L372) |
| `BUILD_LITE_SECRET_MOUNT` | [credentials](#credentials) | [`build_lite.rs:110`](../../crates/oo-cli/src/admin/build_lite.rs#L110) |
| `BUILD_LITE_SERVICE_NOT_RUNNING` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:86`](../../crates/oo-cli/src/admin/build_lite.rs#L86) |
| `BUILD_LITE_STACK` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:156`](../../crates/oo-cli/src/admin/build_lite.rs#L156) |
| `BUILD_LITE_STACK_CHANGED` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:399`](../../crates/oo-cli/src/admin/build_lite.rs#L399) |
| `BUILD_LOAD_FAILED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:243`](../../crates/oo-cli/src/admin/build_retained.rs#L243) |
| `BUILD_LOCK` | [configuration-and-source](#configuration-and-source) | [`build.rs:208`](../../crates/oo-cli/src/admin/build.rs#L208) |
| `BUILD_ORIGIN_INCOMPLETE` | [configuration-and-source](#configuration-and-source) | [`build.rs:181`](../../crates/oo-cli/src/admin/build.rs#L181) |
| `BUILD_ORIGIN_MISSING` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:271`](../../crates/oo-cli/src/admin/build_retained.rs#L271) |
| `BUILD_PROFILE_NOT_PUBLISHED` | [credentials](#credentials) | [`build_lite.rs:493`](../../crates/oo-cli/src/admin/build_lite.rs#L493) |
| `BUILD_PROMPT_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`build.rs:426`](../../crates/oo-cli/src/admin/build.rs#L426) |
| `BUILD_PUBLICATION_FAILED` | [review-and-publication](#review-and-publication) | [`build_retained.rs:349`](../../crates/oo-cli/src/admin/build_retained.rs#L349) |
| `BUILD_PUBLICATION_RECEIPT` | [review-and-publication](#review-and-publication) | [`build_retained.rs:353`](../../crates/oo-cli/src/admin/build_retained.rs#L353) |
| `BUILD_PUBLICATION_REFUSED_UNCHANGED` | [review-and-publication](#review-and-publication) | [`build_retained.rs:347`](../../crates/oo-cli/src/admin/build_retained.rs#L347) |
| `BUILD_RESUME_ARTIFACT_CHANGED` | [configuration-and-source](#configuration-and-source) | [`build.rs:71`](../../crates/oo-cli/src/admin/build.rs#L71) |
| `BUILD_RESUME_CONFIG_CHANGED` | [configuration-and-source](#configuration-and-source) | [`build.rs:59`](../../crates/oo-cli/src/admin/build.rs#L59) |
| `BUILD_RESUME_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`build.rs:63`](../../crates/oo-cli/src/admin/build.rs#L63) |
| `BUILD_REVIEW_DRAFT_BINDING` | [review-and-publication](#review-and-publication) | [`build.rs:327`](../../crates/oo-cli/src/admin/build.rs#L327) |
| `BUILD_REVIEW_INPUT_CHANGED` | [review-and-publication](#review-and-publication) | [`build.rs:316`](../../crates/oo-cli/src/admin/build.rs#L316) |
| `BUILD_REVIEW_INPUT_REQUIRED` | [review-and-publication](#review-and-publication) | [`build.rs:323`](../../crates/oo-cli/src/admin/build.rs#L323) |
| `BUILD_REVIEW_REJECTED` | [review-and-publication](#review-and-publication) | [`build.rs:346`](../../crates/oo-cli/src/admin/build.rs#L346) |
| `BUILD_RUNTIME_CONFIG` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:61`](../../crates/oo-cli/src/admin/build_retained.rs#L61) |
| `BUILD_RUNTIME_CONFIG_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`build.rs:155`](../../crates/oo-cli/src/admin/build.rs#L155) |
| `BUILD_RUNTIME_FAILED` | [configuration-and-source](#configuration-and-source) | [`build_lite.rs:257`](../../crates/oo-cli/src/admin/build_lite.rs#L257) |
| `BUILD_RUNTIME_SECRET_REF_REQUIRED` | [credentials](#credentials) | [`build_retained.rs:82`](../../crates/oo-cli/src/admin/build_retained.rs#L82) |
| `BUILD_RUNTIME_SOURCE` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:40`](../../crates/oo-cli/src/admin/build_retained.rs#L40) |
| `BUILD_SOURCE_CATALOG_FAILED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:411`](../../crates/oo-cli/src/admin/build_retained.rs#L411) |
| `BUILD_SOURCE_CATALOG_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:406`](../../crates/oo-cli/src/admin/build_retained.rs#L406) |
| `BUILD_SOURCE_CONNECTION` | [connectivity](#connectivity) | [`build_retained.rs:431`](../../crates/oo-cli/src/admin/build_retained.rs#L431) |
| `BUILD_SOURCE_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:424`](../../crates/oo-cli/src/admin/build_retained.rs#L424) |
| `BUILD_START` | [configuration-and-source](#configuration-and-source) | [`progress.rs:229`](../../crates/oo-cli/src/admin/progress.rs#L229) |
| `BUILD_STATE_DIRECTORY` | [configuration-and-source](#configuration-and-source) | [`build.rs:191`](../../crates/oo-cli/src/admin/build.rs#L191) |
| `BUILD_STATE_IO` | [configuration-and-source](#configuration-and-source) | [`build.rs:24`](../../crates/oo-cli/src/admin/build.rs#L24) |
| `BUILD_STATE_OR_CONFIGURATION` | [configuration-and-source](#configuration-and-source) | [`draft.py:312`](../../deploy/lite/draft.py#L312) |
| `BUILD_STATE_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`progress.rs:69`](../../crates/oo-cli/src/admin/progress.rs#L69) |
| `BUILD_STDIN` | [configuration-and-source](#configuration-and-source) | [`progress.rs:234`](../../crates/oo-cli/src/admin/progress.rs#L234) |
| `BUILD_STDOUT` | [configuration-and-source](#configuration-and-source) | [`progress.rs:240`](../../crates/oo-cli/src/admin/progress.rs#L240) |
| `BUILD_TAIL_RECEIPT_BINDING` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:25`](../../crates/oo-cli/src/admin/build_tail.rs#L25) |
| `BUILD_TAIL_STAGE` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:272`](../../crates/oo-cli/src/admin/build_tail.rs#L272) |
| `BUILD_TARGET_IO` | [configuration-and-source](#configuration-and-source) | [`progress.rs:220`](../../crates/oo-cli/src/admin/progress.rs#L220) |
| `BUILD_TEST_PAUSE` | [configuration-and-source](#configuration-and-source) | [`build.rs:40`](../../crates/oo-cli/src/admin/build.rs#L40) |
| `BUILD_TOKEN_FILE` | [credentials](#credentials) | [`build_tail.rs:55`](../../crates/oo-cli/src/admin/build_tail.rs#L55) |
| `BUILD_TOKEN_ISSUANCE_REFUSED` | [credentials](#credentials) | [`build_token.rs:102`](../../crates/oo-cli/src/admin/build_token.rs#L102) |
| `BUILD_TOKEN_RANDOM` | [credentials](#credentials) | [`build_token.rs:35`](../../crates/oo-cli/src/admin/build_token.rs#L35) |
| `BUILD_TOKEN_RECEIPT` | [credentials](#credentials) | [`build_tail.rs:79`](../../crates/oo-cli/src/admin/build_tail.rs#L79) |
| `BUILD_TOKEN_REQUEST` | [credentials](#credentials) | [`build_token.rs:160`](../../crates/oo-cli/src/admin/build_token.rs#L160) |
| `BUILD_TOKEN_REQUEST_RETRYABLE` | [credentials](#credentials) | [`build_token.rs:100`](../../crates/oo-cli/src/admin/build_token.rs#L100) |
| `BUILD_TOKEN_RESPONSE` | [credentials](#credentials) | [`build_token.rs:111`](../../crates/oo-cli/src/admin/build_token.rs#L111) |
| `BUILD_TOKEN_RUNTIME` | [credentials](#credentials) | [`build_token.rs:79`](../../crates/oo-cli/src/admin/build_token.rs#L79) |
| `BUILD_TOKEN_TLS` | [credentials](#credentials) | [`build_token.rs:70`](../../crates/oo-cli/src/admin/build_token.rs#L70) |
| `BUILD_TOKEN_WRITE` | [credentials](#credentials) | [`build_token.rs:22`](../../crates/oo-cli/src/admin/build_token.rs#L22) |
| `BUILD_WAIT` | [configuration-and-source](#configuration-and-source) | [`progress.rs:273`](../../crates/oo-cli/src/admin/progress.rs#L273) |
| `BUNDLE_ARCHIVE_CONFIG` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:109`](../../deploy/lite/bundle.py#L109) |
| `BUNDLE_ARCHIVE_CONFIG_SIZE` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:111`](../../deploy/lite/bundle.py#L111) |
| `BUNDLE_ARCHIVE_DUPLICATE` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:91`](../../deploy/lite/bundle.py#L91) |
| `BUNDLE_ARCHIVE_ID` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:103`](../../deploy/lite/bundle.py#L103) |
| `BUNDLE_ARCHIVE_MANIFEST` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:63`](../../deploy/lite/bundle.py#L63) |
| `BUNDLE_ARCHIVE_PATH` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:94`](../../deploy/lite/bundle.py#L94) |
| `BUNDLE_ARCHIVE_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:116`](../../deploy/lite/bundle.py#L116) |
| `BUNDLE_ARCHIVE_TAGS` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:101`](../../deploy/lite/bundle.py#L101) |
| `BUNDLE_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:262`](../../deploy/lite/bundle.py#L262) |
| `BUNDLE_FILE_CHECKSUM` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:185`](../../deploy/lite/bundle.py#L185) |
| `BUNDLE_FILE_SET` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:179`](../../deploy/lite/bundle.py#L179) |
| `BUNDLE_IMAGE_ID` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:189`](../../deploy/lite/bundle.py#L189) |
| `BUNDLE_IMAGE_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:126`](../../deploy/lite/bundle.py#L126) |
| `BUNDLE_IMAGE_SET` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:180`](../../deploy/lite/bundle.py#L180) |
| `BUNDLE_IMMUTABLE_REFERENCE` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:121`](../../deploy/lite/bundle.py#L121) |
| `BUNDLE_LOCAL_ONLY` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:224`](../../deploy/lite/bundle.py#L224) |
| `BUNDLE_MANIFEST_CHECKSUM` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:175`](../../deploy/lite/bundle.py#L175) |
| `BUNDLE_OUTPUT_EXISTS` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:122`](../../deploy/lite/bundle.py#L122) |
| `BUNDLE_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:120`](../../deploy/lite/bundle.py#L120) |
| `BUNDLE_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:177`](../../deploy/lite/bundle.py#L177) |
| `BUNDLE_SERVICE_SET` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:181`](../../deploy/lite/bundle.py#L181) |
| `BUNDLE_SYMLINK` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:174`](../../deploy/lite/bundle.py#L174) |
| `BUNDLE_TARGET_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:203`](../../deploy/lite/bundle.py#L203) |
| `BUNDLE_TRUST_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`bundle.py:173`](../../deploy/lite/bundle.py#L173) |
| `CARGO_MANIFEST_DIR` | [configuration-and-source](#configuration-and-source) | [`mod.rs:333`](../../crates/oo-cli/src/tui/screens/mod.rs#L333) |
| `CARGO_PKG_VERSION` | [configuration-and-source](#configuration-and-source) | [`args.rs:203`](../../crates/oo-cli/src/args.rs#L203) |
| `CARGO_TARGET_DIR` | [configuration-and-source](#configuration-and-source) | [`live_thousand_acceptance.py:125`](../../tools/pg-builder/scripts/live_thousand_acceptance.py#L125) |
| `CA_FILE` | [connectivity](#connectivity) | [`connect.rs:577`](../../crates/oo-cli/src/tui/screens/connect.rs#L577) |
| `CLIENT_DISCONNECTED` | [connectivity](#connectivity) | [`model_egress.py:393`](../../tools/model-egress/model_egress.py#L393) |
| `CLI_UPGRADE_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`lifecycle.rs:453`](../../crates/oo-cli/src/lifecycle.rs#L453) |
| `COLUMN_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:174`](../../crates/oo-cli/src/admin/demo_try.rs#L174) |
| `COMMUNITY_BATCH_BUDGET` | [model](#model) | [`community_adapter.py:93`](../../deploy/lite/community_adapter.py#L93) |
| `COMMUNITY_CALL_BUDGET` | [model](#model) | [`community_adapter.py:82`](../../deploy/lite/community_adapter.py#L82) |
| `COMMUNITY_CARDS_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:20`](../../deploy/lite/community_adapter.py#L20) |
| `COMMUNITY_COLUMN_COVERAGE` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:26`](../../deploy/lite/community_adapter.py#L26) |
| `COMMUNITY_FOREIGN_KEY_SHAPE` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:11`](../../deploy/lite/community_adapter.py#L11) |
| `COMMUNITY_INCOMPLETE` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:96`](../../deploy/lite/community_adapter.py#L96) |
| `COMMUNITY_TABLE_COVERAGE` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:36`](../../deploy/lite/community_adapter.py#L36) |
| `CONFIG_ARGUMENTS` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:236`](../../packages/cli/bin/oo.mjs#L236) |
| `CONFIG_BUDGET` | [model](#model) | [`model_egress.py:592`](../../tools/model-egress/model_egress.py#L592) |
| `CONFIG_DIR_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:117`](../../packages/cli/bin/oo.mjs#L117) |
| `CONFIG_ENDPOINT` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:88`](../../tools/model-egress/model_egress.py#L88) |
| `CONFIG_FILE_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:255`](../../packages/cli/bin/oo.mjs#L255) |
| `CONFIG_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:147`](../../packages/cli/bin/oo.mjs#L147) |
| `CONFIG_OR_TRANSPORT_ERROR` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:605`](../../tools/model-egress/model_egress.py#L605) |
| `CONFIG_PERMISSIONS` | [credentials](#credentials) | [`oo.mjs:127`](../../packages/cli/bin/oo.mjs#L127) |
| `CONFIG_SCHEMA` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:213`](../../tools/model-egress/model_egress.py#L213) |
| `CONFIRM_DISCOVERY_INCOMPLETE` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:319`](../../crates/oo-cli/src/tui/adjudication.rs#L319) |
| `CONFIRM_FILE_EXISTS_OR_IO` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:619`](../../crates/oo-cli/src/tui/adjudication.rs#L619) |
| `CONFIRM_FILE_IO` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:611`](../../crates/oo-cli/src/tui/adjudication.rs#L611) |
| `CONFIRM_REASON_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`ontology_graph.rs:108`](../../crates/oo-cli/src/admin/ontology_graph.rs#L108) |
| `CONFIRM_SUMMARY_MISMATCH` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:315`](../../crates/oo-cli/src/tui/adjudication.rs#L315) |
| `COST_TOO_HIGH` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:401`](../../crates/oo-cli/src/outcome.rs#L401) |
| `DATABASE_EMPTY` | [configuration-and-source](#configuration-and-source) | [`connect.rs:551`](../../crates/oo-cli/src/tui/screens/connect.rs#L551) |
| `DATABASE_URL` | [configuration-and-source](#configuration-and-source) | [`source.rs:732`](../../crates/oo-cli/src/tui/source.rs#L732) |
| `DB2_WORKER_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:144`](../../crates/oo-cli/src/admin/doctor.rs#L144) |
| `DB_URL` | [configuration-and-source](#configuration-and-source) | [`source.rs:735`](../../crates/oo-cli/src/tui/source.rs#L735) |
| `DELIVERY_ARGUMENTS` | [configuration-and-source](#configuration-and-source) | [`profile.rs:188`](../../crates/oo-cli/src/profile.rs#L188) |
| `DEMO_ACTION` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:276`](../../deploy/lite/demo_deploy.py#L276) |
| `DEMO_ADJUDICATION` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:1303`](../../crates/oo-cli/src/admin/wizard.rs#L1303) |
| `DEMO_ADJUDICATION_BINDING` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:70`](../../deploy/lite/demo_runtime.py#L70) |
| `DEMO_ADMIN_SQL` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:85`](../../deploy/lite/demo_deploy.py#L85) |
| `DEMO_ASSETS` | [configuration-and-source](#configuration-and-source) | [`demo.rs:144`](../../crates/oo-cli/src/admin/demo.rs#L144) |
| `DEMO_ASSET_DIGEST` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:63`](../../deploy/lite/demo_runtime.py#L63) |
| `DEMO_AUDIT_BINDING` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:135`](../../deploy/lite/demo_deploy.py#L135) |
| `DEMO_AUDIT_FILE` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:29`](../../deploy/lite/demo_deploy.py#L29) |
| `DEMO_AUDIT_NOT_VERIFIED` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:121`](../../crates/oo-cli/src/admin/demo_try.rs#L121) |
| `DEMO_AUDIT_OUTCOME` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:142`](../../deploy/lite/demo_deploy.py#L142) |
| `DEMO_AUDIT_PERSISTENCE` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:156`](../../deploy/lite/demo_deploy.py#L156) |
| `DEMO_AUDIT_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:119`](../../crates/oo-cli/src/admin/demo_try.rs#L119) |
| `DEMO_AUDIT_REQUEST` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:116`](../../deploy/lite/demo_deploy.py#L116) |
| `DEMO_CLEANUP_INCOMPLETE` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:106`](../../deploy/lite/demo_deploy.py#L106) |
| `DEMO_COMPOSE_FAILED` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:187`](../../deploy/lite/demo_runtime.py#L187) |
| `DEMO_CONTAINER_COUNT` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:190`](../../deploy/lite/demo_runtime.py#L190) |
| `DEMO_DEPLOY_FAILED` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:284`](../../deploy/lite/demo_deploy.py#L284) |
| `DEMO_DEPLOY_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`demo.rs:175`](../../crates/oo-cli/src/admin/demo.rs#L175) |
| `DEMO_EXECUTABLE` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:68`](../../crates/oo-cli/src/admin/demo_try.rs#L68) |
| `DEMO_FAILED` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:232`](../../deploy/lite/demo_runtime.py#L232) |
| `DEMO_FRESHNESS` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:127`](../../crates/oo-cli/src/admin/demo_try.rs#L127) |
| `DEMO_GATEWAY_START` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:223`](../../deploy/lite/demo_deploy.py#L223) |
| `DEMO_GOLD` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:71`](../../crates/oo-cli/src/admin/demo_try.rs#L71) |
| `DEMO_IMAGE_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`demo_deploy.py:66`](../../deploy/lite/demo_deploy.py#L66) |
| `DEMO_LOCAL_IMAGE_DIGEST_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:155`](../../deploy/lite/demo_runtime.py#L155) |
| `DEMO_MANIFEST` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:59`](../../deploy/lite/demo_runtime.py#L59) |
| `DEMO_OWNER` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:51`](../../deploy/lite/demo_deploy.py#L51) |
| `DEMO_OWNERSHIP` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:97`](../../deploy/lite/demo_deploy.py#L97) |
| `DEMO_OWNER_BINDING` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:54`](../../deploy/lite/demo_deploy.py#L54) |
| `DEMO_PACK_BINDING` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:202`](../../deploy/lite/demo_deploy.py#L202) |
| `DEMO_PASSWORD` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:197`](../../deploy/lite/demo_runtime.py#L197) |
| `DEMO_PRIVATE_DIRECTORY` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:34`](../../deploy/lite/demo_runtime.py#L34) |
| `DEMO_PRIVATE_FILE` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:43`](../../deploy/lite/demo_runtime.py#L43) |
| `DEMO_PROFILE_EXISTS` | [credentials](#credentials) | [`demo.rs:138`](../../crates/oo-cli/src/admin/demo.rs#L138) |
| `DEMO_PUBLICATION_BINDING` | [review-and-publication](#review-and-publication) | [`demo_try.rs:138`](../../crates/oo-cli/src/admin/demo_try.rs#L138) |
| `DEMO_PUBLICATION_REFUSED` | [review-and-publication](#review-and-publication) | [`demo_deploy.py:213`](../../deploy/lite/demo_deploy.py#L213) |
| `DEMO_QUERY_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:82`](../../crates/oo-cli/src/admin/demo_try.rs#L82) |
| `DEMO_READINESS_FAILED` | [configuration-and-source](#configuration-and-source) | [`demo.rs:194`](../../crates/oo-cli/src/admin/demo.rs#L194) |
| `DEMO_RENDER` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:164`](../../crates/oo-cli/src/admin/demo_try.rs#L164) |
| `DEMO_RUNTIME_COMMAND` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:41`](../../deploy/lite/demo_deploy.py#L41) |
| `DEMO_SOURCE_BINDING` | [configuration-and-source](#configuration-and-source) | [`demo_deploy.py:184`](../../deploy/lite/demo_deploy.py#L184) |
| `DEMO_STATE` | [configuration-and-source](#configuration-and-source) | [`demo_try.rs:112`](../../crates/oo-cli/src/admin/demo_try.rs#L112) |
| `DEMO_STATE_EXISTS` | [configuration-and-source](#configuration-and-source) | [`demo.rs:133`](../../crates/oo-cli/src/admin/demo.rs#L133) |
| `DEMO_STATE_OUTSIDE_WORKDIR` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:78`](../../deploy/lite/demo_runtime.py#L78) |
| `DEMO_STATE_PARENT` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:83`](../../deploy/lite/demo_runtime.py#L83) |
| `DEMO_SUBPROCESS_FAILED` | [configuration-and-source](#configuration-and-source) | [`demo_runtime.py:26`](../../deploy/lite/demo_runtime.py#L26) |
| `DEMO_WORKDIR` | [configuration-and-source](#configuration-and-source) | [`demo.rs:33`](../../crates/oo-cli/src/admin/demo.rs#L33) |
| `DEVICE_EXPIRED` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:189`](../../packages/cli/bin/auth.mjs#L189) |
| `DM8_TLS_NOT_AVAILABLE` | [connectivity](#connectivity) | [`doctor.rs:170`](../../crates/oo-cli/src/admin/doctor.rs#L170) |
| `DM8_WORKER_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:174`](../../crates/oo-cli/src/admin/doctor.rs#L174) |
| `DOCKER_CONFIG` | [configuration-and-source](#configuration-and-source) | [`lifecycle.py:671`](../../deploy/lite/lifecycle.py#L671) |
| `DOCKER_CONTEXT` | [configuration-and-source](#configuration-and-source) | [`lifecycle.py:671`](../../deploy/lite/lifecycle.py#L671) |
| `DOCKER_DEFAULT_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:671`](../../deploy/lite/lifecycle.py#L671) |
| `DOCKER_HOST` | [configuration-and-source](#configuration-and-source) | [`lifecycle.py:671`](../../deploy/lite/lifecycle.py#L671) |
| `DOCTOR_REPORT` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:528`](../../crates/oo-cli/src/admin/doctor.rs#L528) |
| `DOCTOR_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:517`](../../crates/oo-cli/src/admin/doctor.rs#L517) |
| `DRAFT_AGENT_TLS_REQUIRED` | [connectivity](#connectivity) | [`draft.py:124`](../../deploy/lite/draft.py#L124) |
| `DRAFT_BRIDGE_CALL_BUDGET` | [model](#model) | [`draft.py:240`](../../deploy/lite/draft.py#L240) |
| `DRAFT_BRIDGE_TABLE_BUDGET` | [model](#model) | [`draft.py:186`](../../deploy/lite/draft.py#L186) |
| `DRAFT_ENGINE_PENDING_OO_LOAD` | [configuration-and-source](#configuration-and-source) | [`draft.py:173`](../../deploy/lite/draft.py#L173) |
| `DRAFT_EXPLICIT_SCOPE_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`draft.py:180`](../../deploy/lite/draft.py#L180) |
| `DRAFT_LOAD_COVERAGE` | [configuration-and-source](#configuration-and-source) | [`draft.py:97`](../../deploy/lite/draft.py#L97) |
| `DRAFT_LOAD_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`draft.py:93`](../../deploy/lite/draft.py#L93) |
| `DRAFT_NOT_VALIDATED` | [configuration-and-source](#configuration-and-source) | [`draft.py:274`](../../deploy/lite/draft.py#L274) |
| `DSN_ENGINE_UNKNOWN` | [configuration-and-source](#configuration-and-source) | [`source.rs:462`](../../crates/oo-cli/src/tui/source.rs#L462) |
| `DSN_HOST` | [configuration-and-source](#configuration-and-source) | [`source.rs:423`](../../crates/oo-cli/src/tui/source.rs#L423) |
| `DSN_PORT` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:1404`](../../crates/oo-cli/src/admin/wizard.rs#L1404) |
| `DSN_SCHEME` | [configuration-and-source](#configuration-and-source) | [`source.rs:454`](../../crates/oo-cli/src/tui/source.rs#L454) |
| `EGRESS_PORT_IN_USE_USE_LITE_NETWORK_NAMESPACE` | [model](#model) | [`draft.py:247`](../../deploy/lite/draft.py#L247) |
| `EGRESS_START_FAILED` | [model](#model) | [`draft.py:254`](../../deploy/lite/draft.py#L254) |
| `EGRESS_START_TIMEOUT` | [model](#model) | [`draft.py:261`](../../deploy/lite/draft.py#L261) |
| `ENGINE_NOT_AVAILABLE` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:389`](../../crates/oo-cli/src/admin/build_retained.rs#L389) |
| `ENGINE_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:1033`](../../crates/oo-cli/src/admin/wizard.rs#L1033) |
| `ENGINE_REQUIRED_FOR_NONSTANDARD_PORT` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:257`](../../crates/oo-cli/src/admin/doctor.rs#L257) |
| `ENTITY_CHECK_FAILED` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3837`](../../crates/oo-gateway/src/public_surface.rs#L3837) |
| `ENUMERATION_REFUSED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:405`](../../crates/oo-cli/src/outcome.rs#L405) |
| `F22_USER_ROW_READ` | [configuration-and-source](#configuration-and-source) | [`f22_owned_pg_tests.rs:89`](../../crates/oo-gateway/src/f22_owned_pg_tests.rs#L89) |
| `FUNCTION_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:397`](../../crates/oo-cli/src/outcome.rs#L397) |
| `GATEWAY_CREDENTIAL_REQUIRED` | [credentials](#credentials) | [`oo.mjs:299`](../../packages/cli/bin/oo.mjs#L299) |
| `GATEWAY_INVALID` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:41`](../../packages/cli/bin/auth.mjs#L41) |
| `GATEWAY_ORIGIN` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2114`](../../tools/release-coordinator/src/w73.rs#L2114) |
| `GATEWAY_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:84`](../../packages/cli/bin/auth.mjs#L84) |
| `GATEWAY_REFUSED` | [configuration-and-source](#configuration-and-source) | [`mcp_config.rs:33`](../../crates/oo-cli/src/mcp_config.rs#L33) |
| `GATEWAY_TRANSPORT` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:68`](../../packages/cli/bin/auth.mjs#L68) |
| `GATE_PREFLIGHT_PROOF` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2485`](../../tools/release-coordinator/src/w73.rs#L2485) |
| `GRAPH_FOCUS_UNKNOWN` | [configuration-and-source](#configuration-and-source) | [`ontology_graph.rs:148`](../../crates/oo-cli/src/admin/ontology_graph.rs#L148) |
| `HOST_EMPTY` | [configuration-and-source](#configuration-and-source) | [`netprobe.rs:23`](../../crates/oo-cli/src/tui/netprobe.rs#L23) |
| `HOST_REJECTED` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:510`](../../tools/model-egress/model_egress.py#L510) |
| `HOST_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:1035`](../../crates/oo-cli/src/admin/wizard.rs#L1035) |
| `HOST_UNRESOLVED` | [configuration-and-source](#configuration-and-source) | [`netprobe.rs:27`](../../crates/oo-cli/src/tui/netprobe.rs#L27) |
| `IDEMPOTENCY_CONFLICT` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:410`](../../crates/oo-cli/src/outcome.rs#L410) |
| `INLINE_PASSWORD` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:1039`](../../crates/oo-cli/src/admin/wizard.rs#L1039) |
| `INTERFACE_INVALID` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:59`](../../packages/cli/bin/auth.mjs#L59) |
| `IP_BLOCKED` | [configuration-and-source](#configuration-and-source) | [`model_provider.py:223`](../../tools/pg-builder/scripts/model_provider.py#L223) |
| `JDBC_ORACLE` | [configuration-and-source](#configuration-and-source) | [`source.rs:605`](../../crates/oo-cli/src/tui/source.rs#L605) |
| `JDBC_PREFIX` | [configuration-and-source](#configuration-and-source) | [`source.rs:562`](../../crates/oo-cli/src/tui/source.rs#L562) |
| `JDBC_SCHEME` | [configuration-and-source](#configuration-and-source) | [`source.rs:622`](../../crates/oo-cli/src/tui/source.rs#L622) |
| `JDBC_URL` | [configuration-and-source](#configuration-and-source) | [`source.rs:738`](../../crates/oo-cli/src/tui/source.rs#L738) |
| `LC_ALL` | [configuration-and-source](#configuration-and-source) | [`theme.rs:68`](../../crates/oo-cli/src/tui/theme.rs#L68) |
| `LISTEN_NOT_LOOPBACK` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:477`](../../tools/model-egress/model_egress.py#L477) |
| `LITE_ACTION` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:764`](../../deploy/lite/lifecycle.py#L764) |
| `LITE_AGENT_DB_INSTALL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:478`](../../deploy/lite/lifecycle.py#L478) |
| `LITE_BUILD_ONE_SOURCE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:466`](../../deploy/lite/lifecycle.py#L466) |
| `LITE_CERTIFICATE` | [connectivity](#connectivity) | [`credentials.rs:191`](../../crates/oo-stack/src/credentials.rs#L191) |
| `LITE_CERTIFICATE_DATE` | [connectivity](#connectivity) | [`credentials.rs:149`](../../crates/oo-stack/src/credentials.rs#L149) |
| `LITE_CERTIFICATE_KEY_MISMATCH` | [connectivity](#connectivity) | [`credentials.rs:203`](../../crates/oo-stack/src/credentials.rs#L203) |
| `LITE_COMPOSE_TEMPLATE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:361`](../../crates/oo-cli/src/admin/lite.rs#L361) |
| `LITE_CONTROL_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:138`](../../deploy/lite/lifecycle.py#L138) |
| `LITE_CREDENTIAL_MARKER` | [credentials](#credentials) | [`credentials.rs:169`](../../crates/oo-stack/src/credentials.rs#L169) |
| `LITE_CYCLE_SCHEMA` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:619`](../../deploy/lite/lifecycle.py#L619) |
| `LITE_DOCKER_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:247`](../../deploy/lite/lifecycle.py#L247) |
| `LITE_DOCKER_UNAVAILABLE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:370`](../../crates/oo-cli/src/admin/lite.rs#L370) |
| `LITE_GATEWAY_DATA_LOGIN` | [credentials](#credentials) | [`lifecycle.py:347`](../../deploy/lite/lifecycle.py#L347) |
| `LITE_GATEWAY_RETAINED_GRANT` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:363`](../../deploy/lite/lifecycle.py#L363) |
| `LITE_HEALTH_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:337`](../../deploy/lite/lifecycle.py#L337) |
| `LITE_HEALTH_REFERENCE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:309`](../../deploy/lite/lifecycle.py#L309) |
| `LITE_HEALTH_REFUSED` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:291`](../../deploy/lite/lifecycle.py#L291) |
| `LITE_HEALTH_RUNTIME` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:289`](../../deploy/lite/lifecycle.py#L289) |
| `LITE_HEALTH_TOKEN` | [credentials](#credentials) | [`lifecycle.py:313`](../../deploy/lite/lifecycle.py#L313) |
| `LITE_HEALTH_URL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:302`](../../deploy/lite/lifecycle.py#L302) |
| `LITE_IMAGES_MISSING` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:387`](../../crates/oo-cli/src/admin/lite.rs#L387) |
| `LITE_IMAGE_CHANGE_REQUIRES_EXPLICIT_MIGRATION` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:713`](../../deploy/lite/lifecycle.py#L713) |
| `LITE_IMAGE_DIGEST_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:44`](../../crates/oo-cli/src/admin/lite.rs#L44) |
| `LITE_JOBS_SCHEMA` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:602`](../../deploy/lite/lifecycle.py#L602) |
| `LITE_JOBS_TEMPLATES` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:595`](../../deploy/lite/lifecycle.py#L595) |
| `LITE_LANE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:682`](../../deploy/lite/lifecycle.py#L682) |
| `LITE_LIFECYCLE_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:815`](../../deploy/lite/lifecycle.py#L815) |
| `LITE_LIFECYCLE_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:267`](../../deploy/lite/remote.py#L267) |
| `LITE_LOCAL_ONLY` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:33`](../../crates/oo-cli/src/admin/lite.rs#L33) |
| `LITE_MODEL_MOUNT` | [model](#model) | [`lifecycle.py:738`](../../deploy/lite/lifecycle.py#L738) |
| `LITE_MODEL_SOURCE` | [model](#model) | [`lifecycle.py:195`](../../deploy/lite/lifecycle.py#L195) |
| `LITE_MODEL_STATE_IN_GIT` | [model](#model) | [`lifecycle.py:159`](../../deploy/lite/lifecycle.py#L159) |
| `LITE_OPENSSL_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:76`](../../crates/oo-stack/src/credentials.rs#L76) |
| `LITE_OPENSSL_RUNTIME` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:275`](../../crates/oo-cli/src/admin/lite.rs#L275) |
| `LITE_OPENSSL_START` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:95`](../../crates/oo-stack/src/credentials.rs#L95) |
| `LITE_OPENSSL_STDIN` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:99`](../../crates/oo-stack/src/credentials.rs#L99) |
| `LITE_OPENSSL_WAIT` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:108`](../../crates/oo-stack/src/credentials.rs#L108) |
| `LITE_OPERATOR_CHANGE_REQUIRES_EXPLICIT_MIGRATION` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:397`](../../deploy/lite/lifecycle.py#L397) |
| `LITE_OPERATOR_PASSWORD` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:405`](../../deploy/lite/lifecycle.py#L405) |
| `LITE_OPERATOR_ROLES` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:417`](../../deploy/lite/lifecycle.py#L417) |
| `LITE_OPERATOR_TOKEN` | [credentials](#credentials) | [`lifecycle.py:427`](../../deploy/lite/lifecycle.py#L427) |
| `LITE_OWNERSHIP` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:705`](../../deploy/lite/lifecycle.py#L705) |
| `LITE_PRIVATE_DIRECTORY` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:154`](../../deploy/lite/lifecycle.py#L154) |
| `LITE_PRIVATE_FILE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:219`](../../deploy/lite/lifecycle.py#L219) |
| `LITE_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:132`](../../crates/oo-cli/src/admin/lite.rs#L132) |
| `LITE_PROVISION_CHANGED` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:438`](../../deploy/lite/lifecycle.py#L438) |
| `LITE_PROVISION_PASSWORD` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:482`](../../deploy/lite/lifecycle.py#L482) |
| `LITE_PUBLIC_URL_LOCAL_HTTPS` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:371`](../../deploy/lite/lifecycle.py#L371) |
| `LITE_RANDOM` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:119`](../../crates/oo-stack/src/credentials.rs#L119) |
| `LITE_SCHEDULE_CHANGE_REQUIRES_EXPLICIT_MIGRATION` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:651`](../../deploy/lite/lifecycle.py#L651) |
| `LITE_SCHEDULE_PROJECT` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:96`](../../deploy/lite/lifecycle.py#L96) |
| `LITE_SCHEDULE_ROW` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:658`](../../deploy/lite/lifecycle.py#L658) |
| `LITE_SECRET_CHANGE_REQUIRES_EXPLICIT_ROTATION` | [credentials](#credentials) | [`lifecycle.py:203`](../../deploy/lite/lifecycle.py#L203) |
| `LITE_SECRET_MOUNT_SHADOW` | [credentials](#credentials) | [`lifecycle.py:729`](../../deploy/lite/lifecycle.py#L729) |
| `LITE_SECRET_PATH` | [credentials](#credentials) | [`credentials.rs:127`](../../crates/oo-stack/src/credentials.rs#L127) |
| `LITE_START` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:117`](../../crates/oo-cli/src/admin/lite.rs#L117) |
| `LITE_STATE_DIRECTORY` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:70`](../../crates/oo-cli/src/admin/lite.rs#L70) |
| `LITE_STDIN` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:121`](../../crates/oo-cli/src/admin/lite.rs#L121) |
| `LITE_SUBNET` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:689`](../../deploy/lite/lifecycle.py#L689) |
| `LITE_SUBNET_UNAVAILABLE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:265`](../../deploy/lite/lifecycle.py#L265) |
| `LITE_SYNC_INTERVAL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:579`](../../deploy/lite/lifecycle.py#L579) |
| `LITE_WAIT` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:130`](../../crates/oo-cli/src/admin/lite.rs#L130) |
| `LITE_WORKLOAD_NAME` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:304`](../../crates/oo-stack/src/credentials.rs#L304) |
| `LIVE_COMPLETION_FAILURE` | [configuration-and-source](#configuration-and-source) | [`live_check.py:46`](../../tools/model-egress/live_check.py#L46) |
| `LIVE_HTTP_FAILURE` | [configuration-and-source](#configuration-and-source) | [`live_check.py:41`](../../tools/model-egress/live_check.py#L41) |
| `LIVE_STREAM_FAILURE` | [configuration-and-source](#configuration-and-source) | [`live_check.py:44`](../../tools/model-egress/live_check.py#L44) |
| `LIVE_TRANSPORT` | [configuration-and-source](#configuration-and-source) | [`live_check.py:55`](../../tools/model-egress/live_check.py#L55) |
| `LOCAL_FAILURE` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:534`](../../packages/cli/bin/oo.mjs#L534) |
| `LOGIN_CODE_ARGV` | [credentials](#credentials) | [`lifecycle.rs:256`](../../crates/oo-cli/src/lifecycle.rs#L256) |
| `LOGIN_PROFILE_SAVE_FAILED` | [credentials](#credentials) | [`lifecycle.rs:746`](../../crates/oo-cli/src/lifecycle.rs#L746) |
| `LOGIN_VERIFICATION_FAILED` | [credentials](#credentials) | [`lifecycle.rs:738`](../../crates/oo-cli/src/lifecycle.rs#L738) |
| `MCP_AGENT` | [configuration-and-source](#configuration-and-source) | [`mcp_config.rs:90`](../../crates/oo-cli/src/mcp_config.rs#L90) |
| `MCP_AUTH` | [credentials](#credentials) | [`mcp_config.rs:23`](../../crates/oo-cli/src/mcp_config.rs#L23) |
| `MCP_CONNECTION` | [connectivity](#connectivity) | [`mcp_config.rs:22`](../../crates/oo-cli/src/mcp_config.rs#L22) |
| `MCP_INPUT` | [configuration-and-source](#configuration-and-source) | [`mcp_config.rs:12`](../../crates/oo-cli/src/mcp_config.rs#L12) |
| `MCP_INSTALL` | [configuration-and-source](#configuration-and-source) | [`profile.rs:178`](../../crates/oo-cli/src/profile.rs#L178) |
| `MCP_ROUTE` | [configuration-and-source](#configuration-and-source) | [`mcp_config.rs:20`](../../crates/oo-cli/src/mcp_config.rs#L20) |
| `MCP_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`profile.rs:170`](../../crates/oo-cli/src/profile.rs#L170) |
| `MCP_TRANSPORT` | [configuration-and-source](#configuration-and-source) | [`mcp_config.rs:27`](../../crates/oo-cli/src/mcp_config.rs#L27) |
| `METER_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:582`](../../tools/model-egress/model_egress.py#L582) |
| `METHOD_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`http_error.rs:54`](../../crates/oo-gateway/src/http_error.rs#L54) |
| `MODEL_AUTH` | [model](#model) | [`build.rs:584`](../../crates/oo-cli/src/tui/screens/build.rs#L584) |
| `MODEL_BUDGET` | [model](#model) | [`build.rs:583`](../../crates/oo-cli/src/tui/screens/build.rs#L583) |
| `MODEL_CONFIG` | [model](#model) | [`doctor.rs:493`](../../crates/oo-cli/src/admin/doctor.rs#L493) |
| `MODEL_EGRESS_PROTOCOL` | [model](#model) | [`doctor.rs:503`](../../crates/oo-cli/src/admin/doctor.rs#L503) |
| `MODEL_EGRESS_START` | [model](#model) | [`doctor.rs:487`](../../crates/oo-cli/src/admin/doctor.rs#L487) |
| `MODEL_EGRESS_STDIN` | [model](#model) | [`doctor.rs:491`](../../crates/oo-cli/src/admin/doctor.rs#L491) |
| `MODEL_EGRESS_WAIT` | [model](#model) | [`doctor.rs:501`](../../crates/oo-cli/src/admin/doctor.rs#L501) |
| `MODEL_UNREACHABLE` | [model](#model) | [`build.rs:585`](../../crates/oo-cli/src/tui/screens/build.rs#L585) |
| `MYSQL_DATABASE` | [configuration-and-source](#configuration-and-source) | [`source.rs:694`](../../crates/oo-cli/src/tui/source.rs#L694) |
| `MYSQL_DOCTOR_FAILED` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:77`](../../crates/oo-cli/src/admin/doctor.rs#L77) |
| `MYSQL_DOCTOR_INPUT` | [configuration-and-source](#configuration-and-source) | [`main.rs:16`](../../deploy/lite/mysql-doctor/src/main.rs#L16) |
| `MYSQL_DOCTOR_INSTALL` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:49`](../../crates/oo-cli/src/admin/doctor.rs#L49) |
| `MYSQL_DOCTOR_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:80`](../../crates/oo-cli/src/admin/doctor.rs#L80) |
| `MYSQL_DOCTOR_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`main.rs:57`](../../deploy/lite/mysql-doctor/src/main.rs#L57) |
| `MYSQL_DOCTOR_STDIN` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:67`](../../crates/oo-cli/src/admin/doctor.rs#L67) |
| `MYSQL_DOCTOR_WAIT` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:75`](../../crates/oo-cli/src/admin/doctor.rs#L75) |
| `MYSQL_HOST` | [configuration-and-source](#configuration-and-source) | [`source.rs:687`](../../crates/oo-cli/src/tui/source.rs#L687) |
| `MYSQL_PASSWORD` | [configuration-and-source](#configuration-and-source) | [`source.rs:699`](../../crates/oo-cli/src/tui/source.rs#L699) |
| `MYSQL_PORT` | [configuration-and-source](#configuration-and-source) | [`source.rs:692`](../../crates/oo-cli/src/tui/source.rs#L692) |
| `MYSQL_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`netprobe.rs:185`](../../crates/oo-cli/src/tui/netprobe.rs#L185) |
| `MYSQL_PWD` | [configuration-and-source](#configuration-and-source) | [`source.rs:92`](../../crates/oo-cli/src/tui/source.rs#L92) |
| `MYSQL_REFUSED` | [configuration-and-source](#configuration-and-source) | [`netprobe.rs:193`](../../crates/oo-cli/src/tui/netprobe.rs#L193) |
| `MYSQL_TCP_PORT` | [configuration-and-source](#configuration-and-source) | [`source.rs:691`](../../crates/oo-cli/src/tui/source.rs#L691) |
| `MYSQL_USER` | [configuration-and-source](#configuration-and-source) | [`source.rs:695`](../../crates/oo-cli/src/tui/source.rs#L695) |
| `NATIVE_BINARY_MISSING` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:93`](../../packages/cli/bin/oo.mjs#L93) |
| `NATIVE_DIGEST_MISMATCH` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:96`](../../packages/cli/bin/oo.mjs#L96) |
| `NATIVE_EXECUTION_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:352`](../../packages/cli/bin/oo.mjs#L352) |
| `NATIVE_MANIFEST_INVALID` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:63`](../../packages/cli/bin/oo.mjs#L63) |
| `NATIVE_NOT_EXECUTABLE` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:99`](../../packages/cli/bin/oo.mjs#L99) |
| `NOT_FOUND` | [configuration-and-source](#configuration-and-source) | [`http_error.rs:53`](../../crates/oo-gateway/src/http_error.rs#L53) |
| `NO_COLOR` | [configuration-and-source](#configuration-and-source) | [`theme.rs:93`](../../crates/oo-cli/src/tui/theme.rs#L93) |
| `ONTOLOGY_OPENER_FAILED` | [configuration-and-source](#configuration-and-source) | [`ontology.rs:135`](../../crates/oo-cli/src/admin/ontology.rs#L135) |
| `ONTOLOGY_OUTPUT_IO` | [configuration-and-source](#configuration-and-source) | [`ontology.rs:119`](../../crates/oo-cli/src/admin/ontology.rs#L119) |
| `ONTOLOGY_PACK_BUDGET` | [model](#model) | [`ontology.rs:93`](../../crates/oo-cli/src/admin/ontology.rs#L93) |
| `ONTOLOGY_PACK_CARDINALITY` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:294`](../../crates/oo-cli/src/admin/ontology_export.rs#L294) |
| `ONTOLOGY_PACK_DIGEST` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:78`](../../crates/oo-cli/src/admin/ontology_export.rs#L78) |
| `ONTOLOGY_PACK_DUPLICATE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:148`](../../crates/oo-cli/src/admin/ontology_export.rs#L148) |
| `ONTOLOGY_PACK_IO` | [configuration-and-source](#configuration-and-source) | [`ontology.rs:88`](../../crates/oo-cli/src/admin/ontology.rs#L88) |
| `ONTOLOGY_PACK_JSON` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:28`](../../crates/oo-cli/src/admin/ontology_export.rs#L28) |
| `ONTOLOGY_PACK_MISSING_PAGE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:27`](../../crates/oo-cli/src/admin/ontology_export.rs#L27) |
| `ONTOLOGY_PACK_PAGE_CYCLE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:37`](../../crates/oo-cli/src/admin/ontology_export.rs#L37) |
| `ONTOLOGY_PACK_PATH` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:74`](../../crates/oo-cli/src/admin/ontology_export.rs#L74) |
| `ONTOLOGY_PACK_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:41`](../../crates/oo-cli/src/admin/ontology_export.rs#L41) |
| `ONTOLOGY_PACK_REFERENCE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:158`](../../crates/oo-cli/src/admin/ontology_export.rs#L158) |
| `ONTOLOGY_PACK_SHAPE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:19`](../../crates/oo-cli/src/admin/ontology_export.rs#L19) |
| `ONTOLOGY_ROLE_MISMATCH` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:111`](../../crates/oo-cli/src/admin/ontology_export.rs#L111) |
| `ONTOLOGY_YAML_ROLE_OOS_UNAVAILABLE` | [configuration-and-source](#configuration-and-source) | [`ontology.rs:73`](../../crates/oo-cli/src/admin/ontology.rs#L73) |
| `OO002` | [database-proof](#database-proof) | [`0002_target_segment_null_expand.sql:24`](../../crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql#L24) |
| `OO010` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:731`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L731) |
| `OO011` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:446`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L446) |
| `OO013` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:467`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L467) |
| `OO014` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:802`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L802) |
| `OO015` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:651`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L651) |
| `OO017` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:363`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L363) |
| `OO018` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:443`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L443) |
| `OO019` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:346`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L346) |
| `OO020` | [database-proof](#database-proof) | [`0001_target_fence_expand.sql:323`](../../crates/oo-pipeline/migrations/0001_target_fence_expand.sql#L323) |
| `OO021` | [database-proof](#database-proof) | [`0002_target_segment_null_expand.sql:53`](../../crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql#L53) |
| `OO022` | [database-proof](#database-proof) | [`0004_physical_mapping.sql:642`](../../crates/oo-pipeline/migrations/0004_physical_mapping.sql#L642) |
| `OO024` | [database-proof](#database-proof) | [`mapping_reader_access.rs:695`](../../crates/oo-gateway/src/mapping_reader_access.rs#L695) |
| `OO030` | [database-proof](#database-proof) | [`0005_full_import_expand.sql:818`](../../crates/oo-pipeline/migrations/0005_full_import_expand.sql#L818) |
| `OO031` | [database-proof](#database-proof) | [`0005_full_import_expand.sql:346`](../../crates/oo-pipeline/migrations/0005_full_import_expand.sql#L346) |
| `OO_A359_CA_FILE` | [connectivity](#connectivity) | [`live.rs:60`](../../tools/release-coordinator/src/live.rs#L60) |
| `OO_A359_OBSERVATION` | [configuration-and-source](#configuration-and-source) | [`live.rs:787`](../../tools/release-coordinator/src/live.rs#L787) |
| `OO_A359_PASSWORD_FILE` | [configuration-and-source](#configuration-and-source) | [`live.rs:59`](../../tools/release-coordinator/src/live.rs#L59) |
| `OO_A359_PG_DATABASE` | [configuration-and-source](#configuration-and-source) | [`live.rs:65`](../../tools/release-coordinator/src/live.rs#L65) |
| `OO_A359_PG_HOST` | [configuration-and-source](#configuration-and-source) | [`live.rs:63`](../../tools/release-coordinator/src/live.rs#L63) |
| `OO_A359_PG_PORT` | [configuration-and-source](#configuration-and-source) | [`live.rs:58`](../../tools/release-coordinator/src/live.rs#L58) |
| `OO_A359_READER_PASSWORD_FILE` | [configuration-and-source](#configuration-and-source) | [`live.rs:80`](../../tools/release-coordinator/src/live.rs#L80) |
| `OO_ALL_IN_ONE_LIVE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2490`](../../tools/release-coordinator/src/w73.rs#L2490) |
| `OO_BIN` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2112`](../../tools/release-coordinator/src/w73.rs#L2112) |
| `OO_BUILD_TEST_PAUSE_AFTER` | [configuration-and-source](#configuration-and-source) | [`build.rs:34`](../../crates/oo-cli/src/admin/build.rs#L34) |
| `OO_BUILD_TOKEN_BOUNDARY` | [credentials](#credentials) | [`build_token_tests.rs:49`](../../crates/oo-cli/src/admin/build_token_tests.rs#L49) |
| `OO_BUILD_TOKEN_CHILD` | [credentials](#credentials) | [`build_token_tests.rs:45`](../../crates/oo-cli/src/admin/build_token_tests.rs#L45) |
| `OO_BUILD_TOKEN_LIVE_ROOT` | [credentials](#credentials) | [`build_token_tests.rs:152`](../../crates/oo-cli/src/admin/build_token_tests.rs#L152) |
| `OO_BUILD_TOKEN_PUBLICATION` | [review-and-publication](#review-and-publication) | [`build_token_tests.rs:148`](../../crates/oo-cli/src/admin/build_token_tests.rs#L148) |
| `OO_CONFIG_DIR` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:114`](../../crates/oo-cli/src/admin/build_tail.rs#L114) |
| `OO_DB2_WORKER` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:144`](../../crates/oo-cli/src/admin/doctor.rs#L144) |
| `OO_DM8_WORKER` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:174`](../../crates/oo-cli/src/admin/doctor.rs#L174) |
| `OO_F22_CLI` | [configuration-and-source](#configuration-and-source) | [`f22_owned_pg_tests.rs:432`](../../crates/oo-gateway/src/f22_owned_pg_tests.rs#L432) |
| `OO_F22_OWNED_FIXTURE` | [configuration-and-source](#configuration-and-source) | [`f22_owned_pg_tests.rs:36`](../../crates/oo-gateway/src/f22_owned_pg_tests.rs#L36) |
| `OO_GATE_PREFLIGHT_LIVE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2439`](../../tools/release-coordinator/src/w73.rs#L2439) |
| `OO_LANG` | [configuration-and-source](#configuration-and-source) | [`theme.rs:68`](../../crates/oo-cli/src/tui/theme.rs#L68) |
| `OO_LITE_LANE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:178`](../../crates/oo-cli/src/admin/lite.rs#L178) |
| `OO_LITE_POSTGRES_IMAGE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:362`](../../crates/oo-cli/src/admin/lite.rs#L362) |
| `OO_LITE_PULL_POLICY` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:686`](../../deploy/lite/lifecycle.py#L686) |
| `OO_LITE_RUNTIME_IMAGE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:157`](../../crates/oo-cli/src/admin/lite.rs#L157) |
| `OO_LITE_SUBNET` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:690`](../../deploy/lite/lifecycle.py#L690) |
| `OO_MCP_CLI` | [configuration-and-source](#configuration-and-source) | [`profile.rs:182`](../../crates/oo-cli/src/profile.rs#L182) |
| `OO_MCP_GATEWAY_URL` | [configuration-and-source](#configuration-and-source) | [`profile.rs:183`](../../crates/oo-cli/src/profile.rs#L183) |
| `OO_MCP_PROFILE` | [credentials](#credentials) | [`profile.rs:181`](../../crates/oo-cli/src/profile.rs#L181) |
| `OO_MCP_TOKEN_FILE` | [credentials](#credentials) | [`profile.rs:184`](../../crates/oo-cli/src/profile.rs#L184) |
| `OO_MONGO_COLLECTION_CONFIG` | [configuration-and-source](#configuration-and-source) | [`documents.rs:181`](../../tools/oo-load/src/documents.rs#L181) |
| `OO_NATIVE_ADMISSION_BIN` | [installation-and-runtime](#installation-and-runtime) | [`export_native_collection.py:64`](../../tools/pg-builder/scripts/export_native_collection.py#L64) |
| `OO_NATIVE_BINDING_REGISTRY` | [review-and-publication](#review-and-publication) | [`export_native_collection.py:65`](../../tools/pg-builder/scripts/export_native_collection.py#L65) |
| `OO_NATIVE_BINDING_REGISTRY_SHA256` | [review-and-publication](#review-and-publication) | [`export_native_collection.py:66`](../../tools/pg-builder/scripts/export_native_collection.py#L66) |
| `OO_NO_ANIM` | [configuration-and-source](#configuration-and-source) | [`theme.rs:96`](../../crates/oo-cli/src/tui/theme.rs#L96) |
| `OO_NUMERIC_PG_CONFIG` | [configuration-and-source](#configuration-and-source) | [`policy_catalog_numeric_pg_tests.rs:43`](../../crates/oo-gateway/src/policy_catalog_numeric_pg_tests.rs#L43) |
| `OO_ORACLE_WORKER_IMAGE` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:204`](../../crates/oo-cli/src/admin/doctor.rs#L204) |
| `OO_PARSER_WORKER_SLOW` | [configuration-and-source](#configuration-and-source) | [`parser_worker.rs:728`](../../crates/oo-gateway/src/parser_worker.rs#L728) |
| `OO_PERF_OWNED_FIXTURE` | [configuration-and-source](#configuration-and-source) | [`performance_owned_pg_tests.rs:42`](../../crates/oo-gateway/src/performance_owned_pg_tests.rs#L42) |
| `OO_PERF_PHASE` | [configuration-and-source](#configuration-and-source) | [`performance_owned_pg_tests.rs:293`](../../crates/oo-gateway/src/performance_owned_pg_tests.rs#L293) |
| `OO_PRINT_GRAPH` | [configuration-and-source](#configuration-and-source) | [`graph.rs:1440`](../../crates/oo-cli/src/tui/graph.rs#L1440) |
| `OO_PUBLISHED_CUT_FIXTURE` | [configuration-and-source](#configuration-and-source) | [`published_cut_owned_pg_tests.rs:31`](../../crates/oo-gateway/src/published_cut_owned_pg_tests.rs#L31) |
| `OO_READER` | [configuration-and-source](#configuration-and-source) | [`source.rs:83`](../../crates/oo-cli/src/tui/source.rs#L83) |
| `OO_REDUCED_MOTION` | [configuration-and-source](#configuration-and-source) | [`theme.rs:97`](../../crates/oo-cli/src/tui/theme.rs#L97) |
| `OO_SEARCH_INDEX_CONFIG` | [configuration-and-source](#configuration-and-source) | [`documents.rs:75`](../../tools/oo-load/src/documents.rs#L75) |
| `OO_SELFBUILD_BUDGET_BIN` | [model](#model) | [`build_retained.rs:300`](../../crates/oo-cli/src/admin/build_retained.rs#L300) |
| `OO_SHARE_NPM_REGISTRY` | [review-and-publication](#review-and-publication) | [`lite.rs:101`](../../crates/oo-cli/src/admin/lite.rs#L101) |
| `OO_SQL_CAPABILITIES_PG_CONFIG` | [configuration-and-source](#configuration-and-source) | [`policy_catalog_capabilities_pg_tests.rs:83`](../../crates/oo-gateway/src/policy_catalog_capabilities_pg_tests.rs#L83) |
| `OO_SQL_CAST_MUTATION` | [configuration-and-source](#configuration-and-source) | [`policy_catalog_capabilities_pg_tests.rs:1188`](../../crates/oo-gateway/src/policy_catalog_capabilities_pg_tests.rs#L1188) |
| `OO_SSH_ASKPASS_SECRET` | [credentials](#credentials) | [`remote.rs:28`](../../crates/oo-cli/src/admin/remote.rs#L28) |
| `OO_UPDATE_SAMPLES` | [configuration-and-source](#configuration-and-source) | [`mod.rs:336`](../../crates/oo-cli/src/tui/screens/mod.rs#L336) |
| `OO_UPDATE_SNAPSHOTS` | [configuration-and-source](#configuration-and-source) | [`mod.rs:340`](../../crates/oo-cli/src/tui/screens/mod.rs#L340) |
| `OO_W73_ADMIN_PASSWORD_FILE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:95`](../../tools/release-coordinator/src/w73.rs#L95) |
| `OO_W73_CA_FILE` | [connectivity](#connectivity) | [`w73.rs:81`](../../tools/release-coordinator/src/w73.rs#L81) |
| `OO_W73_CONTROL_DATABASE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:87`](../../tools/release-coordinator/src/w73.rs#L87) |
| `OO_W73_GATEWAY_BIN` | [configuration-and-source](#configuration-and-source) | [`w73.rs:1637`](../../tools/release-coordinator/src/w73.rs#L1637) |
| `OO_W73_ISSUE_BIN` | [configuration-and-source](#configuration-and-source) | [`w73.rs:1638`](../../tools/release-coordinator/src/w73.rs#L1638) |
| `OO_W73_KILL_CANDIDATE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:581`](../../tools/release-coordinator/src/w73.rs#L581) |
| `OO_W73_KILL_EXPECTED` | [configuration-and-source](#configuration-and-source) | [`w73.rs:583`](../../tools/release-coordinator/src/w73.rs#L583) |
| `OO_W73_KILL_LOG` | [configuration-and-source](#configuration-and-source) | [`w73.rs:208`](../../tools/release-coordinator/src/w73.rs#L208) |
| `OO_W73_KILL_SOURCE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:582`](../../tools/release-coordinator/src/w73.rs#L582) |
| `OO_W73_KILL_STEP` | [configuration-and-source](#configuration-and-source) | [`w73.rs:207`](../../tools/release-coordinator/src/w73.rs#L207) |
| `OO_W73_OO_BIN` | [configuration-and-source](#configuration-and-source) | [`w73.rs:1636`](../../tools/release-coordinator/src/w73.rs#L1636) |
| `OO_W73_OPEN_FAULT` | [configuration-and-source](#configuration-and-source) | [`target.rs:597`](../../tools/release-coordinator/src/target.rs#L597) |
| `OO_W73_PG_HOST` | [configuration-and-source](#configuration-and-source) | [`w73.rs:80`](../../tools/release-coordinator/src/w73.rs#L80) |
| `OO_W73_PG_PORT` | [configuration-and-source](#configuration-and-source) | [`w73.rs:62`](../../tools/release-coordinator/src/w73.rs#L62) |
| `OO_W73_PROBE_PASSWORD_FILE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:99`](../../tools/release-coordinator/src/w73.rs#L99) |
| `OO_W73_READER_PASSWORD_FILE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:96`](../../tools/release-coordinator/src/w73.rs#L96) |
| `OO_W73_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`w73.rs:62`](../../tools/release-coordinator/src/w73.rs#L62) |
| `OO_W73_RESULT` | [configuration-and-source](#configuration-and-source) | [`w73.rs:623`](../../tools/release-coordinator/src/w73.rs#L623) |
| `OO_W73_ROLE_PASSWORD_FILE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:98`](../../tools/release-coordinator/src/w73.rs#L98) |
| `OO_W73_SUPER_PASSWORD_FILE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:100`](../../tools/release-coordinator/src/w73.rs#L100) |
| `OO_W73_TARGET_DATABASE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:93`](../../tools/release-coordinator/src/w73.rs#L93) |
| `OO_W73_TARGET_PASSWORD_FILE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:97`](../../tools/release-coordinator/src/w73.rs#L97) |
| `OO_W73_TOKEN_ADMIN_PASSWORD_FILE` | [credentials](#credentials) | [`w73.rs:1639`](../../tools/release-coordinator/src/w73.rs#L1639) |
| `OO_W73_TOKEN_RUNTIME_PASSWORD_FILE` | [credentials](#credentials) | [`w73.rs:1656`](../../tools/release-coordinator/src/w73.rs#L1656) |
| `ORACLE_HOME` | [configuration-and-source](#configuration-and-source) | [`source.rs:1133`](../../crates/oo-cli/src/tui/source.rs#L1133) |
| `ORACLE_HOST` | [configuration-and-source](#configuration-and-source) | [`source.rs:702`](../../crates/oo-cli/src/tui/source.rs#L702) |
| `ORACLE_PASSWORD` | [configuration-and-source](#configuration-and-source) | [`source.rs:93`](../../crates/oo-cli/src/tui/source.rs#L93) |
| `ORACLE_PDB` | [configuration-and-source](#configuration-and-source) | [`source.rs:704`](../../crates/oo-cli/src/tui/source.rs#L704) |
| `ORACLE_PORT` | [configuration-and-source](#configuration-and-source) | [`source.rs:714`](../../crates/oo-cli/src/tui/source.rs#L714) |
| `ORACLE_SERVICE` | [configuration-and-source](#configuration-and-source) | [`source.rs:704`](../../crates/oo-cli/src/tui/source.rs#L704) |
| `ORACLE_SID` | [configuration-and-source](#configuration-and-source) | [`source.rs:705`](../../crates/oo-cli/src/tui/source.rs#L705) |
| `ORACLE_TLS_NOT_AVAILABLE` | [connectivity](#connectivity) | [`doctor.rs:194`](../../crates/oo-cli/src/admin/doctor.rs#L194) |
| `ORACLE_USER` | [configuration-and-source](#configuration-and-source) | [`source.rs:717`](../../crates/oo-cli/src/tui/source.rs#L717) |
| `ORACLE_WORKER_DIGEST_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:210`](../../crates/oo-cli/src/admin/doctor.rs#L210) |
| `OUT_DIR` | [configuration-and-source](#configuration-and-source) | [`lib.rs:16`](../../crates/oo-gateway/src/lib.rs#L16) |
| `OVERRIDE_BINDING` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:245`](../../crates/oo-cli/src/tui/adjudication.rs#L245) |
| `OVERRIDE_CLASS` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:267`](../../crates/oo-cli/src/tui/adjudication.rs#L267) |
| `OVERRIDE_DECISION` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:255`](../../crates/oo-cli/src/tui/adjudication.rs#L255) |
| `OVERRIDE_FANOUT` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:284`](../../crates/oo-cli/src/tui/adjudication.rs#L284) |
| `OVERRIDE_REASON_EMPTY` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:278`](../../crates/oo-cli/src/tui/adjudication.rs#L278) |
| `OVERRIDE_REASON_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:258`](../../crates/oo-cli/src/tui/adjudication.rs#L258) |
| `OVERRIDE_SHAPE` | [configuration-and-source](#configuration-and-source) | [`ontology_graph.rs:95`](../../crates/oo-cli/src/admin/ontology_graph.rs#L95) |
| `OVERRIDE_UNKNOWN_ITEM` | [configuration-and-source](#configuration-and-source) | [`adjudication.rs:252`](../../crates/oo-cli/src/tui/adjudication.rs#L252) |
| `PARTIAL_TRANSACTION` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3836`](../../crates/oo-gateway/src/public_surface.rs#L3836) |
| `PASSWORD_EMPTY` | [configuration-and-source](#configuration-and-source) | [`connect.rs:558`](../../crates/oo-cli/src/tui/screens/connect.rs#L558) |
| `PASTE_EMPTY` | [configuration-and-source](#configuration-and-source) | [`source.rs:757`](../../crates/oo-cli/src/tui/source.rs#L757) |
| `PASTE_UNKNOWN` | [configuration-and-source](#configuration-and-source) | [`connect.rs:863`](../../crates/oo-cli/src/tui/screens/connect.rs#L863) |
| `PORT_CLOSED` | [configuration-and-source](#configuration-and-source) | [`netprobe.rs:49`](../../crates/oo-cli/src/tui/netprobe.rs#L49) |
| `PORT_TIMEOUT` | [query-budget](#query-budget) | [`netprobe.rs:57`](../../crates/oo-cli/src/tui/netprobe.rs#L57) |
| `PREFLIGHT_BLOCKED` | [configuration-and-source](#configuration-and-source) | [`draft.py:170`](../../deploy/lite/draft.py#L170) |
| `PRIVATE_FILE` | [configuration-and-source](#configuration-and-source) | [`draft.py:36`](../../deploy/lite/draft.py#L36) |
| `PROFILE_BINDING` | [credentials](#credentials) | [`backend.rs:90`](../../crates/oo-cli/src/tui/backend.rs#L90) |
| `PROFILE_DIRECTORY` | [credentials](#credentials) | [`wizard.rs:1314`](../../crates/oo-cli/src/admin/wizard.rs#L1314) |
| `PROFILE_EXISTS` | [credentials](#credentials) | [`lifecycle.rs:680`](../../crates/oo-cli/src/lifecycle.rs#L680) |
| `PROFILE_EXISTS_OR_WRITE_FAILED` | [credentials](#credentials) | [`profile.rs:68`](../../crates/oo-cli/src/profile.rs#L68) |
| `PROFILE_INVALID` | [credentials](#credentials) | [`profile.rs:59`](../../crates/oo-cli/src/profile.rs#L59) |
| `PROFILE_NAME` | [credentials](#credentials) | [`mcp_config.rs:80`](../../crates/oo-cli/src/mcp_config.rs#L80) |
| `PROFILE_NOT_PROVISIONED` | [credentials](#credentials) | [`backend.rs:85`](../../crates/oo-cli/src/tui/backend.rs#L85) |
| `PROFILE_READ` | [credentials](#credentials) | [`profile.rs:18`](../../crates/oo-cli/src/profile.rs#L18) |
| `PROFILE_WRITE` | [credentials](#credentials) | [`profile.rs:75`](../../crates/oo-cli/src/profile.rs#L75) |
| `PROTOCOL_UNSUPPORTED` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:121`](../../packages/cli/bin/auth.mjs#L121) |
| `QUERY_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`http_error.rs:52`](../../crates/oo-gateway/src/http_error.rs#L52) |
| `QUERY_TOO_SHORT` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:404`](../../crates/oo-cli/src/outcome.rs#L404) |
| `RATE_LIMITED` | [query-budget](#query-budget) | [`outcome.rs:406`](../../crates/oo-cli/src/outcome.rs#L406) |
| `READINESS_AUTH` | [credentials](#credentials) | [`check.rs:6`](../../crates/oo-cli/src/check.rs#L6) |
| `READINESS_CONNECTION` | [connectivity](#connectivity) | [`check.rs:5`](../../crates/oo-cli/src/check.rs#L5) |
| `READINESS_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`check.rs:21`](../../crates/oo-cli/src/check.rs#L21) |
| `READINESS_REFUSED` | [configuration-and-source](#configuration-and-source) | [`check.rs:12`](../../crates/oo-cli/src/check.rs#L12) |
| `READINESS_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`profile.rs:160`](../../crates/oo-cli/src/profile.rs#L160) |
| `READINESS_TRANSPORT` | [configuration-and-source](#configuration-and-source) | [`check.rs:10`](../../crates/oo-cli/src/check.rs#L10) |
| `RECENT_IO` | [configuration-and-source](#configuration-and-source) | [`source.rs:1266`](../../crates/oo-cli/src/tui/source.rs#L1266) |
| `RECENT_REFERENCE` | [configuration-and-source](#configuration-and-source) | [`source.rs:1253`](../../crates/oo-cli/src/tui/source.rs#L1253) |
| `RECOMPUTE_WHOLE_QUESTION` | [configuration-and-source](#configuration-and-source) | [`f22_owned_pg_tests.rs:533`](../../crates/oo-gateway/src/f22_owned_pg_tests.rs#L533) |
| `RECONCILE_MISMATCH` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3833`](../../crates/oo-gateway/src/public_surface.rs#L3833) |
| `RECONCILE_STALE` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3834`](../../crates/oo-gateway/src/public_surface.rs#L3834) |
| `REDUCE_MOTION` | [configuration-and-source](#configuration-and-source) | [`theme.rs:98`](../../crates/oo-cli/src/tui/theme.rs#L98) |
| `RELATION_NOT_FOUND` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:398`](../../crates/oo-cli/src/outcome.rs#L398) |
| `REMOTE_AGENT_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:596`](../../deploy/lite/remote.py#L596) |
| `REMOTE_BUILD_CYCLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:496`](../../deploy/lite/remote.py#L496) |
| `REMOTE_CREDENTIAL_CHANGE_REQUIRES_EXPLICIT_ROTATION` | [credentials](#credentials) | [`remote.py:75`](../../deploy/lite/remote.py#L75) |
| `REMOTE_IMAGE_CONTENT_MISMATCH` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:128`](../../deploy/lite/remote.py#L128) |
| `REMOTE_IMAGE_LOAD` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:117`](../../deploy/lite/remote.py#L117) |
| `REMOTE_IMAGE_LOAD_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:121`](../../deploy/lite/remote.py#L121) |
| `REMOTE_IMAGE_PULL` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:224`](../../deploy/lite/remote.py#L224) |
| `REMOTE_IMAGE_REFERENCE` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:218`](../../deploy/lite/remote.py#L218) |
| `REMOTE_IMAGE_TAG` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:202`](../../deploy/lite/remote.py#L202) |
| `REMOTE_OWNERSHIP` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:421`](../../deploy/lite/remote.py#L421) |
| `REMOTE_PRIVATE_DIRECTORY` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:54`](../../deploy/lite/remote.py#L54) |
| `REMOTE_PRIVATE_FILE` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:65`](../../deploy/lite/remote.py#L65) |
| `REMOTE_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:581`](../../deploy/lite/remote.py#L581) |
| `REMOTE_REMOVE_` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:423`](../../deploy/lite/remote.py#L423) |
| `REMOTE_REQUEST_BUDGET` | [model](#model) | [`remote.py:578`](../../deploy/lite/remote.py#L578) |
| `REMOTE_SECRET_NAME` | [credentials](#credentials) | [`remote.py:313`](../../deploy/lite/remote.py#L313) |
| `REMOTE_SYSTEMD_RELOAD` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:350`](../../deploy/lite/remote.py#L350) |
| `REMOTE_TIMER_ENABLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:353`](../../deploy/lite/remote.py#L353) |
| `REQUEST_BUDGET` | [model](#model) | [`model_egress.py:289`](../../tools/model-egress/model_egress.py#L289) |
| `REQUEST_REJECTED` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:504`](../../tools/model-egress/model_egress.py#L504) |
| `REQUEST_SCHEMA` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:286`](../../tools/model-egress/model_egress.py#L286) |
| `RESPONSE_BUDGET` | [model](#model) | [`model_egress.py:189`](../../tools/model-egress/model_egress.py#L189) |
| `RESUME_ARTIFACT_CHANGED` | [configuration-and-source](#configuration-and-source) | [`draft.py:76`](../../deploy/lite/draft.py#L76) |
| `RESUME_CONFIG_CHANGED` | [configuration-and-source](#configuration-and-source) | [`draft.py:70`](../../deploy/lite/draft.py#L70) |
| `RESUME_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`draft.py:72`](../../deploy/lite/draft.py#L72) |
| `REVIEW_ADJUDICATION_INPUT_BINDING` | [review-and-publication](#review-and-publication) | [`review.rs:654`](../../crates/oo-cli/src/admin/review.rs#L654) |
| `REVIEW_BINDING` | [review-and-publication](#review-and-publication) | [`review.rs:380`](../../crates/oo-cli/src/admin/review.rs#L380) |
| `REVIEW_COMPILER_INSTALL` | [review-and-publication](#review-and-publication) | [`review.rs:346`](../../crates/oo-cli/src/admin/review.rs#L346) |
| `REVIEW_COMPILER_IO` | [review-and-publication](#review-and-publication) | [`review.rs:359`](../../crates/oo-cli/src/admin/review.rs#L359) |
| `REVIEW_COMPILER_PROTOCOL` | [review-and-publication](#review-and-publication) | [`review.rs:371`](../../crates/oo-cli/src/admin/review.rs#L371) |
| `REVIEW_COMPILER_REJECTED` | [review-and-publication](#review-and-publication) | [`review.rs:369`](../../crates/oo-cli/src/admin/review.rs#L369) |
| `REVIEW_CONFIRM_BINDING` | [review-and-publication](#review-and-publication) | [`review.rs:570`](../../crates/oo-cli/src/admin/review.rs#L570) |
| `REVIEW_DECISION_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:283`](../../crates/oo-cli/src/admin/review.rs#L283) |
| `REVIEW_DUPLICATE_OR_EMPTY` | [review-and-publication](#review-and-publication) | [`review.rs:249`](../../crates/oo-cli/src/admin/review.rs#L249) |
| `REVIEW_EDGE_DECISION` | [review-and-publication](#review-and-publication) | [`review.rs:450`](../../crates/oo-cli/src/admin/review.rs#L450) |
| `REVIEW_EDIT_IMMUTABLE_FIELD` | [review-and-publication](#review-and-publication) | [`review.rs:272`](../../crates/oo-cli/src/admin/review.rs#L272) |
| `REVIEW_EDIT_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:265`](../../crates/oo-cli/src/admin/review.rs#L265) |
| `REVIEW_ENTITY_TABLE` | [review-and-publication](#review-and-publication) | [`review.rs:193`](../../crates/oo-cli/src/admin/review.rs#L193) |
| `REVIEW_FILE_IO` | [review-and-publication](#review-and-publication) | [`review.rs:51`](../../crates/oo-cli/src/admin/review.rs#L51) |
| `REVIEW_FILE_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:679`](../../crates/oo-cli/src/admin/review.rs#L679) |
| `REVIEW_FILE_SHAPE` | [review-and-publication](#review-and-publication) | [`review.rs:675`](../../crates/oo-cli/src/admin/review.rs#L675) |
| `REVIEW_G14_BINDING` | [review-and-publication](#review-and-publication) | [`review.rs:506`](../../crates/oo-cli/src/admin/review.rs#L506) |
| `REVIEW_G14_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:500`](../../crates/oo-cli/src/admin/review.rs#L500) |
| `REVIEW_HANDOFF_BINDING` | [review-and-publication](#review-and-publication) | [`review.rs:479`](../../crates/oo-cli/src/admin/review.rs#L479) |
| `REVIEW_HANDOFF_SHAPE` | [review-and-publication](#review-and-publication) | [`review.rs:468`](../../crates/oo-cli/src/admin/review.rs#L468) |
| `REVIEW_INPUT_SHAPE` | [review-and-publication](#review-and-publication) | [`review.rs:134`](../../crates/oo-cli/src/admin/review.rs#L134) |
| `REVIEW_ITEM_KIND` | [review-and-publication](#review-and-publication) | [`review.rs:212`](../../crates/oo-cli/src/admin/review.rs#L212) |
| `REVIEW_ITEM_POINTER` | [review-and-publication](#review-and-publication) | [`review.rs:428`](../../crates/oo-cli/src/admin/review.rs#L428) |
| `REVIEW_JSON` | [review-and-publication](#review-and-publication) | [`review.rs:43`](../../crates/oo-cli/src/admin/review.rs#L43) |
| `REVIEW_LOCK` | [review-and-publication](#review-and-publication) | [`review.rs:624`](../../crates/oo-cli/src/admin/review.rs#L624) |
| `REVIEW_MISSING_ITEM` | [review-and-publication](#review-and-publication) | [`review.rs:389`](../../crates/oo-cli/src/admin/review.rs#L389) |
| `REVIEW_OUTPUT_EXISTS_OR_IO` | [review-and-publication](#review-and-publication) | [`review.rs:76`](../../crates/oo-cli/src/admin/review.rs#L76) |
| `REVIEW_PENDING_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:838`](../../crates/oo-cli/src/admin/review.rs#L838) |
| `REVIEW_PII_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:413`](../../crates/oo-cli/src/admin/review.rs#L413) |
| `REVIEW_PRIVATE_FILE_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:60`](../../crates/oo-cli/src/admin/review.rs#L60) |
| `REVIEW_SEMANTIC_TYPE_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:415`](../../crates/oo-cli/src/admin/review.rs#L415) |
| `REVIEW_UNRESOLVED_OR_PROTOCOL` | [review-and-publication](#review-and-publication) | [`review.rs:185`](../../crates/oo-cli/src/admin/review.rs#L185) |
| `SCHEMA_CHANGED` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3773`](../../crates/oo-gateway/src/public_surface.rs#L3773) |
| `SCHEMA_INVALID` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:413`](../../crates/oo-cli/src/outcome.rs#L413) |
| `SCOPE_DISCOVERY_NOT_AVAILABLE` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:273`](../../crates/oo-cli/src/admin/doctor.rs#L273) |
| `SCOPE_PATTERN_NOT_AVAILABLE` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:268`](../../crates/oo-cli/src/admin/doctor.rs#L268) |
| `SCOPE_REQUIRES_QUALIFIED_TABLE` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:278`](../../crates/oo-cli/src/admin/doctor.rs#L278) |
| `SECRET_BUDGET` | [model](#model) | [`secret.rs:76`](../../crates/oo-stack/src/secret.rs#L76) |
| `SECRET_CONTENT` | [credentials](#credentials) | [`model_egress.py:80`](../../tools/model-egress/model_egress.py#L80) |
| `SECRET_CREATE` | [credentials](#credentials) | [`secret.rs:114`](../../crates/oo-stack/src/secret.rs#L114) |
| `SECRET_DIRECTORY` | [credentials](#credentials) | [`wizard.rs:168`](../../crates/oo-cli/src/admin/wizard.rs#L168) |
| `SECRET_ECHO` | [credentials](#credentials) | [`live_check.py:38`](../../tools/model-egress/live_check.py#L38) |
| `SECRET_ENV` | [credentials](#credentials) | [`connect.rs:560`](../../crates/oo-cli/src/tui/screens/connect.rs#L560) |
| `SECRET_FILE` | [credentials](#credentials) | [`connect.rs:566`](../../crates/oo-cli/src/tui/screens/connect.rs#L566) |
| `SECRET_HOME` | [credentials](#credentials) | [`doctor.rs:96`](../../crates/oo-cli/src/admin/doctor.rs#L96) |
| `SECRET_NAME` | [credentials](#credentials) | [`wizard.rs:108`](../../crates/oo-cli/src/admin/wizard.rs#L108) |
| `SECRET_NAME_EXHAUSTED` | [credentials](#credentials) | [`wizard.rs:690`](../../crates/oo-cli/src/admin/wizard.rs#L690) |
| `SECRET_OPEN` | [credentials](#credentials) | [`secret.rs:69`](../../crates/oo-stack/src/secret.rs#L69) |
| `SECRET_OWNER` | [credentials](#credentials) | [`secret.rs:207`](../../crates/oo-stack/src/secret.rs#L207) |
| `SECRET_PATH` | [credentials](#credentials) | [`secret.rs:134`](../../crates/oo-stack/src/secret.rs#L134) |
| `SECRET_PATH_REQUIRED` | [credentials](#credentials) | [`secret.rs:90`](../../crates/oo-stack/src/secret.rs#L90) |
| `SECRET_PERMISSIONS` | [credentials](#credentials) | [`model_egress.py:50`](../../tools/model-egress/model_egress.py#L50) |
| `SECRET_READ` | [credentials](#credentials) | [`secret.rs:74`](../../crates/oo-stack/src/secret.rs#L74) |
| `SECRET_REFERENCE` | [credentials](#credentials) | [`model_egress.py:58`](../../tools/model-egress/model_egress.py#L58) |
| `SECRET_STAT` | [credentials](#credentials) | [`secret.rs:140`](../../crates/oo-stack/src/secret.rs#L140) |
| `SECRET_UTF8` | [credentials](#credentials) | [`secret.rs:78`](../../crates/oo-stack/src/secret.rs#L78) |
| `SECRET_WRITE` | [credentials](#credentials) | [`secret.rs:118`](../../crates/oo-stack/src/secret.rs#L118) |
| `SERVICE_NAME` | [configuration-and-source](#configuration-and-source) | [`source.rs:547`](../../crates/oo-cli/src/tui/source.rs#L547) |
| `SOURCE_AUTH` | [credentials](#credentials) | [`build.rs:586`](../../crates/oo-cli/src/tui/screens/build.rs#L586) |
| `SOURCE_CATALOG_ARGUMENT` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:29`](../../crates/oo-cli/src/admin/build_retained.rs#L29) |
| `SOURCE_CATALOG_FAILED` | [configuration-and-source](#configuration-and-source) | [`main.rs:67`](../../deploy/lite/mysql-doctor/src/main.rs#L67) |
| `SOURCE_CATALOG_UNSUPPORTED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:42`](../../crates/oo-cli/src/admin/build_retained.rs#L42) |
| `SOURCE_SIGN_IN_FAILED` | [configuration-and-source](#configuration-and-source) | [`main.rs:79`](../../deploy/lite/mysql-doctor/src/main.rs#L79) |
| `SOURCE_TLS_CA_REQUIRED` | [connectivity](#connectivity) | [`doctor.rs:304`](../../crates/oo-cli/src/admin/doctor.rs#L304) |
| `SQLSERVER_TLS_REQUIRED` | [connectivity](#connectivity) | [`doctor.rs:414`](../../crates/oo-cli/src/admin/doctor.rs#L414) |
| `SQL_PARSE_ERROR` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:395`](../../crates/oo-cli/src/outcome.rs#L395) |
| `SQL_UNSUPPORTED` | [configuration-and-source](#configuration-and-source) | [`http_error.rs:55`](../../crates/oo-gateway/src/http_error.rs#L55) |
| `SSH_ASKPASS` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:68`](../../crates/oo-cli/src/admin/remote.rs#L68) |
| `SSH_ASKPASS_PROMPT` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:41`](../../crates/oo-cli/src/admin/remote.rs#L41) |
| `SSH_ASKPASS_REQUIRE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:69`](../../crates/oo-cli/src/admin/remote.rs#L69) |
| `SSH_AUTH_FAILED` | [credentials](#credentials) | [`remote.rs:173`](../../crates/oo-cli/src/admin/remote.rs#L173) |
| `SSH_CLIENT_UNAVAILABLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:116`](../../crates/oo-cli/src/admin/remote.rs#L116) |
| `SSH_COMPOSE_TEMPLATE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:418`](../../crates/oo-cli/src/admin/remote.rs#L418) |
| `SSH_CONNECT_FAILED` | [connectivity](#connectivity) | [`remote.rs:175`](../../crates/oo-cli/src/admin/remote.rs#L175) |
| `SSH_EXTERNAL_WORKER_NOT_AVAILABLE` | [installation-and-runtime](#installation-and-runtime) | [`doctor.rs:140`](../../crates/oo-cli/src/admin/doctor.rs#L140) |
| `SSH_HOST_KEY_MISMATCH` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:171`](../../crates/oo-cli/src/admin/remote.rs#L171) |
| `SSH_HOST_KEY_REJECTED` | [installation-and-runtime](#installation-and-runtime) | [`wizard.rs:201`](../../crates/oo-cli/src/admin/wizard.rs#L201) |
| `SSH_LITE_ONLY` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:523`](../../crates/oo-cli/src/admin/remote.rs#L523) |
| `SSH_LOCAL_DOCKER` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:352`](../../crates/oo-cli/src/admin/remote.rs#L352) |
| `SSH_LOCAL_IMAGE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:358`](../../crates/oo-cli/src/admin/remote.rs#L358) |
| `SSH_LOCAL_IMAGE_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:362`](../../crates/oo-cli/src/admin/remote.rs#L362) |
| `SSH_LOCAL_IMAGE_SAVE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:507`](../../crates/oo-cli/src/admin/remote.rs#L507) |
| `SSH_LOCAL_IMAGE_UNAVAILABLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:356`](../../crates/oo-cli/src/admin/remote.rs#L356) |
| `SSH_REMOTE_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:432`](../../crates/oo-cli/src/admin/remote.rs#L432) |
| `SSH_REMOTE_LIFECYCLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:661`](../../crates/oo-cli/src/admin/remote.rs#L661) |
| `SSH_REMOTE_PREREQUISITES` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:716`](../../crates/oo-cli/src/admin/remote.rs#L716) |
| `SSH_REMOTE_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:155`](../../crates/oo-cli/src/admin/remote.rs#L155) |
| `SSH_REMOTE_PYTHON_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:153`](../../crates/oo-cli/src/admin/remote.rs#L153) |
| `SSH_RUNTIME` | [installation-and-runtime](#installation-and-runtime) | [`wizard.rs:222`](../../crates/oo-cli/src/admin/wizard.rs#L222) |
| `SSH_RUNTIME_IMAGE_DIGEST_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:407`](../../crates/oo-cli/src/admin/remote.rs#L407) |
| `SSH_STATE_DIRECTORY` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:183`](../../crates/oo-cli/src/admin/remote.rs#L183) |
| `SSH_STATE_PKI` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:762`](../../crates/oo-cli/src/admin/remote.rs#L762) |
| `SSH_STATE_REMOVE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:554`](../../crates/oo-cli/src/admin/remote.rs#L554) |
| `SSH_STATE_WRITE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:200`](../../crates/oo-cli/src/admin/remote.rs#L200) |
| `SSH_STDERR` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:130`](../../crates/oo-cli/src/admin/remote.rs#L130) |
| `SSH_STDIN` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:117`](../../crates/oo-cli/src/admin/remote.rs#L117) |
| `SSH_STREAM_INCOMPLETE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:160`](../../crates/oo-cli/src/admin/remote.rs#L160) |
| `SSH_TARGET` | [installation-and-runtime](#installation-and-runtime) | [`main.rs:61`](../../deploy/lite/mysql-doctor/src/main.rs#L61) |
| `SSH_TARGET_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:521`](../../crates/oo-cli/src/admin/remote.rs#L521) |
| `SSH_UNREACHABLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:232`](../../crates/oo-cli/src/admin/remote.rs#L232) |
| `SSH_WAIT` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:141`](../../crates/oo-cli/src/admin/remote.rs#L141) |
| `STACK_BUDGET` | [model](#model) | [`stack.rs:202`](../../crates/oo-stack/src/stack.rs#L202) |
| `STACK_CONTEXT_WINDOW` | [configuration-and-source](#configuration-and-source) | [`stack.rs:449`](../../crates/oo-stack/src/stack.rs#L449) |
| `STACK_DEPLOY` | [configuration-and-source](#configuration-and-source) | [`stack.rs:454`](../../crates/oo-stack/src/stack.rs#L454) |
| `STACK_DEPLOY_SSH` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:311`](../../crates/oo-cli/src/admin/remote.rs#L311) |
| `STACK_GATEWAY` | [configuration-and-source](#configuration-and-source) | [`stack.rs:474`](../../crates/oo-stack/src/stack.rs#L474) |
| `STACK_INLINE_SECRET` | [credentials](#credentials) | [`stack.rs:379`](../../crates/oo-stack/src/stack.rs#L379) |
| `STACK_INTERVAL` | [configuration-and-source](#configuration-and-source) | [`stack.rs:408`](../../crates/oo-stack/src/stack.rs#L408) |
| `STACK_MODEL` | [model](#model) | [`stack.rs:444`](../../crates/oo-stack/src/stack.rs#L444) |
| `STACK_SCHEMA` | [configuration-and-source](#configuration-and-source) | [`stack.rs:220`](../../crates/oo-stack/src/stack.rs#L220) |
| `STACK_SCOPE` | [configuration-and-source](#configuration-and-source) | [`stack.rs:418`](../../crates/oo-stack/src/stack.rs#L418) |
| `STACK_SOURCE` | [configuration-and-source](#configuration-and-source) | [`stack.rs:403`](../../crates/oo-stack/src/stack.rs#L403) |
| `STACK_SOURCE_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:646`](../../crates/oo-cli/src/admin/wizard.rs#L646) |
| `STACK_SOURCE_SSH` | [installation-and-runtime](#installation-and-runtime) | [`wizard.rs:214`](../../crates/oo-cli/src/admin/wizard.rs#L214) |
| `STACK_SSH` | [installation-and-runtime](#installation-and-runtime) | [`stack.rs:432`](../../crates/oo-stack/src/stack.rs#L432) |
| `STAGE_COMMAND_FAILED` | [configuration-and-source](#configuration-and-source) | [`draft.py:154`](../../deploy/lite/draft.py#L154) |
| `STALE_DATA` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3757`](../../crates/oo-gateway/src/public_surface.rs#L3757) |
| `STATEMENT_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:396`](../../crates/oo-cli/src/outcome.rs#L396) |
| `STATE_DIRECTORY` | [configuration-and-source](#configuration-and-source) | [`draft.py:108`](../../deploy/lite/draft.py#L108) |
| `STATE_LOCK` | [configuration-and-source](#configuration-and-source) | [`draft.py:112`](../../deploy/lite/draft.py#L112) |
| `STREAM_INCOMPLETE` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:375`](../../tools/model-egress/model_egress.py#L375) |
| `STREAM_USAGE_MISSING` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:323`](../../tools/model-egress/model_egress.py#L323) |
| `TABLES_EMPTY` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:592`](../../crates/oo-cli/src/admin/wizard.rs#L592) |
| `TABLES_SKIPPED` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:582`](../../crates/oo-cli/src/admin/wizard.rs#L582) |
| `TABLES_UNKNOWN` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:574`](../../crates/oo-cli/src/admin/wizard.rs#L574) |
| `TABLE_CATALOG_IO` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:516`](../../crates/oo-cli/src/admin/wizard.rs#L516) |
| `TABLE_CATALOG_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:562`](../../crates/oo-cli/src/admin/wizard.rs#L562) |
| `TABLE_CATALOG_SHAPE` | [configuration-and-source](#configuration-and-source) | [`scorer.rs:366`](../../crates/oo-cli/src/tui/scorer.rs#L366) |
| `TABLE_DISCOVERY_CONNECTION_REQUIRED` | [connectivity](#connectivity) | [`wizard.rs:553`](../../crates/oo-cli/src/admin/wizard.rs#L553) |
| `TABLE_PAUSED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:403`](../../crates/oo-cli/src/outcome.rs#L403) |
| `TEST_LOST_RESPONSE` | [configuration-and-source](#configuration-and-source) | [`build_token_tests.rs:111`](../../crates/oo-cli/src/admin/build_token_tests.rs#L111) |
| `TEST_UNREACHABLE` | [configuration-and-source](#configuration-and-source) | [`build_token_tests.rs:159`](../../crates/oo-cli/src/admin/build_token_tests.rs#L159) |
| `TIMEZONE_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:51`](../../packages/cli/bin/oo.mjs#L51) |
| `TIMEZONE_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:34`](../../packages/cli/bin/oo.mjs#L34) |
| `TIME_ARGUMENTS` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:429`](../../packages/cli/bin/oo.mjs#L429) |
| `TIME_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:37`](../../packages/cli/bin/oo.mjs#L37) |
| `TLS_13_ONLY` | [connectivity](#connectivity) | [`netprobe.rs:282`](../../crates/oo-cli/src/tui/netprobe.rs#L282) |
| `TLS_CERTIFICATE` | [connectivity](#connectivity) | [`netprobe.rs:176`](../../crates/oo-cli/src/tui/netprobe.rs#L176) |
| `TLS_CHOOSE` | [connectivity](#connectivity) | [`connect.rs:572`](../../crates/oo-cli/src/tui/screens/connect.rs#L572) |
| `TLS_ENGINE` | [connectivity](#connectivity) | [`netprobe.rs:167`](../../crates/oo-cli/src/tui/netprobe.rs#L167) |
| `TLS_HANDSHAKE` | [connectivity](#connectivity) | [`netprobe.rs:274`](../../crates/oo-cli/src/tui/netprobe.rs#L274) |
| `TLS_IO` | [connectivity](#connectivity) | [`netprobe.rs:145`](../../crates/oo-cli/src/tui/netprobe.rs#L145) |
| `TLS_NOT_OFFERED` | [connectivity](#connectivity) | [`netprobe.rs:149`](../../crates/oo-cli/src/tui/netprobe.rs#L149) |
| `TNS_ADMIN` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:1349`](../../crates/oo-cli/src/admin/wizard.rs#L1349) |
| `TNS_HOST` | [configuration-and-source](#configuration-and-source) | [`source.rs:539`](../../crates/oo-cli/src/tui/source.rs#L539) |
| `TOKEN_FILE` | [credentials](#credentials) | [`w73.rs:2113`](../../tools/release-coordinator/src/w73.rs#L2113) |
| `TOKEN_INPUT_INVALID` | [credentials](#credentials) | [`oo.mjs:215`](../../packages/cli/bin/oo.mjs#L215) |
| `TOKEN_LIFECYCLE` | [credentials](#credentials) | [`auth_lifecycle.rs:136`](../../crates/oo-gateway/src/auth_lifecycle.rs#L136) |
| `TUI_CANCELLED` | [query-budget](#query-budget) | [`ontology_graph.rs:174`](../../crates/oo-cli/src/admin/ontology_graph.rs#L174) |
| `TUI_CA_FILE_REQUIRED` | [connectivity](#connectivity) | [`wizard.rs:1421`](../../crates/oo-cli/src/admin/wizard.rs#L1421) |
| `TUI_CA_PEM_REQUIRED` | [connectivity](#connectivity) | [`wizard.rs:190`](../../crates/oo-cli/src/admin/wizard.rs#L190) |
| `TUI_CONFIG` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:263`](../../crates/oo-cli/src/admin/wizard.rs#L263) |
| `TUI_CONFIG_EXISTS_OR_IO` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:271`](../../crates/oo-cli/src/admin/wizard.rs#L271) |
| `TUI_CONFIG_IO` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:274`](../../crates/oo-cli/src/admin/wizard.rs#L274) |
| `TUI_CONFIRM` | [configuration-and-source](#configuration-and-source) | [`app.rs:317`](../../crates/oo-cli/src/tui/app.rs#L317) |
| `TUI_IMPORT_FILE` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:177`](../../crates/oo-cli/src/admin/wizard.rs#L177) |
| `TUI_IMPORT_UTF8` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:193`](../../crates/oo-cli/src/admin/wizard.rs#L193) |
| `TUI_INPUT` | [configuration-and-source](#configuration-and-source) | [`app.rs:78`](../../crates/oo-cli/src/tui/app.rs#L78) |
| `TUI_PASSWORD_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:1397`](../../crates/oo-cli/src/admin/wizard.rs#L1397) |
| `TUI_PREFLIGHT_BLOCKED` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:280`](../../crates/oo-cli/src/admin/wizard.rs#L280) |
| `TUI_PREFLIGHT_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:1530`](../../crates/oo-cli/src/admin/wizard.rs#L1530) |
| `TUI_RENDER` | [configuration-and-source](#configuration-and-source) | [`app.rs:69`](../../crates/oo-cli/src/tui/app.rs#L69) |
| `TUI_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:718`](../../crates/oo-cli/src/admin/wizard.rs#L718) |
| `TUI_TERMINAL` | [configuration-and-source](#configuration-and-source) | [`app.rs:46`](../../crates/oo-cli/src/tui/app.rs#L46) |
| `UNEXPLAINED_CODES` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3835`](../../crates/oo-gateway/src/public_surface.rs#L3835) |
| `UNQUALIFIED_RELATION` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:400`](../../crates/oo-cli/src/outcome.rs#L400) |
| `UNSUPPORTED_MESSAGE` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:144`](../../tools/model-egress/model_egress.py#L144) |
| `UNSUPPORTED_PARAMETER` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:137`](../../tools/model-egress/model_egress.py#L137) |
| `UNSUPPORTED_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:79`](../../packages/cli/bin/oo.mjs#L79) |
| `UNSUPPORTED_RESPONSE` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:172`](../../tools/model-egress/model_egress.py#L172) |
| `UPSTREAM_AUTH` | [model](#model) | [`model_egress.py:117`](../../tools/model-egress/model_egress.py#L117) |
| `UPSTREAM_ERROR` | [model](#model) | [`model_egress.py:118`](../../tools/model-egress/model_egress.py#L118) |
| `UPSTREAM_PROTOCOL` | [model](#model) | [`model_egress.py:302`](../../tools/model-egress/model_egress.py#L302) |
| `UPSTREAM_REDIRECT` | [model](#model) | [`model_egress.py:263`](../../tools/model-egress/model_egress.py#L263) |
| `UPSTREAM_TIMEOUT` | [model](#model) | [`model_egress.py:387`](../../tools/model-egress/model_egress.py#L387) |
| `USAGE_LIMIT` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:111`](../../tools/model-egress/model_egress.py#L111) |
| `USER_EMPTY` | [configuration-and-source](#configuration-and-source) | [`connect.rs:554`](../../crates/oo-cli/src/tui/screens/connect.rs#L554) |
| `VERSION_UNSUPPORTED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:412`](../../crates/oo-cli/src/outcome.rs#L412) |
| `W73_RAW` | [configuration-and-source](#configuration-and-source) | [`w73.rs:509`](../../tools/release-coordinator/src/w73.rs#L509) |
| `access_denied` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:97`](../../crates/oo-gateway/src/auth_device.rs#L97) |
| `budget_unmeasured` | [model](#model) | [`library_selfbuild.py:593`](../../tools/pg-builder/scripts/library_selfbuild.py#L593) |
| `expired_token` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:230`](../../crates/oo-gateway/src/auth_device.rs#L230) |
| `invalid_grant` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:215`](../../crates/oo-gateway/src/auth_device.rs#L215) |
| `invalid_request` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:94`](../../crates/oo-gateway/src/auth_device.rs#L94) |
| `publication_unverified` | [review-and-publication](#review-and-publication) | [`build_retained.rs:225`](../../crates/oo-cli/src/admin/build_retained.rs#L225) |
| `review_required` | [review-and-publication](#review-and-publication) | [`build.rs:288`](../../crates/oo-cli/src/admin/build.rs#L288) |
| `server_error` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:107`](../../crates/oo-gateway/src/auth_device.rs#L107) |
| `sink_bytes` | [configuration-and-source](#configuration-and-source) | [`c1_repair.rs:28`](../../tools/oo-load/src/c1_repair.rs#L28) |
| `sink_rows` | [configuration-and-source](#configuration-and-source) | [`lib.rs:50`](../../tools/oo-load/src/lib.rs#L50) |
| `sync_mining_floor_state` | [configuration-and-source](#configuration-and-source) | [`log_sync.rs:182`](../../tools/oo-load/src/log_sync.rs#L182) |
| `temporarily_unavailable` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:132`](../../crates/oo-gateway/src/auth_device.rs#L132) |
| `tls_ca_read` | [configuration-and-source](#configuration-and-source) | [`main.rs:664`](../../tools/oo-load/src/main.rs#L664) |
| `tls_ca_required` | [configuration-and-source](#configuration-and-source) | [`main.rs:660`](../../tools/oo-load/src/main.rs#L660) |
