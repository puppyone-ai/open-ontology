# APIBINDING releases

这些是本目录页面读取的 operationID。状态以 `@open-ontology/sdk-ts` 的登记为准。下表写的 unbound 是编写时的登记，不是服务器捕获。OpenAPI 不存在。`invokeOperation` 不发请求。

| operationID | group | page | state | live | journey | release |
|---|---|---|---|---|---|---|
| B05.publish | B05 | A29 | unbound | false | false | false |
| B05.pack | B05 | A29 | unbound | false | false | false |
| B05.gates | B05 | A30 | unbound | false | false | false |
| B05.publish | B05 | A30 | unbound | false | false | false |
| B03.jobs | B03 | A30 | unbound | false | false | false |

live、journey、release 都是 false。没有 fixture_verified。

`createRelease`、`rollbackRelease` 与 `getActiveRolePack` 不在 SDK。release-control 合同还没有进入固定提交。页面不增加这些 id，也不制造发布或回滚结果。
