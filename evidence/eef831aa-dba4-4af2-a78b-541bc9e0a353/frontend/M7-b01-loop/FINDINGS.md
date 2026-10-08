# M7 发现

固定提交 `9c75d686bf21d0859b833b279b36dc9cc78e9912` 上，五个身份 operation 的目录状态是 `fixture_verified`，服务提交是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。`invokeOperation` 仍返回 `not_connected`。运行时 origin 是 null。

管理壳在已解析会话上显示主体、项目和角色。「退出」调用 `logoutManagementSession`，请求是 `POST /control/v1/auth/logout`，头是 `x-oo-csrf`。`revoked` 之后刷新 `GET /control/v1/identity/current`。401 回到「管理登录」，角色页卸载。403 留下「这一步没有完成。」，角色还在。没有提供者时页面写「当前身份尚未接入。」，不调用 `fetch`，也不显示退出。

初始化仍走 G1 的 `/admin/bootstrap` 和 `consumeBootstrapToken`。G2 没有在角色页复制引导表单，也没有复制登录页或账户页。

`B01.projectRoleBinding`、`B06.roles`、`B06.bindings`、`B06.permissionPreview`、`B05.pack` 仍是 unbound。应用、保存、预览保持禁用。`createBuild` 仍是 `schema_frozen`。构建到审核分类再到 compiled pack 的路径没有接。

M4 三处仍在：连接步骤没有 secret 字段，导航没有 `:id`，`ReviewDesk` 在没有真实候选时不渲染 `ReviewDecisionForm`。

`font-coverage.test.ts` 3 个测试通过。清单 504 个码位。G2 没有写字体、字体清单、`fonts.css` 或 SDK。

eslint 退出码 1。4 个 `no-undef` 都在与 `9c75d686b` 逐字节一致的 `apps/web/deploy/b01-adapter.mjs` 和 `apps/web/deploy/b01-adapter.test.mjs`。自有源文件没有 lint 条目。容器退出码因此是 1。类型检查和 vitest 是 0：31 个文件，66 个测试。

检查时工作区还有 G1 提交 `96489b1cffeeaef3bd8ccfeb4d1d7b3449c757e9`，它改了 deploy 适配器。该提交不在这次归档里，这次容器没有执行它。
