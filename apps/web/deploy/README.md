# 前端部署边界

此目录是 G1 的网页部署片段。它不是发布，不修改仓库根目录的 Compose、Caddy 或 CI。

- 生产构建输出是 `apps/web/dist/`。该目录被 Git 忽略。
- 字体从同源 `/fonts` 提供。构建不请求外部字体地址。
- 不写入 `VITE_` 密钥。HTML 不烘入 API 地址。`runtimeConfig.apiBakedIntoHtml` 为 false。
- Storybook 不进入生产包。`runtimeConfig.storybookDeployed` 为 false。
- `runtimeConfig.controlIdentity.origin` 保持 null。五个身份 operation 的状态是 `fixture_verified`，服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`，合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。这是已销毁的本地测试发行方，不是客户 IdP。`live`、`journey`、`release` 为 false，`reachable` 为 false。
- `b01-adapter.mjs` 是可配置的同源适配器。浏览器只访问页面自己的源。`/control/` 转发到部署时填写的身份上游，并用该上游的 CA 校验 TLS。其余路径转发到静态上游。身份上游失败时 `/control/` 返回 502，正文是 `identity_unreachable`，不会改写成 `index.html`。
- 提交的 `b01-adapter.example.json` 把 `pageOrigin` 和两个 upstream 留成 null。已销毁的证明主机 `oo-fe01-edge`、`oo-fe01-proof`、`oo-fe01-idp`、`oo-fe01-pg` 不能作为默认值。
- 适配器拒绝 `rejectUnauthorized: false`，拒绝明文身份上游，并要求 `servername` 与上游主机一致。
- `runtimeConfig.selfbuild.origin` 是 null。B04 仍是提交 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9` 的 `schema_frozen`。
- 性能预算、边缘节点和 FE-28 验收都不在这里声称完成。
