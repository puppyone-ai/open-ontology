# M1 公共接口报告

角色：G1，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a351`。这是基础提交 `2c032402ee3b58801f30dbbcefddee8733e383b1` 之后的公共接口切片。FE-01 未完成。A01–A20 未勾选。live、完整旅程、发布都是 false。本报告不自签验收。G3 `eef831aa-dba4-4af2-a78b-541bc9e0a352` 审基础提交和浏览器，本会话没有重跑那次浏览器，也没有再开审阅 UUID。

写入本文件时的 HEAD 是 `b4deca7a3c64df6baa49b4db12b567ade8bc247e`。暂存前再次核对时 HEAD 是 `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`，三个网关 blob 没有变化，SDK 常量没有改。`2c032402ee3b58801f30dbbcefddee8733e383b1` 在这两次核对时都是祖先。审阅目标是包含本文件的那一次 owned-path 提交。

## 范围

交付公共组件 C01–C20、`ModuleRegistration`、应用壳、认证与错误路由，以及 TypeScript SDK 的绑定登记。只把网关里已经存在的 `GET /v1/pack/metadata` 标成 `schema_frozen`。B01–B12 保持 unbound。`POST /v1/sql` 在进程里返回 501，登记为 unbound，不是 B09。没有假登录、假会话、假审核或假发布。`/` 重定向到 `/login`，不是营销页。

根 `.gitignore` 只追加 `node_modules/` 和 `apps/web` 的构建、Storybook、Playwright、覆盖率目录。证据、规格、crate 和源文件仍可见。

## 已证明

Linux arm64 容器使用 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。容器内 Node v24.21.0，pnpm 12.10.1。每个容器 4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。`HOME` 是容器内 `/home/fe01`。没有 host 网络、没有 docker socket、没有共享宿主机 HOME。

| 容器 | 自己的退出码 | 内容 |
|---|---|---|
| `oo-fe01-node-m1` | 1 | 冻结安装退出码 0。安装前后锁哈希都是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。类型和 lint 为 0。测试失败，见 FINDINGS。日志 `raw/container-m1.log`。 |
| `oo-fe01-node-m1b` | 1 | 复用已通过的安装。只重跑测试，仍失败。日志 `raw/container-m1b.log`。 |
| `oo-fe01-node-m1c` | 0 | 复用同一安装。类型、`tsconfig.node`、SDK 类型、lint、测试、Vite 构建、包体计量都是 0。脚本打印 `browser=withheld`。日志 `raw/container-m1c.log`。 |

最终测试：6 个文件、22 个测试通过。开始时间 21:50:58，耗时 4.92s。明细在 `raw/test.log`。

生产包：`index-BFuR9aus.js` raw 320270、gzip 100357；`index-Bqdf60lJ.css` raw 21518、gzip 5241；`index.html` raw 763、gzip 380。基础页的 gzip 70115 不再描述这个包，因为路由和 dialog 已经进入产物。数字来自 `raw/bundle.log`。构建产物不进 Git。

`apps/web/src` 没有 import TanStack Query、TanStack Table、React Hook Form、Zod 或 MSW。本里程碑没有做包内字符串扫描。

锁文件不在本次 diff 里。没有新增 registry 依赖，所以没有改锁。

网关源在报告前于 HEAD `b4deca7a3c64df6baa49b4db12b567ade8bc247e` 重新核对，blob 与 SDK 常量一致：

| 文件 | blob |
|---|---|
| `crates/oo-gateway/src/http_gateway.rs` | `969e655d7458d8b679933ffb6f361f08741cabea` |
| `crates/oo-gateway/src/http_error.rs` | `482621857488168e85b78a11bec17855fd7649e6` |
| `crates/oo-gateway/src/pack_catalog.rs` | `9848dcafc4c36b6512240f20fec1b09b1146c02a` |

`GATEWAY_SOURCE.observedHead` 仍是首次读取时的 `d22187ea0fce8e420323660841f2da3b447d5de8`。合同以 blob 为准。这三个 blob 的引入提交分别是 metadata `97b6a9eb9caa2b6059d82bdb98b703bf5ee202d8`、error `3f4a358ab1502215e956cdfe1f02ba899b06a8ca`、protocol `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`。

## 未证明

浏览器没有打开这些路由。截图 0 张。axe、键盘、减少动效和字体网络断言都没有在本里程碑执行。基础里程碑的浏览器缺口仍由 G3 处理。本会话没有启动 Playwright。

对比度报告、Storybook 构建、字体子集没有重跑。Storybook 故事文件存在，未构建，不能当成视觉通过。

`invokeOperation` 不发请求。`schema_frozen` 返回 `schema_only`。示例夹具是形状示例，不是服务器捕获。OpenAPI 文件不是这条网关路由的合同。

Rust 冒烟没有在本里程碑重跑。本里程碑不改 Rust。

## 绑定

见 `apps/web/design/APIBINDING.md` 的 M1 节。`gateway.packMetadata.get` 为 `schema_frozen`，live false。其余产品操作 unbound。UI verified 只覆盖这次 Linux 单元测试和生产构建。
