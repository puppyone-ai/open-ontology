# Role Pack bundle

Contract `oo.builder.role-pack-bundle/v1`.

`role-pack-bundle` reads one `oo.builder.role-pack-command/v1` file and writes one new file. The file is a local bundle. The command calls `compile_role_pack_command` and stores that function's artifact bytes and manifest bytes. It does not build a second pack, does not recompute `pack_digest`, and does not read the request's visibility object for identities.

This command does not finish SP07-W06. It does not authenticate, grant, query PostgreSQL, call a model, mount a filesystem, run a tokenizer, trust a cache, or publish. A role name in the bundle is the compiler's manifest field. It is not a live permission proof. The visibility object remains caller-supplied public data, as in `oo.builder.role-pack-command/v1`.

## Command line

```text
role-pack-bundle --request <file> --output <file>
role-pack-bundle --help
role-pack-bundle -h
```

`--request` and `--output` are each required once. The value is the next argument. `--request=file` is not accepted. `--help` and `-h` are accepted only alone. Any other argument, a missing value, or a repeated flag is usage.

| Exit | When |
|---|---|
| 0 | `--help` or `-h` printed the usage text on stdout, or the bundle file was renamed into place |
| 1 | The request, the compiler, the bundle check, or the local write failed |
| 2 | The arguments are not the usage above |

Usage errors write one static line on stderr and write nothing on stdout. The line is `role-pack-bundle: oo.builder.role-pack-bundle/v1/usage arguments are --request, --output, or --help`. The rejected argument is not copied. Help writes the usage text on stdout and does not read a request or create a file. Success writes no stdout and no stderr. The bundle is the output file.

## Request

The request path must already be a regular file. A missing path, a directory, a symlink, or any other file type is refused before the body is parsed. A symlink is not opened and is not followed. The command reads at most 1048577 bytes and then stops. A buffer longer than 1048576 bytes is `oo.builder.role-pack-bundle/v1/json.budget.bytes` with `document exceeds the byte budget`. Bytes that are not UTF-8 are `oo.builder.role-pack-bundle/v1/json.syntax` with `the request file is not UTF-8`. The body is not copied into the diagnostic.

The UTF-8 text is passed to `compile_role_pack_command`. That function's code and static message are the failure when it rejects the document. This command does not add a second closure, a second hash of the pack files, or a rewritten status.

## Bundle

The output is one canonical JSON object, sorted keys, no trailing newline. `pack` is the exact artifact JSON object from the command result, spliced as bytes. `manifest` is the exact `manifest.json` object from that same result, spliced as bytes. They are not re-serialized and they are not JSON strings.

| Field | Source |
|---|---|
| `protocol` | `oo.builder.role-pack-bundle/v1` |
| `source_protocol` | the command result `protocol`, which must be `oo.builder.role-pack-command/v1` |
| `pack_protocol` | `pack.protocol`, which must be `oo.builder.role-pack/v1` |
| `compiler` | `pack.compiler`, which must equal `manifest.compiler` and `oo-builder/role-pack/v1` |
| `pack` | exact artifact bytes |
| `pack_length` | byte length of those artifact bytes |
| `pack_sha256` | lowercase hex from `oo_core::canonical::hash_bytes` of those artifact bytes |
| `pack_digest` | the command result `pack_digest`, which must equal `pack.pack_digest` |
| `manifest` | exact manifest bytes |
| `manifest_length` | byte length of those manifest bytes |
| `manifest_sha256` | lowercase hex from `oo_core::canonical::hash_bytes` of those manifest bytes, which must equal the command result `manifest_sha256` and `pack.manifest_sha256` |
| `snapshot` | identities copied from the manifest object |
| `statuses` | the fixed manifest statuses copied unchanged |

`snapshot` is one object:

| Field | Manifest field |
|---|---|
| `project` | `project` |
| `role` | `role` |
| `ontology_version` | `ontology_version` |
| `catalog_version` | `catalog_version` |
| `policy_id` | `policy.id` |
| `policy_version` | `policy.version` |
| `knowledge_id` | `knowledge.id` |
| `knowledge_version` | `knowledge.version` |

`statuses` copies these manifest values and no others:

| Field | Required value |
|---|---|
| `authentication` | `not_claimed` |
| `authorization` | `not_claimed` |
| `publication` | `not_claimed` |
| `mount` | `not_performed` |
| `samples` | `omitted` |
| `domains` | `unsupported` |
| `file_splitting` | `unsupported` |
| `search` | `unsupported` |
| `row_security_reproven` | false |
| `tokens_status` | `tokens.status`, which must be `unsupported` |
| `tokens_estimator` | `tokens.estimator`, which must be `none` |

The numeric token budgets stay inside the spliced manifest. They are not copied into `statuses` and they are not enforced here. `pack_length` is the artifact JSON length. It is not `total_bytes`.

The bundle does not contain `cache_key`, `cache_key_sha256`, or `principal`. A cache key is not a trust decision. The command does not infer authorization, does not claim a tokenizer ran, does not claim a mount, and does not add a public authentication claim. A status other than the table above is `oo.builder.role-pack-bundle/v1/bundle.status` with `compiled manifest status was not left unchanged`. A missing identity, a protocol mismatch, or a hash that does not match the spliced bytes is `oo.builder.role-pack-bundle/v1/bundle.identity` with `compiled output is missing an identity`. The bundle text must fit in 8388608 bytes. A larger text is `oo.builder.role-pack-bundle/v1/bundle.budget` with `bundle exceeds the byte budget`, and no output file is created.

## Local file

The output parent must already exist and must be a directory. The command does not create the parent. A symlink parent is refused and is not followed. The output path and the staging path must not exist. Existence is `symlink_metadata`, so a symlink counts as present and is not followed.

The output path must be a file path: it has a UTF-8 file name, it does not end in a separator, and the file name is not empty, `.`, or `..`. The staging file is in the same parent directory. Its name is the output file name plus `.role-pack-bundle-staging`. That name must be at most 255 bytes. A name that does not fit is `oo.builder.role-pack-bundle/v1/io.name` with `the output file name does not fit the staging suffix`.

Publish order:

1. Refuse when the staging path already exists. That path is not deleted. The code is `oo.builder.role-pack-bundle/v1/io.collision` with `the staging path already exists`.
2. Create the staging file with `create_new`. Write the bundle, flush, and `sync_all`.
3. Read the staging file back. A short, long, or different body is not renamed.
4. Refuse when the output path exists. The code is `oo.builder.role-pack-bundle/v1/io.destination` with `the output path already exists`.
5. `rename` the staging file onto the output path.

`rename` is the publish step. It is atomic only for a file in the same directory on a filesystem that provides atomic rename. The command does not fsync the parent directory. A crash before rename can leave the staging file. A crash after rename can still lose the directory entry until the parent is durable. This is not a publication barrier and not a power-fail guarantee.

On a failure after this attempt has created the staging file, the command removes that file with `remove_file` and removes nothing else. It does not remove the output path, the parent, a pre-existing staging path, or a neighbor. `remove_file` on a directory fails and does not become `remove_dir_all`. If that removal fails, the diagnostic is `oo.builder.role-pack-bundle/v1/io.cleanup` with `the staging file could not be removed`.

A request, compiler, or bundle-budget failure happens before step 1. Those failures do not create a staging file.

The parent directory is a trusted path supplied by the caller. It is not a jail, a sandbox, or a confinement boundary. The command does not canonicalize, does not chroot, and does not rewrite `..`. The file mode follows the process umask. A destination that appears in the gap before `rename` can be replaced by `rename` on this platform. The parent is trusted not to race this command. The check is not a lock.

## Diagnostics

A failure writes one line on stderr and does not write a success file. The line is `role-pack-bundle: <code> <message>`. The message is a fixed sentence. The request body, a path, and an argument are not copied.

| Code | Message |
|---|---|
| `oo.builder.role-pack-bundle/v1/usage` | `arguments are --request, --output, or --help` |
| `oo.builder.role-pack-bundle/v1/io.missing` | `the request file is missing` |
| `oo.builder.role-pack-bundle/v1/io.read` | `the request file could not be read` |
| `oo.builder.role-pack-bundle/v1/io.kind` | `the request path is not a regular file` |
| `oo.builder.role-pack-bundle/v1/json.syntax` | `the request file is not UTF-8` |
| `oo.builder.role-pack-bundle/v1/json.budget.bytes` | `document exceeds the byte budget` |
| `oo.builder.role-pack-bundle/v1/io.parent` | `the output parent is not a directory` |
| `oo.builder.role-pack-bundle/v1/io.name` | `the output file name does not fit the staging suffix` |
| `oo.builder.role-pack-bundle/v1/io.collision` | `the staging path already exists` |
| `oo.builder.role-pack-bundle/v1/io.staging` | `the staging file could not be created` |
| `oo.builder.role-pack-bundle/v1/io.write` | `the staging file could not be written` |
| `oo.builder.role-pack-bundle/v1/io.sync` | `the staging file could not be synced` |
| `oo.builder.role-pack-bundle/v1/io.readback` | `the staging file could not be read` or `the staging file did not match the bundle` |
| `oo.builder.role-pack-bundle/v1/io.destination` | `the output path already exists` |
| `oo.builder.role-pack-bundle/v1/io.rename` | `the staging file could not be renamed` |
| `oo.builder.role-pack-bundle/v1/io.cleanup` | `the staging file could not be removed` |
| `oo.builder.role-pack-bundle/v1/bundle.identity` | `compiled output is missing an identity` |
| `oo.builder.role-pack-bundle/v1/bundle.status` | `compiled manifest status was not left unchanged` |
| `oo.builder.role-pack-bundle/v1/bundle.budget` | `bundle exceeds the byte budget` |

Compiler failures keep the code and static message from `compile_role_pack_command`. An inspect error on the output path uses `io.destination` and `the output path could not be inspected`. An inspect error on the staging path uses `io.collision` and `the staging path could not be inspected`.

## Outside this slice

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, cache trust, a database re-proof of row security, and publication gates G1–G13 stay open. A written bundle is not a grant, not a mount, and not SQL execution.
