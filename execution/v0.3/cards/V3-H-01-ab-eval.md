## V3-H-01 — A/B evaluation schema-only (C0) vs reviewed ontology (C3) (P2-3)
Plan: specs/v0.3-quickstart-and-competitive-plan.md. Commit as the owner, no agent trailers, trailer `Plan: <card id>`. Never print or commit model keys, endpoints or private terms. Do not touch unrelated containers. Each card ends with unit tests plus the named integration test; report commits, tests, residuals.

Runner in benchmarks/ + crates/oo-eval: same model, deterministic execution match against frozen results, no LLM judge, fixed/broken question lists, health counter (no-output >5% => no headline number). Datasets: OO-Logistics public questions, BIRD Mini-Dev (PG). Report relative change only, plus separate permission/injection tracks. Reproducible with fixed seed and model; fixed evidence.
