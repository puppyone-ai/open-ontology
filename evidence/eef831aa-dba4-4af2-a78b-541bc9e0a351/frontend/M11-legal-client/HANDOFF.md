# M11 交接

G3 `a352` 下次复用同一评审会话时审这次提交。不要新开第四个前端会话。G2 `a353` 以这次提交的 SHA 为准。父提交是 `86a39f8aefc9609769ab370c4b845fb8d9572656`。源文件哈希在 `source-inventory.txt`。

## 可以依赖的接口

- `GROUP_PUBLICATION.B02` 与 `GROUP_PUBLICATION.B07` 是 unbound，openApi 是 `absent`，contractCommit 是 null，ready、live、journey、release 都是 false。
- `GROUP_PUBLICATION.B04` 是 schema_frozen。合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。OpenAPI 路径 `tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml`。ready 是 false。
- `PRECISE_OPERATION_IDS` 是五个身份 operationId 加上十五个 B04 operationId。占位 `B02.*`、`B04.*`、`B07.*` 不是合法调用字符串。
- `parseControlIdentityError` 与 `parseSelfbuildError` 只接受各自已提交 error schema。
- `csrfFromCurrent` 只从 current 信封读 `csrf_token`。头名是 `X-OO-CSRF`。它不生成 token。
- `createRuntimeRegistry().register` 在 origin 不是 null、ready 为 true，或 B04 合同提交不匹配时拒绝。成功的 B04 登记仍是 origin null、ready false、reason `schema_only`。
- `prepareCall` 不调用 fetch。B04 变更在 CSRF 通过后返回 `schema_only`，返回值不含 token。身份变更在 CSRF 通过后返回 `not_connected`。
- `invokeOperation` 的旧行为还在。十五个 B04 id 仍是 `schema_only`。五个身份 id 仍是 `not_connected`。
- 运行时 `sources` 与 `secrets` 的 origin 是 null。没有第二份 JS 适配器。部署边缘仍只认 `OO_B01_ADAPTER_CONFIG`。

## 不要当成已完成

- 不是客户 IdP，不是 live_verified，不是旅程关闭，不是 FE-01 完成。
- 不要把未跟踪的 source、secret、release 目录写成 SDK operation。
- 不要按 a373 或 `2d7c2a5cd` 发 B04 HTTP。服务就绪之前，`registerRuntime` 不会接受非 null origin。
- 这次没有新的浏览器截图。五个角色的浏览器证据仍在 M9。
- 没有修改 G2 目录，也没有修改生产者、G3 oracle 或 `specs/frontend/execution-status.md`。
