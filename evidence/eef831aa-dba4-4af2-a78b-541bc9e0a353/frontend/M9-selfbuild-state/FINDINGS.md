# M9 发现

固定提交 `e4a694f4826bcaaf78927c4ff4c51b95f5c71bd3` 的 SDK 没有新增 operation。`SELFBUILD_HTTP` 仍是十五条，状态 `schema_frozen`，`live` 为 false。`invokeOperation` 对这十五条返回 `schema_only`，测试桩记录到的 `fetch` 次数是 0。

构建、构建详情、审核、审核决定、分类五页把这十五条和原来的占位 id 一起列出来。占位行仍是 unbound / `not_connected`。动作按钮仍是禁用。审核决定页仍是「没有候选」和空证据面板，没有「接受」按钮。

`registerSource`、`getSourceCatalog`、`createRelease`、`rollbackRelease`、`getActiveRolePack` 的 `bindingById` 是 null。源页面和发布页面没有写入这些 id。`tools/source-control-api` 与 `tools/release-control-api` 不在该固定提交里。

eslint 在这份归档上退出 0。G2 没有改 `eslint.config.mjs`。M8 在 `ac8737ab2` 上的退出 1 仍以那份报告为准，这次没有重跑那个归档。类型检查和 vitest 是 0：31 个文件，67 个测试。字体覆盖 3 个测试通过。G2 没有写字体、SDK、路由、共享组件或 eslint 配置。

容器 `oo-a353-node-m9` 退出 0，随后删除。没有清理其他容器。
