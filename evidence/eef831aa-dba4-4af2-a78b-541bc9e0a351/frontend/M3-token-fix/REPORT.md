# M3 Token 补层报告

角色：G1，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a351`。这是回应 G3 `eef831aa-dba4-4af2-a78b-541bc9e0a352` 对基础提交 `2c032402ee3b58801f30dbbcefddee8733e383b1` 的 **APPROVE_LIMITED**。FE-01 未完成。live、完整旅程、发布都是 false。本报告不自签验收。

写入本文件时的 HEAD 是 `f01120f07230e99ae004e997ecce7fb82f1d4418`。`a08b3576969cf06c097a4769bce8f9a4758a81a2` 和 `2c032402ee3b58801f30dbbcefddee8733e383b1` 都是祖先。审阅目标是包含本文件的那一次 owned-path 提交。

## G3 要求的两项

1. `apps/web/design/e2e/foundation.visual.spec.ts` 的减少动效断言改为把 `transitionDuration` 换算成毫秒，再和 0.01 比较。`apps/web/src/styles/foundation.css` 里的 `transition-duration: 0.01ms` 没有改。本里程碑没有开浏览器。Chrome 字符串 `1e-05s` 的换算由 G3 的通过运行记录过；作者的断言现在和那次独立测试同一算法。

2. `apps/web/design/tokens.json` 增加 `semantic` 和 `component`。原始十六进制仍只在 `primitive`。已通过对比度的色值没有改。`button-primary-bg` 引用 `blue-action`，`button-primary-fg` 引用 `on-action`，`input-border` 引用同名 primitive。`input-error` 引用已测的 `danger-text`。`surface-selected` 引用已测的 `accent-bg`。没有为这两个名字新增色值。

## 已证明

Linux arm64 容器 `oo-fe01-node-m3` 使用 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。容器退出码 0。日志 `raw/container-m3.log`。容器已按名字删除。

| 阶段 | 结果 |
|---|---|
| `tsc` | `raw/types.exit` 0 |
| eslint，只含 `contrast.ts` 与 `contrast.test.ts` | `raw/lint.exit` 0 |
| Vitest `src/styles/contrast.test.ts` | `raw/test.exit` 0。1 个文件，4 个测试。开始 22:11:38，耗时 703ms |
| 别名对比度 | `raw/measured.json`。`input-error` 浅色 6.507、深色 8.714，门槛 4.5。`surface-selected` 浅色 12.877、深色 12.968，门槛 4.5 |

锁前后都是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

`tokens.css` 用变量别名暴露 `--oo-surface-selected`、`--oo-button-primary-bg`、`--oo-button-primary-fg`、`--oo-input-error`。基础页按钮规则改为读取按钮组件变量。这两个变量分别是 `var(--oo-accent)` 和 `var(--oo-on-accent)`。

## 未证明

没有重跑整套基础单元测试、字体、对比度报告脚本、Storybook、生产构建或浏览器。减少动效断言没有在本里程碑的浏览器里再跑。按钮别名没有新截图。B01–B12 仍是 unbound。真实登录和 API 仍缺失。
