# M6 测试发行方会话

父提交在写这份报告时是 `d63c388a61f6249cd85b5fc171dbcb0155659d07`。这不是 FE-01 完成，也不是客户上线。live、journey、release 都是 false。

## 证明了什么

同一浏览器里，对着已销毁的私有服务完成了授权码加 PKCE：

- `GET /control/v1/identity/current` 未登录是 401。
- `GET /control/v1/auth/login` 是 302。
- `GET /control/v1/auth/callback` 是 302。
- 登录后的 `GET /control/v1/identity/current` 是 200。主体 `fixture-admin`，安装状态未初始化，没有项目能力。
- `POST /control/v1/auth/bootstrap/consume` 是 200。之后账户显示项目 `alpha`、角色 `admin`，并出现 `project.admin`。
- `/admin` 这时进入总览。总览里的 B02、B03、B04、B08、B10 仍写着未接入。
- 同一测试断言，用 43 位错误 CSRF 调用 `POST /control/v1/auth/logout` 得到 403，会话没有被撤销。`page.on("response")` 看不到 Playwright 的 APIRequestContext，所以 `raw/browser-receipt.json` 没有这一行 403。
- 页面上的退出得到 logout 200。随后的 current 是 401。`/admin` 回到管理登录，不出现总览。

客户集成保持 `not_claimed`。这是本地测试发行方，不是客户 IdP。reviewer、steward、auditor、business_owner 没有在浏览器里证明。

截图四张：`account-uninitialized.png`、`bootstrap-empty.png`、`admin-overview.png`、`admin-after-logout.png`。视口是 Playwright 默认视口，浅色。这一轮没有重跑 1440/1280/768/390、深色、键盘、减少动效或 axe。引导令牌没有出现在截图里。

## 目录

五个 operation 改为 `fixture_verified`：`startAuthorizationCodeLogin`、`completeAuthorizationCodeCallback`、`getIdentityCurrent`、`logoutManagementSession`、`consumeBootstrapToken`。

服务提交 `35bd6b8157cecd89f2ae085b012b4b0137562ddb`。合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。运行时 origin 是 null，`reachable` 是 false。`invokeOperation` 对这五个 id 返回 `not_connected`，不调用 fetch。

B04 十五个 HTTP operation 仍是 `schema_frozen`。占位 `B01.*` 和 `B04.*` 仍是 unbound，method 为 null。`POST /v1/sql` 仍不是 B09。

`adminSessionReady()` 不读目录。没有会话时它是 false。管理页只在解析后的会话拥有 `project.admin` 时挂载。

## 资源

证明容器已删除：`oo-fe01-pg-m6`、`oo-fe01-rust-m6`、`oo-fe01-node-m6`，网络 `oo-fe01-net-m6-eef831aa`，卷 `oo-fe01-m6-cargo`、`oo-fe01-m6-work`、`oo-fe01-m6-pgdata`，以及 `/tmp/oo-fe01-eef831aa-dba4-4af2-a78b-541bc9e0a351/m6-secrets`。

当时的限额：Postgres 1073741824 字节、1 CPU，额外能力只有 CHOWN、DAC_OVERRIDE、FOWNER、SETGID、SETUID。Rust 6442450944 字节、1 CPU，`CapDrop ALL`。Node 4294967296 字节、1 CPU，`CapDrop ALL`。三者 `Privileged=false`，没有 host 网络。

目录复跑使用 `oo-fe01-node-m6sdk`，同样 4294967296 字节、1 CPU、`CapDrop ALL`，默认 bridge。检查后已删除。

锁文件 sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

## 这一轮的检查

浏览器那次：Vitest 8 通过，生产构建 0，Playwright 1 通过。类型和 lint 在目录改动之后的容器里是 0。`vitest-fixture` 12 通过。G2 的 `production-route.test.tsx` 退出码 1，只因为还期待 `schema_frozen`；`adminSessionReady()` 的 false 断言已经通过。没有改 G2 文件。

目录改动之后的 JS 是 `dist/assets/index-DI6Dg6fG.js`，342199 字节。浏览器截图来自改目录之前的 `index-BvHQ7Fvw.js`。构建产物不进入 Git。
