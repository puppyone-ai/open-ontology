# Handoff

## This unit

Fixed archive `0bcd494457091d4b55c82fc6b9922b5bc5dc44c4`.

Workspace: `tools/enum-history`. Contract: `tools/enum-history/CONTRACT.md`. Library entry: `enum_history::compile`. CLI: `enum-history` with a JSON object on stdin and no arguments.

Passing proof: `linux/proof-3`. Independent fresh-target review: `linux/review-1`. Pre-edit smoke: `linux/preoverlay`. All three are Linux arm64 Rust 1.94.0, 6 GiB, 1 CPU, no PostgreSQL. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Child lock `32ff28450510cd4cc375cc49ed52fe4be3a6a09af3d2a17b80c94c9ee90fa7a8`.

`linux/proof-1` and `linux/proof-2` failed after or before clippy and are kept. Recipe origins are in `recipe-origin.txt`. `tools/sha256_manifest.rs` is the a204 byte copy, SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. It checks the empty digest and `abc`, then writes `MANIFEST.sha256` for every regular file except that manifest. The final inventory pass is `linux/manifest-3`. Files that launch writes after that container exits (`docker-run.exit`, `hostconfig.txt`, `console.log`, `container-removed.txt` in `linux/manifest-3`) sit outside the manifest.

## Behavior to keep

Half-open `[valid_from, valid_to)`. Missing ends are infinities. Overlaps refuse, including definitions that do not contain `as_of`. Reserved names `undetermined` and `unexplained` are never a business class. Superseded and deprecated rows stay in the history. A historical business class needs the class key plus `review.state=approved` and `review.by`. Inferred approved rows need those same fields. Declared coverage is echoed. Observed coverage is a separate unreduced `numerator/denominator`. Zero observed rows are undefined, not 1.

Success and refusal reports set `database_used`, `source_modified`, `oo_dict_written`, `metric_certified`, `owner_authenticated`, and `sweeper_ran` to false.

## Next gates

A later child owns the database sweeper, hourly scheduling, `oo_dict` writes, metric certification, and customer owner proof. Those need their own frozen contract before code. This child does not supply them.

## Do not widen this unit

Do not scan a live database from this crate. Do not rewrite producer coverage into the enum file. Do not edit the root manifest or the root lock from here. a207 remains the connector manifest writer. Do not treat this evidence as SEM-D02 acceptance, a finished SP08, or a release.
