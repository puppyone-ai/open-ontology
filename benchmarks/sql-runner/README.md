# sql-runner

Standalone workspace. It reads one `oo.benchmark.sql-runner/v1` suite and writes one `oo.benchmark.sql-runner-report/v1` report. Each planned trial spawns `query-pg`, then the query-result adapter, then the result comparator. Exit 0 is a passed gate, 1 is a failed gate, and 2 is config.

The rules are `CONTRACT.md`. The report lane is trusted SQL. `metadata` is the word `declared`. A truncated result that matches a prefix still fails. This crate does not open a database, call a model, or accept a benchmark release.

This crate is not a member of the root workspace. The proof build points `oo-core` at the fixed archive and keeps serde 1.0.229 and serde_json 1.0.151.
