# M9 交接

G3 `a352` 下次复用同一评审会话时审这次提交。不要新开第四个前端会话，也不要重启仍在运行的 G3。G2 `a353` 以这次提交的 SHA 为准，不要读移动中的工作树。源文件哈希在本目录的 `source-inventory.txt`。父提交是 `3be6264f18811b2ad88eedf986c342b15bdd0a16`。

## 可以依赖的接口

- 五个 operation：`startAuthorizationCodeLogin`、`completeAuthorizationCodeCallback`、`getIdentityCurrent`、`logoutManagementSession`、`consumeBootstrapToken`。
- 状态 `fixture_verified`。live、journey、release 都是 false。`reachable` 是 false。运行时 origin 是 null。
- 服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。
- 回执 `CONTROL_IDENTITY_FIXTURE_RECEIPT`。证据文件是 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M9-b01-roles/raw/browser-receipt.json`。客户集成 `not_claimed`。
- `provedSubject` 仍是 `fixture-admin`，`provedRole` 仍是 `admin`，`provedDeniedSubject` 仍是 `fixture-reviewer`，`provedDeniedRole` 仍是 `reviewer`。新增 `provedRoles`：admin、reviewer、steward、auditor、business_owner。`configCommit` 是 `ec801f46456d62b23679c05e87f45e23cedd710bd3aedb1992d69477a2748152`。项目都是 `alpha`。
- 账户页显示签名会话中的主体、初始化状态、项目、角色和能力。有 `project.admin` 才出现「管理入口」。没有该能力时显示「没有管理能力。」`/admin` 在无此能力时显示「没有权限」。
- `adminSessionReady()` 无参数时仍是 false。传入解析后的会话时，只有 `adminGateDecision` 为 `allow` 才是 true。目录状态和 `{"authorized": true}` 不能打开管理页。
- `invokeOperation` 不发送请求。五个身份 id 返回 `not_connected`。十五个 B04 id 返回 `schema_only`。
- 部署片段仍是 `apps/web/deploy/b01-adapter.mjs`。部署者在运行时填写 TLS、https 身份上游、`caFile`、与主机名一致的 `servername`，以及静态上游。`pageOrigin` 保持 null。没有网关上游时 `/v1` 与 `/api` 返回 `api_unbound`。OIDC 发现路径返回 `oidc_unbound`。没有 ws 上游时返回 `ws_unbound`。这些正文不是 `index.html`。
- 已销毁主机包括 `oo-fe01-m9-edge`、`oo-fe01-m9-proof`、`oo-fe01-m9-idp`、`oo-fe01-m9-pg`，以及更早的 m7 和最初四个名字。它们不能写进运行配置。

## 不要当成已完成

- 不是客户 IdP，不是 `live_verified`，不是整段旅程，不是发布，也不是 FE-01 完成。
- B04 仍是 `schema_frozen`，合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。实现提交 `2d7c2a5cd` 还在，但 a373 写明二进制未交付、E56 未验收。不要按那个提交伪造接通。
- 占位 `B01.*`、`B04.*` 以及其余 B02–B12 仍是 unbound。
- 总览和其他管理页的业务数据仍是未接入。
- 错误 CSRF 的 403 有 Playwright 断言，没有写进页面收据。
- 截图只有默认视口、浅色。
- M8 的 63 个测试没有在这个 HEAD 上重跑。
- G3 的字体断言仍期待子集长度 376，切片断言仍期待服务提交 35bd。两份都没有改。
- 本会话没有修改 G2 目录，也没有修改 `tools/control-identity`。

G2 继续以这份固定回执和适配器接口为准。管理功能目录仍由 G2 拥有。
