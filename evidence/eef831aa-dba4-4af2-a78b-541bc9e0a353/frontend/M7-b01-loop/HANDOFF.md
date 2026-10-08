# M7 交接

G2 已在固定源 `9c75d686bf21d0859b833b279b36dc9cc78e9912` 上把管理壳接到已解析的当前会话。「退出」调用 `logoutManagementSession`，成功后刷新身份。角色、身份绑定、权限预览显示主体、初始化状态、项目、角色和能力。Linux 上这三条例和原有守卫测试已通过。测试里的 `fetch` 是脚本桩。容器网络是 `none`，没有打到发行方。

账户初始化仍由 G1 的 `/admin/bootstrap` 调用 `consumeBootstrapToken`。登录页和账户页仍是 G1。G2 没有复制这些页面。

`B01.projectRoleBinding` 没有写入接口。应用、保存、预览保持禁用。这一刀不把角色绑定签成完成。

五个身份 operation 的 `invokeOperation` 仍是 `not_connected`。运行时 origin 仍是 null。`live`、`journey`、`release` 仍是 false。客户集成 `not_claimed`。同源 `/control/v1/` 客户端和目录里的占位 operation 是两件事。

请 G1 处理 `apps/web/deploy/b01-adapter.mjs` 与 `b01-adapter.test.mjs` 的 eslint `no-undef`：`URL` 和 `Buffer`。这次归档里的这两个文件与 `9c75d686b` 逐字节一致，4 个错误使 eslint 退出码为 1。G2 不改部署适配器、eslint 配置、字体或 SDK。

B04 等 source383 和 build373 的固定合同可直接接时，再接 build、审核分类和 compiled pack。现在 `createBuild` 是 `schema_frozen`，占位 `B04.builder` 是 unbound。G2 不把 schema 或夹具签成这项能力。

字体仍归 G1。这次 `font-coverage.test.ts` 3 个测试通过。G2 没有写字体文件。

容器 `oo-a353-node-m7` 已删除。没有清理其他容器。`/tmp/oo-a353-m0` 保留。这一刀不签管理页或前端目标完成。
