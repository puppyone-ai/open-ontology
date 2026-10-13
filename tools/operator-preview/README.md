# operator-preview

Trusted local operator preview for one already-sealed PostgreSQL target.

The binary reads a private operator file. That file is authority only when `authority.kind` is `trusted-operator-file` and `authority.binds` is `import+mapping+role-policy+pack+target-tls`. A mode-0600 file without that binding is refused. The binds string is not sufficient. The policy table must be an admitted dataset in the sealed pack, and its columns must equal the role-pack columns for that dataset. A missing, ambiguous, or stale physical mapping is the same pack refusal, before SQL and before the audit spool. Policy is read from the path the operator file names. A query document cannot carry `policies` or `row_filter`. Pack text is not policy.

This process does not listen for HTTP, mint a role, or launch Pi. `untrusted_admission` stays the gateway constant `blocked`. The public untrusted query gate, OAuth, arbitrary role creation, and formal publication are pending.

```text
operator-preview describe --authority <file>
operator-preview query --authority <file> --query <file>
operator-preview cancel --authority <file> --execution <id>
```

There is no password flag. The operator file names a password file and a CA file. A trailing newline in the password file is not part of the password.

`describe` prints the bound dataset and its role-pack columns. Other datasets in the sealed pack stay pack admission, not the knowledge of this query. It does not open PostgreSQL.

`query` checks the pack receipt, import identity, role pack, and policy column equality before TLS and before the audit spool. It then loads the stored physical mapping. A mapping miss is a pack refusal and does not open the audit spool. A bound statement runs through `DedicatedSession::execute_trusted_select`. Rows are exact `null`, `bool`, `int`, `text`, and `decimal` cells when the sealed columns have those types. A guard refusal is recorded with `DiskWal::record_not_admitted` and is not printed as row bytes. Each execution id is a fresh UUID.

`cancel` writes a request only while that execution is still `running`. The query process owns `CancelHandle`. A cancel for a finished id does not write a request file.

Exit 0 is success, a described preview, or a confirmed cancel. Exit 1 is I/O, 2 usage, 3 request, 4 TLS, 5 policy, 6 guard, 7 audit, 8 query, 9 pack or import mismatch, 10 cancel not sent. Stderr is `operator-preview: <token>` and does not include SQL or a password.

`examples/` shows the file shape. Those files are not a live authority and contain no credentials.

The tool is its own Cargo workspace. Build it from this directory with the pinned toolchain. Do not add it to the repository root workspace.
