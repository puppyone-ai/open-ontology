# Admission test set (lane AK, D94)

A mixed corpus of 41 synthetic files: records (minutes, a decision, a weekly report, people and project files, a contract, csv tables), instruction files
(AGENTS.md, CLAUDE.md, README, CONTRIBUTING, a style guide, two ADRs, a deploy SOP), five references (three PDF papers, a book chapter, an external
report), code (Rust, Python, TypeScript, shell, manifests, CODEOWNERS), media (a png, a wav) and archive files (a log, a licence, a changelog, json, zip, binary).
All content is synthetic.

* `project/`: the folder to drop into My files.
* `gold/classes.jsonl`: the class (and profile) of every file. `gold/reference.jsonl`, `gold/reference-relations.jsonl`: the Reference instances, authors, `cites`
  and `authored` of the five references. `gold/records.jsonl`: record entities that must enter. `gold/brief.json`: the instruction files and the quote the BRIEF section
  工作约定 must hold. `gold/questions.jsonl`: six questions answered from the BRIEF.
* The D89 entity gold (rules, commands, symbols, methods, datasets) is retired by D94 (instruction and code files put no entity in; a reference gives one instance).
  Gold amendment: the book chapter and the external report were added to the corpus after the paper gold and their reference items were listed after the first scoring
  (precision 0.778 / 0.818 before, 1.0 after), because the annotation lacked them.

`python3 -B tools/super-ontology/profiles/gen.py` rewrites it (`--check` compares). `tools/super-ontology/profiles/run_live.py` runs it through the real pipeline and
`score.py` scores it (hard gates: no entity from instruction, code and other files, a reference only gives Reference and Person, the BRIEF holds the instruction files).
