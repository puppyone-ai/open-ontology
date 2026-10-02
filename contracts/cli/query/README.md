# CLI query contract

This is the `oo query` slice. The binary calls `oo-sdk` over HTTP. It does not open a database, cache rows, or search the process environment for a credential. Markdown is a local rendering of the typed JSON response. The wire `format` is always `json`. This slice does not send `client_context`.

The command spelling is `oo query`. Other commands print that they are not implemented and exit 2. This slice does not use a parser framework.

## Invocation

```
oo query [options] "<sql>"
oo query [options] -f <file.sql>
```

`--` ends flags, so a SQL argument may start with `-`. `--help` and `-h` are help, not SQL, unless they follow `--`.

Required for a network call:

- `--auth external`
- `--token-file <file>` or `--token-stdin`
- `--gateway <origin>`
- `--transport production` or `--transport local-development`

Optional:

- `--json` writes one JSON document and a final newline on stdout
- `--max-rows` is `1..=2000`
- `--timeout-ms` is `1..=120000` and is sent as the server hint. It can only shorten the client connect, request, read, and overall waits. It cannot raise them above the SDK production values (5s connect, 30s request, read, and overall). A server hint above 30s can still be cut by the client wait.
- `--ca-file <pem>` replaces platform roots for this process. Hostname checks stay on.

`--token-file` and `--token-stdin` are mutually exclusive. A positional SQL argument and `-f` are mutually exclusive. `-f -` is a usage error and does not read stdin. Flag values do not use `=`. A value that starts with `-` is a missing value. Duplicate flags are errors. Unknown flags, extra arguments, empty SQL, and invalid numbers fail before any socket.

Help and usage failures are deterministic and do not open a token file.

## Authentication

Mode comes only from `--auth`. The working directory is not consulted.

`sandbox` is a different domain from `external`. `oo_sdk::from_sandbox_env` is not implemented. `--auth sandbox` exits 1 before this process opens a token file, reads stdin, reads `OO_TOKEN`, or touches a home directory. The CLI does not fall through to external mode, invent a bearer token, send a management credential, or speak a proxy protocol itself. A sandbox proxy client is a later change.

`external` reads exactly one source:

- `--token-file`, or
- `--token-stdin`

There is no `OO_TOKEN` variable, home path, keychain, config file, or environment search. A missing source exits 4 and does not connect. The CLI never prints a sample row for that failure.

The token is not accepted on the command line. `--token-file=...` is a usage error and the argument text is not copied into the error. Token bytes are not written to stdout, stderr, or a path diagnostic. An authorized response cell is written only to stdout.

### Token file limits

Unix only. Windows is not implemented.

The file is opened with `O_NOFOLLOW`, then `fstat` on that same descriptor. The check requires a regular file, mode exactly `0600` (`mode & 07777`), and the same owner as a probe file this process creates. `getuid` is unsafe and this crate forbids unsafe code, so the probe is an empty regular file in the temp directory, opened with `O_NOFOLLOW` and `O_EXCL`, `fstat`ed, and unlinked before the token bytes are read. Its uid is the effective uid that created it. If that create fails, the token read fails. Size is at most 4097 bytes so one trailing newline can wrap a 4096-byte token. The token itself is non-empty, at most 4096 bytes, UTF-8, and each byte is in `0x21..=0x7e`. A second newline or a carriage return is rejected.

What this does not prove:

- `O_NOFOLLOW` applies to the final path component. A symlink in a parent directory is followed by the kernel.
- The descriptor check closes the race where the directory entry is replaced after the open. It does not stop the owner from writing the inode after the read.
- ACLs, SELinux labels, and immutable flags are not inspected.
- A hard link is the same inode and passes the same owner and mode check.
- The bytes live in process memory until the `String` is dropped. They are not wiped.
- A successful mode check is not a claim that the host has no other reader.
- The owner comparison is not a `getuid` call. It fails closed when the temp directory cannot create the probe file.

`--token-stdin` reads until EOF, with the same 4097-byte cap and the same token rules. SQL is never read from that stream.

SQL files use `O_NOFOLLOW` and a 65536-byte cap. They are not required to be mode `0600`. The bytes are exact UTF-8: no BOM strip and no newline translation. `--ca-file` uses `O_NOFOLLOW` and a 1 MiB cap. It is not a token.

## Request bytes

`oo-sdk` encodes the request with sorted JSON keys and omits null fields. Two stable bodies, with no `client_context`:

```json
{"format":"json","sql":"select 1"}
```

```json
{"format":"json","max_rows":10,"sql":"select 1","timeout_ms":1000}
```

The client appends `/v1/query`. `production` is HTTPS only. `local-development` allows HTTP only for loopback. TLS verification is never disabled. Redirects are refused. The HTTP client does not use an environment proxy.

## Success output

Exit 0 includes a response whose `truncated` field is true. Truncation is reported. The CLI does not invent `estimated_total_rows`, an RLS count, or a statement that the result is complete.

`--json` re-serializes the typed `QueryResponse`: sorted keys, null fields removed, one trailing newline, nothing else on stdout or stderr. Decimal strings, large integers, and timestamps stay text or integer tokens. They are not passed through `f64`. Empty arrays that the type always carries (`hints`, `truncation_reasons`) remain present. Absent optional fields stay absent.

Markdown writes a pipe table and then a fixed footer. Every footer line is produced by the CLI and starts with `-- `. Cell text, names, warnings, hints, and version labels are escaped so a value cannot become a new row, a new footer line, or a terminal control sequence. The escapes are `\\`, `\|`, `` \` ``, `\n`, `\r`, `\t`, and `\u{hex}` for C0, C1, DEL, and the bidi and isolate characters listed in the renderer. A warning is never dropped to make the table narrower. `estimated_total_rows` is not copied into the footer.

The footer always has query id, returned rows, duration in integer milliseconds, truncated, truncation reasons, freshness, sources, warnings, hints, knowledge version, policy version, and star expansion.

Rendering is capped at 16 MiB. The HTTP body cap is 1 MiB, so the render cap is a backstop. A broken pipe or any other stdout failure exits 1 with `error: output delivery failed` on stderr. The query is not retried and the CLI does not claim the output was delivered.

## Errors

`--json` errors are one JSON object on stdout and an empty stderr. The keys are `class`, optional `code`, `exit`, optional `hint`, `message`, optional `query_id`, `retry`, and `retryable`. `retry` is `attempt_count` (`"unknown"`), `max_extra_attempts`, `max_wait_ms`, `nested_cli_retries` (`0`), and `outcome`. The attempt count is unknown because the SDK does not report how many times it tried.

Text errors go to stderr. Success leaves stderr empty, including when the SDK performed an admission retry, because a success line would have to invent that count.

| Exit | When |
| --- | --- |
| 0 | Help, or a validated response, including `truncated: true` |
| 2 | Usage, an unknown command, or a non-retryable Gateway refusal such as SQL parse, cost, or `TIMEOUT` |
| 3 | `RATE_LIMITED` or `OVERLOADED` after the SDK admission policy finishes, including when that policy declines to wait |
| 4 | Missing or rejected external credentials, HTTP 401, or transport failure |
| 1 | Sandbox mode, local IO, protocol rejection, `INTERNAL`, `AUDIT_UNAVAILABLE`, or any other class |

Usage and a non-retryable Gateway refusal share exit 2. The JSON `class` field distinguishes them.

The SDK may make at most 3 extra attempts, and only for a complete retryable `RATE_LIMITED` or `OVERLOADED` whose HTTP status matches the code. The CLI does not loop again. `Retry-After` of 0 waits zero. A delay greater than or equal to the 2 second policy cap is not retried. Authentication, transport timeout, a partial body, and `TIMEOUT` are not replayed. A 429 or 503 body that fails the JSON door is protocol failure, exit 1, and is not treated as admission retry. `INTERNAL` and `AUDIT_UNAVAILABLE` are retryable in the core code and are still not replayed by the SDK; they exit 1.

Gateway message and hint text that still contains the token, the SQL, the gateway URL, or `Bearer ` is replaced before display. If the rendered error itself contains those secrets, the process prints `error text omitted` and exits 1.

## Not in this slice

Conversation, export, Arrow, discovery, describe, config validate, SQL validate, management calls, a sandbox proxy, a real Gateway, and PostgreSQL permission checks are not implemented. Full arm64 and amd64 matrix coverage is not this contract.
