# M8 交接

G2 已按固定源 `ac8737ab2e9ef3d6de41ce2527f487222ce52373` 把管理路由接到 M9 回执。五个 `provedRoles` 里，只有带 `project.admin` 的 admin 会挂载管理模块。reviewer、steward、auditor、business_owner 在角色页路径上显示「没有权限」。调用方 JSON、`authorized` 和 claims 不成为权限。

当前会话显示和「退出」仍是 M7 的 `logoutManagementSession`。初始化仍由 G1 的 `/admin/bootstrap` 调用 `consumeBootstrapToken`。登录页和账户页仍是 G1。

`B01.projectRoleBinding` 没有写入接口。应用、保存、预览保持禁用。五个身份 operation 的 `invokeOperation` 仍是 `not_connected`。运行时 origin 仍是 null。

`tools/source-control-api` 与 `tools/release-control-api` 还没有进入固定提交。`registerSource`、`getSourceCatalog`、`createRelease`、`rollbackRelease`、`getActiveRolePack` 在 SDK 中不存在。`createBuild` 仍是 `schema_frozen`。在这些合同成为固定提交并且 G1 冻结 SDK 绑定之前，G2 不接源目录、构建、审核分类、compiled pack 或发布，也不制造结果。

请 G1 继续独占字体、SDK、路由和 eslint 配置。这次归档上的 7 个 `no-undef` 在部署适配器里。分支上的 `e4a694f48` 已另改配置，G2 没有执行那个提交，也没有改那份文件。

容器 `oo-a353-node-m8` 已删除。没有清理其他容器。`/tmp/oo-a353-m0` 保留。这一刀不签管理页或前端目标完成。
