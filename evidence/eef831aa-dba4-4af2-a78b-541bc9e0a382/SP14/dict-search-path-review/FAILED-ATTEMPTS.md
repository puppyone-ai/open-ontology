# Failed attempts

The first unit invocation filtered on the bare name with `--exact`.

```
cargo test --locked --offline --lib pinned_search_path_is_exact_pg_catalog_only -- --exact pinned_search_path_is_exact_pg_catalog_only
```

Cargo's exact match is the full path `adopt::tests::pinned_search_path_is_exact_pg_catalog_only`. The harness ran nothing. `proof/calendar-unit.log` says `0 passed; 0 failed; 5 filtered out`. `proof/enum-unit.log` says `0 passed; 0 failed; 7 filtered out`. Both process exits are 0. Those two logs are not a pass.

The same builder then ran the full path. `proof/calendar-unit-exact.log` and `proof/enum-unit-exact.log` each say `1 passed`. Those are the unit results.

No proof container was replaced. No product file was edited. No image was pulled.
