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

## summary_sha256 preimage (fixed by the TUI lane, binding for all producers)
`sha256( compact sorted-key JSON of {"decisions": {<item id>: <decision>}, "graph": <graph block>} )`, with no whitespace, keys sorted recursively, and UTF-8 output (no `\u` escaping of non-ASCII). After overrides are applied, recompute the hash over the overridden decisions; a confirmation is accepted only against that recomputed hash. Reference implementation: `crates/oo-cli/src/tui/adjudication.rs` on branch `v0.3/tui`.

## Reviewed dictionary bindings

The pinned reviewed-community input may supply
`adjudication_specs.dictionary_bindings`. These are operator-reviewed business
definitions, never model guesses or statistical containment:

The retained builder accepts optional `dictionary_bindings_input` with
`path` and `sha256` pointing to
`{"protocol":"oo.reviewed-dictionary-bindings/v1","bindings":[...]}`.
It reads the pin before querying the replica. Adding, changing or removing
these definitions requires fresh adjudication and confirmation; the
data-only reuse shortcut is disabled for such inputs and origins.
When bindings are present, `graph.dictionary_bindings_sha256` contains the
canonical digest of the full definition list. This participates in the
existing graph summary hash: changing a discriminator, convention or
provenance invalidates an old confirmation even if graph edges and decisions
stay the same. The field is absent on legacy documents, preserving their hash.

```json
{
  "id": "event-state",
  "concept": "Event state",
  "from": {"dataset": "app.events", "code": "state", "at": "happened_at"},
  "dictionary": {
    "dataset": "app.dictionary", "discriminator": "kind", "value": "event_state",
    "code": "code", "label": "label", "valid_from": "valid_from", "valid_to": "valid_to"
  },
  "interval": "closed_open_null_end_unbounded",
  "provenance": {
    "origin": "operator_reviewed", "source_sha256": "<reviewed source digest>",
    "reviewer": "<operator>", "reviewed_at": "2026-11-12"
  }
}
```

`from: null` is a dictionary-only lookup: no fact-reference coverage is
claimed. Supported interval conventions are start-inclusive/end-exclusive,
and `closed_closed_date_null_end_unbounded` (inclusive dates, adjacency is
one day). NULL ends are unbounded in both. The convention must come from
the reviewed definition, not inferred from physical types.

Replica adjudication records `code_table/dictionary-<id>` with the canonical
binding digest, rows checked, singly matched and ambiguous references,
invalid intervals, overlap pairs and historical gap pairs. Measurements are
read-only, discriminator-scoped and bound to the existing retained-cut input.
An incomplete reference match or historical gap is disclosed, not silently
filled. Ambiguous or invalid intervals withhold the binding even after a
decision override. Coverage never supplies a missing business meaning.

The compiler exports `dictionaries.md` only when every referenced column is
visible. Aggregate evidence is omitted under RLS or denied global statistics.
Application role codes never become grants. The effective extension carries
review provenance and evidence; publication authentication remains the
publisher's responsibility. No source-current or atomic-snapshot claim is made.
