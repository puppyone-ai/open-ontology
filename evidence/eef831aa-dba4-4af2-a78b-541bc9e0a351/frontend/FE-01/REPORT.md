# FE-01 / G1 基础里程碑报告

角色：G1，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a351`。这是基础切片的部分交付，不是 FE-01 完成，也不是前端总任务完成。

父提交：`96fc054598a916737452e74bb9ddaa46180057bb`。审阅目标是包含本文件的那一次 owned-path 提交。G3 `eef831aa-dba4-4af2-a78b-541bc9e0a352` 尚未启动，本报告不自签验收。

## 范围

交付 Web 工具链、锁、自托管字体、三层 Token、主题、密度和一页静态基线。页面声明自己不是产品功能，没有登录、审核、发布或会话。`/` 与 `/admin` 仍留给后续路由组装。没有业务路由、共享业务组件、SDK 实现、Rust、CI 或部署改动。

## 已证明

Linux arm64 容器使用 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`，Node v24.21.0，pnpm 12.10.1。资源为 4GiB、1 CPU、`--cap-drop ALL`、私有 bridge、私有 home 与 pnpm store。

| 阶段 | 容器退出记录 | 结果 |
|---|---|---|
| 字体子集与完整回退 | `raw/fonts.exit` | 0。180 个子集码点，完整回退 7762252 字节 |
| `tsc` | `raw/types.exit` | 0 |
| eslint | `raw/lint.exit` | 0 |
| Vitest | `raw/test.exit` | 0。6 个测试，3 个文件 |
| Token 对比度 | `raw/contrast-report.exit` | 0。`contrast-tokens.json` 中要求的配对没有 `pass: false` |
| Vite 生产构建 | `raw/build.exit` | 0 |
| 包体扫描 | `raw/bundle.exit` | 0。产物不含 react-router、TanStack、msw、radix-ui 或 Google Fonts |
| Storybook 构建 | `raw/storybook.exit` | 0 |
| Playwright 浏览器下载 | `raw/playwright-install.exit` | 0。Chromium 153.0.8010.12，build v1243 |

生产 JS `index-BApyONEH.js` 226525 字节，容器内 gzip 70115 字节。CSS gzip 4117 字节。Geist 可变字体 69652 字节。这些数字来自 `raw/bundle.log`。

锁 sha256：`73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。后续容器核对过，没有改写。

Rust 1.94.0 身份冒烟在编辑前完成，`raw/rust-smoke.exit` 为 0。没有跑工作区 `cargo test` 或 Clippy，因为本里程碑不改 Rust。

`packages/sdk-ts` 只有包名和 `exports`。没有 `src`。引导容器里的 `tsc` 得到预期的 TS18003，`sdk_tsc_exit=1`，不计入 Web 失败。日志在 `raw/bootstrap-attempt-4.log`。

## 未证明

浏览器没有打开过这页。截图 0 张。axe、键盘、减少动效和页面内字体网络断言都没有执行。UI verified 只覆盖单元测试和生产构建。live、旅程、发布都是 false。B01–B12 未绑定。

## 绑定

见 `apps/web/design/APIBINDING.md`。state `unbound`，live `false`，method/path `not_applicable`。
