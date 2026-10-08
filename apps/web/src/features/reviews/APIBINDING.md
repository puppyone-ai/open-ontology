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

页面预检调用 `prepareCall`，current 为 null。`listReviewItems` 与 `getReviewItem` 预检是 `schema_only`。`previewReviewBatch` 与 `decideReviewItem` 预检是 `csrf_required`。占位 `B04.reviews` 与 `B04.candidates` 预检是 `not_precise`。预检不发请求。`schema_only` 目录行不是审核完成。
