# M13 selfbuild 列表报告

这次提交的父提交是 `e5658235504c7e32b974ba7d22046e509dce1445`。驱动归档基线是 `83312860b76330284984e7751f16c6b92ab926e0`，见 `raw/archive-base.txt`。拒绝名单复测的归档基线是 `b9f1e4fc30f144d1cf7dc61aa7765f58164a9caa`，见 `raw/refuse-archive-base.txt`。从 `5071cc84c6ddb36ac103f44756a3199244bf9968` 到这个父提交，`apps/web` 与 `packages/sdk-ts` 的已提交内容没有差异。

身份服务提交是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。身份合同是 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。B04 合同钉仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。这次消费的实现提交是 `45644fca81325cf1405a38ceb79435da6e8bb090`。OpenAPI blob 是 `8f94655da2d765ed0de75d002e6ff7b736c44b32`。CONTRACT blob 是 `8b149c46f1be207bb08e558b0c9abb5347a347c8`。

这是测试发行方上的一次私有空库读取。客户集成 `not_claimed`。`live`、`journey`、`release` 都是 false。FE-01 没有完成。E56、100k、代理闭环和发布都未验收。

## 路由与解析

`/control/v1/builds`、`/control/v1/reviews`、`/control/v1/classifications` 先于身份前缀匹配，转到 `selfbuild.upstream`。提交的示例里该上游是 null。此时 `GET /control/v1/builds` 返回 502，正文是 `selfbuild_unbound`。

`callOperation` 在 `registerSameOriginTransport` 之后发送请求。`listBuilds` 的 HTTP 200 只有在 `parseBuildList` 接受已提交 BuildList 时记为成功。空数组的 `count` 是 0，不是完成的构建。其他 selfbuild 成功状态仍是 `invalid_schema`。

`invokeOperation` 对 15 个 B04 id 仍是 `schema_only`，不发送请求。`prepareCall` 不发送请求。页面仍调用 `prepareCall`。`registerRuntime` 仍拒绝非 null origin。B04 的登记状态仍是 `schema_frozen`。ready 仍是 false。

## 私有空库

PostgreSQL、固定 `a6f92` 身份源码、测试发行方，以及 `45644fca` 的 selfbuild 二进制按私有主机名 m13 安装。二进制路径指向不存在的文件。没有调用 `createBuild`。没有加载 a373 的 100k 数据库，没有使用他们的卷。

`raw/b04-receipt.json` 记录：

- admin：login 302，callback 302，current 200，bootstrap consume 200，current 200
- admin `listBuilds?project_id=alpha`：200，`ok` true，`count` 0，`published` false，`active` false，`compiled_pack` `not_published`
- admin `getBuild` 使用 `b_` 加 32 个 0：403，`reason` `http_error`，`errorCode` `forbidden`
- 重新读取 current 之后 logout 200，再读 current 是 401 `unauthenticated`
- `fixture-owner`：login 302，callback 302，current 200，`listBuilds` 403 `forbidden`，重新读取 current 之后 logout 200

服务在每次 B04 读取时会再取 current，CSRF 因此轮换。logout 使用的是读取之后的新 current。

这次发行方的 config commit 是 `95bcd295cc74107110a580b50e4c2e1e8eb8b3d7cebfbf564d7b761f90f5cd42`。M9 常量 `ec801f46456d62b23679c05e87f45e23cedd710bd3aedb1992d69477a2748152` 与 M12 回执里的 `e24a947d3f4aec69e2c00d35e3cd132e42f4712bbdb6753a8cc583295c1536c9` 都没有改。

13 个 operation 没有发送：`createBuild`、`cancelBuild`、`getBuildCards`、`getBuildCandidates`、`listReviewItems`、`getReviewItem`、`previewReviewBatch`、`decideReviewItem`、`listClassifications`、`previewClassifications`、`confirmClassifications`、`compileBuildPack`、`getBuildPack`。

## Linux 检查

锁 sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。Node 镜像是 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Rust 镜像是 `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`。PostgreSQL 镜像是 `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`。

套件容器 Node v24.21.0，cgroup `memory.max=4294967296`，`cpu.max=100000 100000`。HostConfig `Memory=4294967296`，`NanoCpus=1000000000`，`CapDrop=["ALL"]`，`Privileged=false`。三份 tsc、eslint、适配器 `node:test`、`sdk-client.test.ts` 的 Vitest 退出码都是 0。Vitest 是 1 个文件、8 个测试。eslint 有 2 个 warning、0 个 error：从 `apps/web` 检查 SDK 文件时，文件在 base path 之外。

驱动容器的 Node 预算相同。Rust HostConfig `Memory=6442450944`，`NanoCpus=1000000000`，`CapDrop=["ALL"]`，`Privileged=false`。PostgreSQL `Memory=1073741824`，`NanoCpus=1000000000`，`CapDrop=["ALL"]`，只加了 `CAP_CHOWN`、`CAP_DAC_OVERRIDE`、`CAP_FOWNER`、`CAP_SETGID`、`CAP_SETUID`。没有 host network、docker socket 或 host HOME。

`raw/drive.exit`、`raw/edge.exit`、`raw/b04-install.exit`、`raw/b04-serve.exit`、`raw/rust-ready.exit`、`raw/launch-loop.exit` 都是 0。Vitest 是 1 个文件、1 个测试，763ms。install 与 serve 的合同行都打印了对应 operation。

拒绝名单写入 m13 的五个主机名之后，SDK tsc、eslint、适配器 5 个测试和 `sdk-client.test.ts` 的 8 个测试退出码都是 0。`raw/refuse.exit` 与 `raw/launch-refuse.exit` 是 0。容器、卷、网络和私钥目录已按名字删除，见 `raw/cleanup.txt`。

## 第一次驱动

`raw/failed-attempts/` 保留第一次闭环。`launch-loop-1.exit` 是 1。`drive-1.log` 在当时的 `b04-list-drive.test.ts:163` 失败：`expect(revoked.ok).toBe(true)` 得到 false。install、serve、edge 和 rust-ready 那次已是 0。第二次在 B04 读取之后重新取 current，logout 变为 200。没有重跑已经通过的套件、字体或五角色浏览器。
