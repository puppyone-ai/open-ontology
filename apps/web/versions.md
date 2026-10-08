# FE-01 版本合同

记录时间：2026-10-08，Asia/Shanghai。此文件写在首次 Linux 安装之前。锁文件此时还不存在。没有既有 `node_modules`、`package.json` 或 `pnpm-lock.yaml` 基线。

版本来自当日 npm `latest` 与 Node.js 官方发布索引，不是记忆中的旧版本。与 `latest` 不一致的条目写明原因。

## 运行时

| 件 | 冻结 | 来源与原因 |
|---|---|---|
| Node.js | 24.21.0 Active LTS（Krypton，2026-09-07） | `https://nodejs.org/dist/index.json`。26.11.0 仍是 Current，Active LTS 开始日为 2026-10-28，今日不用。22.23.3 是 Maintenance LTS。 |
| 镜像 | `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413` | 官方标签 `24.21.0-bookworm` 的 linux/arm64/v8 manifest。索引摘要 `sha256:3d27e5c11e5786e309ec3e03f93ae536eb36e6e5eb3714d5eb3300a36157add0`。 |
| pnpm | 12.10.1 | `https://registry.npmjs.org/pnpm/latest`。`engines.node` 为 `>=18`。 |
| React / React DOM | 19.3.0 | npm latest。MIT。 |
| TypeScript | 6.0.3（2026-04-16） | npm latest 是 7.0.2，但 `typescript-eslint@8.71.1` 的 peer 是 `typescript >=4.8.4 <6.1.0`。6.0.3 是该范围内的最新稳定版。 |
| Vite | 8.3.3 | npm latest。`@vitejs/plugin-react` 6.1.2。 |
| Tailwind CSS | 4.3.3 | npm latest，连同 `@tailwindcss/vite` 4.3.3。 |
| Radix | `radix-ui` 1.7.0 | 单一 shadcn 基础包，不混用另一套 Ariakit/Base。各 `@radix-ui/react-*` 由该包锁定，不另列直接依赖。 |
| Storybook | 10.6.1 | `storybook`、`@storybook/react-vite`、`@storybook/addon-a11y`、`@storybook/addon-themes`。 |
| Vitest | 5.0.3 | peer `vite ^6.4 \|\| ^7 \|\| ^8`。jsdom 30.1.2。 |
| Testing Library | react 16.3.3，dom 10.4.2，jest-dom 7.0.1，user-event 14.6.7 | dom 是 react 包的 peer。 |
| Playwright | 1.63.0 | npm latest。浏览器在 Linux 容器内单独安装，不进宿主。 |
| axe | `@axe-core/playwright` 4.13.0，`axe-core` 4.13.0 | Playwright 绑定的依赖范围是 `axe-core ~4.13.0`。npm latest 的 axe-core 4.14.0 不在该范围内，因此不升。许可证 MPL-2.0。 |
| React Router | 8.4.0 | npm latest。peer `react >=19.2.7`，由 19.3.0 满足。本卡不注册业务路由。 |
| TanStack Query | 5.104.1 | 预期依赖，本卡不发请求。 |
| TanStack Table | 9.2.6 | 预期依赖，本卡不建业务表。 |
| React Hook Form | 7.89.0 | 连同 `@hookform/resolvers` 5.9.1。本卡不建业务表单。 |
| Zod | 4.6.5 | resolver peer `zod ^3.25 \|\| ^4`。 |
| ESLint | 10.12.0 | `@eslint/js` 10.0.1，`typescript-eslint` 8.71.1，`eslint-plugin-react-hooks` 7.1.1，`eslint-plugin-react-refresh` 0.5.7。 |
| 类型包 | `@types/react` 19.3.0，`@types/react-dom` 19.3.0，`@types/node` 24.19.1 | Node 类型钉在 24，不用 latest 的 26.6.4。 |

## 给后续卡预锁、本卡不调用的包

| 包 | 版本 | 处理 |
|---|---|---|
| `clsx` / `tailwind-merge` / `class-variance-authority` | 2.1.1 / 3.7.0 / 0.7.1 | shadcn 组合工具。FE-04 使用。本卡生产包不导入。 |
| `lucide-react` | 1.52.0 | shadcn 默认图标包。本卡不导入。 |
| `tw-animate-css` | 1.4.0 | Tailwind v4 动效。本卡只在减少动效基线里使用 CSS。 |
| `msw` | 2.15.0 | 设计系统建议冻结。npm latest 是 3.0.2，但 Vitest 5.0.3 的 `@vitest/mocker` 要求 `msw ^2.4.9`。2.15.0 是该范围内的最新稳定版。本卡测试不导入，不建后端 mock。 |
| `subset-font` | 2.9.0 | 字体子集工具，开发依赖。 |

## 明确不冻结

| 包 | 原因 |
|---|---|
| `geist` npm 1.7.2 | peer 依赖 Next.js。本产品是 Vite。字体字节来自 GitHub `vercel/geist-font` v1.7.2。 |
| TypeScript 7.0.2 | 与当前 typescript-eslint peer 冲突。 |
| Recharts、React Flow、ELK | 设计系统标为待审或图表阶段再固定。本卡不改图。 |
| Next.js、Google Fonts CDN | 规范禁止。 |

## 构建脚本

pnpm 12 默认拒绝依赖生命周期脚本。合同只允许 `esbuild` 执行安装脚本，因为它是 Vite 的平台二进制。`msw` 的安装脚本明确设为 false：本卡不启用 mock worker。不使用 `dangerouslyAllowAllBuilds`。

## 字体

字体在工具链冒烟通过之后生成。来源、子集码位和文件哈希写在 `apps/web/design/font-manifest.json`。

| 文件 | 字节 | sha256 |
|---|---|---|
| Geist-wght.woff2 | 69652 | `a369fcf5628ea2aa4e1b9e2ec6a5b3624e365bda588e1f0f2f12b564f728fbb8` |
| GeistMono-Regular.woff2 | 50356 | `e4507fb4fb5f832fbbb6c06aea4206274ba3083007f23fa8cbc0e87a10acf95b` |
| NotoSansSC-300-subset.woff2 | 40388 | `f74d2d7f70a4c14984baad08201b0d6ffff5329b1843b6a20f31858d054be137` |
| NotoSansSC-400-subset.woff2 | 40792 | `20670d9d6a534542dd804fe844b65fc880153874504023b79deee252f848a67e` |
| NotoSansSC-500-subset.woff2 | 41044 | `11d1d010925340b94827773728c6a86cfa604e2f8a1a595206c2f986bf281535` |
| NotoSansSC-full.woff2 | 7762252 | `bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b` |

Geist 可变字体低于拉丁预载 120KiB。中文子集和完整回退按实测保留，没有为了预算删字。许可证在 `apps/web/public/fonts/licenses/`。

## 安装后状态

上面“锁文件此时还不存在”是安装前合同原文，保留不改。随后的事实是：

- 首次 Linux 安装没有既有锁。尝试 1 到 3 失败并已保留日志。尝试 4 生成当前锁。
- `pnpm-lock.yaml` sha256 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。之后的类型检查、构建和浏览器容器都核对过这个值，锁没有被改写。
- Linux arm64 Node 24.21.0 上，类型检查、lint、6 个单元测试、对比度报告、Vite 生产构建和 Storybook 构建退出码都是 0。
- 生产 JS `dist/assets/index-BApyONEH.js` 为 226525 字节，gzip 70115 字节。CSS gzip 4117 字节。构建产物不进入 Git。
- Playwright Chromium 153.0.8010.12（build v1243）已下载到任务私有目录。页面截图、axe、键盘和减少动效没有执行。原因见证据 FINDINGS。

## M1 公共接口（2026-10-08）

- 锁文件没有改写。sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。容器 `oo-fe01-node-m1` 在冻结安装前后都打印了这个值，`oo-fe01-node-m1c` 复用同一安装前再次核对。
- 没有新增 registry 依赖。`@open-ontology/sdk-ts` 由 Vite、Vitest 和 TypeScript 的路径别名指向 `packages/sdk-ts/src/index.ts`，不写入 workspace 依赖。
- 生产包现在包含 react-router 和 radix dialog。Linux 实测 `dist/assets/index-BFuR9aus.js` 为 320270 字节，gzip 100357 字节；`dist/assets/index-Bqdf60lJ.css` 为 21518 字节，gzip 5241 字节。上一节的 70115 只描述基础页产物 `index-BApyONEH.js`。
- 同一容器里类型检查、lint、22 个单元测试和 Vite 生产构建退出码都是 0。两次更早的测试失败日志保留在 M1 证据里。浏览器仍未执行。对比度报告、Storybook 构建和字体没有重跑。

## M2 用户路由（2026-10-08）

- 锁文件没有改写。sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。容器 `oo-fe01-node-m2` 在类型检查前和构建后都打印了这个值。没有安装，没有新增 registry 依赖。
- Linux arm64 Node v24.21.0 上，类型检查、lint、28 个单元测试和 Vite 生产构建退出码都是 0。浏览器没有执行。对比度报告、Storybook 构建和字体没有重跑。
- 生产 JS `dist/assets/index-CDiTiiKo.js` 为 325770 字节，gzip 101884 字节。CSS `dist/assets/index-CFLc_wNN.css` 为 21556 字节，gzip 5254 字节。上一节的 100357 只描述 M1 产物 `index-BFuR9aus.js`。

## M3 Token 补层（2026-10-08）

- 锁文件没有改写。sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。容器 `oo-fe01-node-m3` 在检查前后都打印了这个值。没有安装，没有新增 registry 依赖。
- primitive 色值没有改。`input-error` 引用已测的 `danger-text`，浅色对白底 6.507:1，深色对 `#171C24` 8.714:1。`surface-selected` 引用已测的 `accent-bg`，正文对比浅色 12.877:1，深色 12.968:1。
- 只重跑了类型检查、改动文件的 lint，以及 `contrast.test.ts` 的 4 个测试，退出码都是 0。没有重跑整套基础测试、字体、Storybook、生产构建或浏览器。

## M4 字体子集与生产减少动效（2026-10-08）

- 锁文件没有改写。sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。容器 `oo-fe01-node-m4` 在字体生成前后都打印了这个值，`oo-fe01-node-m4b` 在复跑浏览器前再次核对。没有安装，没有新增 registry 依赖。
- Noto 子集从 180 个码位扩到 376 个，包含 G3 列出的 98 个路由缺口和当时的 G1 文案。U+9F98 仍只由完整字体覆盖。完整字体 sha256 仍是 `bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b`，7762252 字节。Geist 与两份许可证字节没有改。已通过的色值没有改。
- Linux arm64 Node v24.21.0 上，字体生成、`font-coverage.test.ts` 的 3 个测试、类型检查、改动文件的 lint 和生产构建退出码都是 0。第一次 Playwright 里减少动效通过，字体请求因把 304 当成失败而失败。第二次只重跑字体请求，1 个测试通过。没有重跑 19 张基础页截图、整套单元测试、对比度报告或 Storybook。
- 生产 JS `dist/assets/index-ppVptCPZ.js` 为 325770 字节，gzip 101884 字节。CSS `dist/assets/index-BNkIVGpf.css` 为 25712 字节，gzip 5911 字节。上一节的 5254 只描述 M3 之前的 `index-CFLc_wNN.css`。构建产物不进入 Git。

## M5 管理模块守卫与 B04 schema（2026-10-08）

- 锁文件没有改写。sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。没有新增 registry 依赖。
- Noto 子集从 376 个码位扩到 506 个，加入固定提交 `d416874eee65e5f30ad9858cef4429f608066c7a` 的 19 个管理目录文案。U+9F98 仍不在子集里。完整字体 sha256 仍是 `bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b`，7762252 字节。Geist 与两份许可证没有改。色值没有改。
- 未登录访问 `/admin` 和 `/admin/sources` 仍落到管理登录。模块的 `load()` 在 `getIdentityCurrent` 变成 `live_verified` 之前不会被调用。当前该状态是 `schema_frozen`。
- B01 的五个 HTTP operation 和 B04 的十五个 HTTP operation 只登记为 `schema_frozen`。`invokeOperation` 返回 `schema_only`，不调用 fetch。占位的 `B01.*` 和 `B04.*` 仍是 unbound。两个 origin 都是 null。
- 最终生产 JS `dist/assets/index-NShXIz5d.js` 为 344710 字节，gzip 108337 字节。CSS `dist/assets/index-DYD7xV9p.css` 为 28241 字节，gzip 6278 字节。`index.html` 不预加载管理页分块。构建产物不进入 Git。

## M6 测试发行方会话（2026-10-08）

- 锁文件没有改写。sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。没有新增 registry 依赖。
- 五个控制身份 operation 从 `schema_frozen` 改为 `fixture_verified`。服务提交是 `35bd6b8157cecd89f2ae085b012b4b0137562ddb`，合同提交仍是 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。这是销毁后的本地测试发行方，不是客户 IdP。`live`、`journey`、`release` 仍是 false。运行时 origin 仍是 null，`reachable` 是 false。
- `invokeOperation` 对这五个 id 返回 `not_connected`，不调用 fetch。B04 的十五个 HTTP operation 仍是 `schema_frozen`。占位的 `B01.*` 和 `B04.*` 仍是 unbound。
- 管理页是否挂载看解析后的会话和 `project.admin`，不看目录状态。没有目录记录时 `adminSessionReady()` 仍是 false。
- 目录改动之后的生产 JS `dist/assets/index-DI6Dg6fG.js` 为 342199 字节，`gzip -c` 为 105906 字节。CSS 仍是 `dist/assets/index-DYD7xV9p.css`，28241 字节，`gzip -c` 为 6276 字节。浏览器截图来自改目录之前的 `index-BvHQ7Fvw.js`。构建产物不进入 Git。

