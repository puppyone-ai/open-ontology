# question-bank

Standalone workspace. It reads one `oo.benchmark.question-bank/v1` document from stdin and writes a manifest, an agent view, and a scorer view. Each document has a content hash. Exit 0 means every declared label is `fully_approved`. Exit 1 means at least one question is `needs_reannotation` and that question stays in the denominator. Exit 2 is config.

The rules are `CONTRACT.md`. The agent view has no gold SQL, expected answer, control total, or annotation. Review labels are `label_only`. They are not a signature.

This crate is not a member of the root workspace. The proof build points `oo-core` at the fixed archive and keeps serde 1.0.229 and serde_json 1.0.151. The fixtures are a small public synthetic set. They are not 300 questions, not a human annotation roster, and not a customer holdout.
