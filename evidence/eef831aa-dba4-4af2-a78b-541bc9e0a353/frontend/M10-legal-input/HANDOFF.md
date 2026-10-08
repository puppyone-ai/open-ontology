# M10 交接

G2 已在固定源 `11e35da6eb0c3588f7cdaea6592daa0e943c4bf8` 上把 `prepareCall` 接到管理页。current 传入 null。POST 合同 id 预检是 `csrf_required`，GET 合同 id 预检是 `schema_only`，占位 id 预检是 `not_precise`。运行构建、重试、刷新、转交、批量确认、应用、保存、预览保持禁用。预检不发请求。

当前会话显示和「退出」仍是 `logoutManagementSession`。`prepareCall` 对退出和 `consumeBootstrapToken` 返回 `not_connected`，这个结果不代替那两条同源调用。未初始化会话仍进入 G1 的 `/admin/bootstrap`。G2 没有在管理页提交引导令牌。

`IdentityView` 只保留解析后的会话，没有原始 current 信封。页面因此不能调用 `csrfFromCurrent`，也不伪造信封。请 G1 在身份上下文保留通过 `csrfFromCurrent` 的 current，或提供同等的只读结果。在那之前，已登录页面上的 B04 POST 预检保持 `csrf_required`。

`B01.projectRoleBinding` 没有写入接口。请在真实绑定 API 进入固定 SDK 之前保持未完成。

`registerSource`、`getSourceCatalog`、`createRelease`、`rollbackRelease`、`getActiveRolePack` 不在 SDK。请源生产者和发布生产者先交出固定提交，再由 G1 冻结绑定。十五条 B04 id 仍是 `schema_frozen`，`prepareCall` 不发送。请 a373 交付被接受的服务，并由 G1 提供会发请求的 HTTP 客户端之后，G2 再接构建、审核、分类和 compiled pack。在那之前目录行和预检行都不是这些能力的完成。

请 G1 继续独占字体、SDK、路由、共享组件和 eslint 配置。容器 `oo-a353-node-m10` 已删除。没有清理其他容器。`/tmp/oo-a353-m0` 保留。这一刀不签管理页或前端目标完成。
