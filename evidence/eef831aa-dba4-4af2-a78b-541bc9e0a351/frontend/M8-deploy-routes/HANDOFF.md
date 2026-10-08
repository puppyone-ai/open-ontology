# M8 交接

G3 `a352` 下次复用同一评审会话时审这次提交。不要新开第四个前端会话。G2 `a353` 以这次提交的 SHA 为准。源文件哈希在 `source-inventory.txt`。

## 可以依赖的部署行为

- 固定路由在 `ROUTE_POLICY`。`/control` 进身份上游。`/v1` 与 `/api` 有网关上游才转发，否则正文是 `api_unbound`。OIDC 发现路径正文是 `oidc_unbound`。WebSocket 正文是 `ws_unbound`。这些都不是 `index.html`。
- 页面深链的 SPA 回退只发生在静态 404 且请求接受 HTML 的 GET 或 HEAD。资源和字体的 404 保持 404。
- 示例上游是 null。要运行边缘，部署必须另填 TLS、https 身份上游和静态上游。`pageOrigin` 仍是 null。已销毁的证明主机不是默认值。
- IP 上游不发送 SNI。域名的 `servername` 必须等于主机名。不能关闭证书校验。
- 五个身份 operation 仍是 `fixture_verified`。服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。合同提交 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。`invokeOperation` 不发送请求。
- 改钉之后的组合测试是这次 Linux 归档上的 63 个通过，包含两份 G2 测试。归档基线 `2d7c2a5cd9c3193a7fd3d3124642da194ef8683f`。

## 不要当成已完成

- null 示例不是正在运行的部署，也不是 A20。
- 网关和 WebSocket 没有 live 服务。
- 不是客户 IdP。live、journey、release 都是 false。
- 没有重跑 1440 视口矩阵、axe、键盘或减少动效。
- B04 仍是 `schema_frozen`。占位 `B01.*`、`B04.*` 和其余 B02–B12 仍是 unbound。
- FE-01、整段旅程和发布都未完成。
