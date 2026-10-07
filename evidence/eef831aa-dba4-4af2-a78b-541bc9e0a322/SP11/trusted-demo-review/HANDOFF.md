# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a322`. Evidence `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a322/SP11/trusted-demo-review/`. Product freeze `895864a5bf64e50cef84075d20fd7b0c9e41c264`, demo tree `1b6e8229107039059a94b7bd65a1c59fc198cc96`. CLI archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.

Verdict: APPROVE_LIMITED for the combined `deploy/trusted-demo` public-fixture command. This handoff does not accept the parent, M1, a release, authentication, natural language, a harness, or an enterprise endpoint. No product path was edited. `a313` remains the root lock writer.

## What the next reader should open

- `deploy/trusted-demo/CONTRACT.md` at the freeze, for the manifest grammar. The copy exercised here is `source/deploy/trusted-demo/`.
- `recipes/ORIGIN.md` for the a204, a200, and a199 launch files and the 6 GiB / 1 CPU change.
- `linux/preoverlay/` for the smoke before the chain. `docker-run.exit` is 0. `test-count.txt` is `passed=24 failed=0 ignored=1`. `smoke-ok.txt` says `chain=not_run`.
- `boundaries/` for missing, bad, owner, and UUID refusals. `summary.exit` is 0.
- `product/live/receipt.json` for the raw receipt. `product/live/run/steps/query-north-customer.stdout` and `product/live/smoke/north-customer.stdout` for the three customer rows.
- `product/live/bad-run/` for the malformed review that stops before column ACL.
- `product/linux/public/` and `product/linux/smoke/` for the in-command phase exits.
- `cleanup/collision.exit`, `cleanup/stop.exit`, and `cleanup/clean.exit` are 2, 0, and 0.
- `linux/preoverlay/deploy-sha256.txt` for the 23 frozen demo files. `SHA256SUMS` for this evidence directory. `linux/hash-manifest/console.log` and the other hash logs beside `launch.sh` are copied after that walk and are outside `SHA256SUMS`.

## Command that was run

From the private checkout of `895864a5`. The manifest is one canonical line, owner `a322`, run id `eef831aa-dba4-4af2-a78b-541bc9e0a322`, fixed source `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`, source id `public_shop`.

```bash
./deploy/trusted-demo/start.sh --manifest boundaries/good.json --public-fixture
./deploy/trusted-demo/start.sh --manifest boundaries/good.json --public-fixture
./deploy/trusted-demo/start.sh --manifest boundaries/good.json --stop
./deploy/trusted-demo/start.sh --manifest boundaries/good.json --clean
```

The first command includes the restart. The second exits 2 and does not overwrite volumes. `--clean` removes this manifest's containers, volumes, network, and `/tmp/oo-a322-<run_id>`. It does not prune.

## Cleanup already run

`oo-a322-pg` `c067edebf70017fd1708c1a239339619906362d280ba26d6c721a5f8931e12e2` was removed by `--stop`. The three `oo-a322-eef831aa-*` volumes, network `oo-a322-eef831aa-net`, and the private tmp were removed by `--clean`. The preoverlay volumes and network were removed before the product command. The private clone `/tmp/oo-a322-eef831aa-dba4-4af2-a78b-541bc9e0a322-repo` was removed after the product tree was copied to `product/`. Repeating `--public-fixture` with this manifest still exits 2 while `product/live/run` remains.

## Outside this review

The ops console, Compose, M1, authentication, natural language, the harness, and an enterprise connection remain later work. A new expansion needs its own frozen contract before code.
