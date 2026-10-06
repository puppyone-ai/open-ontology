# question-bank-suite

Standalone workspace. It reads one `oo.benchmark.question-bank-suite/v1` document and writes an `oo.benchmark.sql-runner/v1` suite for one explicit role and one `as_of`. Admission is `oo_question_bank::evaluate` from the fixed question-bank crate. This program does not reimplement that gate, does not spawn the runner executables, and does not open PostgreSQL.

The rules are `CONTRACT.md`. Gold SQL is copied into the trusted request. Typed gold stays in the scorer-owned gold file. The agent view and the stdout report do not carry gold cells. Exit 0 means the suite directory was written. It does not mean a model passed, and it does not mean the bank's own label gate passed.

This crate is not a member of the root workspace. The proof build points `oo-core` and `oo-question-bank` at the fixed archive and keeps serde 1.0.229 and serde_json 1.0.151. The tests use a small public synthetic bank. They are not 300 questions, not a human annotation roster, and not a customer holdout.
