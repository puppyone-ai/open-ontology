# source-support contract `oo.source-support/v1`

`source-support` is a standalone registration-admission tool. It reads `docs/database-support/support-matrix.yaml` and prints one JSON object to stdout. It is not the `oo` CLI. It does not call `source-control-api`, and it does not change the matrix file.

The frozen registration shape is `oo.source-control-api/source-register/v1` (`pg_local`, `file_local` only). This tool checks that shape when `admit` carries a registration. It does not resolve `secret_ref` and it does not evaluate the host allowlist. Those results are `not_evaluated`. Parent HTTP, CI, build review, release, and customer integration stay pending.

## Operations

```text
source-support supported --matrix MATRIX
source-support check --matrix MATRIX --doc DOC [--evidence DIR] [--allow-private]
source-support select --matrix MATRIX --request REQUEST [--evidence DIR]
source-support admit --matrix MATRIX --request REQUEST --schema SCHEMA [--evidence DIR]
source-support downgrade --matrix MATRIX --event EVENT
```

`admit` requires a registration object. `select` rejects one. Request contract: `oo.source-support.admit/v1`. Downgrade event contract: `oo.source-support.nightly/v1`.

Stdout is one JSON object. The process exit is the object's `exit` field.

| Exit | Meaning |
| --- | --- |
| 0 | Checked, listed, admitted, experimental admission, downgrade held, downgrade applied, or security block recorded. An applied downgrade does not stop an existing source. |
| 1 | Malformed JSON, YAML, usage, or registration input. Credential key names use reason `credential`. A closed registration key uses code `closed_input` and the key name as reason. |
| 2 | Matrix refusal: unknown connector, unverified version, unverified capability, unknown compatibility mode, or missing driver. |
| 3 | Evidence or contract integrity: forged, missing, handwritten, scope, doc drift, or registration-schema drift. |

`supported` lists only `verified` and `certified` rows. `experimental` rows are separate and are not a production grant. The public matrix currently has neither.

Explicit `sync` names one capability. `sync: auto` selects the single verified or certified capability. It does not choose `experimental`, and it does not replace a requested capability with a lower one.

`delete_native` and `delete_reconcile` are not the `del` / `delete` capability. Patch versions are not exact matches. A patch admission needs a generated evidence pack whose payload hash matches its manifest. Two consecutive nightly failures downgrade a verified or certified capability to `experimental` in the decision only. Consecutive means the last two results are failures whose Gregorian dates differ by one day, including month ends, year ends, and a real leap day. 2000-02-29 is a date. 2100-02-29 is not. An earlier success or failure stays in the history and does not replace those two days. A missed day stays held. Invalid, reversed, and duplicate dates are malformed (`reason` `day`). The matrix file is not written. `matrix_mutated` is false. Existing sources are returned with `stopped: false`. A security trigger pauses that capability, keeps `data_retained: true`, and still does not stop the source.

Secret values, `secret_ref`, host, and username are not copied into the JSON object.
