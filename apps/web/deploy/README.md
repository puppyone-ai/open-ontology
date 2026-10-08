# 前端部署边界

此目录只说明当前前端构建的边界。它不是发布，不拥有仓库根目录的 Compose、Caddy 或 CI。

- 生产构建输出是 `apps/web/dist/`。该目录被 Git 忽略，不随此说明提交。
- 字体从同源 `/fonts` 提供。构建不请求外部字体地址。
- 不写入 `VITE_` 密钥。HTML 不烘入 API 地址。`runtimeConfig.apiBakedIntoHtml` 为 false。
- Storybook 构建产物不复制进生产包。`runtimeConfig.storybookDeployed` 为 false。
- `runtimeConfig.controlIdentity.origin` 是 null。控制身份只登记了提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b` 的冻结 schema，没有可调用的服务地址。
- `runtimeConfig.selfbuild.origin` 是 null。B04 只登记了提交 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9` 的冻结 schema。这里不生成 CSRF，也不把钉住的身份 JSON 当成登录成功。
- 性能预算、边缘节点和 FE-28 验收都不在这里声称完成。
