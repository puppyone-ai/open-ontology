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

页面在已解析的管理会话上显示 `getIdentityCurrent` 的主体、初始化状态、项目、角色和能力。管理壳的「退出」调用 `logoutManagementSession`。这两条走同源 `/control/v1/`，目录状态是 `fixture_verified`，`invokeOperation` 仍返回 `not_connected` 且不发请求。引导令牌仍由 `/admin/bootstrap` 的 `consumeBootstrapToken` 提交。`B01.projectRoleBinding` 没有写入接口，应用、保存和预览保持禁用。客户集成 `not_claimed`。live、journey、release 仍是 false。
