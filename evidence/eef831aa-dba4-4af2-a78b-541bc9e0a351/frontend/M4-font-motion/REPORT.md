# M4 字体子集与生产减少动效

角色：G1，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a351`。这是回应 G3 `eef831aa-dba4-4af2-a78b-541bc9e0a352` 对 M1–M3 的 **APPROVE_LIMITED**。FE-01 未完成。live、完整旅程、发布都是 false。本报告不自签验收。

写入本文件时的 HEAD 是 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。`866cd4e2ba2571d73428f6303e175d2ef85c78ca`、`a08b3576969cf06c097a4769bce8f9a4758a81a2` 和 `e61c0fd98b6c8b09c9c8e2db3a441aafaf952909` 都是祖先。审阅目标是包含本文件的那一次 owned-path 提交。

## 改了什么

1. `apps/web/design/e2e/foundation.visual.spec.ts` 的减少动效测试改为打开生产 `/`。页面落到 `/login`。`[data-motion-sample]` 数量是 0。登录页 `.oo-panel` 和账户页「菜单」按钮在 `prefers-reduced-motion: reduce` 下的 `transition-property` 是 `none`，时长换算后是 0 毫秒。已发布的 `.oo-motion` 规则仍是 `transition-duration: 0.01ms`，换算后与 `toBeCloseTo(0.01, 5)` 比较。`foundation.css` 没有改。没有新增 Foundation 路由，没有把问题框改成可发送。

2. Noto 子集从 180 个码位扩到 376 个。来源是 G1 目录里的现有文案，加上 G3 在 `866cd4e2` 上列出的 98 个缺口码位。缺口文件是 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a352/frontend/M1-M3/raw/subset-gap.txt`，码位副本在 `apps/web/design/subset-sources.json`。罕见字 U+9F98 仍只在完整回退字体里。完整字体、Geist、两份许可证和锁的字节没有改。

G2 在这次运行期间提交的管理目录没有扫进子集，也没有被修改。

## 已证明

镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0。pnpm 12.10.1。两次容器都是 4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、私有网桥。CapEff 是 0。第二次容器的 cgroup `memory.max` 是 4294967296，`cpu.max` 是 `100000 100000`。没有安装 npm 包。锁前后都是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

| 阶段 | 结果 |
|---|---|
| 字体生成 | `raw/font-build.log`，退出 0。`subsetPoints=376`，完整字体 7762252 字节 |
| Vitest `font-coverage.test.ts` | `raw/vitest.log`，1 个文件，3 个测试，23:05:26，841ms |
| `tsc -p tsconfig.json` | `raw/exits.txt` 的 `types=0` |
| eslint，只含本次改动的三个文件 | `raw/exits.txt` 的 `lint=0` |
| Vite 生产构建 | `raw/build.log`，退出 0 |
| 包检查 | `raw/bundle.log`。JS `index-ppVptCPZ.js` 325770 字节，gzip 101884。CSS `index-BNkIVGpf.css` 25712 字节，gzip 5911。HTML 763 字节，gzip 382 |
| apt 与 ldd | 两次退出都是 0。浏览器库装在容器里，缓存在任务私有 home |
| Playwright 第一次 | `raw/playwright-attempt-1.log`。减少动效测试通过。字体请求测试失败，原因见 FINDINGS |
| Playwright 第二次 | `raw/playwright-retry.log`。只跑字体请求测试，1 个通过，1.5s。容器 `oo-fe01-node-m4b` 退出 0 |

子集 sha256：

| 文件 | 字节 | sha256 |
|---|---|---|
| `NotoSansSC-300-subset.woff2` | 52212 | `7991e0f16b08889d67a002a6c2b47eca29d4f99d19b1d53b8ba833dc10a9271d` |
| `NotoSansSC-400-subset.woff2` | 53300 | `5957ae0d7734cd1c0452cfc75e8a00ee5b0afe47377aebabcd2607793d09a6e6` |
| `NotoSansSC-500-subset.woff2` | 53892 | `fb09039bf3aaf67d60f553d41c1f2223f9247cc67ea914b6c9a3eee559e7b3b6` |
| `NotoSansSC-full.woff2` | 7762252 | `bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b` |

Geist 与两份许可证的 sha256 与 M3 之前的已提交文件相同，见 `raw/fonts-after.txt`。

浏览器截图都大于 2000 字节：

- `screenshots/production-reduced-motion-1280x800-light.png`，30166 字节，登录页
- `screenshots/production-account-1280x800-light.png`，39221 字节，账户页，可见「菜单」和「账户」
- `screenshots/production-conversation-disabled-1280x800-light.png`，43411 字节，「发送」和「问题」保持禁用

第二次字体请求共 39 条，状态只有 200 和 304。路径只有 Geist 与三份 Noto 子集。没有 `NotoSansSC-full.woff2`。地址都在 `http://127.0.0.1:4173`。

## 未证明

没有重跑 19 张基础页截图、整套单元测试、对比度报告、Storybook 或全部字体基线。B01–B12 仍是 unbound。真实登录和 API 仍缺失。这次浏览器只覆盖减少动效和普通路由的字体请求。
