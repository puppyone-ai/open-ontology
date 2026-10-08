# M10 发现

1. G2 指出的 4 个 `no-undef` 属于 G1 的 eslint 配置。`.mjs` 现在声明 `Buffer`、`URL` 和 `structuredClone`。这三项都是运行时已有的全局，不是新依赖。全量 eslint 退出码是 0。

2. 全量 lint 还在 `tests/acceptance/slices-oracle.mjs` 上报了 `structuredClone`。oracle 源文件没有改。

3. 提交的适配器示例不再带 `127.0.0.1` 或端口 `8443`。缺少 `listen.host` 会被拒绝。null 仍不是正在运行的边缘。

4. Linux 上三份类型检查、5 个适配器测试、31 个 Vitest 文件共 67 个测试都是 0。`production-route.test.tsx` 和 `modules.test.tsx` 都跑了。没有跳过。锁文件没有变。

5. B04 仍是 `schema_frozen`。a373 交接没有变成可绑定的固定实现。没有伪造 B04 会话或响应。

6. 这一轮没有浏览器。M9 的五角色证明仍以 `ac8737ab2e9ef3d6de41ce2527f487222ce52373` 为准。live、journey、release 都是 false。客户集成仍是 `not_claimed`。
