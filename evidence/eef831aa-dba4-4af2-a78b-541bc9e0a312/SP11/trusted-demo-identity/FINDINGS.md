# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a312`. Scope is the identity of `deploy/trusted-demo` plus this evidence directory. The author of the change wrote these notes. They are not an independent review and they do not accept the parent unit.

## Corrected during the demonstration

`linux/pre-edit-smoke` exited 127 because `bash -lc` replaced the image `PATH`. The next pre-edit attempt used `bash -c` and passed 5 driver tests. No product file was edited for that failure.

The first manifest parser built the `run_id` separator without the quote between `owner` and the comma, so a canonical line would have been refused. The parser now requires `owner","run_id":"`. A later edit also refuses a carriage return and more than one trailing newline. Host checks accepted the `a312` line, the explicit historical `a305` line, and refused a swapped owner, a trailing space, a second newline, and a carriage return.

`historical_manifest` was marked `#[test]` and returned `&str`. `linux/post-edit-drive-attempt-1` exited 1 with `E0277`. The attribute was removed. `linux/post-edit-drive-attempt-2` passed 7 tests and Clippy `-D warnings`.

The README said the command does not read uncommitted product source. The reviewed CLIs come from the fixed archive. The driver and the shell scripts come from `deploy/trusted-demo`. The README now says that. `linux/hash-source-attempt-1/` keeps the walk from before that sentence changed.

## Checked on the passing run

Bare identity is not implied. `start.sh` without `--manifest` exits 2. Owner must be the last four lowercase hex characters of `run_id`. Docker owner label `a312` and inspect `owner` UUID are different fields, both derived from the same manifest. `source_id` stays `public_shop` because the fixed import CLI rejects a hyphen.

The public fixture and the restart smoke both exited 0. Onboarding tests were `passed=24 failed=0 ignored=1`. Public phases were 33 zeros. Smoke phases were 7 zeros. North rows were 41, 42, and 43. Hidden column and insert both exited 6 with `query-pg: guard`. The bad column `not_a_column` stopped at `reviewed-effective-oos` with `decision.unknown` before column ACL. After stop, a second `--public-fixture` exited 2 and left the volume timestamps unchanged. `--clean` removed only the `a312` containers, volumes, network, and private tmp. A later `--public-fixture` still exited 2 because `live/run` remains. A later `--clean` exited 0.

`oo-a300-pg` kept id `a68ce9caf5d6489211e27aaa5f5bbf75e75a883f110129679c76ecec7166bbd1`. No `a305` object was created. Root `Cargo.toml` and `Cargo.lock` were not edited. `tools/sha256_manifest.rs` stayed `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.

## Observed and left as the tool wrote it

The bad-run `outcome.json` is still the copied inspect outcome (`awaiting_review`). The apply result is `apply-outcome.json` (`failed` at `reviewed-effective-oos`). The driver checks the apply file.

`query-north-pos.exit` is 6 with `query-pg: guard`. The driver treats that as the narrow positional guard already in the fixture. It was not given a new test.

Secret scans report `secret_absent` and do not contain password bytes. Password files lived under `/tmp/oo-a312-eef831aa-dba4-4af2-a78b-541bc9e0a312/secrets` and were removed with `--clean`.

## Cleanup

`cleanup/stop.exit` is 0, `cleanup/collision.exit` is 2, `cleanup/clean.exit` is 0. Evidence was kept. Failed attempt directories were kept.

## Not claimed

Authentication, authorization, human approval, semantic publication, natural language, a model provider, a harness, M1, I01, I02, parent acceptance, a release, Compose, and an enterprise connection are not claimed. The receipt records the same non-claims. A fresh clone supplies its own run id. It does not inherit `a305` or this session's containers.
