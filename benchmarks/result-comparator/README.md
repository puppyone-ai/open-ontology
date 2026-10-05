# result-comparator

Standalone workspace. It reads one `oo.benchmark.result-comparison/v1` document from stdin and writes one exact comparison report to stdout. Exit 0 is a match, 1 is a mismatch, and 2 is config.

The rules are `CONTRACT.md`. Decimals compare by `oo-core`'s exact rescale. Rows are ordered or an explicit unordered multiset. Partial does not match complete.

This crate is not a member of the root workspace. The proof build points `oo-core` at the fixed archive and keeps serde 1.0.229 and serde_json 1.0.151. It does not score a question bank, call a model, or accept a benchmark release.
