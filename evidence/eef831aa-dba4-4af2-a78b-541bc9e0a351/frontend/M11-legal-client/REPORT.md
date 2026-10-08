# M11 合法客户端

父提交按写报告时的 HEAD：`86a39f8aefc9609769ab370c4b845fb8d9572656`。提交脚本若发现 HEAD 变了，会停住，不提交。这不是 FE-01 完成，也不是客户上线。live、journey、release 都是 false。没有真实服务调用，全旅程没有关闭。

## 发布面

B04 的已提交合同仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。OpenAPI blob `8f94655da2d765ed0de75d002e6ff7b736c44b32`，error schema sha256 `ab989b7de7c402f3bfe44a08e87053b296901ec172914ebc8d2a48518efd08fe`。工作区里 `tools/selfbuild-control-api/src` 有别人的未提交修改，归档用的是 git 里的 OpenAPI，没有把那些修改当合同。

B02、B07 在 git 里没有 CONTRACT 或 OpenAPI。`tools/source-control-api`、`tools/secret-control-api`、`tools/release-control-api` 仍是未跟踪目录。它们没有被读成冻结，也没有把里面的 operationId 写进 SDK。G2 在 `e86a4900585a9e3a0f5af8d243626d57e4e27eea` 的交接写了同一件事：源和发布要先有固定提交，G1 再冻结绑定。

`session-control-api` 是已提交的 B08 合同，不在这张卡的 B02/B04/B07 里，这次没有改它的绑定。

a373 交接仍写明身份产品读取未验收、生产者二进制未交付、E56 未验收。B04 保持 `schema_frozen`，ready 是 false，origin 是 null。`prepareCall` 和 `invokeOperation` 都不发送 fetch。

## 客户端

`packages/sdk-ts/src/client.ts` 导出这些名字：

- `PRECISE_OPERATION_IDS`：控制身份 5 个 operationId，加上 B04 的 15 个。占位 `B02.*`、`B04.*`、`B07.*` 返回 `not_precise`。
- `CONTROL_IDENTITY_ERROR_CODES` 与 `SELFBUILD_ERROR_CODES`，以及 `parseControlIdentityError`、`parseSelfbuildError`。code 必须落在已提交 error schema 的 enum 里，message 按码点 1 到 160，额外字段拒绝。
- `csrfFromCurrent` 只接受 current schema 的信封，取出 `X-OO-CSRF` 和整数 `revision`。`{"authorized": true}`、短 token、小数 revision、多余字段都是 `not_current`。
- `registerRuntime` 与 `createRuntimeRegistry`。origin 不是 null、ready 为 true、合同提交不匹配，都拒绝，并且登记结果的 origin 仍是 null。B02 与 B07 的原因是 `unpublished`。B04 只有 origin null、ready false、serviceCommit 等于 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9` 时接受，原因是 `schema_only`。
- `prepareCall` 对 B04 变更先要求 current CSRF，然后返回 `schema_only`，返回值里不带 token。GET 不要求 CSRF，仍是 `schema_only`。身份变更在 CSRF 绑定后返回 `not_connected`。

`apps/web/src/app/runtime-config.ts` 增加 `sources` 与 `secrets`，状态是 unbound，origin 是 null。selfbuild 增加 `ready: false`。没有新的 JS 反向代理，没有写 395 的后端栈目录。

## 证明

镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。两次容器都是 linux/arm64，Memory 4294967296，NanoCpus 1000000000，CapDrop ALL，Network bridge，Privileged false。cgroup memory.max 4294967296，cpu.max 100000 100000。Node v24.21.0。锁前后都是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。容器名 `oo-fe01-node-m11`，两次结束后都已删除。

第一次归档 `e86a4900585a9e3a0f5af8d243626d57e4e27eea`，覆盖当时的客户端文件。三份 tsc 退出码 0。eslint 覆盖三个改过的 web 文件，退出码 0。Vitest 3 个文件、9 个测试里 1 个失败：OpenAPI 把 `previewReviewBatch` 排在 `getReviewItem` 前面，`SELFBUILD_HTTP` 的顺序相反。`sdk-binding.test.ts` 4 个和 `runtime-config.test.ts` 1 个通过。日志在 `attempt-1/`。

第二次只改了测试断言，改为比较 operationId 集合，没有改 `SELFBUILD_HTTP` 的顺序。归档 `86a39f8aefc9609769ab370c4b845fb8d9572656`。`e86a4900` 到这次归档之间，`apps/web` 和 `packages/sdk-ts` 没有提交差异。eslint 只跑 `sdk-client.test.ts`，退出码 0。Vitest 该文件 4 个测试通过，1.05 秒。`raw/vitest.exit` 是 0。没有重跑已经通过的 tsc，也没有重跑 M10 的 67 个测试、字体或浏览器。

## 没有做的事

没有构建、审核、分类、发布、用户问答的 HTTP 调用。没有客户 IdP。没有把 schema_frozen 写成 live。没有改 G2 目录、生产者、G3 oracle、`execution-status.md`。
