# Doris findings, 2026-10-14

## Retained failures

`/tmp/oo-doris-connection-whole/whole-3/journal.json` stays `failed` with 5 steps. Its recovery remains unverified from the bad-loader probe. The journal and its pins were not edited. Later runs `whole-4` and `f9a9dffe2caf41f493321641a64fb03a` stay failed evidence.

On `e875`, attempts 0 and 1 exited 2 after dialogue. Attempt 2 exited 2 because `export_oos.py` refuses an existing `product/oos` directory (`mkdir` with `exist_ok` false). That directory was moved to `product/oos-attempt1`. Attempt 3 exited 0.

`doris-cancel-once` (`aa01df3b-1290-4425-a7d5-a5435806c6fe`) and `doris-cancel-maintain` are `terminal|cancelled` in `jobs.tasks`. Neither row was rewritten.

## Shared entry

`tools/source-onboarding/product_flow.py` and `onboard.py` are owned by shared-entry. Commit `8fa9968c38a630ba40ae7d92f9bb5f524a3ffaf9` changed both after this run had pinned product_flow `b20148b6a634819f8503d35dd019d64d19b71e1efdd280dbf846e3e40f93674b`. The succeeding step and the registry proof ran while the worktree file matched that pin. The worktree file is restored to `22ac3d0a774c2a6c2d7adf8a7d53e363699f25c971a124c4c4db3b13d1df253d`. A later resume of `e875` hits `pin_drift` against the new shared bytes. The run is complete and stays as recorded.

`onboard.py` now accepts a frozen `engine_registry_sha256` when `selected_engine_sha256` is absent and the password-file pointer still matches. That is what allowed this resume after other engines had rewritten `engines.json`. This source did not edit that file.

The pinned API ELF does not implement maintenance trigger `source_generation_advanced` (HTTP 400 `malformed` on an earlier attempt). The current gateway binding was produced by the pinned Linux `pg-builder sealed-review` and installed on the existing container `oo-doris-connection-whole-gateway`, which was restarted so it would read the new files. The sealed bundle catalog used by S5 remains `ed7c4168a49851d802e7aade1a03401618c38627cfa281c0b925d40576a65d52`.

## Scope held

No root Cargo or lockfile write. No pipeline SQL, gateway policy, installer, or pg-builder source write. No reader rebuild. No new container. Customer metadata was not written. `oo_reader` was not granted write. Publication stays false. Business owner signature stays false. External fixed review stays owner-pending.
