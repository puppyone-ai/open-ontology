# Profile test set (lane AK, D89)

A synthetic engineering repository (Harbor Queue: AGENTS.md, README, CONTRIBUTING, a style guide, two ADRs, a CHANGELOG, Rust, Python and TypeScript code,
three package manifests) and three research papers (text layer PDFs, two in English and one in Chinese, with abstracts, DOI or arXiv ids and references that
cite each other). All content is synthetic.

* `project/`: the folder to drop into My files (`harborq/` and `papers/`).
* `gold/instruction.jsonl`, `gold/code.jsonl`, `gold/paper.jsonl`: the expected `Rule`, `Command`, `Tool`, `Path`; `Function`, `Class`, `Type`, `Dependency`,
  `Package`; `Paper`, `Person`, `Venue`, `Method`, `Dataset`, `Metric` per file. Written down in the generator next to the text they describe and frozen before
  the first run: they are not edited after a result.
* `gold/questions.jsonl`: 10 questions (5 on instruction documents, 2 on code, 2 on papers, 1 unanswerable) for Ask, scored by
  `tools/super-ontology/acceptance/score_ask.py`.

`python3 -B tools/super-ontology/profiles/gen.py` rewrites it (`--check` compares). Score a run with `tools/super-ontology/profiles/score.py`.
