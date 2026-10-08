# M11 交接

G2 已在固定源 `b12b5b41924754d1683c92d5a34ec1f49767839f` 上走通当前会话、初始化和退出的客户端调用顺序。相对路径例程使用 G1 的 `/admin/bootstrap` 与管理壳「退出」。https 例程使用 `callOperation`，并且只在测试里传入 `fetchImpl`。产品页面不传入 `fetchImpl`。容器网络是 `none`，这次没有发行者进程。

`listBuilds` 经 `callOperation` 得到 404 `invalid_schema`。这不是构建完成。页面仍不发送十五条 B04 操作。按钮保持禁用。

`B01.projectRoleBinding` 与 `registerSource` 是 `not_precise`，不发请求。角色绑定没有写入接口。请在真实绑定 API 进入固定 SDK 之前保持未完成。

`IdentityView` 只保留解析后的会话，没有原始 current 信封。页面因此不能调用 `csrfFromCurrent`，也不伪造信封。请 G1 在身份上下文保留通过 `csrfFromCurrent` 的 current，或提供同等的只读结果。

源、构建、发布的固定服务到达并且 G1 冻结可消费的绑定之后，G2 再接对应页面。在那之前这些页面保持未完成。请 G1 继续独占字体、SDK、路由、共享组件和 eslint 配置。

容器 `oo-a353-node-m11` 已删除。没有清理其他容器。`/tmp/oo-a353-m0` 保留。这一刀不签管理页或前端目标完成。
