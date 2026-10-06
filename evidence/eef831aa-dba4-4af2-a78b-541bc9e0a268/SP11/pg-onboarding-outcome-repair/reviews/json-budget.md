# Review: JSON budget

Scope: `tools/pg-onboarding/src/json_doc.rs` and the call sites in commit `50d4e705a2bb4ba014919ef1e071da065cf8e9b7`.

The visitor runs before `serde_json::from_slice`. A repeated key, compared after escape decoding, is `shape`. Depth above 32, counting the root as 1, is `budget`. More than 50000 values is `budget`. A document longer than the caller cap is `budget`. Trailing non-whitespace is `shape`. Trailing whitespace is accepted.

Operator reads use 1048576 bytes: config, inspect manifest, inspect outcome, reviews, decisions, and visibility. Child JSON uses the existing capture cap 8388608. SQL and hashed artifact reads still use `read_file` and are not forced through the 1 MiB cap.

`linux/red-attempt-4` shows the archive turning duplicate keys, a 1048576-byte space padding, one extra byte, depth 32, 50000 nulls, and trailing `{}` into detail `config`, or accepting a duplicate manifest and duplicate decisions. `linux/green` reports `shape` or `budget` as specified, and schema-invalid documents inside the caps stay `config`.

`Cargo.toml` and `Cargo.lock` are unchanged from `56710cc73cfe1663272ecaa503984e1f663428ad`. There is no path dependency and no `cargo update`.

No separate test feeds duplicate compiler stdout. The same visitor parses that stdout after spawn.
