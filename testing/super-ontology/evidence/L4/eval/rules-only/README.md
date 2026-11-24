# L4 evaluation, rules only (no model)

`cargo run -p oo-eval -- docs --golden testing/super-ontology/golden --predictions <dir> --out <dir>`
over the output of `rules/1` for the golden files the local parsers read (md, txt, csv, xlsx;
28 of 51). `<dir>` is written by
`OO_DOCS_PREDICTIONS_DIR=<dir> cargo test -p oo-docs --lib golden_predictions`.

Not evaluated here: docx, pptx and pdf, because the Docling sidecar is not run in this phase; their
gold labels count as misses, so the overall recall is a lower bound for the full pipeline. The target
with the model (precision 0.85, recall 0.75) is a different run; this one is the no-model number.
