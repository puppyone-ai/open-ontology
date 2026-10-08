# M13 发现

固定提交 `61c2d356124f8631fb5fbc490de67c87c00d2964` 的 `parseBuildList` 接受空 `builds`。count 是 0。`published`、`active` 为 false，`compiled_pack` 为 `not_published`。页面在 `project.admin` 的项目上调用 `callOperation({ operationId: "listBuilds", query: { project_id } })`。

没有同源传输时，列表结果是 `origin_rejected`，不制造构建行。`运行构建` 点击后仍不发请求。

详情地址 `b_` 加 32 个 0 的 `getBuild` 显示 `http_error forbidden`。纯文本 HTTP 200 显示 `invalid_schema`。`secret-token` 不发送 `getBuild`。`重试` 保持禁用。S0 到 S10 仍是「没有运行记录」。

审核和分类的页面不调用 `callOperation`。这 13 个 operation 没有用测试响应充当完成。

类型检查、eslint 和 vitest 是 0：35 个文件，84 个测试。G2 没有写字体、SDK、路由或 eslint 配置。

容器 `oo-a353-node-m13` 退出 0，随后删除。没有清理其他容器。
