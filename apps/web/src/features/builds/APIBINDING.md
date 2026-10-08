# APIBINDING builds

这些是本目录页面读取的 operationID。状态以 `@open-ontology/sdk-ts` 的登记为准。下表写的 unbound 是编写时的登记，不是服务器捕获。OpenAPI 不存在。`invokeOperation` 不发请求。

| operationID | group | page | state | live | journey | release |
|---|---|---|---|---|---|---|
| B04.builder | B04 | A11 | unbound | false | false | false |
| B04.cards | B04 | A12 | unbound | false | false | false |
| B04.candidates | B04 | A12 | unbound | false | false | false |
| B03.jobs | B03 | A12 | unbound | false | false | false |
| createBuild | selfbuild | A11 | schema_frozen | false | false | false |
| listBuilds | selfbuild | A11 | schema_frozen | false | false | false |
| getBuild | selfbuild | A12 | schema_frozen | false | false | false |
| cancelBuild | selfbuild | A12 | schema_frozen | false | false | false |
| getBuildCards | selfbuild | A12 | schema_frozen | false | false | false |
| getBuildCandidates | selfbuild | A12 | schema_frozen | false | false | false |
| compileBuildPack | selfbuild | A12 | schema_frozen | false | false | false |
| getBuildPack | selfbuild | A12 | schema_frozen | false | false | false |

占位行仍是 unbound，`invokeOperation` 返回 `not_connected`。合同 operation 来自 SDK 的 `SELFBUILD_HTTP`，状态 `schema_frozen`，合同提交 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。`invokeOperation` 返回 `schema_only`，不发请求。运行构建和重试保持禁用。这不是构建完成，也不是 compiled pack 已发布。

页面预检调用固定源 `11e35da6eb0c3588f7cdaea6592daa0e943c4bf8` 的 `prepareCall`。页面没有原始 current 信封，所以传入 null。POST 预检是 `csrf_required`，GET 预检是 `schema_only`，占位 `B04.*` 预检是 `not_precise`。预检不发请求，返回值不含 CSRF。目录上的 `schema_only` 行不是构建完成。

固定源 `61c2d356124f8631fb5fbc490de67c87c00d2964` 的 `callOperation` 可以发送 `listBuilds` 和 `getBuild`。构建页在具有 `project.admin` 的已解析会话上发送 `listBuilds?project_id=`。空列表的 count 是 0，不是完成的构建。详情页只在 `b_` 加 32 位十六进制的地址上发送 `getBuild`。缺失 id 的回执是 403 `forbidden`。`getBuild` 的 HTTP 200 仍是 `invalid_schema`。其余 13 个 operation 不从本页发送：`createBuild`、`cancelBuild`、`getBuildCards`、`getBuildCandidates`、`listReviewItems`、`getReviewItem`、`previewReviewBatch`、`decideReviewItem`、`listClassifications`、`previewClassifications`、`confirmClassifications`、`compileBuildPack`、`getBuildPack`。登记仍是 `schema_frozen`。运行构建和重试保持禁用。
