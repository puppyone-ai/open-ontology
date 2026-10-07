# M2 用户路由报告

角色：G1，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a351`。这是公共接口提交 `e61c0fd98b6c8b09c9c8e2db3a441aafaf952909` 之后的用户路由切片。FE-01 未完成。A01–A20 未勾选。live、完整旅程、发布都是 false。本报告不自签验收。

写入本文件时的 HEAD 是 `942cbc43bba19f593192b9578f83e5f285823f85`。`e61c0fd98b6c8b09c9c8e2db3a441aafaf952909` 是它的祖先。三个网关 blob 与 SDK 常量一致，本切片没有改 SDK。审阅目标是包含本文件的那一次 owned-path 提交。

## 范围

U02 `/` 保持未认证时进入 `/login`。未挂载的 `ChatHomePage` 是对话未接入表面，由单元测试直接渲染。U03 `/conversations/:conversationId`、U04 `/conversations/:conversationId/files`、U05 `/knowledge`、U06 `/account` 进入生产路由，只显示未接入。没有历史、消息、下载、目录名、凭证或退出动作。普通导航只有「对话」「知识」「账户」。

没有新的 operationID。B01–B12 仍是 unbound。`GET /v1/pack/metadata` 仍是上一提交里的 `schema_frozen`，live false。

## 已证明

Linux arm64 容器 `oo-fe01-node-m2` 使用 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。复用已有 `node_modules`，没有安装。容器退出码 0。日志 `raw/container-m2.log`。检查记录 `raw/containers.txt`。容器已按名字删除，`raw/cleanup.txt`。

| 阶段 | 退出记录 | 结果 |
|---|---|---|
| `tsc` app / node / sdk | `raw/types.exit`、`types-node.exit`、`types-sdk.exit` | 0 |
| eslint | `raw/lint.exit` | 0 |
| Vitest | `raw/test.exit` | 0。7 个文件，28 个测试。开始 22:00:56，耗时 6.38s |
| Vite 生产构建 | `raw/build.exit` | 0 |
| 包体计量 | `raw/bundle.exit` | 0 |

锁在构建前后都是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

生产包：`index-CDiTiiKo.js` raw 325770、gzip 101884；`index-CFLc_wNN.css` raw 21556、gzip 5254；`index.html` raw 763、gzip 379。M1 的 gzip 100357 不再描述这个包。构建产物不进 Git。

## 未证明

浏览器没有打开这些路由。截图、axe、键盘和减少动效都没有执行。对比度报告、Storybook 构建和字体没有重跑。

`ChatHomePage` 不在生产路由上，所以生产包里的 `/` 仍是登录重定向。单元测试覆盖了这个未挂载表面。不能把它说成已经上线的对话页。

Rust 没有改，也没有重跑 Rust 冒烟。
