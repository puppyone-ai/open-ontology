# M7 同源适配器与测试发行方闭环

写这份报告时仓库 HEAD 是 `8fc96b66d653c7e5a9a37f4a2ba03b40b804af0c`。提交脚本若发现 HEAD 已经变了，会停住。这不是 FE-01 完成，也不是客户上线。live、journey、release 都是 false。

## 消费的服务

服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。`tools/control-identity/CONTRACT.md` 的 blob 是 `fab0fe4e1fc52976af11faedf5653478f28a956d`。OpenAPI blob 仍是 `3fdeef6b0d22a3be9646520f82275edcb2e3f037`。五个 schema blob 仍是 089b 公布的那一组。本会话没有修改 `tools/control-identity`。

`ready.json`、`raw/consumed-service.txt` 和 `raw/identity-head.txt` 都是这个服务提交。`config_commit` 是 `4b5fda29f333586dbf4357179d5e24b825b490c7725fb353dcb510a24c82ffaf`。配置里有两个绑定：`fixture-admin` 的角色是 admin，`fixture-reviewer` 的角色是 reviewer，项目都是 `alpha`。客户集成是 `not_claimed`。

第一次闭环的二进制是 `35bd6b8157cecd89f2ae085b012b4b0137562ddb`。那时 `tools/control-identity` 的 HEAD 已经是 a6f。那次 Playwright 也是 1 通过。归档在 `raw/service-35bd/`，说明写在 `raw/service-35bd/NOTE.txt`。当前 SDK 钉的是 a6f，不是那次归档。

## 浏览器闭环

浏览器只访问页面源 `https://oo-fe01-m7-edge:8443`。`apps/web/deploy/b01-adapter.mjs` 把 `/control/` 转到身份上游，并用该上游的 CA 校验 TLS。`ignoreHTTPSErrors` 是 false。Playwright 1 通过，用时 2.7 秒。`raw/playwright.exit`、`raw/build.exit`、`raw/edge.exit`、`raw/launch-loop.exit`、`raw/rust-ready.exit` 都是 0。

`raw/browser-receipt.json` 里的状态依次是：current 401，login 302，callback 302，current 200，consume 200，logout 200，current 401，然后 reviewer 的 login 302、callback 302、current 200、logout 200、current 401。`proved_roles` 是 `admin` 和 `reviewer`。live、journey、release 都是 false。

页面顺序是：fixture-admin 登录后账户未初始化，提交引导令牌，`/admin` 进入总览，退出后 `/admin` 回到管理登录。测试发行方把主体换成 fixture-reviewer 之后，再次登录，初始化状态已经完成，`/admin` 显示「没有权限」和「当前身份没有执行此操作的权限。」账户显示项目 alpha、角色 reviewer，以及「没有管理能力。」

截图两张：`screenshots/admin-overview.png` 87615 字节，`screenshots/forbidden.png` 23053 字节。视口是 Playwright 默认视口，浅色。这一轮没有重跑 1440、1280、768、390、深色、键盘、减少动效、axe，也没有重跑 M6 的四张截图。引导令牌没有出现在截图里。

测试还断言错误 CSRF 的 logout 是 403，并且会话仍在。`page.request` 的响应没有进 `browser-receipt.json`，所以收据里没有 403。

steward、auditor、business_owner 没有在浏览器里证明。

## 字体

`design/font-manifest.json` 的 `subsetCodepoints` 长度是 504。`sourceBinding.g2Snapshot` 是 `0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8`，roots 27 个，`gapCommit` 是 `866cd4e2ba2571d73428f6303e175d2ef85c78ca`。缺口码位仍是 98 个。U+4FEE、U+8BA2、U+9F98 不在子集里。`src/styles/fonts.css` 不含 U+4FEE 和 U+9F98。

完整字体 `public/fonts/NotoSansSC-full.woff2` 的清单记录是 sha256 `bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b`，7762252 字节。`raw/fonts.log` 写着 `subsetPoints=504 fullBytes=7762252`。三个子集的清单记录：

- 300：69988 字节，`f613dcc778eeb119ee2c56aee1b338f02d830c6d39a06b24f75b15e09b4545cc`
- 400：71292 字节，`f658cb908b82692a0bb51d799c8aded90635082a820f3bde3dd238dc460a1d07`
- 500：72180 字节，`d1a915a48f333a8d16aeb5c62343aa38c0eb987860d354b12e420c48793a4071`

Geist 清单记录没有改：`Geist-wght.woff2` `a369fcf5628ea2aa4e1b9e2ec6a5b3624e365bda588e1f0f2f12b564f728fbb8`，`GeistMono-Regular.woff2` `e4507fb4fb5f832fbbb6c06aea4206274ba3083007f23fa8cbc0e87a10acf95b`。锁文件 sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

`apps/web/tests/acceptance/m4-font-oracle.mjs` 仍断言子集长度等于 376。本会话没有改它。

## 适配器

`b01-adapter.example.json` 的 `pageOrigin`、TLS 和两个上游都是 null。读取配置时拒绝明文身份上游、`rejectUnauthorized: false`，以及已销毁的主机名 `oo-fe01-edge`、`oo-fe01-proof`、`oo-fe01-idp`、`oo-fe01-pg`。身份上游失败时正文是 `identity_unreachable`，静态上游失败时正文是 `static_unreachable`。`/control/` 不会落到 `index.html`。

`node:test` 第二次 4 通过，`raw/adapter-test.exit` 是 0。第一次失败留在 `raw/adapter-test-attempt-1.log`，原因是 openssl 找不到 `ca.srl`。`raw/launch-verify-attempt-1.exit` 是 1。修复后 `raw/verify.exit` 和 `raw/launch-verify.exit` 是 0。通过的那次日志里有 Node 的 DEP0123：把 TLS ServerName 设成 IP。测试仍然通过。没有把 `rejectUnauthorized` 设成 false。

## 组合测试

钉还写着 35bd 时，Linux 上的 Vitest 是 30 个文件、63 个测试通过，用时 26.04 秒。`raw/vitest.exit` 是 0。其中 `production-route.test.tsx` 3 个测试、`modules.test.tsx` 3 个测试都通过。这两份测试在当前 HEAD 里比较 `fixture_verified` 和导入的常量，没有写死 35bd。本会话没有改 G2 目录。

服务钉改成 a6f 之后，`tsc` 对 `apps/web`、`tsconfig.node` 和 `sdk-ts` 的退出码都是 0：`raw/types-after.exit`、`raw/types-node-after.exit`、`raw/types-sdk-after.exit`。同一次把仓库只读挂进容器，Vitest 要写 `node_modules/.vite-temp`，得到 EROFS。`raw/vitest-after-pin.exit` 是 1。这是挂载方式，不是产品断言失败。没有在可写挂载上重跑这 63 个测试。

钉改之前的类型检查和 lint 退出码是 0。字体覆盖的 3 个测试包含在那 63 个里面。

## 构建体积

容器里的 Vite 日志：`dist/assets/index-D1qkCcpU.js` 342.19 kB，gzip 106.86 kB；`dist/assets/index-DxLGl__k.css` 28.22 kB，gzip 6.48 kB。构建用时 801 毫秒。容器销毁后主机上没有这份 `dist`，所以没有再跑 `gzip -c` 的精确字节。构建产物不进入 Git。

## 资源

证明容器、网络和卷已经删除：`oo-fe01-pg-m7`、`oo-fe01-rust-m7`、`oo-fe01-node-m7`、`oo-fe01-node-m7v`、`oo-fe01-node-m7sdk`，网络 `oo-fe01-net-m7-eef831aa`，卷 `oo-fe01-m7-cargo`、`oo-fe01-m7-work`、`oo-fe01-m7-pgdata`。`/tmp/oo-fe01-eef831aa-dba4-4af2-a78b-541bc9e0a351/m7-secrets` 和 `m7-source` 不存在。包里没有密钥、`identity.json` 或 `ca.srl`。

当时的限额：Postgres 1073741824 字节、1 CPU，额外能力只有 CHOWN、DAC_OVERRIDE、FOWNER、SETGID、SETUID。Rust 6442450944 字节、1 CPU，`CapDrop ALL`。浏览器 Node 4294967296 字节、1 CPU，`CapDrop ALL`，cgroup `memory.max=4294967296`，`cpu.max=100000 100000`。后一次类型检查容器同样是 4294967296 和 `100000 100000`。三者 `Privileged=false`。

证明 origin `https://oo-fe01-m7-edge:8443` 已经随容器销毁。运行时 origin 保持 null。
