# trial-statistics

Standalone workspace. It reads one `oo.benchmark.trial-statistics/v1` document from stdin and writes one paired pass^k and bootstrap report to stdout. Exit 0 is a passed gate, 1 is a failed gate, and 2 is config.

The rules are `CONTRACT.md`. pass^k counts questions whose every trial passed. A paired bootstrap resamples whole questions with one shared index list. Bounds are exact rationals at the floor ranks for 2.5% and 97.5%.

This crate is not a member of the root workspace. The proof build points `oo-core` at the fixed archive and keeps serde 1.0.229 and serde_json 1.0.151. It does not score a question bank, call a model, or accept a benchmark release.
