# Description candidate CLI

Contract `oo.builder.description-candidate-cli/v1`.

Binary `description-candidate`. Cargo discovers it from `crates/oo-builder/src/bin/description-candidate.rs`. This unit does not edit a manifest, a lock, or the description-candidate library. The library contract remains `contracts/builder/description-candidate-v1.md`.

The process reads one description-request command from stdin and calls one public function. The operator names the phase. The phase is not inferred from `captured_response`.

| Phase argument | Public function |
|---|---|
| `prepare_request` | `prepare_description_request` |
| `complete_candidates` | `complete_description_candidates` |

`prepare_description_request` and `complete_description_candidates` are function names, not phase arguments. Any other argument is an unknown phase.

This command does not call a model, open a network connection, read credentials, use PostgreSQL, write curated or effective text, grant, certify, authenticate, or publish. A successful exit is not approval. This is not full S5.

## Stdin

The document is one `oo.builder.description-request-command/v1` object, the shape frozen by the library contract. This CLI does not add a field and does not accept a second document.

The process reads at most 1048577 bytes. It does not read a longer stream. A buffer longer than 1048576 bytes fails before UTF-8 validation and before the public function. The code is `oo.builder.description-request-command/v1/json.budget.bytes`. The message is `document exceeds the byte budget`. The bytes are not copied to stderr or stdout.

Within that budget, a read error is `oo.builder.description-request-command/v1/io.read` with `stdin could not be read`. A non-UTF-8 buffer is `oo.builder.description-request-command/v1/json.syntax` with `stdin is not UTF-8`.

The selected public function then applies the library scanner and schema. This CLI does not reimplement duplicate keys, depth, node count, trailing data, seal, selection, budget, or captured-response checks.

`prepare_request` passes the text to `prepare_description_request`. A `captured_response` field is that function's `field.unknown`.

`complete_candidates` passes the text to `complete_description_candidates`. A missing `captured_response` is that function's `field.missing`.

## Stdout

Exit 0 writes the function's returned string and nothing else. The process does not add a newline, a wrapper, or pretty-printing. The string is already the library's canonical document:

- `oo.builder.description-request/v1` when prepare builds a request, or
- `oo.builder.description-candidate/v1` when the library returns an outcome.

`budget_failure`, `truncated`, `incomplete`, `provider_failure`, and `format_rejected` are outcome documents. They exit 0. `candidates` stays the library array, empty when the library keeps no candidate. `structural_draft` stays the library object. The captured draft artifact is not rewritten.

## Stderr

Exit 1 writes nothing to stdout. Stderr is one line:

```text
description-candidate: <code> <message>
```

`<code>` and `<message>` are the public `DescriptionCandidateError` fields. The error path is not printed. Stdin bytes, catalog names, comment text, response text, and the rejected argument are not printed.

A stdout write failure uses `DescriptionCandidateError::command` with name `io.write`, path `/`, and message `stdout could not be written`. The code is `oo.builder.description-request-command/v1/io.write`. The exit is 1.

## Usage

Exit 2 writes nothing to stdout.

| Condition | Stderr |
|---|---|
| no argument | the usage text below |
| one argument other than the two phases | `description-candidate: unknown phase` |
| a phase and any further argument | `description-candidate: the phase takes no arguments` |

Usage lines:

```text
description-candidate: usage: description-candidate <phase>
description-candidate:   prepare_request        one oo.builder.description-request-command/v1 document on stdin, without captured_response
description-candidate:   complete_candidates    the same document plus captured_response
description-candidate: the phase is required and is not inferred
description-candidate: stdout is the public compiler document and has no added newline
description-candidate: this command does not call a model, open a network connection, read credentials, use PostgreSQL, write curated or effective text, grant, certify, authenticate, or publish
```

## Out of scope

Model transport, residency approval, token cost, column-level candidates, samples, comment bodies, flags other than the phase, manifest edits, and lock edits are outside this unit. A new expansion needs its own frozen contract before code.

This unit does not claim parent acceptance, publication, or residency.
