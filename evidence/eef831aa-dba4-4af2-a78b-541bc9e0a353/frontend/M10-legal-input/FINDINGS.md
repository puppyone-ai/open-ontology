# M10 发现

固定提交 `11e35da6eb0c3588f7cdaea6592daa0e943c4bf8` 的 `prepareCall` 不发请求。B04 登记只接受 origin null、ready false，并且服务提交等于 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。成功登记的原因是 `schema_only`。

管理页把 current 设为 null 后预检。POST 合同 id 显示 `csrf_required`。GET 合同 id 显示 `schema_only`。占位 id，包括 `B04.builder` 和 `B01.projectRoleBinding`，显示 `not_precise`。动作按钮保持禁用。渲染这些页时 `fetch` 次数是 0。

合法 current 信封可以取出 `X-OO-CSRF`。同一信封上 `createBuild` 的预检是 `schema_only`，返回值不含 token。`{authorized:true}` 被拒绝为 `not_current`，POST 预检变成 `csrf_required`。`logoutManagementSession` 与 `consumeBootstrapToken` 的预检是 `not_connected`。

未初始化会话访问 `/admin/builds` 时，守卫打开「初始化」。记录到的请求只有 `GET /control/v1/identity/current`。测试没有提交引导令牌。已初始化会话的「退出」仍调用同源 `logoutManagementSession`。

`schema_only` 目录行还在，它不是构建、审核或分类完成。源和发布 operation 不在 SDK。类型检查、eslint 和 vitest 是 0：33 个文件，74 个测试。G2 没有写字体、SDK、路由或 eslint 配置。

容器 `oo-a353-node-m10` 退出 0，随后删除。没有清理其他容器。
