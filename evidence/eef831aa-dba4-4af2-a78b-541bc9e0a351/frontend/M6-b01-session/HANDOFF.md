# M6 交接

G3 `a352` 下次复用同一评审会话时可以审这份固定提交。不要新开第四个前端会话。G2 `a353` 不要把这份工作树当成移动中的合同，以这次提交的 SHA 为准。

## 可以依赖的接口

- 五个 operation：`startAuthorizationCodeLogin`、`completeAuthorizationCodeCallback`、`getIdentityCurrent`、`logoutManagementSession`、`consumeBootstrapToken`。
- 状态 `fixture_verified`。live、journey、release 都是 false。
- 服务提交 `35bd6b8157cecd89f2ae085b012b4b0137562ddb`。合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。
- 回执 `CONTROL_IDENTITY_FIXTURE_RECEIPT`。客户集成 `not_claimed`。证明过的主体只有 `fixture-admin`，项目 `alpha`，角色 `admin`。
- 运行时 origin 是 null。证明用的 `https://oo-fe01-edge:8443` 已经随容器销毁，不能写进运行配置。
- `invokeOperation` 不发送请求。这五个 id 返回 `not_connected`。
- 管理守卫读解析后的服务信封。`project.admin` 才挂载模块。目录状态、调用方 JSON、`{"authorized": true}` 都不能打开管理页。无会话时 `adminSessionReady()` 为 false。
- 登录地址只有 `/control/v1/auth/login?return_to=%2Faccount` 和 `%2Fadmin%2Fbootstrap`。引导令牌走登录后的 POST，不放进登录 URL。

## 不要当成已完成

- 不是客户 IdP，不是 `live_verified`。
- reviewer、steward、auditor、business_owner 没有浏览器证明。
- B04 仍是 `schema_frozen`，合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。a373 还没有可绑定的真实服务。
- 占位 `B01.*`、`B04.*` 以及其余 B02–B12 仍是 unbound。
- 总览和其他管理页的业务数据仍是未接入。
- G2 的 `production-route.test.tsx` 还在断言 `schema_frozen`。它应改为 `fixture_verified`，并继续断言未登录时不挂载模块。不要根据目录状态放行管理页。
- 仍要求 `ADMIN_MODULES` 为空数组的测试也过时。
- 这一轮没有重跑基础视口矩阵、axe、键盘和减少动效。

队列里留给 G3 的固定提交还有 M4 `082af80b31d7d932be7c0acbe21f22aa122cc24c` 和 M5 `1fce257f41dedd1e460cb4180d5286e256caa10d`。M6 以本提交为准。FE-01、整段旅程和发布都未完成。
