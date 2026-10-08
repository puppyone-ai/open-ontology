# M12 同源传输报告

证明归档基线是 `b05a892deceb93268404eb54348845f65ada9ec0`，见 `raw/archive-base.txt` 和 `raw/refuse-archive-base.txt`。这次提交的父提交是 `1d5ef4a5184cea0e48b83ea46e2535268a5b1f74`。身份服务提交仍是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。合同仍是 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。B04 合同仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。

这是测试发行方上的 `fixture_verified` 客户端。客户集成 `not_claimed`。`live`、`journey`、`release` 都是 false。FE-01 没有完成。

## 传输

`registerSameOriginTransport` 只接受部署者给出的 https origin。origin 与 pageOrigin 必须相同，不能带用户信息、路径、查询，也不能是已销毁的证明主机。提交的运行时配置仍把 origin 留成 null。

`callOperation` 在登记之后发送 fetch。POST 的 `X-OO-CSRF` 只从 current 信封读取。Node 路径把 `Origin` 设成传输 origin 一次。浏览器路径使用相对 URL，不手写 Origin，并要求 `window.location.origin` 等于传输 origin。

`invokeOperation` 仍不发送请求。五个身份 operation 仍返回 `not_connected`。`prepareCall` 仍不发送请求。`registerRuntime` 仍拒绝非 null origin，B04 登记仍是 `schema_only`。

https 页面在 `IdentityProvider` 里登记当前页 origin。http 测试页不登记，账户和守卫仍走原来的相对路径。`production-route.test.tsx` 与 `current-session.test.tsx` 在拒绝名单变更后的 Linux 运行里通过。

## 五角色

私有 PostgreSQL、固定 `a6f` 源码、测试发行方和同源适配器按 M9 配方重跑，主机名换成 m12。Node 驱动使用 `callOperation` 和按主机分开的 cookie jar。没有 Playwright，没有重跑 19 张基础截图。

`raw/client-receipt.json` 记录：

- login 302 共 5 次，callback 302 共 5 次，logout 200 共 5 次，logout 403 共 1 次，bootstrap consume 200 共 1 次
- current 200 共 12 次，current 401 共 5 次
- 角色是 admin、reviewer、steward、auditor、business_owner，项目是 alpha
- admin 在 consume 前是 uninitialized，之后能力是 `binding.read`、`identity.current`、`project.admin`、`session.revoke`
- 错误 CSRF 的 logout 是 403，`errorCode` 是 `csrf.rejected`，随后的 current 仍是 admin
- `listBuilds` 是 HTTP 404，`fetched` true，`ok` false，`reason` 是 `invalid_schema`。响应对不上已提交的 selfbuild error schema。没有把它记成构建成功

这次发行方的 config commit 是 `e24a947d3f4aec69e2c00d35e3cd132e42f4712bbdb6753a8cc583295c1536c9`，与 ready.json 一致。SDK 里 M9 的 config commit 常量 `ec801f46456d62b23679c05e87f45e23cedd710bd3aedb1992d69477a2748152` 没有改。

驱动结束后，m12 的四个主机名写入适配器和 SDK 的拒绝名单，容器、卷、网络和私钥目录已按名字删除。

## Linux 检查

锁 sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

第一轮 Node v24.21.0，cgroup `memory.max=4294967296`，`cpu.max=100000 100000`。三份 tsc、eslint、Vitest 退出码都是 0。Vitest 是 2 个文件、10 个测试。

第二轮在拒绝名单写入之后。SDK tsc 与 eslint 退出码是 0。Vitest 是 4 个文件、16 个测试，包含 `sdk-client.test.ts`、`identity-session.test.ts`、`production-route.test.tsx`、`current-session.test.tsx`。没有重跑 M10 的 67 个测试。

驱动容器的 Node 预算是 4GiB、1 CPU，cap-drop ALL。Rust 是 6GiB、1 CPU，cap-drop ALL。PostgreSQL 是 1GiB、1 CPU，只加了 CHOWN、DAC_OVERRIDE、FOWNER、SETUID、SETGID。没有 host network、docker socket 或 host HOME。

`raw/drive.exit`、`raw/edge.exit`、`raw/launch-loop.exit`、`raw/rust-ready.exit` 都是 0。
