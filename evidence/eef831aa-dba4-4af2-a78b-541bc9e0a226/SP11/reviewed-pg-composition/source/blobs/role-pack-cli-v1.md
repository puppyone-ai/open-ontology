# Role Pack command

Contract `oo.builder.role-pack-command/v1`.

`oo-builder role-pack` reads one JSON document from stdin and writes one command result to stdout. No arguments. The command calls `compile_role_pack` from `oo.builder.role-pack/v1`. It does not reimplement visibility closure, SQL scanning, or pack hashing.

This command does not finish SP07-W06. It does not authenticate, grant, mount a filesystem, query PostgreSQL, call a model, or publish.

## Visibility is public data

`visibility` is caller-supplied public data. A future policy adapter would be the component that produces a snapshot. This command does not check the object against a live permission system. A role name, a principal, `review.state: approved`, and `classification.auto_accept` are not authentication and are not a grant. The command passes the object to `compile_role_pack` and leaves that compiler's manifest statuses unchanged:

| Field | Value |
|---|---|
| `authentication` | `not_claimed` |
| `authorization` | `not_claimed` |
| `publication` | `not_claimed` |
| `mount` | `not_performed` |
| `samples` | `omitted` |
| `row_security_reproven` | false |

The command does not add a claim that the caller was authenticated or that the pack was published.

## Input

The document is one object. The core scanner `oo_core::parse_json_value` runs first, with contract `oo.builder.role-pack-command/v1`. Duplicate keys, depth, node count, trailing data, and a document over 1048576 bytes fail with the scanner's code. The command replaces the scanner message with a static sentence and does not echo the body.

| Field | Shape |
|---|---|
| `protocol` | the string `oo.builder.role-pack-command/v1` |
| `effective_oos` | one object, the `oo.builder.effective-oos/v1` document |
| `visibility` | one object, the `oo.builder.role-visibility/v1` snapshot |

`serde` `deny_unknown_fields` rejects any other top-level field. `effective_oos` and `visibility` must be objects. A null, array, or string is `schema.invalid`. A protocol other than the command protocol is `protocol.mismatch`.

The command writes each object as key-sorted JSON and calls `compile_role_pack`. The compiler scans those two documents again. Unknown nested fields, a stale policy or knowledge version, samples, and the rest of the role-pack input rules are that compiler's results. This command does not add a second closure.

Stdin is read up to 1048577 bytes by the shared builder reader. A longer stream is not read further. The command scanner then rejects a document above 1048576 bytes. A read failure or a non-UTF-8 stream uses that shared reader: `oo.builder.catalog-evidence/v1/io.read` with `stdin could not be read`, or `oo.builder.catalog-evidence/v1/json.syntax` with `stdin is not UTF-8`.

## Result

Stdout is one JSON object with sorted keys and no trailing newline. Nothing is written until that object parses. `artifact` is the object whose bytes are `RolePack.artifact`. `manifest` is the object whose bytes are `RolePack::file("manifest.json")`. They are not escaped JSON strings.

| Key | Source |
|---|---|
| `protocol` | `oo.builder.role-pack-command/v1` |
| `artifact` | `RolePack.artifact` |
| `manifest` | `manifest.json` from that pack |
| `pack_digest` | `RolePack.pack_digest` |
| `manifest_sha256` | `RolePack.manifest_sha256` |
| `cache_key` | `RolePack.cache_key` |
| `cache_key_sha256` | `RolePack.cache_key_sha256` |

File contents are `artifact.files[].text`. Each text is the pack file at that path. `artifact.files[].sha256` and `artifact.files[].bytes` are the compiler's file record. The command does not compute a second digest and does not rewrite a status.

The artifact has no principal. The cache key does. Two principals with the same role snapshot share `artifact` and `pack_digest` and do not share `cache_key`. That is the compiler's rule. This command does not change it.

The command does not create a file, a directory, a mount, or a publication record.

## Exits

A failure writes one static diagnostic to stderr and writes nothing to stdout. The diagnostic is `oo-builder: role-pack: <code> <message>`. The message is a fixed sentence. The request body is not copied. An argument on the command line is not copied.

| Exit | Cause |
|---|---|
| 0 | The command result was written |
| 1 | Stdin could not be read, is not UTF-8, the command document or `compile_role_pack` rejected it, or stdout could not be written |
| 2 | The command is missing or unknown, or `role-pack` was given an argument |

`structure-draft`, `captured-pg-draft`, and `review-pg-draft` keep their existing stdin, stdout, and exits. With no arguments the process exits 2 and prints usage for the commands that exist, including `role-pack`. Usage states that the visibility object is caller-supplied public data and is not a live permission proof.

## Outside this slice

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, a database re-proof of row security, and publication gates G1–G13 stay open. A successful command is not SQL execution and not a grant.
