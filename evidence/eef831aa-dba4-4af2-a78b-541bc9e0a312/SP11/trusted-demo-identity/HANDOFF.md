# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a312`. Product tree `deploy/trusted-demo`. Evidence `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a312/SP11/trusted-demo-identity/`. Fixed archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.

Independent fixed review is still required. This handoff does not accept the parent, M1, a release, authentication, natural language, a harness, or an enterprise endpoint.

## What the next reader should open

- `deploy/trusted-demo/CONTRACT.md` for the manifest grammar and the historical `a305` line, which applies only when that file is passed explicitly.
- `deploy/trusted-demo/linux/manifest.sh` for the shell parser. `deploy/trusted-demo/src/drive.rs` parses the same line. Driver source SHA-256 `60f2ba592e456418e6da94c90e0a4815aa69299bc62adc74046ed07432b9aef1`. Built artifact `0e0e3a8c90c9d67526c35cd720d7daab30fa69619d04676da4ca2167e23f051c` in `linux/public/bin-sha256.txt`.
- `live/receipt.json` for the raw receipt. `live/run/steps/query-north-customer.stdout` and `live/smoke/north-customer.stdout` for the three customer rows.
- `linux/public/console.log` and `linux/smoke/console.log` for phase exits. `linux/public-fixture.host.exit` is 0.
- `cleanup/stop.exit`, `cleanup/collision.exit`, and `cleanup/clean.exit` are 0, 2, and 0.
- `linux/hash-source/source-sha256.txt` for the 23 source files. `SHA256SUMS` for this evidence directory. `linux/hash-manifest/` is copied after that walk and is outside `SHA256SUMS`.

## Commands

From the repository root. A new clone changes `RUN`. `OWNER` is the last four characters. Do not use `eef831aa-dba4-4af2-a78b-541bc9e0a305`.

```bash
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a312
OWNER=${RUN:32:4}
cat > /tmp/oo-demo-manifest.json <<EOF
{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"${OWNER}","run_id":"${RUN}","source_id":"public_shop"}
EOF
./deploy/trusted-demo/start.sh --manifest /tmp/oo-demo-manifest.json --public-fixture
./deploy/trusted-demo/start.sh --manifest /tmp/oo-demo-manifest.json --stop
./deploy/trusted-demo/start.sh --manifest /tmp/oo-demo-manifest.json --public-fixture
./deploy/trusted-demo/start.sh --manifest /tmp/oo-demo-manifest.json --clean
```

The second `--public-fixture`, after `--stop` and before `--clean`, exits 2 and does not overwrite volumes. `--clean` removes this manifest's containers, volumes, network, and `/tmp/oo-<owner>-<run_id>`. It does not prune and does not remove another owner's containers.

## Cleanup already run

For this session the resources are already gone. `oo-a312-pg` `902b13fdf96f327549a143e92166f5ffe235bc934b574c2c065318b3ace604dc` was removed by `--stop`. The three `oo-a312-eef831aa-*` volumes, network `oo-a312-eef831aa-net`, and the private tmp were removed by `--clean`. `oo-a300-pg` `a68ce9caf5d6489211e27aaa5f5bbf75e75a883f110129679c76ecec7166bbd1` was still running. Repeating `--clean` with the same manifest is safe when those objects are already absent: stop and clean exit 0 on an empty owned set. Repeating `--public-fixture` still exits 2 while `live/run` remains in this evidence tree.

## Outside this demo

The ops console, Compose, M1, authentication, natural language, the harness, and an enterprise connection remain later work. A new expansion needs its own frozen contract before code. `a313` remains the root lock writer. This child did not edit `Cargo.toml` or `Cargo.lock`.
