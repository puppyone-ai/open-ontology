# 前端部署边界

此目录是 G1 的网页部署片段。它不是发布，不修改仓库根目录的 Compose、Caddy 或 CI。

- 生产构建输出是 `apps/web/dist/`。该目录被 Git 忽略。
- 字体从同源 `/fonts` 提供。构建不请求外部字体地址。
- 不写入 `VITE_` 密钥。HTML 不烘入 API 地址。`runtimeConfig.apiBakedIntoHtml` 为 false。
- Storybook 不进入生产包。`runtimeConfig.storybookDeployed` 为 false。
- `runtimeConfig.controlIdentity.origin` 保持 null。五个身份 operation 的状态是 `fixture_verified`，服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`，合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。这是已销毁的本地测试发行方，不是客户 IdP。`live`、`journey`、`release` 为 false，`reachable` 为 false。
- `b01-adapter.mjs` 是可配置的同源适配器。浏览器只访问页面自己的源。部署时必须填 TLS 证书、https 身份上游和静态上游。`pageOrigin` 保持 null。
- `/control` 转发到身份上游，并用该上游的 CA 校验 TLS。身份上游失败时返回 502，正文是 `identity_unreachable`。
- `/v1` 和 `/api` 在填写了网关上游时转发到网关。没有网关上游时返回 502，正文是 `api_unbound`。当前没有可绑定的网关服务，示例里的网关上游是 null。
- `/.well-known`、`/oauth`、`/oidc` 返回 502，正文是 `oidc_unbound`。登录仍走 `/control/v1/auth/login`，不把 OIDC 发现页改写成 `index.html`。
- WebSocket 升级和 `/ws` 在没有 ws 上游时返回 502，正文是 `ws_unbound`。当前没有可绑定的 ws 服务。
- 页面深链只在静态上游对 GET 或 HEAD 返回 404、且请求接受 HTML 时，回退一次 `/index.html`。字体、`/assets` 和带文件后缀的资源保持上游状态码，404 不改写成页面。
- 上述 `api_unbound`、`oidc_unbound`、`ws_unbound` 都是纯文本，不是 `index.html`。
- 提交的 `b01-adapter.example.json` 把上游留成 null。null 不是正在运行的边缘，也不是部署完成。已销毁的证明主机不能作为默认值。路由策略是固定的，不能把 API、OIDC 或 WebSocket 改到静态页。
- 适配器拒绝 `rejectUnauthorized: false`，拒绝明文身份上游，并要求 `servername` 与上游主机一致。
- `runtimeConfig.selfbuild.origin` 是 null。B04 仍是提交 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9` 的 `schema_frozen`。
- 性能预算、边缘节点和 FE-28 验收都不在这里声称完成。
