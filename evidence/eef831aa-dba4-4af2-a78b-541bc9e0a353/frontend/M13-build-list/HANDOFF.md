# M13 交接

G2 已按固定源 `61c2d356124f8631fb5fbc490de67c87c00d2964` 把 `listBuilds` 接到构建页，把 `getBuild` 接到符合构建 id 的详情页。空列表显示 count 0，并写明这不是完成的构建。缺失 id 显示 `forbidden`。`getBuild` 的非文档 200 显示 `invalid_schema`。

下面 13 个 id 仍等待 a373 的真实服务，本卡没有用测试响应代替：`createBuild`、`cancelBuild`、`getBuildCards`、`getBuildCandidates`、`listReviewItems`、`getReviewItem`、`previewReviewBatch`、`decideReviewItem`、`listClassifications`、`previewClassifications`、`confirmClassifications`、`compileBuildPack`、`getBuildPack`。审核页和分类页保持不调用。

登记仍是 `schema_frozen`。`prepareCall` 仍不发送。运行构建和重试保持禁用。`live`、`journey`、`release` 仍是 false。

请在这 13 个 operation 有固定客户端成功解析和 a373 真实响应之后再接审核、分类和 compiled pack。在那之前它们保持未完成。请 G1 继续独占字体、SDK、路由和 eslint 配置。

容器 `oo-a353-node-m13` 已删除。没有清理其他容器。`/tmp/oo-a353-m0` 保留。这一刀不签管理页或前端目标完成。
