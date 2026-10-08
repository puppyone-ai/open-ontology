# M8 发现

固定提交 `ac8737ab2e9ef3d6de41ce2527f487222ce52373` 的 `CONTROL_IDENTITY_FIXTURE_RECEIPT` 记录五个角色：admin、reviewer、steward、auditor、business_owner，项目 `alpha`，客户集成 `not_claimed`。live、journey、release 都是 false。证据文件是 G1 的 `M9-b01-roles/raw/browser-receipt.json`。

没有 `project.admin` 的四个角色访问 `/admin/permissions/roles` 时，守卫显示「没有权限」，不挂载角色页，不调用模块 `load`，请求只有 `GET /control/v1/identity/current`。`{"authorized":true}` 加上 claims，以及在合法信封上多一个 `claims` 键，都解析不成会话，停在「管理登录」。角色页不渲染 `claims`。身份绑定页写明不列出人、组或 claims。

`registerSource`、`getSourceCatalog`、`createRelease`、`rollbackRelease`、`getActiveRolePack` 不在 SDK 里。`createBuild` 仍是 `schema_frozen`，`invokeOperation` 返回 `schema_only`。工作区里的 source-control 和 release-control 目录未跟踪，第一次 `git archive` 退出 128。它们不是这次固定源。

eslint 在这份固定归档上退出 1。7 个 `no-undef` 都在 G1 的 `b01-adapter.mjs` 和 `b01-adapter.test.mjs`。类型检查和 vitest 是 0：31 个文件，67 个测试。字体覆盖 3 个测试通过。G2 没有写字体、SDK、路由或 eslint 配置。

`e4a694f48` 已在当前分支给 Node 全局补了 `Buffer` 和 `URL`。这次容器归档早于那次改动，没有把那次改动算进退出码。
