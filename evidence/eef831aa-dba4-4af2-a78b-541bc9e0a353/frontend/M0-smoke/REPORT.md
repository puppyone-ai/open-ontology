# M0 前端冒烟报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这是改管理页之前的 Linux 冒烟。不签前端验收，不签 A01–A20，live、旅程、发行都是 false。

## 源

归档的是当时的 HEAD `1b67d44f83790a76caf1bfee2861f65a06541c27`。`e61c0fd98b6c8b09c9c8e2db3a441aafaf952909` 和 `866cd4e2ba2571d73428f6303e175d2ef85c78ca` 都是它的祖先。归档只含 `package.json`、`pnpm-lock.yaml`、`pnpm-workspace.yaml`、`.npmrc`、`.node-version`、`apps/web`、`packages/sdk-ts`。工作区里别人的未提交改动没有进这次运行。

`node_modules` 从工作区已有安装复制到这次运行的私有目录，没有执行安装，没有改锁。锁前后都是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

## 已证明

准备容器 `oo-a353-node-prepare` 用私有 bridge `oo-a353-net`，只让 corepack 下载 pnpm 12.10.1 的 linux-arm64 包。随后冒烟容器 `oo-a353-node-smoke` 使用 `network none`。两个容器都是 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`，Node v24.21.0，pnpm 12.10.1。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、`no-new-privileges`。没有 host 网络、没有端口发布、没有 docker socket、没有宿主 HOME。`HOME` 是容器内 `/home/fe02`。corepack 目录只属于这个 UUID。

linux pnpm 原生文件 `pnpm-native` 的 sha256 是 `4690693a2c1cb37eca3004436ccd003bb161fb13706c20bad620b3c43345c72c`。

| 阶段 | 退出码 |
|---|---|
| corepack prepare | 0 |
| `tsc` app | 0 |
| `tsc` node config | 0 |
| `tsc` sdk | 0 |
| eslint | 0 |
| vitest | 0。7 个文件，29 个测试。开始 22:34:49，耗时 5.89s |
| vite build | 0 |

生产包在私有归档里：`index-DGD3fIPZ.js` raw 325770、gzip 102.83 kB；`index-GQYUKJ5S.css` raw 20.07 kB、gzip 5.14 kB；`index.html` raw 0.76 kB、gzip 0.37 kB。构建产物不进 Git。

## 未证明

没有开浏览器。没有跑 Rust。本会话不改 Rust，也不占用 Rust builder。B01–B12 仍是 unbound。这次冒烟只证明改管理页之前的固定前端基线能在 Linux 上通过类型、lint、现有单元测试和生产构建。
