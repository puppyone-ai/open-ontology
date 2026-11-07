## Card G-S4 — lane G (reassigned from A): oo ontology open / export

TODO: S4-A-03 (reassigned to lane G by the coordinator; lane A is on the S2 critical path). Spec: REQ-ONB-06.
Rebase on v0.2/product-loops first. Input: the Pack's `relationships/graph.json` (your G-S3 work) and the role Pack.
Deliver: `oo ontology export --format html|mermaid|yaml [--role R] --out PATH` and `oo ontology open` (writes the HTML to a
temp path and opens it with the OS opener; `--print-path` for headless). HTML: ONE self-contained file — JavaScript and CSS
inlined, no network requests (assert in a test by scanning for http(s) URLs / fetch / external src), interactive
(search, filter by domain/community, click an entity to see tables, columns, relations, metrics), usable with ≥1,000
entities (layout computed once, virtualized lists; measure render time in a headless browser test). Mermaid: ER diagram
paged per community when large. YAML: the reviewed OOS export path that already exists (reuse). Respect role visibility
(only what the role Pack shows). Any vendored JS must have an admissible license recorded under supply-chain/.
Write scope: crates/oo-cli/src/admin/ontology* (new), crates/oo-builder export helpers, tests, supply-chain records.
Work rule: keep going; commit as you go.
