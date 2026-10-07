# M1 公共接口交接

本提交冻结 G2 可以依赖的公共类型。FE-01 未完成。浏览器证明仍 withheld，由 G3 在基础提交上独立做。G2 `eef831aa-dba4-4af2-a78b-541bc9e0a353` 可以在本提交进入历史之后开始，但只能消费下面这些接口。G2 不改锁、Token、样式、SDK、路由和 G1 功能目录。

## 稳定导入

- `@open-ontology/sdk-ts` 指向 `packages/sdk-ts/src/index.ts`。导出 `OPERATIONS`、`bindingById`、`operationsByGroup`、`invokeOperation`、`parsePackMetadata`、`parseGatewayError`、`GATEWAY_SOURCE`。`invokeOperation` 不发 HTTP。`live`、`journey`、`release` 在类型里就是 false。
- 唯一 `schema_frozen` 操作：`gateway.packMetadata.get`，`GET /v1/pack/metadata`。schema sha256 `f77d55d45bfe86e237f167881a415f67304e9dc11251474c4155a3ac2a9353e3`。错误信封 schema sha256 `0839b3e4798ece2ceaddee80ae2c71d54fd491f919d2025616884b97cbc8b5f2`。
- 共享组件桶：`apps/web/src/components/shared/index.ts`。C18 用 CSS 条和精确表格，不引入 Recharts。条形长度的可见句子写明是近似。
- 模块登记：`apps/web/src/app/module-registration.ts`。`ModuleRegistration` 要求 `layout: "admin"` 和 `load(): Promise<{ default: ComponentType }>`。`ADMIN_MODULES` 是空的只读数组。G2 写自己的 `module.ts`，不改路由器。G1 以后把固定模块追加进这个数组。
- 路由表：`apps/web/src/app/router.tsx` 的 `appRoutes`。生产路径是 `/` → `/login`，`/login`，`/admin` → `/admin/login`，`/admin/login`，`/admin/bootstrap`，`/forbidden`，`/unsupported`，以及 `*`。根 `errorElement` 是 `RouteFailure`。这些页面写明能力未接入，依赖 B01，没有密码框，没有本地账号，没有假 OIDC。
- `safeReturnTo` 只保留同源路径，并去掉 query 和 hash。
- 偏好键只允许 `oo.pref.theme`、`oo.pref.density`、`oo.pref.lang`。值域是 `light|dark|system`、`compact|comfortable|roomy`、`zh-CN|en`。这不是身份存储。基础页自己的单选仍然不写 localStorage。主题和密度同时落在 `<html data-theme>` 和 `<html data-density>`。
- 未接入文案是「此能力尚未接入。」无权限文案是「当前身份没有执行此操作的权限。」

## 锁与忽略规则

`pnpm-lock.yaml` sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。G1 仍是唯一锁写入者。新的 registry 依赖必须先改合同再改锁。

`.gitignore` 的 JavaScript 追加只覆盖 `node_modules/`、`apps/web/dist/`、`apps/web/storybook-static/`、`apps/web/test-results/`、`apps/web/playwright-report/`、`apps/web/coverage/`。不要把证据、规格或其他工作区文件加进去。

## 下一刀

G1 下一固定里程碑是用户侧页面：对话、知识、账户。后端缺失时页面只显示未接入和依赖项。不造消息、不造会话、不把管理对象名露给普通身份。那一刀单独提交，并再跑聚焦的 Linux 类型、lint、测试和构建。

G3 不从本会话派生。它继续审 `2c032402ee3b58801f30dbbcefddee8733e383b1` 和真实浏览器。本提交的浏览器状态是 withheld。
