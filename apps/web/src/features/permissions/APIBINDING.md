# APIBINDING permissions

这些是本目录页面读取的 operationID。状态以 `@open-ontology/sdk-ts` 的登记为准。下表写的 unbound 是编写时的登记，不是服务器捕获。OpenAPI 不存在。`invokeOperation` 不发请求。

| operationID | group | page | state | live | journey | release |
|---|---|---|---|---|---|---|
| B06.roles | B06 | A26 | unbound | false | false | false |
| B01.projectRoleBinding | B01 | A27 | unbound | false | false | false |
| B06.bindings | B06 | A27 | unbound | false | false | false |
| B06.permissionPreview | B06 | A28 | unbound | false | false | false |
| B05.pack | B05 | A28 | unbound | false | false | false |

live、journey、release 都是 false。上表五个占位没有 fixture_verified。

M9 回执是 `CONTROL_IDENTITY_FIXTURE_RECEIPT`，证据 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M9-b01-roles/raw/browser-receipt.json`。`provedRoles` 是 admin、reviewer、steward、auditor、business_owner，项目 `alpha`，客户集成 `not_claimed`。调用方 JSON、`authorized` 和 claims 不是权限。没有 `project.admin` 的会话停在「没有权限」，不挂载本目录页面。`source-control` 的 `registerSource` 与 `getSourceCatalog`、`release-control` 的 `createRelease` 与 `rollbackRelease`、`getActiveRolePack` 都没有进入 SDK。`createBuild` 仍是 `schema_frozen`。这三份合同可以读，页面不调用它们，也不制造结果。

页面在已解析的管理会话上显示 `getIdentityCurrent` 的主体、初始化状态、项目、角色和能力。管理壳的「退出」调用 `logoutManagementSession`。这两条走同源 `/control/v1/`，目录状态是 `fixture_verified`，`invokeOperation` 仍返回 `not_connected` 且不发请求。引导令牌仍由 `/admin/bootstrap` 的 `consumeBootstrapToken` 提交。`B01.projectRoleBinding` 没有写入接口，应用、保存和预览保持禁用。客户集成 `not_claimed`。live、journey、release 仍是 false。
