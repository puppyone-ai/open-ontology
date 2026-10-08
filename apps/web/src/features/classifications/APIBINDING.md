# APIBINDING classifications

这些是本目录页面读取的 operationID。状态以 `@open-ontology/sdk-ts` 的登记为准。下表写的 unbound 是编写时的登记，不是服务器捕获。OpenAPI 不存在。`invokeOperation` 不发请求。

| operationID | group | page | state | live | journey | release |
|---|---|---|---|---|---|---|
| B04.classifications | B04 | A19 | unbound | false | false | false |
| B06.roles | B06 | A19 | unbound | false | false | false |
| listClassifications | selfbuild | A19 | schema_frozen | false | false | false |
| previewClassifications | selfbuild | A19 | schema_frozen | false | false | false |
| confirmClassifications | selfbuild | A19 | schema_frozen | false | false | false |

占位行仍是 unbound。合同 operation 是 `schema_frozen`，`invokeOperation` 返回 `schema_only`，不发请求。批量确认保持禁用。这不是分类完成。
