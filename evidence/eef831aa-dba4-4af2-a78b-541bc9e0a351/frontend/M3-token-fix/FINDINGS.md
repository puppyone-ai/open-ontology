# M3 Token 补层发现

G3 的基础复审裁决是 APPROVE_LIMITED。需要作者改的两点都在本提交里。G3 的测试目录 `apps/web/tests/` 没有改。`specs/frontend/execution-status.md` 仍由 Codex 写入，本会话没有追加裁决。

## 减少动效

作者断言原先要求字符串 `0.01ms`。G3 在 Chrome for Testing 153.0.8010.12 上看到 `1e-05s`。现在的断言先看单位是不是 `ms`，否则按秒乘 1000，再 `toBeCloseTo(0.01, 5)`。样式时长保持 `0.01ms`。

## 颜色

`input-error` 没有单独的 primitive。它引用 `danger-text`。这组已经按 `danger/surface` 测过：浅色 `#B42332` 对 `#FFFFFF` 为 6.507:1，深色 `#FF9EA7` 对 `#171C24` 为 8.714:1。

`surface-selected` 没有单独的 primitive。它引用 `accent-bg`。这组已经按 `text/accent-bg` 测过：浅色 `#202733` 对 `#E7EEFB` 为 12.877:1，深色 `#EDF0F5` 对 `#1A2744` 为 12.968:1。

`semantic` 和 `component` 的 JSON 里没有 `#`。深色 `on-action` 仍是 `#101318`。

`table-row-height` 和 `chat-message-max-width` 引用已有尺度，不是颜色。`review-evidence-bg` 引用 `neutral-subtle`，对应已测的 `text/subtle`。

## 没有重跑的部分

基础页整套 Vitest、字体、Storybook、生产构建和浏览器都没有重跑。G3 已经审过那一页。这次只跑了类型检查、两个样式测试文件的 lint，以及对比度单测。
