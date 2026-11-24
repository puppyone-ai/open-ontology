# Super Ontology app: npm dependency delta (D85, plan F-S19)

| | |
|---|---|
| Scope | `apps/super-ontology-desktop` and the `pnpm-lock.yaml` rows it adds. Not a Rust change and not part of the default OO artifact. |
| New direct packages | `electron` 44.11.1 (runtime of the app), `electron-builder` 26.18.1 (build tool). `vite` 8.3.3, `vitest` 5.0.3 and `@vitejs/plugin-react` 6.1.2 are the versions `apps/web` already pins. |
| Records | [electron](super-ontology-app-electron-audit.json), [electron-builder](super-ontology-app-electron-builder-audit.json), plus one record per remaining direct package (the tables below link each), the [WTFPL build-time review](super-ontology-app-wtfpl-build-time-review.json) and the [renderer license ledger](super-ontology-app-renderer-licenses.md). |
| Workspace setting | `pnpm-workspace.yaml` `allowBuilds`: `electron: true` (downloads and verifies the Electron binary), `electron-winstaller: false` (Windows installer, never run). |
| Runtime npm dependencies of the app | `lucide-react`, `react`, `react-dom`, `react-markdown`, `remark-gfm` (app manifest) and `intl-messageformat` with its three `@formatjs` dependencies (through `@open-ontology/design-system`). Each has its own record, linked in the table below; their transitive licenses are listed in the renderer license ledger. |

## Seven-day rule

| Package | Published (UTC) | Adopted (UTC) | Elapsed |
|---|---|---|---|
| electron 44.11.1 | 2026-11-11T01:57:01.959Z | 2026-11-19T11:50:00Z | 8 d 9 h |
| electron-builder 26.18.1 | 2026-10-26T12:34:16.990Z | 2026-11-19T11:50:00Z | 24 d |

Newer 44.x, 43.x and 45.x releases exist but are younger than seven days. Electron 41 (the line the source app used) has open advisories (one high, several moderate; patched from 42.11.16) and is not adopted.

### Rows added by the renderer and tooling records (adopted 2026-11-19T14:05:47Z)

All times UTC. Every row passes the seven-day rule; the youngest is vite 8.3.3 (44 d). Registry digests match `pnpm-lock.yaml` for every package. No package has an install script.

Runtime (ships in the DMG):

| Package | Published | Adopted | Elapsed | License |
|---|---|---|---|---|
| [lucide-react 1.52.0](super-ontology-app-lucide-react-audit.json) | 2026-10-04T08:17:01.328Z | 2026-11-19T14:05:47Z | 46 d 5 h | ISC AND MIT |
| [react 19.3.0](super-ontology-app-react-audit.json) | 2026-09-09T17:21:30.071Z | 2026-11-19T14:05:47Z | 70 d 20 h | MIT |
| [react-dom 19.3.0](super-ontology-app-react-dom-audit.json) | 2026-09-09T17:17:39.744Z | 2026-11-19T14:05:47Z | 70 d 20 h | MIT |
| [react-markdown 10.1.0](super-ontology-app-react-markdown-audit.json) | 2025-03-07T10:32:53.222Z | 2026-11-19T14:05:47Z | 622 d 3 h | MIT |
| [remark-gfm 4.0.1](super-ontology-app-remark-gfm-audit.json) | 2025-02-10T12:43:10.735Z | 2026-11-19T14:05:47Z | 647 d 1 h | MIT |
| [intl-messageformat 11.2.2](super-ontology-app-intl-messageformat-audit.json) | 2026-04-24T18:59:41.776Z | 2026-11-19T14:05:47Z | 208 d 19 h | BSD-3-Clause |
| [@formatjs/fast-memoize 3.1.2](super-ontology-app-formatjs__fast-memoize-audit.json) | 2026-04-13T20:19:18.837Z | 2026-11-19T14:05:47Z | 219 d 17 h | MIT |
| [@formatjs/icu-messageformat-parser 3.5.5](super-ontology-app-formatjs__icu-messageformat-parser-audit.json) | 2026-04-24T18:59:22.128Z | 2026-11-19T14:05:47Z | 208 d 19 h | MIT |
| [@formatjs/icu-skeleton-parser 2.1.5](super-ontology-app-formatjs__icu-skeleton-parser-audit.json) | 2026-04-24T18:59:14.438Z | 2026-11-19T14:05:47Z | 208 d 19 h | MIT |

Build, lint and test time (not in the DMG):

| Package | Published | Adopted | Elapsed | License |
|---|---|---|---|---|
| [@eslint/js 10.0.1](super-ontology-app-eslint__js-audit.json) | 2026-02-06T22:34:56.290Z | 2026-11-19T14:05:47Z | 285 d 15 h | MIT |
| [@playwright/test 1.63.0](super-ontology-app-playwright__test-audit.json) | 2026-09-04T22:44:00.304Z | 2026-11-19T14:05:47Z | 75 d 15 h | Apache-2.0 |
| [@testing-library/dom 10.4.2](super-ontology-app-testing-library__dom-audit.json) | 2026-09-13T18:08:36.116Z | 2026-11-19T14:05:47Z | 66 d 19 h | MIT |
| [@testing-library/jest-dom 7.0.1](super-ontology-app-testing-library__jest-dom-audit.json) | 2026-08-09T23:44:33.598Z | 2026-11-19T14:05:47Z | 101 d 14 h | MIT |
| [@testing-library/react 16.3.3](super-ontology-app-testing-library__react-audit.json) | 2026-08-27T17:41:18.735Z | 2026-11-19T14:05:47Z | 83 d 20 h | MIT |
| [@testing-library/user-event 14.6.7](super-ontology-app-testing-library__user-event-audit.json) | 2026-09-02T01:56:32.109Z | 2026-11-19T14:05:47Z | 78 d 12 h | MIT |
| [@types/node 24.19.1](super-ontology-app-types__node-audit.json) | 2026-10-01T22:38:02.894Z | 2026-11-19T14:05:47Z | 48 d 15 h | MIT |
| [@types/react 19.3.0](super-ontology-app-types__react-audit.json) | 2026-09-09T18:08:49.750Z | 2026-11-19T14:05:47Z | 70 d 19 h | MIT |
| [@types/react-dom 19.3.0](super-ontology-app-types__react-dom-audit.json) | 2026-09-09T18:07:51.886Z | 2026-11-19T14:05:47Z | 70 d 19 h | MIT |
| [@vitejs/plugin-react 6.1.2](super-ontology-app-vitejs__plugin-react-audit.json) | 2026-10-05T10:08:27.143Z | 2026-11-19T14:05:47Z | 45 d 3 h | MIT |
| [ajv 8.20.0](super-ontology-app-ajv-audit.json) | 2026-04-24T15:22:16.529Z | 2026-11-19T14:05:47Z | 208 d 22 h | MIT |
| [eslint 10.12.0](super-ontology-app-eslint-audit.json) | 2026-10-02T20:08:19.213Z | 2026-11-19T14:05:47Z | 47 d 17 h | MIT |
| [eslint-plugin-react-hooks 7.1.1](super-ontology-app-eslint-plugin-react-hooks-audit.json) | 2026-04-17T18:03:19.591Z | 2026-11-19T14:05:47Z | 215 d 20 h | MIT |
| [eslint-plugin-react-refresh 0.5.7](super-ontology-app-eslint-plugin-react-refresh-audit.json) | 2026-09-14T16:12:50.394Z | 2026-11-19T14:05:47Z | 65 d 21 h | MIT |
| [jsdom 30.1.2](super-ontology-app-jsdom-audit.json) | 2026-10-04T09:15:00.333Z | 2026-11-19T14:05:47Z | 46 d 4 h | MIT |
| [typescript 6.0.3](super-ontology-app-typescript-audit.json) | 2026-04-16T23:38:27.905Z | 2026-11-19T14:05:47Z | 216 d 14 h | Apache-2.0 |
| [typescript-eslint 8.71.1](super-ontology-app-typescript-eslint-audit.json) | 2026-10-05T17:08:41.691Z | 2026-11-19T14:05:47Z | 44 d 20 h | MIT |
| [vite 8.3.3](super-ontology-app-vite-audit.json) | 2026-10-06T04:10:19.326Z | 2026-11-19T14:05:47Z | 44 d 9 h | MIT |
| [vitest 5.0.3](super-ontology-app-vitest-audit.json) | 2026-09-30T11:30:42.429Z | 2026-11-19T14:05:47Z | 50 d 2 h | MIT |

The three WTFPL packages (`truncate-utf8-bytes`, `sanitize-filename`, `utf8-byte-length`) are covered by one [dedicated review](super-ontology-app-wtfpl-build-time-review.json) (11 d to 3 y old at adoption; all pass).

## Licenses of the installed tree (335 packages for this importer, build and test time)

MIT 257, ISC 37, BSD-3-Clause 11, Apache-2.0 9, BlueOak-1.0.0 8, BSD-2-Clause 4, MPL-2.0 2, (MIT OR CC0-1.0) 2, 0BSD 1, Python-2.0 1, and three WTFPL-licensed packages.

Items for owner review (all build-time or test-time; none is copied into the DMG; verified by listing app.asar of a pack of this tree, see the WTFPL review):

- `truncate-utf8-bytes` 1.0.2 (WTFPL only), `sanitize-filename` 1.6.5 (WTFPL OR ISC, ISC relied on), `utf8-byte-length` 1.0.5 (WTFPL OR MIT, MIT relied on): reached through `electron-builder` > `app-builder-lib` > `sanitize-filename`. WTFPL is not OSI-approved; it is permissive and is accepted here only because the build tool is not part of any shipped artifact. Replacing `electron-builder` removes it.
- `lightningcss` 1.33.0 and `lightningcss-darwin-arm64` 1.33.0 (MPL-2.0), `argparse` 2.0.1 (Python-2.0): reached through `vite` / `vitest`; already in the lock through `apps/web`. File-level weak copyleft with no modification, no distribution.

## Advisories

`pnpm audit --json` on 2026-11-19, rows whose path starts at this importer:

- electron 44.11.1: none.
- electron-builder tree: `minimatch` 10.2.6 and 9.0.9, moderate denial of service on pathological glob patterns; patterns come from the repository's own `electron-builder.yml`.
- `vitest` > `jsdom` > `undici` 8.11.2 (several moderate and high): `jsdom` is an auto-installed optional peer of `vitest`; the app's tests run in the Node environment and never load it, it is not shipped, and `apps/web` carries the same row.

None of these is exploitable in the shipped artifact.

## Shipped renderer bundle: license list

108 packages are packed into `app.asar` (and bundled into `dist/renderer`): MIT 105, ISC 1, ISC AND MIT 1, BSD-3-Clause 1; first-party `@open-ontology/design-system` is Apache-2.0. The per-package list with LICENSE digests is [super-ontology-app-renderer-licenses.md](super-ontology-app-renderer-licenses.md). Direct packages: react 19.3.0 (MIT, with scheduler 0.28.0), react-dom 19.3.0 (MIT), lucide-react 1.52.0 (ISC AND MIT), react-markdown 10.1.0 (MIT, 81 transitive packages: MIT and one ISC), remark-gfm 4.0.1 (MIT, 68 transitive), intl-messageformat 11.2.2 (BSD-3-Clause) with @formatjs/fast-memoize 3.1.2, @formatjs/icu-messageformat-parser 3.5.5 and @formatjs/icu-skeleton-parser 2.1.5 (MIT).

## Advisories for the new rows

`pnpm audit --json` and `pnpm audit --prod --json` on 2026-11-19; the filtered evidence is [evidence/super-ontology-app-pnpm-audit-20261119.json](evidence/super-ontology-app-pnpm-audit-20261119.json). The production audit has no advisory with a path from this app (its one finding, zod 4.6.5, is under `apps/web`). Open advisories on the build and test side, none shipped and none reachable:

- `zod` 4.6.5 (high, memory exhaustion on oversized collections; patched in 4.6.6) through `eslint-plugin-react-hooks`: validates the plugin's own rule options at lint time.
- `undici` 8.x (several high and moderate; patched in 8.12.2) through `jsdom`: `jsdom` is now a direct devDependency, but vitest runs with `environment: "node"` and nothing imports it, so undici never loads. This corrects the earlier statement above that `jsdom` is only an optional peer.
- `minimatch` (moderate) through eslint, typescript-eslint and the builder tree: repository-owned patterns only.

The records for the packages above carry the value `resolved-before-adoption` because the record shape accepts only that or `no-known-advisories`; the advisories are not patched, they are judged not reachable, and each such record says so in `disposition_note`.

## Owner-review flags

1. WTFPL (not OSI-approved) in the builder tree: three packages, build-time only, absent from `app.asar` (verified). `truncate-utf8-bytes` 1.0.2 ships no LICENSE file. See the [review](super-ontology-app-wtfpl-build-time-review.json).
2. `intl-messageformat` (BSD-3-Clause) and three `@formatjs` packages ship in the renderer but are missing from the generated `THIRD_PARTY_NOTICES.md` (see the renderer license ledger).
3. electron-builder packs the whole production `node_modules` closure (108 entries, about 3,100 files) into `app.asar` although vite already bundles the renderer; this is redundant weight and widens what the notice must cover. Not changed here.
4. Open dev-time advisories (zod high, undici high) with a patched version available; a lockfile bump or override would clear them.
5. The two earlier records (electron, electron-builder) use a `sha512-` registry value as `digest`, `null` for `default_features_disabled`, and flat LICENSE paths, so `governance/qa_record.py audited_record_errors` rejects them. The new records use a tarball `sha256:` digest and per-package license folders and pass it. The older two were left as they were.
