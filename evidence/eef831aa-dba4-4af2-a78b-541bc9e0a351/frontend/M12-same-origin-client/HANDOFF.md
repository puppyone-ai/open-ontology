# M12 交接

G3 `a352` 下次复用同一评审会话时审这次提交。不要新开第四个前端会话。G2 `a353` 以这次提交的 SHA 为准，不要读移动中的工作区。父提交是 `1d5ef4a5184cea0e48b83ea46e2535268a5b1f74`。证明归档基线是 `b05a892deceb93268404eb54348845f65ada9ec0`。源文件哈希在 `source-inventory.txt`。

## 可以依赖的接口

- `registerSameOriginTransport({ origin, pageOrigin, fetchImpl })`。成功时 origin 与 pageOrigin 相同，并且是 https。失败原因是 `origin_rejected`。可选的 `fetchImpl` 只给 Node 证明和单测注入 cookie，产品页面不传它。
- `registeredTransport()` 返回当前传输，没有登记时是 null。`clearSameOriginTransport()` 只给测试清状态。
- `callOperation({ operationId, query, params, body, current })` 会发送 fetch。成功体带 `fetched: true`，以及 `live: false`、`journey: false`、`release: false`、`customerIntegration: "not_claimed"`。失败原因是 `origin_rejected`、`not_precise`、`csrf_required`、`invalid_schema`、`http_error` 或 `transport_failed`。它不返回 `not_connected`。
- 浏览器里传输 origin 必须等于 `window.location.origin`，请求用相对路径，不设置 Origin。Node 里请求用绝对 URL，Origin 只写一次。
- POST 必须带通过 `csrfFromCurrent` 的 current。登录查询拒绝 `bootstrap_token`。`return_to` 沿用合同里的站内路径规则。
- `parseLogoutResult` 与 `parseBootstrapResult` 只接受已提交的成功 schema。
- `invokeOperation`、`prepareCall`、`registerRuntime` 的 M11 行为保留。B04 的 `registerRuntime` 仍只接受 origin null、ready false 和合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。
- 五个身份 operation 仍是 `fixture_verified`。占位 `B01.*`、`B02.*`、`B04.*`、`B07.*` 仍是 unbound。B02 与 B07 没有已提交的 OpenAPI。
- https 页面由 `IdentityProvider` 登记页 origin。logout 和 consume 会重新读取 current，调用方手里的 CSRF 必须和这份 current 一致。

## 这次证明过的范围

- 回执 `raw/client-receipt.json`。五个角色的 login、current、logout，admin 的 bootstrap consume，以及一次错误 CSRF logout 403。
- 服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`，合同 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。
- 这次主机的 config commit 是 `e24a947d3f4aec69e2c00d35e3cd132e42f4712bbdb6753a8cc583295c1536c9`。不要用它替换 `CONTROL_IDENTITY_FIXTURE_RECEIPT.configCommit`。
- m12 主机已经销毁，适配器和 SDK 都会拒绝它们。示例配置的 pageOrigin、listen 和上游仍是 null。

## 不要当成已完成

- 不是客户 IdP，不是 live_verified，不是旅程关闭，不是 FE-01 完成。
- `listBuilds` 的 404 不是 B04 成功。不要在服务验收前把构建、审核、分类、发布或用户问答标成完成。
- 不要把未跟踪的 source、secret、release 树写成 SDK operation。
- 不要改 G2 目录。G2 继续消费这次固定提交里的共享接口。
- 没有新的浏览器截图。M9 的页面截图仍是那一轮的浏览器证据。
- 没有修改 G3 oracle、生产者或 `specs/frontend/execution-status.md`。
