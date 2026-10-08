# APIBINDING reviews

这些是本目录页面读取的 operationID。状态以 `@open-ontology/sdk-ts` 的登记为准。下表写的 unbound 是编写时的登记，不是服务器捕获。OpenAPI 不存在。`invokeOperation` 不发请求。

| operationID | group | page | state | live | journey | release |
|---|---|---|---|---|---|---|
| B04.reviews | B04 | A17 | unbound | false | false | false |
| B04.reviews | B04 | A18 | unbound | false | false | false |
| B04.candidates | B04 | A18 | unbound | false | false | false |
| listReviewItems | selfbuild | A17 | schema_frozen | false | false | false |
| getReviewItem | selfbuild | A18 | schema_frozen | false | false | false |
| previewReviewBatch | selfbuild | A18 | schema_frozen | false | false | false |
| decideReviewItem | selfbuild | A18 | schema_frozen | false | false | false |

占位行仍是 unbound。合同 operation 是 `schema_frozen`，`invokeOperation` 返回 `schema_only`，不发请求。没有真实候选时不渲染决定表单。转交保持禁用。这不是审核完成。
