# Super Ontology app: npm dependency delta (D85, plan F-S19)

| | |
|---|---|
| Scope | `apps/super-ontology-desktop` and the `pnpm-lock.yaml` rows it adds. Not a Rust change and not part of the default OO artifact. |
| New direct packages | `electron` 44.11.1 (runtime of the app), `electron-builder` 26.18.1 (build tool). `vite` 8.3.3, `vitest` 5.0.3 and `@vitejs/plugin-react` 6.1.2 are the versions `apps/web` already pins. |
| Records | [electron](super-ontology-app-electron-audit.json), [electron-builder](super-ontology-app-electron-builder-audit.json) |
| Workspace setting | `pnpm-workspace.yaml` `allowBuilds`: `electron: true` (downloads and verifies the Electron binary), `electron-winstaller: false` (Windows installer, never run). |
| Runtime npm dependencies of the app | None. The main process uses Node.js and Electron only; renderer packages are added by the renderer lane and need their own rows. |

## Seven-day rule

| Package | Published (UTC) | Adopted (UTC) | Elapsed |
|---|---|---|---|
| electron 44.11.1 | 2026-11-11T01:57:01.959Z | 2026-11-19T11:50:00Z | 8 d 9 h |
| electron-builder 26.18.1 | 2026-10-26T12:34:16.990Z | 2026-11-19T11:50:00Z | 24 d |

Newer 44.x, 43.x and 45.x releases exist but are younger than seven days. Electron 41 (the line the source app used) has open advisories (one high, several moderate; patched from 42.11.16) and is not adopted.

## Licenses of the installed tree (335 packages for this importer, build and test time)

MIT 257, ISC 37, BSD-3-Clause 11, Apache-2.0 9, BlueOak-1.0.0 8, BSD-2-Clause 4, MPL-2.0 2, (MIT OR CC0-1.0) 2, 0BSD 1, Python-2.0 1, and three WTFPL-licensed packages.

Items for owner review (all build-time or test-time; none is copied into the DMG):

- `truncate-utf8-bytes` 1.0.2 (WTFPL only), `sanitize-filename` 1.6.5 (WTFPL OR ISC, ISC relied on), `utf8-byte-length` 1.0.5 (WTFPL OR MIT, MIT relied on): reached through `electron-builder` > `app-builder-lib` > `sanitize-filename`. WTFPL is not OSI-approved; it is permissive and is accepted here only because the build tool is not part of any shipped artifact. Replacing `electron-builder` removes it.
- `lightningcss` 1.33.0 and `lightningcss-darwin-arm64` 1.33.0 (MPL-2.0), `argparse` 2.0.1 (Python-2.0): reached through `vite` / `vitest`; already in the lock through `apps/web`. File-level weak copyleft with no modification, no distribution.

## Advisories

`pnpm audit --json` on 2026-11-19, rows whose path starts at this importer:

- electron 44.11.1: none.
- electron-builder tree: `minimatch` 10.2.6 and 9.0.9, moderate denial of service on pathological glob patterns; patterns come from the repository's own `electron-builder.yml`.
- `vitest` > `jsdom` > `undici` 8.11.2 (several moderate and high): `jsdom` is an auto-installed optional peer of `vitest`; the app's tests run in the Node environment and never load it, it is not shipped, and `apps/web` carries the same row.

None of these is exploitable in the shipped artifact.
