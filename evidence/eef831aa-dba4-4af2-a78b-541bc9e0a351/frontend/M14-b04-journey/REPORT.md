# M14 十五条 selfbuild 调用报告

这次提交的父提交是 `34e436f867128822aec5ac2198a74ff915b4ad8f`。驱动归档基线是 `e2bd526e96175efb52fa1181843fbe991ae7992b`，见 `raw/archive-base.txt`。拒绝名单复测的归档基线是 `6fd1bf22bce9c8190f8dbd3f69d918d29bc0bc78`，见 `raw/refuse-archive-base.txt`。

身份服务提交是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。身份合同是 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。B04 合同钉仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。这次消费的实现提交是 `45644fca81325cf1405a38ceb79435da6e8bb090`。OpenAPI blob 是 `8f94655da2d765ed0de75d002e6ff7b736c44b32`。CONTRACT blob 是 `8b149c46f1be207bb08e558b0c9abb5347a347c8`。

这是测试发行方上的一次私有空库调用。客户集成 `not_claimed`。`live`、`journey`、`release` 都是 false。FE-01 没有完成。E56、100k、代理闭环和发布都未验收。

## 路由与解析

`/control/v1/builds`、`/control/v1/reviews`、`/control/v1/classifications` 先于身份前缀匹配，转到 `selfbuild.upstream`。提交的示例里该上游是 null。此时 `GET /control/v1/builds` 返回 502，正文是 `selfbuild_unbound`。

`callOperation` 在 `registerSameOriginTransport` 之后发送请求。schema 通过的 selfbuild 文档按操作记为成功：`listBuilds`、审核列表、审核预览、分类列表是 200；`createBuild` 与 `cancelBuild` 是 202；`getBuild` 是 200。预览 `items` 的每一项必须是对象。字符串数组保持 `invalid_schema`。

`invokeOperation` 对 15 个 B04 id 仍是 `schema_only`，不发送请求。`prepareCall` 不发送请求。页面仍调用 `prepareCall`。`registerRuntime` 仍拒绝非 null origin。B04 的登记状态仍是 `schema_frozen`。ready 仍是 false。

## 私有空库

PostgreSQL、固定 `a6f92` 身份源码、测试发行方，以及 `45644fca` 的 selfbuild 二进制按私有主机名 m14 安装。五个工具二进制路径指向不存在的文件。配置有一列可见列 `public.party.id`，以及一个 `fixture-owner` 的 owner principal。没有加载 a373 的数据库，没有使用他们的卷，没有在 `127.0.0.1:8317` 上监听。

`raw/b04-journey-receipt.json` 记录：

- admin：login 302，callback 302，current 200，bootstrap consume 200，current 200
- `listBuilds` 200，`ok` true。随后 current 200
- `createBuild` 503，`errorCode` `dependency.not_ready`
- `getBuild` 使用 `b_` 加 32 个 0：403 `forbidden`。随后 current 200
- `cancelBuild` 403 `forbidden`
- `getBuildCards` 403 `forbidden`
- `getBuildCandidates` 403 `forbidden`
- `listReviewItems` 200，`ok` true，items 为空
- `getReviewItem` 403 `forbidden`。随后 current 200
- `previewReviewBatch` 200，`ok` true，`wrote` false，items 为空
- `listClassifications` 200，`ok` true，一列 `public.party.id`。随后 current 200
- `previewClassifications` HTTP 200，`ok` false，`reason` `invalid_schema`，`errorCode` null。服务返回的 items 是字符串。随后 current 200
- `compileBuildPack` 403 `forbidden`
- `getBuildPack` 403 `forbidden`。随后 current 200，logout 200
- reviewer：login 302，callback 302，current 200，`decideReviewItem` 403 `forbidden`，current 200，logout 200
- owner：login 302，callback 302，current 200，`confirmClassifications` 403 `forbidden`，current 200，logout 200

回执字段 `completed_build`、`model_called`、`binaries_present` 都是 false。15 个 B04 operationId 都出现在回执里。install 与 serve 的启动行写着 `dependency` `ready`。那一行是进程启动。二进制是否存在由 `createBuild` 的 503 证明。

这次发行方的 config commit 是 `0698fb16b7af07495b028af3494148a44057139cf200f7b390498cde6f4cce08`。M9 常量 `ec801f46456d62b23679c05e87f45e23cedd710bd3aedb1992d69477a2748152`、M12 回执里的 `e24a947d3f4aec69e2c00d35e3cd132e42f4712bbdb6753a8cc583295c1536c9`、M13 回执里的 `95bcd295cc74107110a580b50e4c2e1e8eb8b3d7cebfbf564d7b761f90f5cd42` 都没有改。

## Linux 检查

锁 sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。套件前后与拒绝名单复测的锁哈希相同，见 `raw/lock-before.txt`、`raw/lock-after.txt`、`raw/refuse-lock.txt`。Node 镜像是 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Rust 镜像是 `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`。PostgreSQL 镜像是 `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`。

套件容器 Node v24.21.0，cgroup `memory.max=4294967296`，`cpu.max=100000 100000`。HostConfig `Memory=4294967296`，`NanoCpus=1000000000`，`CapDrop=["ALL"]`，`Network=bridge`，`Privileged=false`。三份 tsc、eslint、适配器 `node:test`、`sdk-client.test.ts` 的 Vitest 退出码都是 0。Vitest 是 1 个文件、9 个测试。eslint 有 2 个 warning、0 个 error：从 `apps/web` 检查 SDK 文件时，文件在 base path 之外。保留的 `raw/launch-suite.exit` 是 0。

驱动容器的 Node 预算相同，网络是 `oo-fe01-net-m14-eef831aa`。Rust HostConfig `Memory=6442450944`，`NanoCpus=1000000000`，`CapDrop=["ALL"]`，`Privileged=false`。PostgreSQL `Memory=1073741824`，`NanoCpus=1000000000`，`CapDrop=["ALL"]`，只加了 `CAP_CHOWN`、`CAP_DAC_OVERRIDE`、`CAP_FOWNER`、`CAP_SETGID`、`CAP_SETUID`。没有 host network、docker socket 或 host HOME。

`raw/drive.exit`、`raw/edge.exit`、`raw/b04-install.exit`、`raw/b04-serve.exit`、`raw/rust-ready.exit`、`raw/launch-loop.exit` 都是 0。Vitest 是 1 个文件、1 个测试，2052ms。

五个 m14 主机名写入拒绝名单之后，SDK tsc、eslint、适配器 5 个测试和 `sdk-client.test.ts` 的 9 个测试退出码都是 0。`raw/refuse.exit` 与 `raw/launch-refuse.exit` 是 0。拒绝名单复测的 Node HostConfig 与套件相同，网络是 `bridge`。没有再跑驱动。容器、卷、网络和私钥目录已按名字删除，见 `cleanup.txt`。
