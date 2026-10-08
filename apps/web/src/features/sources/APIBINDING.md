# APIBINDING sources

这些是本目录页面读取的 operationID。状态以 `@open-ontology/sdk-ts` 的登记为准。下表写的 unbound 是编写时的登记，不是服务器捕获。OpenAPI 不存在。`invokeOperation` 不发请求。

| operationID | group | page | state | live | journey | release |
|---|---|---|---|---|---|---|
| B02.connectorMatrix | B02 | A04 | unbound | false | false | false |
| B02.sourceConfig | B02 | A04 | unbound | false | false | false |
| B02.inspect | B02 | A05 | unbound | false | false | false |
| B02.catalog | B02 | A05 | unbound | false | false | false |
| B02.dba | B02 | A05 | unbound | false | false | false |
| B02.plan | B02 | A05 | unbound | false | false | false |
| B02.apply | B02 | A05 | unbound | false | false | false |
| B07.secretWrite | B07 | A05 | unbound | false | false | false |
| B02.sourceConfig | B02 | A06 | unbound | false | false | false |
| B03.jobs | B03 | A06 | unbound | false | false | false |

live、journey、release 都是 false。没有 fixture_verified。

`registerSource` 与 `getSourceCatalog` 不在 SDK。source-control 合同还没有进入固定提交。页面不增加这些 id，也不制造目录或导入结果。
