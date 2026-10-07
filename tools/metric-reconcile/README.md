# metric-reconcile

Compile one core-valid OOS metric and, on `evaluate`, compare it with a separate operator control statement. This is the SEM-D05 / E42 metric template and G5b component. It does not publish, authenticate an owner, or certify the other SP08 gates.

The contract is [CONTRACT.md](CONTRACT.md).

## Commands

```text
metric-reconcile compile
metric-reconcile evaluate --config CONFIG
```

The request is one JSON object on stdin, at most 2_097_152 bytes. `CONFIG` is a filesystem path. The password and the CA PEM are paths inside that file. They are not argv, not request fields, and not written to stdout, stderr, or the report.

```text
metric-reconcile compile < request.json
metric-reconcile evaluate --config /secrets/config.json < request.json
```

`config.json` names `password_file` and `ca_file`. The password file is mode `0600` or stricter, at most 4096 bytes, with no NUL. One trailing newline is removed. The CA file is at most 1_048_576 bytes.

## Exits

| condition | exit | stdout |
|---|---|---|
| malformed argv, stdin, JSON, or config | 1 | empty |
| `outcome` `refused` | 2 | one report |
| `outcome` `compiled` or `evaluated` | 0 | one report |

Stderr is `metric-reconcile: malformed <reason>` or `metric-reconcile: refused <reason>`. The reason is a static token. An evaluated report whose gates are `failed` or `block` still exits 0. There is no `expected` field and no force flag.

The report is compact JSON. The process writes one newline after it. `receipt_sha256` is the SHA-256 of the JSON object with `receipt_sha256` set to `""`, without that newline.

`owner_authenticated`, `publication`, `full_gates`, and `source_modified` are false. `metric_certified` is true only when evaluate passed G5a and G5b, the enum classes are reviewed at the closed anchor, the control path is independent, and a supplied previous document does not require owner review. Compile never certifies. Omitted gates stay `not_run`.

`evaluate` opens one TLS session only after the local checks in the contract pass. A connect failure is `outcome` `evaluated` with both gates `block`, `database_used` false, and exit 0.
