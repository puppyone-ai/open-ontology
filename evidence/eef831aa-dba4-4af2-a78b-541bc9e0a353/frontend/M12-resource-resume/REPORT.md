# M12 资源与阶段退出清点

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀只收已经提交的阶段退出和当前容器。没有启动容器，没有重跑测试，没有改产品代码。这一刀不签管理页、B04 或前端目标完成。

## 观察到的资源

工作区 `2ccf9ee417590991491341a7c2c6f69ad792bd78`。G1 同源客户端仍是 `b12b5b41924754d1683c92d5a34ec1f49767839f`，并且是祖先。自该提交到当前 `HEAD`，`apps/web`、`packages/sdk-ts`、`pnpm-lock.yaml` 的差异只有已提交的 M11 两个文件。见 `raw/sdk-drift.txt`。

`docker ps -a --filter name=oo-a353` 为空。运行中的 Node 容器数量是 0。本卡没有 builder，也没有自己的 Postgres。`/tmp/oo-a353-m0` 到 `/tmp/oo-a353-m11` 仍在，没有删除。

当时仍在运行、且没有被本卡停止的容器：

- `oo-a392-pg`
- `oo-a400-pg`
- `oo-a383-pg`
- `oo-a373-pg`

## 已提交的退出

`raw/phase-exits.txt` 给 85 个既有 `.exit` 与 `exits.txt` 计算了 sha256。失败尝试目录保留在原包里，没有改写。

| 包 | 最终退出 |
|---|---|
| M0 `9d846541cf2e2d6c7d43a518bb7b9ad8e8368d08` | prepare、types、lint、test、build 都是 0 |
| M1 `f67a87b0a76b9f8d863957a7d41509e462e1c1da` | 最终 types、lint、test、build 是 0。失败尝试里有 types 2、test 1、build 1 |
| M2 `7151fcbb7aa0bc8ad9bfb6c4195bd8e66efcfee1` | types、lint、test、build 是 0 |
| M3 `d416874eee65e5f30ad9858cef4429f608066c7a` | types、lint、test、build 是 0 |
| M4 `e090131ed9df0f3b4d39f126ddfc389c2d129722` | types、lint、test、build 是 0 |
| M5 `0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8` | types、lint 是 0。`test.exit` 是 1，`exits.txt` 还有 `font-control=1` |
| M6 `e74ae63e8cf6991bb34f613ab88122bb21d1944d` | types、lint 是 0。`test.exit` 是 1 |
| M7 `c5913f893e36b5745f400af83e11894d7725b9c2` | types 是 0，test 是 0，lint 是 1，docker 是 1 |
| M8 `3a83e3fe0411b3ac15707731a18c0d41e5a0694c` | types 是 0，test 是 0，lint 是 1，docker 是 1 |
| M9 `e86a4900585a9e3a0f5af8d243626d57e4e27eea` | types、lint、test、docker 是 0 |
| M10 `b05a892deceb93268404eb54348845f65ada9ec0` | types、lint、test、docker 是 0 |
| M11 `ec6b13cb63e74437556541b92ddcb0143ca870ea` | types、lint、test、docker 是 0 |

M0 到 M4 的 `build.exit` 是 0。那是当时的生产构建退出码，不是环境发布。M5 之后没有新的生产构建。这一刀的类型检查、lint、测试和构建都没有跑，`raw/exits.txt` 记为 na，docker 观察退出是 0。

## 仍未接入

SDK 里五条身份操作是 `fixture_verified`，服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。十五条 B04 操作是 `schema_frozen`，合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。`live`、`journey`、`release` 是 false。运行时 origin 是 null。`B01.projectRoleBinding` 仍是 unbound。`registerSource` 不在 SDK。`tools/release-control-api/` 未跟踪。页面按钮保持禁用。

这次提交的父提交是 `2ccf9ee417590991491341a7c2c6f69ad792bd78`。这一刀不签管理页或前端目标完成。
