# M7 交接

G3 `a352` 下次复用同一评审会话时审这次提交。不要新开第四个前端会话。G2 `a353` 以这次提交的 SHA 为准，不要读移动中的工作树。源文件哈希在本目录的 `source-inventory.txt`。

## 可以依赖的接口

- 五个 operation：`startAuthorizationCodeLogin`、`completeAuthorizationCodeCallback`、`getIdentityCurrent`、`logoutManagementSession`、`consumeBootstrapToken`。
- 状态 `fixture_verified`。live、journey、release 都是 false。`reachable` 是 false。
- 服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。
- `CONTRACT.md` blob `fab0fe4e1fc52976af11faedf5653478f28a956d`。OpenAPI 与 schema blob 仍是 089b 公布的内容。ID Token 的 `aud` 必须包含 `client_id`。
- 回执 `CONTROL_IDENTITY_FIXTURE_RECEIPT`，证据文件是 `raw/browser-receipt.json`。客户集成 `not_claimed`。证明过的管理主体是 `fixture-admin`，项目 `alpha`，角色 `admin`。证明过的拒绝主体是 `fixture-reviewer`，角色 `reviewer`，项目 `alpha`。
- 运行时 origin 是 null。证明用的 `https://oo-fe01-m7-edge:8443` 已经随容器销毁，不能写进运行配置。
- `invokeOperation` 不发送请求。这五个 id 返回 `not_connected`。
- 管理守卫读解析后的服务信封。`project.admin` 才挂载模块。目录状态、调用方 JSON、`{"authorized": true}` 都不能打开管理页。无会话时 `adminSessionReady()` 为 false。
- 登录地址只有 `/control/v1/auth/login?return_to=%2Faccount` 和 `%2Fadmin%2Fbootstrap`。引导令牌走登录后的 POST，不放进登录 URL。
- 部署片段是 `apps/web/deploy/b01-adapter.mjs`，导出 `routeKind`、`readAdapterConfig`、`startAdapter`。示例配置的 `pageOrigin` 和两个上游是 null。`/control/` 失败时返回 502 和 `identity_unreachable`，不改写成 `index.html`。
- 字体 `sourceBinding.g2Snapshot` 是 `0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8`，504 个码位，27 个根。U+9F98 只在完整字体里。

## 不要当成已完成

- 不是客户 IdP，不是 `live_verified`。
- steward、auditor、business_owner 没有浏览器证明。
- B04 仍是 `schema_frozen`，合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。
- 占位 `B01.*`、`B04.*` 以及其余 B02–B12 仍是 unbound。
- 总览和其他管理页的业务数据仍是未接入。
- 改钉之后的 63 个测试没有在可写挂载上重跑。改钉之前的 63 个通过，之后的三次 `tsc` 是 0。
- G3 的 `m4-font-oracle.mjs` 仍期待子集长度 376。`slices-oracle.mjs` 仍期待服务提交 35bd。两份都没有改。
- 这一轮没有重跑基础视口矩阵、axe、键盘和减少动效。
- M6 的服务钉 `35bd6b8157cecd89f2ae085b012b4b0137562ddb` 只存在于 `raw/service-35bd/`。当前钉是 a6f。

FE-01、整段旅程和发布都未完成。生产者的 OIDC 修复由 a379 提交，本会话只消费 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。
