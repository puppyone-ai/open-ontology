# Offline ontology renderer evidence

Card: S4-A-03 / G-S4. Run: `g-s4-20261104` (2026-11-04).
Base: `36c2014f8`; rebase on `v0.2/product-loops` was already up to date.
Raw outputs remain in ignored `evidence/g-s4-20261104/`.

- `cargo check --workspace --locked`: passed.
- `cargo test -p oo-cli --locked`: 50 tests passed, including two renderer tests.
- `node crates/oo-cli/tests/ontology_browser.mjs EVIDENCE_DIR CHROME`:
  Chrome 156.0.8078.80 headless, 1,200 synthetic entities, desktop width 1280
  and mobile width 390. Both passed all 13 assertions. Initial render through
  two animation frames: 42.7 ms / 38.5 ms (threshold 2,000 ms). These are local
  samples, not a cross-device benchmark. Lists mount fewer than 20 buttons;
  last entity, search, filters, details and single layout pass are verified.
- HTML URL/network primitive scan and browser zero-subresource assertion pass.
  CSP also blocks connections, external resources, forms and base URLs.
- Tests reject role mismatch, corruption, hidden-column references and cyclic
  summaries. Graph and summary continuations are consumed in full.
- Mermaid includes 24 community table pages plus a bounded relationship page
  retaining the cross-community edge. Syntax is asserted; no Mermaid engine
  has been executed. No JavaScript library is vendored.
- Initial `--dump-dom --virtual-time-budget` browser harness failed because
  animation frames had not run. Its output is retained as
  `browser-virtual-time-failure.log`; final harness uses real-time CDP over
  anonymous pipes, a private profile and no listening port.

SHA-256:

| Raw output | SHA-256 |
| --- | --- |
| renderer-check.log | 703e7eca104f790350e17d972e1a99183de1c7b70c586dad54684bf0b710ec76 |
| renderer-tests.log | d0aa082db3396378bf3514c3aad18fc80f614f86fcaa222364d19a5ccce59cee |
| browser-cdp-final.log | eaf85bc557b771ba3b6922938296fa16f9cfb44b114044e7fa490d5351e32ae9 |

## Acceptance boundaries

The renderer accepts a supplied compiler Pack artifact, not an effective OOS
or a source connection. Digest checks establish internal integrity, not
identity/authentication; `--role` is only an equality assertion on that Pack.
Metrics are explicitly Pack-wide: current graph/summary documents do not carry
entity-to-metric attribution. No attribution is guessed from SQL text.

This card is **not complete**. Admin dispatch is outside the allowed write
scope and still needs coordinator wiring. YAML cannot safely call the existing
`tools/selfbuild-control-api/recipes/export_oos.py` on full effective OOS:
that serializer has no role visibility filter, and the role Pack contains no
reviewed OOS document. A role-closed reviewed OOS contract is needed before
reusing that serializer; the command fails closed rather than exporting hidden
fields or substituting graph JSON as OOS YAML. No Cargo.lock changes.
