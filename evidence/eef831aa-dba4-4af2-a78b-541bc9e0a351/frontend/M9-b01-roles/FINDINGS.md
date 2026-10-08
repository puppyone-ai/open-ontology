# M9 发现

1. 五个角色都对着服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b` 走完登录、账户和守卫。admin 能进总览。reviewer、steward、auditor、business_owner 的账户显示本人主体和该角色的能力，并被 `/admin` 拒绝。客户集成是 `not_claimed`。这是测试发行方，不是客户 IdP。live、journey、release 都是 false。

2. Playwright 断言错误 CSRF 的 logout 返回 403，并且随后账户仍是 fixture-admin。`page.request` 的响应没有进页面监听器，所以 `raw/browser-receipt.json` 里没有 403。收据里的 logout 都是 200。

3. 截图是 Playwright 默认视口、浅色。没有重跑 1440、1280、768、390、深色、键盘、减少动效和 axe。引导令牌是密码框，截图里看不到令牌。

4. B04 合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9` 已冻结。实现提交 `2d7c2a5cd9c3193a7fd3d3124642da194ef8683f` 存在，但是 a373 交接写明二进制未交付、`a6f92da8` 未被独立验收、原 E56 未验收。这次没有把 B04 标成 `fixture_verified` 或 `live_verified`，也没有伪造响应。SDK 里十五个 B04 operation 仍是 `schema_frozen`，`invokeOperation` 返回 `schema_only`。占位 `B01.*` 和 `B04.*` 仍是 unbound。总览上的 `B04.builder` 仍显示 unbound。

5. 示例配置的上游仍是 null。null 不是正在运行的边缘。证明主机在证明结束后写入拒绝名单，不能再作为部署默认值。`raw/adapter.used.json` 只记录已经销毁的那一次运行。

6. 字体没有重做。通过的断言只说明这次页面没有请求完整回退字体或外部字体地址。`apps/web/tests/acceptance/m4-font-oracle.mjs` 仍断言子集长度等于 376，当前清单是 504。`slices-oracle.mjs` 仍断言服务提交 `35bd6b8157cecd89f2ae085b012b4b0137562ddb`，当前钉是 a6f。两份评审断言都没有改。

7. M8 的 63 个测试没有重跑。归档基线 `3be6264f18811b2ad88eedf986c342b15bdd0a16` 在 M8 之后包含别人提交的权限页文件。这次只跑了改过的 `identity-session.test.ts`，3 个测试通过。没有把任何测试标成跳过。

8. `apps/web/.storybook/preview.tsx` 仍有别人留下的未提交改动。这次没有暂存，也没有还原。G2 目录没有改。`specs/frontend/execution-status.md` 没有改。

9. 容器、私有网络、三份卷、密钥目录和身份源归档都已按精确名称删除。`oo-a373-pg` 以及其他人的容器没有删除。
