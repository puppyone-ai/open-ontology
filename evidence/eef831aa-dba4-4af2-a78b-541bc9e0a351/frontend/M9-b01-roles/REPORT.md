# M9 五个管理角色与本人账户

写这份报告时仓库 HEAD 是 `3be6264f18811b2ad88eedf986c342b15bdd0a16`。提交脚本若发现 HEAD 已经变了，会停住。这不是 FE-01 完成，也不是客户上线。live、journey、release 都是 false。

## 消费的服务

服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。`raw/identity-head.txt` 与 `raw/consumed-service.txt` 都是这个服务提交。`tools/control-identity` 的工作区差异是空的。本会话没有修改生产者。

`ready.json` 的 `config_commit` 是 `ec801f46456d62b23679c05e87f45e23cedd710bd3aedb1992d69477a2748152`。五个绑定都在项目 `alpha`：`fixture-admin` / admin（引导主体）、`fixture-reviewer` / reviewer、`fixture-steward` / steward、`fixture-auditor` / auditor、`fixture-owner` / business_owner。客户集成是 `not_claimed`。`b04` 字段是 `schema_frozen`。

## 浏览器闭环

浏览器只访问页面源 `https://oo-fe01-m9-edge:8443`。适配器把 `/control` 转到 `https://oo-fe01-m9-proof:7443`，`servername` 与主机名相同，并用该上游的 CA 校验 TLS。`ignoreHTTPSErrors` 是 false。网关上游和 WebSocket 上游是 null。`pageOrigin` 是 null。

Playwright 1 通过，用时 7.7 秒。`raw/playwright.exit`、`raw/build.exit`、`raw/vitest-identity.exit`、`raw/edge.exit`、`raw/launch-loop.exit`、`raw/rust-ready.exit` 都是 0。聚焦的 `identity-session.test.ts` 是 3 个测试通过。

`raw/browser-receipt.json` 记录了 51 条页面响应：login 302 共 5 次，callback 302 共 5 次，consume 200 共 1 次，logout 200 共 5 次，current 200 共 21 次，current 401 共 14 次。`proved_roles` 是 admin、reviewer、steward、auditor、business_owner。live、journey、release 都是 false。

页面顺序：fixture-admin 登录后账户显示未初始化，提交引导令牌，账户显示主体、项目 alpha、角色 admin 和能力 `binding.read identity.current project.admin session.revoke`，并出现管理入口。总览标题是「总览」，刷新不可用，正文写着「此能力尚未接入。」以及「依赖 B02、B03、B04、B08、B10。」错误 CSRF 的 logout 被 Playwright 断言为 403，会话仍是 fixture-admin，然后正常退出。

之后测试发行方依次换成 fixture-reviewer、fixture-steward、fixture-auditor、fixture-owner。每人登录后账户显示本人主体和对应能力，显示「没有管理能力。」，没有管理入口。`/admin` 显示「没有权限」和「当前身份没有执行此操作的权限。」退出后回到管理登录。

能力文本与服务端 `capabilities_for` 一致：reviewer 是 `identity.current review.decide`，steward 是 `identity.current ontology.draft`，auditor 是 `audit.read identity.current`，business_owner 是 `business.record identity.current`。

截图十张，都是 Playwright 默认视口、浅色：

- `screenshots/account-admin.png` 42032 字节
- `screenshots/admin-overview.png` 88343 字节
- `screenshots/account-reviewer.png` 41869 字节
- `screenshots/account-steward.png` 42703 字节
- `screenshots/account-auditor.png` 41729 字节
- `screenshots/account-business_owner.png` 42357 字节
- `screenshots/forbidden-reviewer.png` 23053 字节
- `screenshots/forbidden-steward.png` 23053 字节
- `screenshots/forbidden-auditor.png` 23053 字节
- `screenshots/forbidden-business_owner.png` 23053 字节

引导令牌没有出现在截图里。这一轮没有重跑 1440、1280、768、390、深色、键盘、减少动效和 axe。

测试断言字体请求不含 `NotoSansSC-full`、`fonts.googleapis` 和 `fonts.gstatic`。该断言通过。没有重跑字体构建。子集长度仍是 504，来源钉仍是 `0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8`。

## B04

合同提交 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9` 已冻结。实现提交 `2d7c2a5cd9c3193a7fd3d3124642da194ef8683f` 是当前 HEAD 的祖先。a373 的 `HANDOFF.md` 写明：身份产品 `a6f92da8` 未被独立验收，生产者二进制缺失，原 E56 未验收，完整角色矩阵、发布和父目标仍开放。`oo-a373-pg` 仍在运行，本会话没有动它。因此 B04 保持 `schema_frozen`，origin 为 null，`invokeOperation` 对十五个 B04 operation 返回 `schema_only`。没有伪造 B04 响应、CSRF 或会话。

## 适配器

`apps/web/deploy/b01-adapter.example.json` 的上游仍是 null。读取示例会因为缺少身份上游而拒绝。证明配置只写在 `raw/adapter.used.json`，不作为仓库默认值。证明结束后，`oo-fe01-m9-edge`、`oo-fe01-m9-proof`、`oo-fe01-m9-idp`、`oo-fe01-m9-pg` 进入 `DESTROYED_HOSTS`。主机 Node v26.10.0 上 `node --test deploy/b01-adapter.test.mjs` 是 5 通过，`raw/adapter-test.exit` 是 0。

## 构建

容器构建的归档基线是 `3be6264f18811b2ad88eedf986c342b15bdd0a16`，再覆盖本会话尚未提交的会话测试和 Playwright 规格。生产包不包含这两份测试。Vite 日志：`dist/assets/index-UTjtgt1_.js` 342.22 kB，gzip 106.87 kB；`dist/assets/index-B4C0dAhr.css` 25.94 kB，gzip 6.04 kB。构建用时 305 毫秒。构建产物不进入 Git。锁文件 sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

M8 的 63 个测试没有重跑。HEAD 在 M8 之后已包含别人提交的权限页改动，那些测试不在这次聚焦范围内。

## 资源

证明容器、网络和卷已经删除：`oo-fe01-pg-m9`、`oo-fe01-rust-m9`、`oo-fe01-node-m9`，网络 `oo-fe01-net-m9-eef831aa`，卷 `oo-fe01-m9-cargo`、`oo-fe01-m9-work`、`oo-fe01-m9-pgdata`。`/tmp/oo-fe01-eef831aa-dba4-4af2-a78b-541bc9e0a351/m9-secrets` 和 `m9-source` 不存在。包里没有密钥、`identity.json` 或私钥。

启动时 `raw/loop-capacity.txt` 是 `own_rust=0 global_rust=0 host_cargo=0 own_pg=0 node_image=0 own_node=0`。Postgres 1073741824 字节、1 CPU，额外能力只有 CHOWN、DAC_OVERRIDE、FOWNER、SETGID、SETUID。Rust 6442450944 字节、1 CPU，`CapDrop ALL`。Node 4294967296 字节、1 CPU，`CapDrop ALL`，cgroup `memory.max=4294967296`，`cpu.max=100000 100000`。三者 `Privileged=false`，网络是私有网络。

证明 origin `https://oo-fe01-m9-edge:8443` 已经随容器销毁。运行时 origin 保持 null。
