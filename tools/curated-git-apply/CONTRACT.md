# Curated Git apply

Contract `oo.curated-git-apply/v1`. Compiler id `oo-curated-git-apply/v1`.

E57-D1/1 is frozen before code. One explicit operator snapshot plus one re-executed description review becomes at most one curated Git commit and one typed receipt. This slice does not authenticate, grant, publish, or replace generated ontology text by itself. A later authenticated B04 service may call this adapter only after a real identity check. Until that parent exists, every receipt says `authenticated: false` and `parent_auth: "required"`.

The typed compiler is `description_review::apply_description_review` from the reviewed source pin `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba`. Tree ids are in `PINNED`. The adapter does not trust a review document supplied beside the original command. It does not invent classification, PII, semantic type, owner, confidence, or grant writes. The pinned description compiler applies only dataset `label`, `description`, and `grain`, and, on a v2 column review, column `label` and `description`.

Normative documents:

- `docs/06-ontology-layer.md` §3.1. Curated datasets live at `ontology/curated/datasets/<schema>/<table>.yaml`. Builder output stays under `ontology/generated/`. This adapter writes only the curated dataset file for a bound table.
- `docs/12-configuration-reference.md` §1. `roles.yaml`, `oo.yaml`, `sources/`, `hooks/`, and `agent.yaml` are configuration. This adapter does not write them.
- `specs/open-ontology-v0.1-spec.md` REQ-CTL-01, REQ-CTL-02, and REQ-CTL-03. Declarative edits are Git commits. `validate`, `plan`, and `apply` stay distinct. This slice is the curated apply step for an already reviewed description, not a control-API identity proof. REQ-CTL-03 authentication is parent work.

## Operator registry

`OperatorRegistry` is trusted process state. It is not parsed out of the request.

| Field | Rule |
|---|---|
| `project_root` | Absolute path of one canonical checkout. The request `project_root` must resolve to the same directory |
| `git_binary` | Absolute path of the `git` executable the operator selected. The request cannot name it |

The approved ref is the constant `refs/heads/curated`. A request ref that is any other string is `ref.unregistered`. There is no request field for author name, author email, commit time, hook path, or config path.

The service author is `oo-curated-git-apply` `<curated-git-apply@open-ontology.invalid>` at the fixed instant `1791417600 +0000`. That identity is the product service. It is not a customer and not the declared reviewer. Reviewer labels stay on the receipt as declared text with `authenticated: false`.

## Request

The scanner rejects a document above 2097152 bytes, a duplicate key, a depth above 64, a node overrun, and trailing data. `serde` `deny_unknown_fields` rejects an unknown key. Messages are static and do not echo paths, hashes, reviewer text, or Git stderr.

| Field | Rule |
|---|---|
| `protocol` | `oo.curated-git-apply/v1` |
| `project_root` | Absolute path. Must be the registered checkout |
| `ref` | `refs/heads/curated` |
| `expected_head` | 40 lowercase hex characters. Parent of a new commit |
| `idempotency_key` | 1 to 128 characters from `A-Za-z0-9._:-` |
| `expected_schema_hash` | 64 lowercase hex characters. Server seal |
| `expected_evidence_hash` | 64 lowercase hex characters. Server seal |
| `review_command` | The original `oo.builder.description-review-command/v1` document, at most 1048576 bytes |
| `bindings` | 1 to 16 objects. The only datasets this call may write |

`review_command` is passed unchanged to `apply_description_review`. A compiler `Err` is `review.refused`. No ref moves.

### Binding

| Field | Rule |
|---|---|
| `dataset` | Core qualified table `{schema}.{table}` |
| `expected_blob` | 40 lowercase hex characters. Git blob of `ontology/curated/datasets/<schema>/<table>.yaml` at `expected_head` |
| `columns` | Column names. Omitted or empty allows no column write. At most 64 names |

A repeated dataset or a repeated column is `binding.duplicate`. A bound dataset that is not a curated key in the review command is `binding.unknown`. There is no field for a free path, a role file, a secret, or a hook.

## Review gate

The adapter reads the compiler's review document. `review_sha256` must match the canonical document with that field removed. Any decision or column decision whose `authenticated` value is not false is `review.identity`. The adapter does not add `review.state`, `owner`, `confidence`, or a grant.

`expected_schema_hash` must equal the review command's `current.schema_hash`. `expected_evidence_hash` must equal `current.evidence_hash`. The compiler already refuses when those current hashes disagree with the sealed structural draft inside the re-executed command. A server hash that disagrees with the command is `seal.schema` or `seal.evidence`.

| Outcome | Result |
|---|---|
| `not_candidates` | `review.declined`. No commit |
| Queue reason `schema`, `evidence`, `request`, `card`, `candidate`, `curated`, or `cas` | `review.stale`. No commit |
| Queue kind `core_rejected` | `review.core`. No commit |
| Bound column with queue reason `column_missing` | `review.column`. No commit |
| No dataset document changed | Success `unchanged`. No commit. The ref must still be `expected_head` |
| One or more documents changed | Continue to the blob gate |

`reject`, `defer`, and an approval that keeps every selected field are `unchanged` when no queue reason above fired. Caveats and open questions stay on the review record. They are not a file write.

## Blob gate

For every binding, the tree entry at `expected_head` for `ontology/curated/datasets/<schema>/<table>.yaml` must be mode `100644` and the blob id must equal `expected_blob`. A missing entry is `blob.missing`. A symlink, gitlink, or other mode is `blob.mode`. A different blob id is `blob.cas`.

The blob is parsed with `oo_core::yaml::parse_yaml_typed` as `DatasetFile`. A parse failure is `yaml.parse`. The typed value must equal the curated `DatasetFile` in the review command. A difference is `curated.drift`.

A changed dataset outside the bindings is `scope.dataset`. A changed column outside that binding's `columns` is `scope.column`. The typed diff may touch only dataset `label`, `description`, and `grain`, and column `label` and `description`. Any other difference, including classification, PII, semantic type, status, entity, metric, tier, keys, provenance, `review`, or column set, is `field.unapproved`.

The new file bytes are canonical JSON of the typed `DatasetFile`, plus a trailing newline. Those bytes are a YAML document only when `parse_yaml_typed` accepts them and the typed value is unchanged. Failure is `yaml.roundtrip` and no commit. Other files are not rewritten. The private index is loaded from `expected_head` with `read-tree`, then only the changed dataset paths are replaced.

## Git execution

Git is `std::process::Command` with a fixed argument vector. The adapter does not start a shell. The child environment is cleared. The child receives only:

- `PATH` set to the directory of the operator `git_binary`
- `HOME` and `TMPDIR` set to a new private directory
- `GIT_DIR` set to `<root>/.git`
- `GIT_INDEX_FILE` set to a private index outside the checkout
- `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_SYSTEM=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`
- `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0`, `GIT_NO_REPLACE_OBJECTS=1`, `GIT_ALLOW_PROTOCOL=` empty
- On `commit-tree` only, the service author name, email, and fixed date

Every invocation also passes `-c` overrides: `core.hooksPath` is an empty private directory, `core.fsmonitor=false`, `commit.gpgsign=false`, `tag.gpgsign=false`, `credential.helper=` empty, `protocol.allow=never`, `gc.auto=0`, `maintenance.auto=false`, `core.autocrlf=false`, and `safe.directory` is the registered root. Host Git config, credential helpers, SSH sockets, and private keys are not inherited.

The checkout must be a canonical directory. Any symlink component is `root.symlink`. `.git` must be a real directory, not a file and not a symlink. `.git/objects` must be a real directory. `.git/objects/info/alternates`, `.gitmodules`, a gitlink (`160000`), or a `[submodule` section in `.git/config` is `root.embedded`. The object format must be `sha1`.

Commands used, and no others: `rev-parse`, `cat-file`, `ls-tree`, `read-tree`, `hash-object --no-filters`, `update-index --cacheinfo`, `write-tree`, `commit-tree`, `update-ref`, `diff-files`, `ls-files`, `rev-list`. There is no `fetch`, `push`, `clone`, `submodule`, `checkout`, `reset`, `clean`, `gc`, or `prune`.

`read-tree` loads `expected_head` into the private index. It is not passed `-u`, `-m`, or `--reset`. `hash-object -w` and `commit-tree` write objects into the repository object store. `update-ref refs/heads/curated <new> <expected_head>` is the compare-and-swap. A mismatch does not overwrite the ref. Objects written by the loser stay unreachable. The adapter does not delete them.

The shared index and the worktree files are not written. A dirty worktree does not stop an object-level commit and is never reset or cleaned. Every success receipt sets `checkout` to `require_fresh_checkout` and `worktree` to `not_updated`.

The deadline is 30 seconds for the CLI, measured from process start. A child that is still running at the deadline is killed and waited for before the error returns. The request cannot raise the deadline.

## Idempotency

The commit is a pure function of the canonical request, the service author, and `expected_head`. A repeated call builds the same commit id.

| Observed ref | Action |
|---|---|
| Equal to the recomputed commit | Return the same receipt. Do not create another commit |
| Equal to `expected_head` | `update-ref` compare-and-swap |
| The recomputed commit is within the first 32 first-parent commits | Return the same receipt. Do not move the ref |
| Tip message carries the same idempotency key and a different request hash | `idempotency.conflict`. Do not move the ref |
| Anything else | `ref.cas`. Do not move the ref |

`unchanged` succeeds only when the ref is still `expected_head`. A lost reply is recovered by reading the ref, not by forcing it.

## Receipt

Success writes one canonical JSON object to stdout and no extra newline. Exit 0.

| Field | Value |
|---|---|
| `protocol` | `oo.curated-git-apply/v1` |
| `compiler` | `oo-curated-git-apply/v1` |
| `outcome` | `committed` or `unchanged` |
| `authenticated` | false |
| `parent_auth` | `required` |
| `identity` | `declared_only` |
| `ref` | `refs/heads/curated` |
| `old_commit`, `new_commit` | Parent and created commit. `new_commit` is null when unchanged |
| `old_tree`, `new_tree` | Trees. Equal when unchanged |
| `command_sha256` | SHA-256 of the exact `review_command` bytes |
| `request_sha256` | SHA-256 of the canonical request |
| `review_sha256`, `outcome_sha256` | From the re-executed review |
| `schema_hash`, `evidence_hash` | The server seals |
| `idempotency_key` | The request key |
| `author_name`, `author_email` | The service identity |
| `author_is_reviewer`, `author_is_customer` | false |
| `checkout` | `require_fresh_checkout` |
| `worktree` | `not_updated` |
| `worktree_dirty` | Whether the private inspect saw a worktree difference |
| `decisions` | Declared reviewer, decision, `authenticated: false`, `applied` |
| `blobs` | Path, dataset, old blob, new blob for each changed file |
| `statements` | The fixed object below |
| `receipt_sha256` | SHA-256 of this document with `receipt_sha256` removed |

`statements` is fixed:

| Field | Value |
|---|---|
| `authentication` | `not_claimed` |
| `classification` | `not_applied` |
| `grants` | `not_authorized` |
| `hooks` | `not_executed` |
| `index` | `private` |
| `network` | `not_used` |
| `parent_auth` | `required` |
| `publication` | `not_performed` |
| `roles` | `not_written` |
| `secrets` | `not_written` |
| `worktree` | `not_updated` |

The commit message records the protocol, idempotency key, request hash, review hash, outcome hash, schema hash, evidence hash, `authenticated false`, and `parent_auth required`. It does not copy reviewer prose into the author header.

## Errors

Exit 1 writes nothing to stdout. Stderr is one line:

```text
curated-git-apply: <code> <message>
```

Exit 2 writes nothing to stdout.

| Condition | Stderr |
|---|---|
| no argument, or any argument other than `apply --root <path>` | `curated-git-apply: usage: curated-git-apply apply --root <registered-checkout>` |
| `apply` without `--root`, or a further argument | `curated-git-apply: the phase takes --root and no other arguments` |

Usage lines:

```text
curated-git-apply: usage: curated-git-apply apply --root <registered-checkout>
curated-git-apply:   apply    one oo.curated-git-apply/v1 document on stdin
curated-git-apply: stdout is the receipt and has no added newline
curated-git-apply: this command does not authenticate, publish, grant, push, fetch, or run hooks
curated-git-apply: authenticated is false and parent authentication is required
```

| Code | Message |
|---|---|
| `schema.invalid` | the request is not the curated git apply command |
| `schema.unknown` | the request has an unknown field |
| `json.syntax` | the request is not JSON |
| `json.budget.bytes` | document exceeds the byte budget |
| `root.unregistered` | the project root is not the registered checkout |
| `root.unsafe` | the checkout is not a canonical git directory |
| `root.symlink` | the checkout path contains a symlink |
| `root.embedded` | the checkout contains another git directory or submodule |
| `root.escape` | a path escapes the checkout |
| `ref.unregistered` | the ref is not the approved branch |
| `ref.missing` | the approved ref does not resolve |
| `ref.cas` | the ref does not match the expected commit |
| `head.missing` | the expected commit does not exist |
| `head.type` | the expected object is not a commit |
| `blob.cas` | the curated blob does not match the expected blob |
| `blob.missing` | the curated dataset blob is not in the tree |
| `blob.mode` | the curated path is not a regular file |
| `binding.duplicate` | a binding dataset or column is repeated |
| `binding.unknown` | a bound dataset is not in the review command |
| `scope.dataset` | a change is outside the bound datasets |
| `scope.column` | a change is outside the bound columns |
| `seal.schema` | the schema hash does not match the sealed artifact |
| `seal.evidence` | the evidence hash does not match the sealed artifact |
| `curated.drift` | the git dataset does not match the review command |
| `field.unapproved` | a change is outside compiler-approved descriptive fields |
| `review.refused` | the description review compiler refused the command |
| `review.stale` | the review facts do not match the sealed artifact |
| `review.declined` | the compiler did not return candidates |
| `review.core` | the compiler rejected a descriptive value |
| `review.identity` | the review claims an authenticated reviewer |
| `review.hash` | the review document hash does not match |
| `review.column` | a bound column is not in the curated dataset |
| `yaml.parse` | the curated document is not a core dataset file |
| `yaml.roundtrip` | the written document does not parse back to the same dataset |
| `idempotency.conflict` | the idempotency key was used for a different request |
| `deadline.exceeded` | the git operation exceeded the deadline |
| `git.exit` | git returned a non-zero status |
| `git.spawn` | git could not be started |
| `limit.files` | the change set exceeds the file bound |
| `limit.bytes` | the document exceeds the byte bound |
| `object.format` | the repository object format is not sha1 |
| `io.read` | the request could not be read |
| `io.write` | the receipt could not be written |

## CLI

Binary `curated-git-apply`. The only phase is `apply --root <registered-checkout>`. It reads one request from stdin and writes the receipt to stdout.

## Out of scope

Public authentication, grants, publication, role edits, secret edits, hook execution, network Git, and writing `ontology/generated/` are parent work. This unit does not claim parent acceptance of SP02 or SP07.

## Pin

`PINNED` records `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba` and the tree ids of `crates/oo-core`, `crates/oo-builder`, `crates/oo-jobs`, `tools/description-review`, and `tools/description-pack`. The adapter links the first four through the existing path crates. `tools/description-pack` is the sibling pin and is not called. A moving tree fails the pin test.
