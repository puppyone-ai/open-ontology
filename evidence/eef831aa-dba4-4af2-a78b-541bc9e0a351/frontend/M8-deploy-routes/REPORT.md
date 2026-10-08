# M8 部署路由与改钉后的组合测试

组合测试使用的归档基线是 `2d7c2a5cd9c3193a7fd3d3124642da194ef8683f`。从这次归档到写报告时，已提交的 `apps/web`、`packages/sdk-ts` 和锁没有新的差异。提交父提交写在 `raw/commit-parent.txt`。这不是 FE-01 完成，也不是 A20 或发布。live、journey、release 都是 false。

## 部署条件

`apps/web/deploy/b01-adapter.mjs` 按固定路由工作。示例里的上游仍是 null。null 不是正在运行的边缘，部署时必须另填 TLS、https 身份上游和静态上游。已销毁的证明主机不能当默认值。`pageOrigin` 保持 null。

- `/control` 转发到身份上游，证书校验保持打开。失败时正文是 `identity_unreachable`。
- `/v1` 和 `/api` 在配置了网关上游时转发到网关。没有网关上游时返回 502，正文是 `api_unbound`。
- `/.well-known`、`/oauth`、`/oidc` 返回 502，正文是 `oidc_unbound`。登录仍是 `/control/v1/auth/login`。
- WebSocket 升级和 `/ws` 在没有 ws 上游时返回 502，正文是 `ws_unbound`。响应里没有 HTML。
- 页面深链只在静态上游对 GET 或 HEAD 返回 404、且请求接受 HTML 时，回退一次 `/index.html`，并加上 `cache-control: no-store`。
- `/fonts`、`/assets` 和带文件后缀的资源保持上游状态码。404 不改写成页面。
- 路由策略不能改成把 API、OIDC 或 WebSocket 交给静态页。配置里的 `routing` 若与代码不一致，读取配置会失败。
- 上游主机是 IP 时不发送 SNI。域名仍发送与主机名一致的 SNI。`rejectUnauthorized` 不能为 false。

当前没有可绑定的网关或 WebSocket 服务，所以示例里这两项保持 null。五个身份 operation 仍是 `fixture_verified`，服务提交仍是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`，合同提交仍是 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。运行时 origin 仍是 null。

## 组合测试

容器 `oo-fe01-node-m8v`，Node v24.21.0。cgroup `memory.max=4294967296`，`cpu.max=100000 100000`。HostConfig 内存 4294967296、1 CPU、`CapDrop ALL`、`Privileged=false`、bridge。源是私有可写归档，Vite 缓存写在归档外的私有目录。没有把仓库只读挂进去。

`node --test deploy/b01-adapter.test.mjs` 5 通过，`raw/adapter-test.exit` 是 0。Vitest 30 个文件、63 个测试通过，用时 36.69 秒，`raw/vitest.exit` 是 0。其中 `production-route.test.tsx` 3 个、`modules.test.tsx` 3 个都跑了，没有跳过。锁在运行前后都是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

没有重跑字体生成，也没有重跑浏览器矩阵。M7 里只读挂载的 EROFS 仍留在 `M7-b01-adapter/raw/vitest-after-pin.exit`，这次没有覆盖它。

容器、归档目录和 Vite 临时目录已经删除。
