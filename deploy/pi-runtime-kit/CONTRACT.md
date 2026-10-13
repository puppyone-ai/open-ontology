# Pi runtime kit

Owner `eef831aa-dba4-4af2-a78b-541bc9e0a406`. Consumer `eef831aa-dba4-4af2-a78b-541bc9e0a397`.

Contract name: `oo.pi-runtime-kit/v1`.

This kit installs a real Pi 1.0.4 process and probes that process. It does not accept SP10-W04, SP10-W05, the parent phase, MP-03, or the short-term goal. `qa_complete` stays false. The probe is an installation check, not a business answer.

## Locked inputs

Pi `@earendil-works/pi-coding-agent` 1.0.4, git `7c10bd4337495ee613f2224843ecdf349b80d1df`, from `testing/spikes/p09-pi-rpc/PIN.md`. Node is v22.23.3 linux-arm64. The pin hashes are in `pins.env`. Nothing in this kit upgrades those versions.

The image is `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Rust inside that image is 1.94.0. The repo `rust-toolchain.toml` channel is not used. `oo` and `query-pg` are debug binaries from a clean source commit recorded in the handoff. Profile is `debug`.

## Prefix

Docker volume `oo-a406-eef831aa-pi-kit`, mounted at `/opt/pi-runtime-kit`.

| Path | Role |
| --- | --- |
| `install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js` | Pi executable |
| `node/bin/node` | Node runtime |
| `bin/oo` and `node/bin/oo` | `oo` CLI, same bytes |
| `bin/query-pg` | `query-pg` |
| `node/bin/bash` | GNU bash copied from the image |
| `node/bin/rg` | ripgrep 14.1.1 official aarch64 binary |
| `lib/libpg_query.a` | static library linked into `query-pg` |
| `fonts/NotoSansCJKsc-Regular.otf` | Noto Sans CJK SC Sans2.004 |
| `manifest.sha256` | sha256 of the deliverable files |
| `NOTICE` and `licenses/` | license texts that belong with the binaries |

The binaries, the font, and `node_modules` stay in the volume. Git holds the recipe, the verifier, and the evidence.

## Installation probe

`linux/probe-inside.sh` runs `kit probe` with network `none`.

1. `node cli.js --version` must print `1.0.4`.
2. RPC `get_state` with id `init-1` must return success.
3. A local fixture on `127.0.0.1` asks Pi to read `/knowledge/README.md`.
4. The next fixture response asks Pi to run `<absolute oo> --help` with the bash tool.
5. Later fixture responses stop. The prompt is not a business question and contains no SQL.

The child environment is cleared. The only variables are `HOME`, `PATH` (the Node `bin` directory), `TERM=dumb`, and `PI_CODING_AGENT_DIR`. argv includes `--no-mcp` and does not include the public marker. The marker `public-proxy-marker` is written only as `apiKey` in `models.json`. No real key is passed to Pi. No PostgreSQL is started. The tool image does not download at probe time.

Success is the contract tool events for that read and that bash help. Unknown Pi event types fail the probe. A failed probe does not write the handoff.

## Consume

Mount volume `oo-a406-eef831aa-pi-kit` read-only at `/opt/pi-runtime-kit` on the same image. `KIT-HANDOFF.json` carries the absolute paths, sha256, argv, static library, and CJK ranges. The agent-entry fields are `pi_executable`, `pi_runtime`, `oo_bin`, and `query_pg_bin`.

## Bounds

One builder for this task, including the install and the probe, never both at once. 6 GiB, 1 CPU, `cap-drop ALL`, no new privileges, no host network, no Docker socket, no host home, no shared cargo cache. Global Rust builders stay at or below 10. This task does not start PostgreSQL.
