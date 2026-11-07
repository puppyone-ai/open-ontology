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
| `ADMIN_ARGUMENT` | [configuration-and-source](#configuration-and-source) | [`build.rs:140`](../../crates/oo-cli/src/admin/build.rs#L140) |
| `ADMIN_CONFIG_IO` | [configuration-and-source](#configuration-and-source) | [`lite.rs:18`](../../crates/oo-cli/src/admin/lite.rs#L18) |
| `ADMIN_CONFIG_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`mod.rs:307`](../../crates/oo-cli/src/admin/mod.rs#L307) |
| `ADMIN_IDENTITY_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:47`](../../packages/cli/bin/auth.mjs#L47) |
| `ADMIN_NOT_IMPLEMENTED` | [configuration-and-source](#configuration-and-source) | [`mod.rs:299`](../../crates/oo-cli/src/admin/mod.rs#L299) |
| `ALL_IN_ONE_CLI` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2680`](../../tools/release-coordinator/src/w73.rs#L2680) |
| `ALL_IN_ONE_FAILURE` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2745`](../../tools/release-coordinator/src/w73.rs#L2745) |
| `ALL_IN_ONE_MAPPING` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2719`](../../tools/release-coordinator/src/w73.rs#L2719) |
| `ALL_IN_ONE_PUBLISH` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2531`](../../tools/release-coordinator/src/w73.rs#L2531) |
| `ALL_IN_ONE_READY` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2690`](../../tools/release-coordinator/src/w73.rs#L2690) |
| `ARGUMENT_ERROR` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:556`](../../tools/model-egress/model_egress.py#L556) |
| `AUDIT_UNAVAILABLE` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:409`](../../crates/oo-cli/src/outcome.rs#L409) |
| `AUTH_ARGUMENTS` | [credentials](#credentials) | [`auth.mjs:196`](../../packages/cli/bin/auth.mjs#L196) |
| `AUTH_FILE_INVALID` | [credentials](#credentials) | [`auth.mjs:26`](../../packages/cli/bin/auth.mjs#L26) |
| `AUTH_RENEW_REQUIRED` | [credentials](#credentials) | [`auth.mjs:96`](../../packages/cli/bin/auth.mjs#L96) |
| `AUTH_REQUIRED` | [credentials](#credentials) | [`auth.mjs:228`](../../packages/cli/bin/auth.mjs#L228) |
| `AUTO_INCREMENT` | [configuration-and-source](#configuration-and-source) | [`library_observation.py:117`](../../tools/pg-builder/scripts/library_observation.py#L117) |
| `A_B` | [configuration-and-source](#configuration-and-source) | [`policy_catalog_capabilities_pg_tests.rs:755`](../../crates/oo-gateway/src/policy_catalog_capabilities_pg_tests.rs#L755) |
| `BUILD_AGENT_TARGET_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`build.rs:150`](../../crates/oo-cli/src/admin/build.rs#L150) |
| `BUILD_ARTIFACT_PATH` | [configuration-and-source](#configuration-and-source) | [`build.rs:47`](../../crates/oo-cli/src/admin/build.rs#L47) |
| `BUILD_BUILDER_FAILED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:267`](../../crates/oo-cli/src/admin/build_retained.rs#L267) |
| `BUILD_BUILDER_INTERRUPTED_REQUIRES_RECONCILIATION` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:249`](../../crates/oo-cli/src/admin/build_retained.rs#L249) |
| `BUILD_BUILDER_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`build.rs:293`](../../crates/oo-cli/src/admin/build.rs#L293) |
| `BUILD_CANDIDATE_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:197`](../../crates/oo-cli/src/admin/build_retained.rs#L197) |
| `BUILD_CANDIDATE_REFUSED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:201`](../../crates/oo-cli/src/admin/build_retained.rs#L201) |
| `BUILD_CHAIN_EXISTS` | [configuration-and-source](#configuration-and-source) | [`mod.rs:250`](../../crates/oo-cli/src/admin/mod.rs#L250) |
| `BUILD_CLIENT_DIRECTORY` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:90`](../../crates/oo-cli/src/admin/build_tail.rs#L90) |
| `BUILD_COMMAND_OUTPUT_LIMIT` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:117`](../../crates/oo-cli/src/admin/build_retained.rs#L117) |
| `BUILD_COMMAND_PIPE` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:83`](../../crates/oo-cli/src/admin/build_retained.rs#L83) |
| `BUILD_COMMAND_START` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:78`](../../crates/oo-cli/src/admin/build_retained.rs#L78) |
| `BUILD_COMMAND_TIMEOUT_OR_IO` | [query-budget](#query-budget) | [`build_retained.rs:114`](../../crates/oo-cli/src/admin/build_retained.rs#L114) |
| `BUILD_CYCLE_REQUIRES_RESUME_AND_ID` | [configuration-and-source](#configuration-and-source) | [`build.rs:171`](../../crates/oo-cli/src/admin/build.rs#L171) |
| `BUILD_ERROR_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`build.rs:384`](../../crates/oo-cli/src/admin/build.rs#L384) |
| `BUILD_EXECUTION_FAILED` | [configuration-and-source](#configuration-and-source) | [`draft.py:287`](../../deploy/lite/draft.py#L287) |
| `BUILD_EXISTS_USE_RESUME` | [configuration-and-source](#configuration-and-source) | [`build.rs:228`](../../crates/oo-cli/src/admin/build.rs#L228) |
| `BUILD_GATEWAY_NOT_READY` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:209`](../../crates/oo-cli/src/admin/build_tail.rs#L209) |
| `BUILD_GATEWAY_PUBLICATION_BINDING` | [review-and-publication](#review-and-publication) | [`build_tail.rs:134`](../../crates/oo-cli/src/admin/build_tail.rs#L134) |
| `BUILD_GATEWAY_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:150`](../../crates/oo-cli/src/admin/build_tail.rs#L150) |
| `BUILD_GATEWAY_START` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:193`](../../crates/oo-cli/src/admin/build_tail.rs#L193) |
| `BUILD_GATEWAY_TLS` | [connectivity](#connectivity) | [`build_tail.rs:160`](../../crates/oo-cli/src/admin/build_tail.rs#L160) |
| `BUILD_GATEWAY_URL` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:137`](../../crates/oo-cli/src/admin/build_tail.rs#L137) |
| `BUILD_INSTALL` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:61`](../../crates/oo-cli/src/admin/build_retained.rs#L61) |
| `BUILD_INTERRUPTED` | [configuration-and-source](#configuration-and-source) | [`progress.rs:204`](../../crates/oo-cli/src/admin/progress.rs#L204) |
| `BUILD_ISSUANCE_UNCERTAIN_RECONCILE_AUDIT` | [connectivity](#connectivity) | [`build_tail.rs:33`](../../crates/oo-cli/src/admin/build_tail.rs#L33) |
| `BUILD_LIFECYCLE_FAILED_RECONCILE_AUDIT` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:120`](../../crates/oo-cli/src/admin/build_tail.rs#L120) |
| `BUILD_LIFECYCLE_START` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:118`](../../crates/oo-cli/src/admin/build_tail.rs#L118) |
| `BUILD_LITE_GATEWAY_SERVICE` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:369`](../../crates/oo-cli/src/admin/build_lite.rs#L369) |
| `BUILD_LITE_NOT_PROVISIONED` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:36`](../../crates/oo-cli/src/admin/build_lite.rs#L36) |
| `BUILD_LITE_NO_PROGRESS` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:386`](../../crates/oo-cli/src/admin/build_lite.rs#L386) |
| `BUILD_LITE_OWNER` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:46`](../../crates/oo-cli/src/admin/build_lite.rs#L46) |
| `BUILD_LITE_PATH` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:62`](../../crates/oo-cli/src/admin/build_lite.rs#L62) |
| `BUILD_LITE_RETAINED_GRANT` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:321`](../../crates/oo-cli/src/admin/build_lite.rs#L321) |
| `BUILD_LITE_SERVICE_NOT_RUNNING` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:79`](../../crates/oo-cli/src/admin/build_lite.rs#L79) |
| `BUILD_LITE_STACK` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:129`](../../crates/oo-cli/src/admin/build_lite.rs#L129) |
| `BUILD_LITE_STACK_CHANGED` | [installation-and-runtime](#installation-and-runtime) | [`build_lite.rs:338`](../../crates/oo-cli/src/admin/build_lite.rs#L338) |
| `BUILD_LOAD_FAILED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:207`](../../crates/oo-cli/src/admin/build_retained.rs#L207) |
| `BUILD_LOCK` | [configuration-and-source](#configuration-and-source) | [`build.rs:207`](../../crates/oo-cli/src/admin/build.rs#L207) |
| `BUILD_ORIGIN_INCOMPLETE` | [configuration-and-source](#configuration-and-source) | [`build.rs:180`](../../crates/oo-cli/src/admin/build.rs#L180) |
| `BUILD_ORIGIN_MISSING` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:235`](../../crates/oo-cli/src/admin/build_retained.rs#L235) |
| `BUILD_PROMPT_RECEIPT` | [configuration-and-source](#configuration-and-source) | [`build.rs:415`](../../crates/oo-cli/src/admin/build.rs#L415) |
| `BUILD_PUBLICATION_FAILED` | [review-and-publication](#review-and-publication) | [`build_retained.rs:292`](../../crates/oo-cli/src/admin/build_retained.rs#L292) |
| `BUILD_PUBLICATION_RECEIPT` | [review-and-publication](#review-and-publication) | [`build_retained.rs:296`](../../crates/oo-cli/src/admin/build_retained.rs#L296) |
| `BUILD_PUBLICATION_REFUSED_UNCHANGED` | [review-and-publication](#review-and-publication) | [`build_retained.rs:290`](../../crates/oo-cli/src/admin/build_retained.rs#L290) |
| `BUILD_RESUME_ARTIFACT_CHANGED` | [configuration-and-source](#configuration-and-source) | [`build.rs:71`](../../crates/oo-cli/src/admin/build.rs#L71) |
| `BUILD_RESUME_CONFIG_CHANGED` | [configuration-and-source](#configuration-and-source) | [`build.rs:59`](../../crates/oo-cli/src/admin/build.rs#L59) |
| `BUILD_RESUME_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`build.rs:63`](../../crates/oo-cli/src/admin/build.rs#L63) |
| `BUILD_REVIEW_DRAFT_BINDING` | [review-and-publication](#review-and-publication) | [`build.rs:317`](../../crates/oo-cli/src/admin/build.rs#L317) |
| `BUILD_REVIEW_INPUT_CHANGED` | [review-and-publication](#review-and-publication) | [`build.rs:306`](../../crates/oo-cli/src/admin/build.rs#L306) |
| `BUILD_REVIEW_INPUT_REQUIRED` | [review-and-publication](#review-and-publication) | [`build.rs:313`](../../crates/oo-cli/src/admin/build.rs#L313) |
| `BUILD_REVIEW_REJECTED` | [review-and-publication](#review-and-publication) | [`build.rs:335`](../../crates/oo-cli/src/admin/build.rs#L335) |
| `BUILD_RUNTIME_CONFIG` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:25`](../../crates/oo-cli/src/admin/build_retained.rs#L25) |
| `BUILD_RUNTIME_CONFIG_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`build.rs:154`](../../crates/oo-cli/src/admin/build.rs#L154) |
| `BUILD_RUNTIME_FAILED` | [configuration-and-source](#configuration-and-source) | [`build_lite.rs:213`](../../crates/oo-cli/src/admin/build_lite.rs#L213) |
| `BUILD_RUNTIME_SECRET_REF_REQUIRED` | [credentials](#credentials) | [`build_retained.rs:46`](../../crates/oo-cli/src/admin/build_retained.rs#L46) |
| `BUILD_RUNTIME_SOURCE` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:226`](../../crates/oo-cli/src/admin/build_retained.rs#L226) |
| `BUILD_SOURCE_CATALOG_FAILED` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:354`](../../crates/oo-cli/src/admin/build_retained.rs#L354) |
| `BUILD_SOURCE_CATALOG_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:349`](../../crates/oo-cli/src/admin/build_retained.rs#L349) |
| `BUILD_SOURCE_CONNECTION` | [connectivity](#connectivity) | [`build_retained.rs:374`](../../crates/oo-cli/src/admin/build_retained.rs#L374) |
| `BUILD_SOURCE_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:367`](../../crates/oo-cli/src/admin/build_retained.rs#L367) |
| `BUILD_START` | [configuration-and-source](#configuration-and-source) | [`progress.rs:163`](../../crates/oo-cli/src/admin/progress.rs#L163) |
| `BUILD_STATE_DIRECTORY` | [configuration-and-source](#configuration-and-source) | [`build.rs:190`](../../crates/oo-cli/src/admin/build.rs#L190) |
| `BUILD_STATE_IO` | [configuration-and-source](#configuration-and-source) | [`build.rs:24`](../../crates/oo-cli/src/admin/build.rs#L24) |
| `BUILD_STATE_OR_CONFIGURATION` | [configuration-and-source](#configuration-and-source) | [`draft.py:312`](../../deploy/lite/draft.py#L312) |
| `BUILD_STATE_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`progress.rs:67`](../../crates/oo-cli/src/admin/progress.rs#L67) |
| `BUILD_STDIN` | [configuration-and-source](#configuration-and-source) | [`progress.rs:168`](../../crates/oo-cli/src/admin/progress.rs#L168) |
| `BUILD_STDOUT` | [configuration-and-source](#configuration-and-source) | [`progress.rs:174`](../../crates/oo-cli/src/admin/progress.rs#L174) |
| `BUILD_TAIL_RECEIPT_BINDING` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:25`](../../crates/oo-cli/src/admin/build_tail.rs#L25) |
| `BUILD_TAIL_STAGE` | [configuration-and-source](#configuration-and-source) | [`build_tail.rs:272`](../../crates/oo-cli/src/admin/build_tail.rs#L272) |
| `BUILD_TARGET_IO` | [configuration-and-source](#configuration-and-source) | [`progress.rs:154`](../../crates/oo-cli/src/admin/progress.rs#L154) |
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
| `BUILD_WAIT` | [configuration-and-source](#configuration-and-source) | [`progress.rs:207`](../../crates/oo-cli/src/admin/progress.rs#L207) |
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
| `CARGO_MANIFEST_DIR` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2109`](../../tools/release-coordinator/src/w73.rs#L2109) |
| `CARGO_PKG_VERSION` | [configuration-and-source](#configuration-and-source) | [`args.rs:200`](../../crates/oo-cli/src/args.rs#L200) |
| `CARGO_TARGET_DIR` | [configuration-and-source](#configuration-and-source) | [`live_thousand_acceptance.py:125`](../../tools/pg-builder/scripts/live_thousand_acceptance.py#L125) |
| `CLIENT_DISCONNECTED` | [connectivity](#connectivity) | [`model_egress.py:384`](../../tools/model-egress/model_egress.py#L384) |
| `CLI_UPGRADE_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`lifecycle.rs:452`](../../crates/oo-cli/src/lifecycle.rs#L452) |
| `COLUMN_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:399`](../../crates/oo-cli/src/outcome.rs#L399) |
| `COMMUNITY_BATCH_BUDGET` | [model](#model) | [`community_adapter.py:93`](../../deploy/lite/community_adapter.py#L93) |
| `COMMUNITY_CALL_BUDGET` | [model](#model) | [`community_adapter.py:82`](../../deploy/lite/community_adapter.py#L82) |
| `COMMUNITY_CARDS_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:20`](../../deploy/lite/community_adapter.py#L20) |
| `COMMUNITY_COLUMN_COVERAGE` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:26`](../../deploy/lite/community_adapter.py#L26) |
| `COMMUNITY_FOREIGN_KEY_SHAPE` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:11`](../../deploy/lite/community_adapter.py#L11) |
| `COMMUNITY_INCOMPLETE` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:96`](../../deploy/lite/community_adapter.py#L96) |
| `COMMUNITY_TABLE_COVERAGE` | [configuration-and-source](#configuration-and-source) | [`community_adapter.py:36`](../../deploy/lite/community_adapter.py#L36) |
| `CONFIG_ARGUMENTS` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:214`](../../packages/cli/bin/oo.mjs#L214) |
| `CONFIG_BUDGET` | [model](#model) | [`model_egress.py:583`](../../tools/model-egress/model_egress.py#L583) |
| `CONFIG_DIR_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:106`](../../packages/cli/bin/oo.mjs#L106) |
| `CONFIG_ENDPOINT` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:79`](../../tools/model-egress/model_egress.py#L79) |
| `CONFIG_FILE_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:233`](../../packages/cli/bin/oo.mjs#L233) |
| `CONFIG_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:136`](../../packages/cli/bin/oo.mjs#L136) |
| `CONFIG_OR_TRANSPORT_ERROR` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:596`](../../tools/model-egress/model_egress.py#L596) |
| `CONFIG_PERMISSIONS` | [credentials](#credentials) | [`oo.mjs:116`](../../packages/cli/bin/oo.mjs#L116) |
| `CONFIG_SCHEMA` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:204`](../../tools/model-egress/model_egress.py#L204) |
| `COST_TOO_HIGH` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:401`](../../crates/oo-cli/src/outcome.rs#L401) |
| `DB2_WORKER_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:144`](../../crates/oo-cli/src/admin/doctor.rs#L144) |
| `DEVICE_EXPIRED` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:189`](../../packages/cli/bin/auth.mjs#L189) |
| `DM8_TLS_NOT_AVAILABLE` | [connectivity](#connectivity) | [`doctor.rs:170`](../../crates/oo-cli/src/admin/doctor.rs#L170) |
| `DM8_WORKER_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:174`](../../crates/oo-cli/src/admin/doctor.rs#L174) |
| `DOCKER_CONFIG` | [configuration-and-source](#configuration-and-source) | [`lifecycle.py:524`](../../deploy/lite/lifecycle.py#L524) |
| `DOCKER_CONTEXT` | [configuration-and-source](#configuration-and-source) | [`lifecycle.py:524`](../../deploy/lite/lifecycle.py#L524) |
| `DOCKER_DEFAULT_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:524`](../../deploy/lite/lifecycle.py#L524) |
| `DOCKER_HOST` | [configuration-and-source](#configuration-and-source) | [`lifecycle.py:524`](../../deploy/lite/lifecycle.py#L524) |
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
| `EGRESS_PORT_IN_USE_USE_LITE_NETWORK_NAMESPACE` | [model](#model) | [`draft.py:247`](../../deploy/lite/draft.py#L247) |
| `EGRESS_START_FAILED` | [model](#model) | [`draft.py:254`](../../deploy/lite/draft.py#L254) |
| `EGRESS_START_TIMEOUT` | [model](#model) | [`draft.py:261`](../../deploy/lite/draft.py#L261) |
| `ENGINE_NOT_AVAILABLE` | [configuration-and-source](#configuration-and-source) | [`build_retained.rs:332`](../../crates/oo-cli/src/admin/build_retained.rs#L332) |
| `ENGINE_REQUIRED_FOR_NONSTANDARD_PORT` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:257`](../../crates/oo-cli/src/admin/doctor.rs#L257) |
| `ENTITY_CHECK_FAILED` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3616`](../../crates/oo-gateway/src/public_surface.rs#L3616) |
| `ENUMERATION_REFUSED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:405`](../../crates/oo-cli/src/outcome.rs#L405) |
| `F22_USER_ROW_READ` | [configuration-and-source](#configuration-and-source) | [`f22_owned_pg_tests.rs:89`](../../crates/oo-gateway/src/f22_owned_pg_tests.rs#L89) |
| `FUNCTION_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:397`](../../crates/oo-cli/src/outcome.rs#L397) |
| `GATEWAY_CREDENTIAL_REQUIRED` | [credentials](#credentials) | [`oo.mjs:276`](../../packages/cli/bin/oo.mjs#L276) |
| `GATEWAY_INVALID` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:41`](../../packages/cli/bin/auth.mjs#L41) |
| `GATEWAY_ORIGIN` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2114`](../../tools/release-coordinator/src/w73.rs#L2114) |
| `GATEWAY_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:84`](../../packages/cli/bin/auth.mjs#L84) |
| `GATEWAY_REFUSED` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:99`](../../packages/cli/bin/auth.mjs#L99) |
| `GATEWAY_TRANSPORT` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:68`](../../packages/cli/bin/auth.mjs#L68) |
| `GATE_PREFLIGHT_PROOF` | [configuration-and-source](#configuration-and-source) | [`w73.rs:2485`](../../tools/release-coordinator/src/w73.rs#L2485) |
| `HOST_REJECTED` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:501`](../../tools/model-egress/model_egress.py#L501) |
| `IDEMPOTENCY_CONFLICT` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:410`](../../crates/oo-cli/src/outcome.rs#L410) |
| `INTERFACE_INVALID` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:59`](../../packages/cli/bin/auth.mjs#L59) |
| `IP_BLOCKED` | [configuration-and-source](#configuration-and-source) | [`model_provider.py:223`](../../tools/pg-builder/scripts/model_provider.py#L223) |
| `LISTEN_NOT_LOOPBACK` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:468`](../../tools/model-egress/model_egress.py#L468) |
| `LITE_ACTION` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:607`](../../deploy/lite/lifecycle.py#L607) |
| `LITE_AGENT_DB_INSTALL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:346`](../../deploy/lite/lifecycle.py#L346) |
| `LITE_BUILD_ONE_SOURCE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:334`](../../deploy/lite/lifecycle.py#L334) |
| `LITE_CERTIFICATE` | [connectivity](#connectivity) | [`credentials.rs:191`](../../crates/oo-stack/src/credentials.rs#L191) |
| `LITE_CERTIFICATE_DATE` | [connectivity](#connectivity) | [`credentials.rs:149`](../../crates/oo-stack/src/credentials.rs#L149) |
| `LITE_CERTIFICATE_KEY_MISMATCH` | [connectivity](#connectivity) | [`credentials.rs:203`](../../crates/oo-stack/src/credentials.rs#L203) |
| `LITE_COMPOSE_TEMPLATE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:328`](../../crates/oo-cli/src/admin/lite.rs#L328) |
| `LITE_CONTROL_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:76`](../../deploy/lite/lifecycle.py#L76) |
| `LITE_CREDENTIAL_MARKER` | [credentials](#credentials) | [`credentials.rs:169`](../../crates/oo-stack/src/credentials.rs#L169) |
| `LITE_DOCKER_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:115`](../../deploy/lite/lifecycle.py#L115) |
| `LITE_DOCKER_UNAVAILABLE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:337`](../../crates/oo-cli/src/admin/lite.rs#L337) |
| `LITE_GATEWAY_DATA_LOGIN` | [credentials](#credentials) | [`lifecycle.py:215`](../../deploy/lite/lifecycle.py#L215) |
| `LITE_GATEWAY_RETAINED_GRANT` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:231`](../../deploy/lite/lifecycle.py#L231) |
| `LITE_HEALTH_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:205`](../../deploy/lite/lifecycle.py#L205) |
| `LITE_HEALTH_REFERENCE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:177`](../../deploy/lite/lifecycle.py#L177) |
| `LITE_HEALTH_REFUSED` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:159`](../../deploy/lite/lifecycle.py#L159) |
| `LITE_HEALTH_RUNTIME` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:157`](../../deploy/lite/lifecycle.py#L157) |
| `LITE_HEALTH_TOKEN` | [credentials](#credentials) | [`lifecycle.py:181`](../../deploy/lite/lifecycle.py#L181) |
| `LITE_HEALTH_URL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:170`](../../deploy/lite/lifecycle.py#L170) |
| `LITE_IMAGES_MISSING` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:354`](../../crates/oo-cli/src/admin/lite.rs#L354) |
| `LITE_IMAGE_CHANGE_REQUIRES_EXPLICIT_MIGRATION` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:565`](../../deploy/lite/lifecycle.py#L565) |
| `LITE_IMAGE_DIGEST_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:27`](../../crates/oo-cli/src/admin/lite.rs#L27) |
| `LITE_JOBS_SCHEMA` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:465`](../../deploy/lite/lifecycle.py#L465) |
| `LITE_JOBS_TEMPLATES` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:458`](../../deploy/lite/lifecycle.py#L458) |
| `LITE_LANE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:534`](../../deploy/lite/lifecycle.py#L534) |
| `LITE_LIFECYCLE_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:648`](../../deploy/lite/lifecycle.py#L648) |
| `LITE_LIFECYCLE_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`remote.py:267`](../../deploy/lite/remote.py#L267) |
| `LITE_LOCAL_ONLY` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:16`](../../crates/oo-cli/src/admin/lite.rs#L16) |
| `LITE_OPENSSL_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:76`](../../crates/oo-stack/src/credentials.rs#L76) |
| `LITE_OPENSSL_RUNTIME` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:242`](../../crates/oo-cli/src/admin/lite.rs#L242) |
| `LITE_OPENSSL_START` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:95`](../../crates/oo-stack/src/credentials.rs#L95) |
| `LITE_OPENSSL_STDIN` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:99`](../../crates/oo-stack/src/credentials.rs#L99) |
| `LITE_OPENSSL_WAIT` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:108`](../../crates/oo-stack/src/credentials.rs#L108) |
| `LITE_OPERATOR_CHANGE_REQUIRES_EXPLICIT_MIGRATION` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:265`](../../deploy/lite/lifecycle.py#L265) |
| `LITE_OPERATOR_PASSWORD` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:273`](../../deploy/lite/lifecycle.py#L273) |
| `LITE_OPERATOR_ROLES` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:285`](../../deploy/lite/lifecycle.py#L285) |
| `LITE_OPERATOR_TOKEN` | [credentials](#credentials) | [`lifecycle.py:295`](../../deploy/lite/lifecycle.py#L295) |
| `LITE_OWNERSHIP` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:557`](../../deploy/lite/lifecycle.py#L557) |
| `LITE_PRIVATE_DIRECTORY` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:90`](../../deploy/lite/lifecycle.py#L90) |
| `LITE_PRIVATE_FILE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:98`](../../deploy/lite/lifecycle.py#L98) |
| `LITE_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:109`](../../crates/oo-cli/src/admin/lite.rs#L109) |
| `LITE_PROVISION_CHANGED` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:306`](../../deploy/lite/lifecycle.py#L306) |
| `LITE_PROVISION_PASSWORD` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:350`](../../deploy/lite/lifecycle.py#L350) |
| `LITE_PUBLIC_URL_LOCAL_HTTPS` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:239`](../../deploy/lite/lifecycle.py#L239) |
| `LITE_RANDOM` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:119`](../../crates/oo-stack/src/credentials.rs#L119) |
| `LITE_SCHEDULE_CHANGE_REQUIRES_EXPLICIT_MIGRATION` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:504`](../../deploy/lite/lifecycle.py#L504) |
| `LITE_SCHEDULE_PROJECT` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:48`](../../deploy/lite/lifecycle.py#L48) |
| `LITE_SCHEDULE_ROW` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:511`](../../deploy/lite/lifecycle.py#L511) |
| `LITE_SECRET_CHANGE_REQUIRES_EXPLICIT_ROTATION` | [credentials](#credentials) | [`lifecycle.py:581`](../../deploy/lite/lifecycle.py#L581) |
| `LITE_SECRET_PATH` | [credentials](#credentials) | [`credentials.rs:127`](../../crates/oo-stack/src/credentials.rs#L127) |
| `LITE_START` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:94`](../../crates/oo-cli/src/admin/lite.rs#L94) |
| `LITE_STATE_DIRECTORY` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:51`](../../crates/oo-cli/src/admin/lite.rs#L51) |
| `LITE_STDIN` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:98`](../../crates/oo-cli/src/admin/lite.rs#L98) |
| `LITE_SUBNET` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:541`](../../deploy/lite/lifecycle.py#L541) |
| `LITE_SUBNET_UNAVAILABLE` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:133`](../../deploy/lite/lifecycle.py#L133) |
| `LITE_SYNC_INTERVAL` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:442`](../../deploy/lite/lifecycle.py#L442) |
| `LITE_WAIT` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:107`](../../crates/oo-cli/src/admin/lite.rs#L107) |
| `LITE_WORKLOAD_NAME` | [installation-and-runtime](#installation-and-runtime) | [`credentials.rs:304`](../../crates/oo-stack/src/credentials.rs#L304) |
| `LIVE_COMPLETION_FAILURE` | [configuration-and-source](#configuration-and-source) | [`live_check.py:46`](../../tools/model-egress/live_check.py#L46) |
| `LIVE_HTTP_FAILURE` | [configuration-and-source](#configuration-and-source) | [`live_check.py:41`](../../tools/model-egress/live_check.py#L41) |
| `LIVE_STREAM_FAILURE` | [configuration-and-source](#configuration-and-source) | [`live_check.py:44`](../../tools/model-egress/live_check.py#L44) |
| `LIVE_TRANSPORT` | [configuration-and-source](#configuration-and-source) | [`live_check.py:55`](../../tools/model-egress/live_check.py#L55) |
| `LOCAL_FAILURE` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:500`](../../packages/cli/bin/oo.mjs#L500) |
| `LOGIN_CODE_ARGV` | [credentials](#credentials) | [`lifecycle.rs:255`](../../crates/oo-cli/src/lifecycle.rs#L255) |
| `LOGIN_PROFILE_SAVE_FAILED` | [credentials](#credentials) | [`lifecycle.rs:745`](../../crates/oo-cli/src/lifecycle.rs#L745) |
| `LOGIN_VERIFICATION_FAILED` | [credentials](#credentials) | [`lifecycle.rs:737`](../../crates/oo-cli/src/lifecycle.rs#L737) |
| `METER_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:573`](../../tools/model-egress/model_egress.py#L573) |
| `METHOD_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`http_error.rs:54`](../../crates/oo-gateway/src/http_error.rs#L54) |
| `MODEL_CONFIG` | [model](#model) | [`doctor.rs:493`](../../crates/oo-cli/src/admin/doctor.rs#L493) |
| `MODEL_EGRESS_PROTOCOL` | [model](#model) | [`doctor.rs:503`](../../crates/oo-cli/src/admin/doctor.rs#L503) |
| `MODEL_EGRESS_START` | [model](#model) | [`doctor.rs:487`](../../crates/oo-cli/src/admin/doctor.rs#L487) |
| `MODEL_EGRESS_STDIN` | [model](#model) | [`doctor.rs:491`](../../crates/oo-cli/src/admin/doctor.rs#L491) |
| `MODEL_EGRESS_WAIT` | [model](#model) | [`doctor.rs:501`](../../crates/oo-cli/src/admin/doctor.rs#L501) |
| `MYSQL_DOCTOR_FAILED` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:77`](../../crates/oo-cli/src/admin/doctor.rs#L77) |
| `MYSQL_DOCTOR_INPUT` | [configuration-and-source](#configuration-and-source) | [`main.rs:16`](../../deploy/lite/mysql-doctor/src/main.rs#L16) |
| `MYSQL_DOCTOR_INSTALL` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:49`](../../crates/oo-cli/src/admin/doctor.rs#L49) |
| `MYSQL_DOCTOR_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:80`](../../crates/oo-cli/src/admin/doctor.rs#L80) |
| `MYSQL_DOCTOR_RUNTIME` | [configuration-and-source](#configuration-and-source) | [`main.rs:59`](../../deploy/lite/mysql-doctor/src/main.rs#L59) |
| `MYSQL_DOCTOR_STDIN` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:67`](../../crates/oo-cli/src/admin/doctor.rs#L67) |
| `MYSQL_DOCTOR_WAIT` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:75`](../../crates/oo-cli/src/admin/doctor.rs#L75) |
| `NATIVE_BINARY_MISSING` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:92`](../../packages/cli/bin/oo.mjs#L92) |
| `NATIVE_DIGEST_MISMATCH` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:95`](../../packages/cli/bin/oo.mjs#L95) |
| `NATIVE_EXECUTION_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:329`](../../packages/cli/bin/oo.mjs#L329) |
| `NATIVE_MANIFEST_INVALID` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:71`](../../packages/cli/bin/oo.mjs#L71) |
| `NATIVE_NOT_EXECUTABLE` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:98`](../../packages/cli/bin/oo.mjs#L98) |
| `NOT_FOUND` | [configuration-and-source](#configuration-and-source) | [`http_error.rs:53`](../../crates/oo-gateway/src/http_error.rs#L53) |
| `ONTOLOGY_OPENER_FAILED` | [configuration-and-source](#configuration-and-source) | [`ontology.rs:134`](../../crates/oo-cli/src/admin/ontology.rs#L134) |
| `ONTOLOGY_OUTPUT_IO` | [configuration-and-source](#configuration-and-source) | [`ontology.rs:118`](../../crates/oo-cli/src/admin/ontology.rs#L118) |
| `ONTOLOGY_PACK_BUDGET` | [model](#model) | [`ontology.rs:92`](../../crates/oo-cli/src/admin/ontology.rs#L92) |
| `ONTOLOGY_PACK_CARDINALITY` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:294`](../../crates/oo-cli/src/admin/ontology_export.rs#L294) |
| `ONTOLOGY_PACK_DIGEST` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:78`](../../crates/oo-cli/src/admin/ontology_export.rs#L78) |
| `ONTOLOGY_PACK_DUPLICATE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:148`](../../crates/oo-cli/src/admin/ontology_export.rs#L148) |
| `ONTOLOGY_PACK_IO` | [configuration-and-source](#configuration-and-source) | [`ontology.rs:87`](../../crates/oo-cli/src/admin/ontology.rs#L87) |
| `ONTOLOGY_PACK_JSON` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:28`](../../crates/oo-cli/src/admin/ontology_export.rs#L28) |
| `ONTOLOGY_PACK_MISSING_PAGE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:27`](../../crates/oo-cli/src/admin/ontology_export.rs#L27) |
| `ONTOLOGY_PACK_PAGE_CYCLE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:37`](../../crates/oo-cli/src/admin/ontology_export.rs#L37) |
| `ONTOLOGY_PACK_PATH` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:74`](../../crates/oo-cli/src/admin/ontology_export.rs#L74) |
| `ONTOLOGY_PACK_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:41`](../../crates/oo-cli/src/admin/ontology_export.rs#L41) |
| `ONTOLOGY_PACK_REFERENCE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:158`](../../crates/oo-cli/src/admin/ontology_export.rs#L158) |
| `ONTOLOGY_PACK_SHAPE` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:19`](../../crates/oo-cli/src/admin/ontology_export.rs#L19) |
| `ONTOLOGY_ROLE_MISMATCH` | [configuration-and-source](#configuration-and-source) | [`ontology_export.rs:111`](../../crates/oo-cli/src/admin/ontology_export.rs#L111) |
| `ONTOLOGY_YAML_ROLE_OOS_UNAVAILABLE` | [configuration-and-source](#configuration-and-source) | [`ontology.rs:72`](../../crates/oo-cli/src/admin/ontology.rs#L72) |
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
| `OO024` | [database-proof](#database-proof) | [`mapping_reader_access.rs:693`](../../crates/oo-gateway/src/mapping_reader_access.rs#L693) |
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
| `OO_LITE_LANE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:155`](../../crates/oo-cli/src/admin/lite.rs#L155) |
| `OO_LITE_POSTGRES_IMAGE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:329`](../../crates/oo-cli/src/admin/lite.rs#L329) |
| `OO_LITE_PULL_POLICY` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:538`](../../deploy/lite/lifecycle.py#L538) |
| `OO_LITE_RUNTIME_IMAGE` | [installation-and-runtime](#installation-and-runtime) | [`lite.rs:134`](../../crates/oo-cli/src/admin/lite.rs#L134) |
| `OO_LITE_SUBNET` | [installation-and-runtime](#installation-and-runtime) | [`lifecycle.py:542`](../../deploy/lite/lifecycle.py#L542) |
| `OO_MONGO_COLLECTION_CONFIG` | [configuration-and-source](#configuration-and-source) | [`documents.rs:181`](../../tools/oo-load/src/documents.rs#L181) |
| `OO_NATIVE_ADMISSION_BIN` | [installation-and-runtime](#installation-and-runtime) | [`export_native_collection.py:64`](../../tools/pg-builder/scripts/export_native_collection.py#L64) |
| `OO_NATIVE_BINDING_REGISTRY` | [review-and-publication](#review-and-publication) | [`export_native_collection.py:65`](../../tools/pg-builder/scripts/export_native_collection.py#L65) |
| `OO_NATIVE_BINDING_REGISTRY_SHA256` | [review-and-publication](#review-and-publication) | [`export_native_collection.py:66`](../../tools/pg-builder/scripts/export_native_collection.py#L66) |
| `OO_NUMERIC_PG_CONFIG` | [configuration-and-source](#configuration-and-source) | [`policy_catalog_numeric_pg_tests.rs:43`](../../crates/oo-gateway/src/policy_catalog_numeric_pg_tests.rs#L43) |
| `OO_ORACLE_WORKER_IMAGE` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:204`](../../crates/oo-cli/src/admin/doctor.rs#L204) |
| `OO_PARSER_WORKER_SLOW` | [configuration-and-source](#configuration-and-source) | [`parser_worker.rs:720`](../../crates/oo-gateway/src/parser_worker.rs#L720) |
| `OO_PERF_OWNED_FIXTURE` | [configuration-and-source](#configuration-and-source) | [`performance_owned_pg_tests.rs:42`](../../crates/oo-gateway/src/performance_owned_pg_tests.rs#L42) |
| `OO_PERF_PHASE` | [configuration-and-source](#configuration-and-source) | [`performance_owned_pg_tests.rs:293`](../../crates/oo-gateway/src/performance_owned_pg_tests.rs#L293) |
| `OO_PUBLISHED_CUT_FIXTURE` | [configuration-and-source](#configuration-and-source) | [`published_cut_owned_pg_tests.rs:31`](../../crates/oo-gateway/src/published_cut_owned_pg_tests.rs#L31) |
| `OO_SEARCH_INDEX_CONFIG` | [configuration-and-source](#configuration-and-source) | [`documents.rs:75`](../../tools/oo-load/src/documents.rs#L75) |
| `OO_SELFBUILD_BUDGET_BIN` | [model](#model) | [`build_retained.rs:264`](../../crates/oo-cli/src/admin/build_retained.rs#L264) |
| `OO_SHARE_NPM_REGISTRY` | [review-and-publication](#review-and-publication) | [`lite.rs:78`](../../crates/oo-cli/src/admin/lite.rs#L78) |
| `OO_SQL_CAPABILITIES_PG_CONFIG` | [configuration-and-source](#configuration-and-source) | [`policy_catalog_capabilities_pg_tests.rs:83`](../../crates/oo-gateway/src/policy_catalog_capabilities_pg_tests.rs#L83) |
| `OO_SSH_ASKPASS_SECRET` | [credentials](#credentials) | [`remote.rs:28`](../../crates/oo-cli/src/admin/remote.rs#L28) |
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
| `ORACLE_TLS_NOT_AVAILABLE` | [connectivity](#connectivity) | [`doctor.rs:194`](../../crates/oo-cli/src/admin/doctor.rs#L194) |
| `ORACLE_WORKER_DIGEST_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:210`](../../crates/oo-cli/src/admin/doctor.rs#L210) |
| `OUT_DIR` | [configuration-and-source](#configuration-and-source) | [`lib.rs:16`](../../crates/oo-gateway/src/lib.rs#L16) |
| `PARTIAL_TRANSACTION` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3615`](../../crates/oo-gateway/src/public_surface.rs#L3615) |
| `PREFLIGHT_BLOCKED` | [configuration-and-source](#configuration-and-source) | [`draft.py:170`](../../deploy/lite/draft.py#L170) |
| `PRIVATE_FILE` | [configuration-and-source](#configuration-and-source) | [`draft.py:36`](../../deploy/lite/draft.py#L36) |
| `PROFILE_EXISTS` | [credentials](#credentials) | [`lifecycle.rs:679`](../../crates/oo-cli/src/lifecycle.rs#L679) |
| `PROTOCOL_UNSUPPORTED` | [configuration-and-source](#configuration-and-source) | [`auth.mjs:121`](../../packages/cli/bin/auth.mjs#L121) |
| `QUERY_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`http_error.rs:52`](../../crates/oo-gateway/src/http_error.rs#L52) |
| `QUERY_TOO_SHORT` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:404`](../../crates/oo-cli/src/outcome.rs#L404) |
| `RATE_LIMITED` | [query-budget](#query-budget) | [`outcome.rs:406`](../../crates/oo-cli/src/outcome.rs#L406) |
| `RECOMPUTE_WHOLE_QUESTION` | [configuration-and-source](#configuration-and-source) | [`f22_owned_pg_tests.rs:533`](../../crates/oo-gateway/src/f22_owned_pg_tests.rs#L533) |
| `RECONCILE_MISMATCH` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3612`](../../crates/oo-gateway/src/public_surface.rs#L3612) |
| `RECONCILE_STALE` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3613`](../../crates/oo-gateway/src/public_surface.rs#L3613) |
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
| `REQUEST_BUDGET` | [model](#model) | [`model_egress.py:280`](../../tools/model-egress/model_egress.py#L280) |
| `REQUEST_REJECTED` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:495`](../../tools/model-egress/model_egress.py#L495) |
| `REQUEST_SCHEMA` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:277`](../../tools/model-egress/model_egress.py#L277) |
| `RESPONSE_BUDGET` | [model](#model) | [`model_egress.py:180`](../../tools/model-egress/model_egress.py#L180) |
| `RESUME_ARTIFACT_CHANGED` | [configuration-and-source](#configuration-and-source) | [`draft.py:76`](../../deploy/lite/draft.py#L76) |
| `RESUME_CONFIG_CHANGED` | [configuration-and-source](#configuration-and-source) | [`draft.py:70`](../../deploy/lite/draft.py#L70) |
| `RESUME_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`draft.py:72`](../../deploy/lite/draft.py#L72) |
| `REVIEW_BINDING` | [review-and-publication](#review-and-publication) | [`review.rs:368`](../../crates/oo-cli/src/admin/review.rs#L368) |
| `REVIEW_COMPILER_INSTALL` | [review-and-publication](#review-and-publication) | [`review.rs:334`](../../crates/oo-cli/src/admin/review.rs#L334) |
| `REVIEW_COMPILER_IO` | [review-and-publication](#review-and-publication) | [`review.rs:347`](../../crates/oo-cli/src/admin/review.rs#L347) |
| `REVIEW_COMPILER_PROTOCOL` | [review-and-publication](#review-and-publication) | [`review.rs:359`](../../crates/oo-cli/src/admin/review.rs#L359) |
| `REVIEW_COMPILER_REJECTED` | [review-and-publication](#review-and-publication) | [`review.rs:357`](../../crates/oo-cli/src/admin/review.rs#L357) |
| `REVIEW_DECISION_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:271`](../../crates/oo-cli/src/admin/review.rs#L271) |
| `REVIEW_DUPLICATE_OR_EMPTY` | [review-and-publication](#review-and-publication) | [`review.rs:237`](../../crates/oo-cli/src/admin/review.rs#L237) |
| `REVIEW_EDGE_DECISION` | [review-and-publication](#review-and-publication) | [`review.rs:438`](../../crates/oo-cli/src/admin/review.rs#L438) |
| `REVIEW_EDIT_IMMUTABLE_FIELD` | [review-and-publication](#review-and-publication) | [`review.rs:260`](../../crates/oo-cli/src/admin/review.rs#L260) |
| `REVIEW_EDIT_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:253`](../../crates/oo-cli/src/admin/review.rs#L253) |
| `REVIEW_ENTITY_TABLE` | [review-and-publication](#review-and-publication) | [`review.rs:181`](../../crates/oo-cli/src/admin/review.rs#L181) |
| `REVIEW_FILE_IO` | [review-and-publication](#review-and-publication) | [`review.rs:39`](../../crates/oo-cli/src/admin/review.rs#L39) |
| `REVIEW_FILE_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:526`](../../crates/oo-cli/src/admin/review.rs#L526) |
| `REVIEW_FILE_SHAPE` | [review-and-publication](#review-and-publication) | [`review.rs:522`](../../crates/oo-cli/src/admin/review.rs#L522) |
| `REVIEW_G14_BINDING` | [review-and-publication](#review-and-publication) | [`review.rs:474`](../../crates/oo-cli/src/admin/review.rs#L474) |
| `REVIEW_G14_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:468`](../../crates/oo-cli/src/admin/review.rs#L468) |
| `REVIEW_HANDOFF_BINDING` | [review-and-publication](#review-and-publication) | [`review.rs:461`](../../crates/oo-cli/src/admin/review.rs#L461) |
| `REVIEW_HANDOFF_SHAPE` | [review-and-publication](#review-and-publication) | [`review.rs:452`](../../crates/oo-cli/src/admin/review.rs#L452) |
| `REVIEW_INPUT_SHAPE` | [review-and-publication](#review-and-publication) | [`review.rs:122`](../../crates/oo-cli/src/admin/review.rs#L122) |
| `REVIEW_ITEM_KIND` | [review-and-publication](#review-and-publication) | [`review.rs:200`](../../crates/oo-cli/src/admin/review.rs#L200) |
| `REVIEW_ITEM_POINTER` | [review-and-publication](#review-and-publication) | [`review.rs:416`](../../crates/oo-cli/src/admin/review.rs#L416) |
| `REVIEW_JSON` | [review-and-publication](#review-and-publication) | [`review.rs:54`](../../crates/oo-cli/src/admin/review.rs#L54) |
| `REVIEW_LOCK` | [review-and-publication](#review-and-publication) | [`review.rs:512`](../../crates/oo-cli/src/admin/review.rs#L512) |
| `REVIEW_MISSING_ITEM` | [review-and-publication](#review-and-publication) | [`review.rs:377`](../../crates/oo-cli/src/admin/review.rs#L377) |
| `REVIEW_OUTPUT_EXISTS_OR_IO` | [review-and-publication](#review-and-publication) | [`review.rs:64`](../../crates/oo-cli/src/admin/review.rs#L64) |
| `REVIEW_PII_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:401`](../../crates/oo-cli/src/admin/review.rs#L401) |
| `REVIEW_PRIVATE_FILE_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:48`](../../crates/oo-cli/src/admin/review.rs#L48) |
| `REVIEW_SEMANTIC_TYPE_REQUIRED` | [review-and-publication](#review-and-publication) | [`review.rs:403`](../../crates/oo-cli/src/admin/review.rs#L403) |
| `REVIEW_UNRESOLVED_OR_PROTOCOL` | [review-and-publication](#review-and-publication) | [`review.rs:173`](../../crates/oo-cli/src/admin/review.rs#L173) |
| `SCHEMA_INVALID` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:413`](../../crates/oo-cli/src/outcome.rs#L413) |
| `SCOPE_DISCOVERY_NOT_AVAILABLE` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:273`](../../crates/oo-cli/src/admin/doctor.rs#L273) |
| `SCOPE_PATTERN_NOT_AVAILABLE` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:268`](../../crates/oo-cli/src/admin/doctor.rs#L268) |
| `SCOPE_REQUIRES_QUALIFIED_TABLE` | [configuration-and-source](#configuration-and-source) | [`doctor.rs:278`](../../crates/oo-cli/src/admin/doctor.rs#L278) |
| `SECRET_BUDGET` | [model](#model) | [`secret.rs:66`](../../crates/oo-stack/src/secret.rs#L66) |
| `SECRET_CONTENT` | [credentials](#credentials) | [`model_egress.py:71`](../../tools/model-egress/model_egress.py#L71) |
| `SECRET_CREATE` | [credentials](#credentials) | [`secret.rs:97`](../../crates/oo-stack/src/secret.rs#L97) |
| `SECRET_DIRECTORY` | [credentials](#credentials) | [`wizard.rs:340`](../../crates/oo-cli/src/admin/wizard.rs#L340) |
| `SECRET_ECHO` | [credentials](#credentials) | [`live_check.py:38`](../../tools/model-egress/live_check.py#L38) |
| `SECRET_FILE` | [credentials](#credentials) | [`secret.rs:113`](../../crates/oo-stack/src/secret.rs#L113) |
| `SECRET_HOME` | [credentials](#credentials) | [`doctor.rs:96`](../../crates/oo-cli/src/admin/doctor.rs#L96) |
| `SECRET_NAME` | [credentials](#credentials) | [`wizard.rs:160`](../../crates/oo-cli/src/admin/wizard.rs#L160) |
| `SECRET_OPEN` | [credentials](#credentials) | [`secret.rs:59`](../../crates/oo-stack/src/secret.rs#L59) |
| `SECRET_OWNER` | [credentials](#credentials) | [`secret.rs:156`](../../crates/oo-stack/src/secret.rs#L156) |
| `SECRET_PATH` | [credentials](#credentials) | [`secret.rs:122`](../../crates/oo-stack/src/secret.rs#L122) |
| `SECRET_PERMISSIONS` | [credentials](#credentials) | [`model_egress.py:50`](../../tools/model-egress/model_egress.py#L50) |
| `SECRET_READ` | [credentials](#credentials) | [`secret.rs:64`](../../crates/oo-stack/src/secret.rs#L64) |
| `SECRET_REFERENCE` | [credentials](#credentials) | [`model_egress.py:56`](../../tools/model-egress/model_egress.py#L56) |
| `SECRET_STAT` | [credentials](#credentials) | [`secret.rs:106`](../../crates/oo-stack/src/secret.rs#L106) |
| `SECRET_UTF8` | [credentials](#credentials) | [`secret.rs:68`](../../crates/oo-stack/src/secret.rs#L68) |
| `SECRET_WRITE` | [credentials](#credentials) | [`secret.rs:101`](../../crates/oo-stack/src/secret.rs#L101) |
| `SOURCE_CATALOG_FAILED` | [configuration-and-source](#configuration-and-source) | [`main.rs:69`](../../deploy/lite/mysql-doctor/src/main.rs#L69) |
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
| `SSH_HOST_KEY_REJECTED` | [installation-and-runtime](#installation-and-runtime) | [`wizard.rs:242`](../../crates/oo-cli/src/admin/wizard.rs#L242) |
| `SSH_LITE_ONLY` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:523`](../../crates/oo-cli/src/admin/remote.rs#L523) |
| `SSH_LOCAL_DOCKER` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:352`](../../crates/oo-cli/src/admin/remote.rs#L352) |
| `SSH_LOCAL_IMAGE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:358`](../../crates/oo-cli/src/admin/remote.rs#L358) |
| `SSH_LOCAL_IMAGE_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:362`](../../crates/oo-cli/src/admin/remote.rs#L362) |
| `SSH_LOCAL_IMAGE_SAVE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:507`](../../crates/oo-cli/src/admin/remote.rs#L507) |
| `SSH_LOCAL_IMAGE_UNAVAILABLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:356`](../../crates/oo-cli/src/admin/remote.rs#L356) |
| `SSH_REMOTE_FAILED` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:432`](../../crates/oo-cli/src/admin/remote.rs#L432) |
| `SSH_REMOTE_LIFECYCLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:661`](../../crates/oo-cli/src/admin/remote.rs#L661) |
| `SSH_REMOTE_PREREQUISITES` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:714`](../../crates/oo-cli/src/admin/remote.rs#L714) |
| `SSH_REMOTE_PROTOCOL` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:155`](../../crates/oo-cli/src/admin/remote.rs#L155) |
| `SSH_REMOTE_PYTHON_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:153`](../../crates/oo-cli/src/admin/remote.rs#L153) |
| `SSH_RUNTIME` | [installation-and-runtime](#installation-and-runtime) | [`wizard.rs:263`](../../crates/oo-cli/src/admin/wizard.rs#L263) |
| `SSH_RUNTIME_IMAGE_DIGEST_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:407`](../../crates/oo-cli/src/admin/remote.rs#L407) |
| `SSH_STATE_DIRECTORY` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:183`](../../crates/oo-cli/src/admin/remote.rs#L183) |
| `SSH_STATE_PKI` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:760`](../../crates/oo-cli/src/admin/remote.rs#L760) |
| `SSH_STATE_REMOVE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:554`](../../crates/oo-cli/src/admin/remote.rs#L554) |
| `SSH_STATE_WRITE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:200`](../../crates/oo-cli/src/admin/remote.rs#L200) |
| `SSH_STDERR` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:130`](../../crates/oo-cli/src/admin/remote.rs#L130) |
| `SSH_STDIN` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:117`](../../crates/oo-cli/src/admin/remote.rs#L117) |
| `SSH_STREAM_INCOMPLETE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:160`](../../crates/oo-cli/src/admin/remote.rs#L160) |
| `SSH_TARGET` | [installation-and-runtime](#installation-and-runtime) | [`main.rs:63`](../../deploy/lite/mysql-doctor/src/main.rs#L63) |
| `SSH_TARGET_REQUIRED` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:521`](../../crates/oo-cli/src/admin/remote.rs#L521) |
| `SSH_UNREACHABLE` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:232`](../../crates/oo-cli/src/admin/remote.rs#L232) |
| `SSH_WAIT` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:141`](../../crates/oo-cli/src/admin/remote.rs#L141) |
| `STACK_BUDGET` | [model](#model) | [`stack.rs:192`](../../crates/oo-stack/src/stack.rs#L192) |
| `STACK_CONTEXT_WINDOW` | [configuration-and-source](#configuration-and-source) | [`stack.rs:419`](../../crates/oo-stack/src/stack.rs#L419) |
| `STACK_DEPLOY` | [configuration-and-source](#configuration-and-source) | [`stack.rs:424`](../../crates/oo-stack/src/stack.rs#L424) |
| `STACK_DEPLOY_SSH` | [installation-and-runtime](#installation-and-runtime) | [`remote.rs:311`](../../crates/oo-cli/src/admin/remote.rs#L311) |
| `STACK_GATEWAY` | [configuration-and-source](#configuration-and-source) | [`stack.rs:444`](../../crates/oo-stack/src/stack.rs#L444) |
| `STACK_INLINE_SECRET` | [credentials](#credentials) | [`stack.rs:349`](../../crates/oo-stack/src/stack.rs#L349) |
| `STACK_INTERVAL` | [configuration-and-source](#configuration-and-source) | [`stack.rs:378`](../../crates/oo-stack/src/stack.rs#L378) |
| `STACK_MODEL` | [model](#model) | [`stack.rs:414`](../../crates/oo-stack/src/stack.rs#L414) |
| `STACK_SCHEMA` | [configuration-and-source](#configuration-and-source) | [`stack.rs:210`](../../crates/oo-stack/src/stack.rs#L210) |
| `STACK_SCOPE` | [configuration-and-source](#configuration-and-source) | [`stack.rs:388`](../../crates/oo-stack/src/stack.rs#L388) |
| `STACK_SOURCE` | [configuration-and-source](#configuration-and-source) | [`stack.rs:373`](../../crates/oo-stack/src/stack.rs#L373) |
| `STACK_SOURCE_PORT` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:350`](../../crates/oo-cli/src/admin/wizard.rs#L350) |
| `STACK_SOURCE_SSH` | [installation-and-runtime](#installation-and-runtime) | [`wizard.rs:255`](../../crates/oo-cli/src/admin/wizard.rs#L255) |
| `STACK_SSH` | [installation-and-runtime](#installation-and-runtime) | [`stack.rs:402`](../../crates/oo-stack/src/stack.rs#L402) |
| `STAGE_COMMAND_FAILED` | [configuration-and-source](#configuration-and-source) | [`draft.py:154`](../../deploy/lite/draft.py#L154) |
| `STALE_DATA` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3617`](../../crates/oo-gateway/src/public_surface.rs#L3617) |
| `STATEMENT_NOT_ALLOWED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:396`](../../crates/oo-cli/src/outcome.rs#L396) |
| `STATE_DIRECTORY` | [configuration-and-source](#configuration-and-source) | [`draft.py:108`](../../deploy/lite/draft.py#L108) |
| `STATE_LOCK` | [configuration-and-source](#configuration-and-source) | [`draft.py:112`](../../deploy/lite/draft.py#L112) |
| `STREAM_INCOMPLETE` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:366`](../../tools/model-egress/model_egress.py#L366) |
| `STREAM_USAGE_MISSING` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:314`](../../tools/model-egress/model_egress.py#L314) |
| `TABLE_PAUSED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:403`](../../crates/oo-cli/src/outcome.rs#L403) |
| `TEST_LOST_RESPONSE` | [configuration-and-source](#configuration-and-source) | [`build_token_tests.rs:111`](../../crates/oo-cli/src/admin/build_token_tests.rs#L111) |
| `TEST_UNREACHABLE` | [configuration-and-source](#configuration-and-source) | [`build_token_tests.rs:159`](../../crates/oo-cli/src/admin/build_token_tests.rs#L159) |
| `TIMEZONE_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:51`](../../packages/cli/bin/oo.mjs#L51) |
| `TIMEZONE_REQUIRED` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:34`](../../packages/cli/bin/oo.mjs#L34) |
| `TIME_ARGUMENTS` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:395`](../../packages/cli/bin/oo.mjs#L395) |
| `TIME_INVALID` | [configuration-and-source](#configuration-and-source) | [`oo.mjs:37`](../../packages/cli/bin/oo.mjs#L37) |
| `TOKEN_FILE` | [credentials](#credentials) | [`w73.rs:2113`](../../tools/release-coordinator/src/w73.rs#L2113) |
| `TOKEN_INPUT_INVALID` | [credentials](#credentials) | [`oo.mjs:193`](../../packages/cli/bin/oo.mjs#L193) |
| `TOKEN_LIFECYCLE` | [credentials](#credentials) | [`auth_lifecycle.rs:136`](../../crates/oo-gateway/src/auth_lifecycle.rs#L136) |
| `TUI_CANCELLED` | [query-budget](#query-budget) | [`wizard.rs:82`](../../crates/oo-cli/src/admin/wizard.rs#L82) |
| `TUI_CA_FILE_REQUIRED` | [connectivity](#connectivity) | [`wizard.rs:365`](../../crates/oo-cli/src/admin/wizard.rs#L365) |
| `TUI_CA_PEM_REQUIRED` | [connectivity](#connectivity) | [`wizard.rs:231`](../../crates/oo-cli/src/admin/wizard.rs#L231) |
| `TUI_CONFIG` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:304`](../../crates/oo-cli/src/admin/wizard.rs#L304) |
| `TUI_CONFIG_EXISTS_OR_IO` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:312`](../../crates/oo-cli/src/admin/wizard.rs#L312) |
| `TUI_CONFIG_IO` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:315`](../../crates/oo-cli/src/admin/wizard.rs#L315) |
| `TUI_IMPORT_FILE` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:218`](../../crates/oo-cli/src/admin/wizard.rs#L218) |
| `TUI_IMPORT_UTF8` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:234`](../../crates/oo-cli/src/admin/wizard.rs#L234) |
| `TUI_INPUT` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:76`](../../crates/oo-cli/src/admin/wizard.rs#L76) |
| `TUI_PREFLIGHT_BLOCKED` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:321`](../../crates/oo-cli/src/admin/wizard.rs#L321) |
| `TUI_PREFLIGHT_PROTOCOL` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:427`](../../crates/oo-cli/src/admin/wizard.rs#L427) |
| `TUI_RENDER` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:75`](../../crates/oo-cli/src/admin/wizard.rs#L75) |
| `TUI_TERMINAL` | [configuration-and-source](#configuration-and-source) | [`wizard.rs:26`](../../crates/oo-cli/src/admin/wizard.rs#L26) |
| `UNEXPLAINED_CODES` | [configuration-and-source](#configuration-and-source) | [`public_surface.rs:3614`](../../crates/oo-gateway/src/public_surface.rs#L3614) |
| `UNQUALIFIED_RELATION` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:400`](../../crates/oo-cli/src/outcome.rs#L400) |
| `UNSUPPORTED_MESSAGE` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:135`](../../tools/model-egress/model_egress.py#L135) |
| `UNSUPPORTED_PARAMETER` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:128`](../../tools/model-egress/model_egress.py#L128) |
| `UNSUPPORTED_PLATFORM` | [installation-and-runtime](#installation-and-runtime) | [`oo.mjs:78`](../../packages/cli/bin/oo.mjs#L78) |
| `UNSUPPORTED_RESPONSE` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:163`](../../tools/model-egress/model_egress.py#L163) |
| `UPSTREAM_AUTH` | [model](#model) | [`model_egress.py:108`](../../tools/model-egress/model_egress.py#L108) |
| `UPSTREAM_ERROR` | [model](#model) | [`model_egress.py:109`](../../tools/model-egress/model_egress.py#L109) |
| `UPSTREAM_PROTOCOL` | [model](#model) | [`model_egress.py:293`](../../tools/model-egress/model_egress.py#L293) |
| `UPSTREAM_REDIRECT` | [model](#model) | [`model_egress.py:254`](../../tools/model-egress/model_egress.py#L254) |
| `UPSTREAM_TIMEOUT` | [model](#model) | [`model_egress.py:378`](../../tools/model-egress/model_egress.py#L378) |
| `USAGE_LIMIT` | [configuration-and-source](#configuration-and-source) | [`model_egress.py:102`](../../tools/model-egress/model_egress.py#L102) |
| `VERSION_UNSUPPORTED` | [configuration-and-source](#configuration-and-source) | [`outcome.rs:412`](../../crates/oo-cli/src/outcome.rs#L412) |
| `W73_RAW` | [configuration-and-source](#configuration-and-source) | [`w73.rs:509`](../../tools/release-coordinator/src/w73.rs#L509) |
| `access_denied` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:97`](../../crates/oo-gateway/src/auth_device.rs#L97) |
| `budget_unmeasured` | [model](#model) | [`library_selfbuild.py:593`](../../tools/pg-builder/scripts/library_selfbuild.py#L593) |
| `expired_token` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:230`](../../crates/oo-gateway/src/auth_device.rs#L230) |
| `invalid_grant` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:215`](../../crates/oo-gateway/src/auth_device.rs#L215) |
| `invalid_request` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:94`](../../crates/oo-gateway/src/auth_device.rs#L94) |
| `publication_unverified` | [review-and-publication](#review-and-publication) | [`build_retained.rs:189`](../../crates/oo-cli/src/admin/build_retained.rs#L189) |
| `review_required` | [review-and-publication](#review-and-publication) | [`build.rs:287`](../../crates/oo-cli/src/admin/build.rs#L287) |
| `server_error` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:107`](../../crates/oo-gateway/src/auth_device.rs#L107) |
| `sink_bytes` | [configuration-and-source](#configuration-and-source) | [`lib.rs:62`](../../tools/oo-load/src/lib.rs#L62) |
| `sink_rows` | [configuration-and-source](#configuration-and-source) | [`lib.rs:49`](../../tools/oo-load/src/lib.rs#L49) |
| `temporarily_unavailable` | [configuration-and-source](#configuration-and-source) | [`auth_device.rs:132`](../../crates/oo-gateway/src/auth_device.rs#L132) |
| `tls_ca_read` | [configuration-and-source](#configuration-and-source) | [`main.rs:537`](../../tools/oo-load/src/main.rs#L537) |
| `tls_ca_required` | [configuration-and-source](#configuration-and-source) | [`main.rs:533`](../../tools/oo-load/src/main.rs#L533) |
