# M10 交接

G3 `a352` 下次复用同一评审会话时审这次提交。不要新开第四个前端会话。G2 `a353` 以这次提交的 SHA 为准。父提交是 `7169d2a839fffb898f14f2ed790e99180a5542bb`。测试归档是 `3b35c82795a0ac9a4d9c0838d93e99703a938266`，其间前端源没有变化。源文件哈希在 `source-inventory.txt`。

## 可以依赖的接口

- `.mjs` 的 eslint 全局包含 `console`、`process`、`Buffer`、`URL`、`structuredClone`。部署适配器不再因这几个名字使 lint 失败。
- `apps/web/deploy/b01-adapter.example.json` 的监听地址、端口、TLS 和上游都是 null。`readAdapterConfig` 要求显式的 `listen.host`，不补 `127.0.0.1`。
- 运行配置仍由 `OO_B01_ADAPTER_CONFIG` 提供。`pageOrigin` 保持 null。没有网关上游时返回 `api_unbound`，OIDC 发现路径返回 `oidc_unbound`，没有 ws 上游时返回 `ws_unbound`。
- 五个身份 operation 仍是 `fixture_verified`。回执仍是 M9 的 `CONTROL_IDENTITY_FIXTURE_RECEIPT`。服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。live、journey、release 都是 false。运行时 origin 是 null。
- `invokeOperation` 不发送请求。五个身份 id 返回 `not_connected`。十五个 B04 id 返回 `schema_only`。

## 不要当成已完成

- 不是客户 IdP，不是 `live_verified`，不是 FE-01 完成。
- B04 仍是 `schema_frozen`，合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。a373 写明二进制未交付、E56 未验收。不要按 `2d7c2a5cd` 伪造接通。
- 示例 null 不是已部署的边缘。
- 这一轮没有新的浏览器截图。五个角色的浏览器证据在 M9 包。
- 没有修改 G2 目录，也没有修改 `tools/control-identity` 或 `tools/selfbuild-control-api`。
