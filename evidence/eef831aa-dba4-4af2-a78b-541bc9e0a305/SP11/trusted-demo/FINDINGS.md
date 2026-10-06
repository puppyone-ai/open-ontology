# Findings

Same-session review of the frozen OPS-D01 demo after `linux/public-attempt-3` and `linux/smoke` exited 0. This session also wrote the demo. The review is not an independent approval and does not accept the parent unit, I01, I02, a release, authentication, or a harness.

Scope stayed on `deploy/trusted-demo/**` and this evidence directory, plus the boundary checks the contract names: not-ready probe, bad decision before apply, north positive query, hidden and write guards, secret scan, restart smoke. No new child unit was opened.

## Corrected during the demonstration

Attempt 2 inspect failed before init. `live/run` at that time is preserved as `linux/public-attempt-2/failed-run/`. `import-not-ready.stderr` is `oo-pipeline: source config source_id`. `import-not-ready.stdout` is empty. Exit is 1. `SourceId::new` in the fixed archive accepts `^[a-z][a-z0-9_]{0,62}$`. The frozen draft identity `public-shop` contains a hyphen, so the probe never connected and never emitted `target schema is not ready`.

The source id and the project token are now `public_shop`. Apply copies `source_id` into the project field, and the gateway project check also refuses a hyphen. `CONTRACT.md` records the correction. Attempt 3's not-ready stderr is `oo-pipeline: target schema is not ready`. No product source, root manifest, or lock was edited.

Earlier attempt exits, kept as their own directories:

- `linux/public`: `drive-test-build.exit` 1. Unused imports and `Bound` without `Debug`.
- `linux/public-attempt-1`: `drive-clippy.exit` 1. `needless_range_loop` and `useless_vec`.
- `linux/public-attempt-2`: `drive.exit` 1. The source-id shape failure above.

Those three `docker-run.exit` files are 1. Attempt 3 and smoke are 0.

## Checked on the passing run

- Inspect status remains `awaiting_review`. Apply status is `ready_for_trusted_query`. `target_fence` is 1 and `source_cut` is 48.
- Bad-run step stems are only `import-not-ready`, `init-pg`, `import-pg`, `catalog-pg`, `captured-pg-draft`, `review-pg-draft`, and `reviewed-effective-oos`. The failure step is `reviewed-effective-oos` with child text `decision.unknown`.
- Query exits are 0 for the five positive reads, 5 for `query-north-before` (`query-pg: policy`), and 6 for hidden, insert, and `_oo_pos` (`query-pg: guard`).
- North role-pack SHA-256 `74e5069b398b8a50118ead3630fefba8b0bc66adcc9c76e33bc8a3237bcbfa38` and narrow `4530c2c34cd31e3a4f44710416a23274af426771e56e8c5b71af4bf3bcc0c8ac` match the receipt. Smoke printed `role_pack_unchanged` and `query_rows_repeated`.
- `甲注评` is absent from `catalog.json` and `captured-draft.json`. `private_note` is absent from the positive customer and narrow order stdout. `柜员甲` is present in the north order stdout and absent from the narrow order stdout.
- Secret reports say `secret_absent`. Builder hostconfig for attempt 3 and smoke: memory `6442450944`, NanoCpus `1000000000`, privileged false, cap-drop ALL, OOMKilled false. Mount text has no docker socket and no exact host-home bind. PostgreSQL publishes no host port.
- Onboarding archive tests `passed=24 failed=0 ignored=1`. Clippy exit 0. `cargo-update-invoked.txt` is `no`.

## Observed and left as the tool wrote it

Apply provision effects are north `installed` and narrow `replay`. The driver does not require a particular effect string. The outcome file is the record.

The in-container SHA-256 walks finished before the host wrote `hostconfig.txt`, `mounts.txt`, `tmpfs.txt`, `invocation.txt`, `docker-run.exit`, `container-removed.txt`, and the finished `console.log`. Those files are in the directories and are called out in `HANDOFF.md`. `SHA256SUMS` was produced on the host after this review text, so it includes the reports and excludes itself.

The wrapper log `start-public.stdout` is one stream for attempt 3 and the smoke phase. A copy sits in both phase directories. `linux/smoke/host-log-note.txt` says so.

## Not claimed

No dual-architecture matrix. No runsc. No shared multitenant deployment. No model call. No natural-language loop. No parent signoff. Cleanup of the private PostgreSQL, volumes, network, and `/tmp/oo-a305-eef831aa-dba4-4af2-a78b-541bc9e0a305` was not run. The containers `oo-a305-build` and `oo-a305-smoke` were removed by the launch script after they exited.
