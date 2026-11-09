# Contract: oo.adjudication/v1 (V3-G-01 produces, V3-A-02 TUI consumes)

The Builder writes `adjudication.json` beside the reviewed-community input. The TUI reads it, and so does the non-TTY `oo ontology graph`.

```json
{
  "protocol": "oo.adjudication/v1",
  "input_sha256": "<sha256 of the reviewed-community input>",
  "thresholds": {"relation_containment_strict": 0.98, "relation_containment_with_signal": 0.9, "code_coverage": 0.99},
  "items": [
    {
      "id": "relation/<stable-id>",
      "class": "relation | sensitive | metric | entity_key | code_table",
      "subject": {"from": "schema.table.col", "to": "schema.table.col"},
      "decision": "adopted | rejected | hidden | visible | certified | demoted | complete | incomplete",
      "evidence": {"containment": 0.997, "parent_unique": true, "name_similarity": 0.8, "join_cooccurrence": 3, "cardinality": "many_to_one", "rows_checked": 120000},
      "scores": {"confidence": 0.97},
      "rule_id": "relation.containment_strict",
      "reason": "Short human-readable sentence, Chinese or English. Never sample values for sensitive columns."
    }
  ],
  "graph": {
    "entities": [{"id": "entity/<id>", "label": "客户", "table": "s.customers", "key": ["id"], "display": "name", "rows": 120000}],
    "edges": [{"relation": "relation/<id>", "from": "entity/<a>", "to": "entity/<b>", "cardinality": "many_to_one", "adopted": true}]
  },
  "summary_sha256": "<sha256 of canonical graph + decisions; the one confirmation binds this>"
}
```

Overrides: `overrides.json` = `{"protocol":"oo.adjudication-override/v1","summary_sha256":..., "items": {"<id>": {"decision": "...", "reason": "..."}}}`.
Confirmation: `{"protocol":"oo.publish-confirm/v1","summary_sha256":...,"overrides_sha256":...,"reason":"...","confirmed_at":...}`.
The compiler accepts publication only with a confirmation whose summary hash matches the summary recomputed after the overrides are applied.
