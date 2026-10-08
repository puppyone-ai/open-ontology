# SP13-W01 findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a388`.

## Delivery

The stdio server is `tools/mcp-gateway`. Protocol types, handshake, discover, child-process transport, tool calls, and resource reads go through rmcp 3.5.0. Gateway HTTP is a loopback client in `src/http.rs`. Results are the Gateway JSON body. The live test compares that body with a separate TCP client.

`linux/launch.exit` is 0. `linux/marker.txt` is `completed=1`. Production `cargo tree` has no `oo-gateway` and no `pg_query`.

## License and MSRV risk

Accepted child crate: rmcp 3.5.0, registry checksum `fae7019994ae0fe4ada40b732f798f3ff26f0f04facb1477f1bf37eb4f18a2d3`.

Upstream MSRV 1.88. Proof toolchain 1.94.0 satisfies that MSRV. The repo `rust-toolchain.toml` channel is 1.98.1 and was not edited. The builder overlaid a read-only toolchain file with channel 1.94.0 so rustup did not download 1.98.1.

License: Apache-2.0 on the rmcp manifest. The upstream LICENSE text leaves unrelicensed contributions under MIT and non-specification docs under CC-BY-4.0. This card ships no those docs. No advisory scan was run. A later `cargo-audit` could still report an issue in rmcp 3.5.0 or a transitive crate. That scan is not claimed here.

`oo-gateway` cannot be an optional dev-dependency on Cargo 1.94. It is an optional normal dependency gated by feature `live`. The production build does not enable `live`.

## Failures before the green run

`linux/failed-attempts.md` records attempts 1 through 10. They are harness and compile failures. The green differential is the later launcher, not those attempts.

Attempt 10 is the run that exited in about 20 seconds. Phase reached `clippy-protocol_exit=0`, `test-protocol_exit=0`, `build-gateway_exit=0`, then `clippy-live_exit=101`. The only diagnostic was `clippy::assertions_on_constants` on `assert!(cfg!(target_os = "linux"))`. The check is now a const assertion. Raw stderr is `linux/attempt-10-clippy-live.stderr`. Attempt 9 raw stderr is `linux/attempt-9-clippy-live.stderr` (`E0507` moving `instructions` out of the peer-info `Arc`).

The green phase reused the cargo and target volumes from those attempts, then the exit-0 trap removed them. See `CLEANUP.md`.

## Observations on the green evidence

- `linux/lock-generated.txt` says the child lock was already present. `cargo generate-lockfile` did not run on the green attempt. `linux/lock-generate.stdout` is empty because that log is from a later skip. The checksum above is the lock that was fetched with `--locked`.
- `CHILD-LOCK-APPROVAL.md` had pointed at `linux/proof/lock-generated.txt`. That path was never written. The approval row now names `linux/lock-generated.txt`.
- The fresh container's rustup home is the image, so Clippy was downloaded again (`linux/clippy-component.stderr`: `info: downloading component clippy`) and exited 0 in the same launcher. That is the reviewed component command, not a separate install campaign.
- In-container root hashes cover `Cargo.lock` and `Cargo.toml` only (`linux/root-inside-before.txt` equals `linux/root-inside-after.txt`). The host launcher also hashed `rust-toolchain.toml` before and after (`linux/root-before.txt`, `linux/root-after.txt`). All three match the values in `REPORT.md`.
- Owned text, log, stdout, and stderr files were scanned for `ooct_`, `oost_`, and private-key markers after redaction. The scan found no remaining token or key material. `linux/server-san.txt` has the certificate subject and DNS SAN only.

## Left open on purpose

Remote OAuth and remote export stay unimplemented. The server instructions and the startup line say so. SP13-W02, AT-MCP-02, and AT-MCP-03 are not passed.

Docs/08 names with no live HTTP route are not tools of this binary.

Gateway `POST /v1/sql` still returns 501 `SQL_UNSUPPORTED`. This card returns that envelope. It does not add SQL execution.

Parent SP13 acceptance and the development ledger are for Codex. This packet does not edit the ledger and does not mark the parent done.
