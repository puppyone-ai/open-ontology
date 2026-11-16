# CI

> Last reviewed: 2026-11-16

## 1. 工作流

| 工作流 | 触发 | 运行机器 | 作用 |
|---|---|---|---|
| `pr.yml` | PR / 手动 | `ubuntu-24.04` | 全部门禁，见 §2 |
| `nightly.yml` | 每天 / 手动 | `ubuntu-24.04` | 引擎矩阵（engine-matrix）等夜间检查 |
| `l3-rotation.yml` | 每天 / 手动 | `ubuntu-24.04` | 每晚跑一个引擎的 L3 全量，三晚轮换一遍；加上故障配对和缺陷闸门 |
| `lite-images.yml` | 手动，仅限 `v0.2/product-loops` 分支 | `ubuntu-24.04`、`ubuntu-24.04-arm` | 构建并签名 lite 运行时多架构镜像 |
| `lite-bundle-release.yml` | 手动，仅限 master | 两种架构的原生机器 | 把镜像打成离线包，上传到 GitHub Release |
| `cli-binaries.yml` | 手动 | `ubuntu-24.04`、`ubuntu-24.04-arm`、`macos-15` | 构建四个平台的二进制并打 npm 包 |

全部是 GitHub 托管机，没有自建 runner。

## 2. PR 门禁

`gate-fmt`、`gate-clippy`、`gate-unit`、`gate-docs`、`gate-contracts`、`gate-architecture`、`gate-dependencies`、`gate-evidence`、`gate-integration-a`、`gate-security-b`、`gate-ontology-d`、`gate-nightly-a-m`、`gate-ab-replay`、`secret-scan`、`oo-test-fast-l0`、`oo-test-fast-l1-l2`，最后由 `aggregate` 汇总。

- 门禁策略在 `ci/gates.yaml`，由 `cargo run -p xtask -- gates` 校验。改了某个路径，覆盖这个路径的门禁就必须是 `required`，否则 `aggregate` 报 `policy_capability_inactive`。
- `gate-ab-replay`：只覆盖 A/B 评测框架（`crates/oo-eval`），用录制的响应做逐字节重放，不调用模型，也不联网。完整的公开基准门禁仍按原计划，到 SP12 才启用。
- 工作流权限默认不允许 `contents: write`。唯一例外是 `lite-bundle-release.yml` 里创建 Release 的那一个 job，并有自测证明其他地方仍然会被拒绝。
- `oo-test-fast-l1-l2` 在托管机上要跑约 70–80 分钟，因为它按改动影响选格子，并且串行运行。

## 3. 托管机与本机的差异（写测试时按 Linux 托管机假设）

| 现象 | 原因 | 做法 |
|---|---|---|
| 容器访问不到宿主机上的服务 | Linux 上没有 `host.docker.internal`；端口绑在宿主机回环地址上，容器访问不到 | 容器之间用默认 bridge 加 link 别名互连，访问容器内端口 |
| 删除容器生成的文件时报 PermissionError | 文件属主是容器内的用户 | 起一个临时容器，以 root 身份在同一挂载点里删除；不用 sudo |
| 写时复制克隆失败 | ext4 不支持 reflink | 运行时先探测，不支持就退回普通复制 |
| 交叉编译时找不到 OpenSSL 头文件 | zigbuild 把 glibc 版本写进 target 三元组，并且不使用系统的 libssl | 从固定 sha256 的源码包静态编译 OpenSSL，再用 `OPENSSL_DIR` / `OPENSSL_STATIC=1` 构建 |
| 仅 CI 上出现"源码不干净" | 测试留下了 `__pycache__` | Python 测试一律用 `-B` 运行 |

## 4. 常见失败与处理

- **secret-scan**：改了 `supply-chain/secret-review.json` 绑定的文件后，要更新它的 sha256 和审阅过的行号，并逐行确认命中的都是代码表达式。被绑定的测试快照文件尽量不动，新测试单独放一个文件。
- **gate-docs**：改了 CLI 或错误码之后，重新生成 `tools/docs/gen_error_reference.py` 和 `gen_cli_reference.py`。help 文本里不要写以 "oo xxx" 开头的句子，生成器会把它当作命令。
- **gate-dependencies**：遇到安全公告时，升级到已修复的版本。新版本必须满足 7 天的发布年龄下限，豁免要有记录。可以只改动相关的锁文件条目，其他依赖不要顺带升降级。
- **重跑**：不要用"只重跑失败的 job"，那样产物会重复，`aggregate` 会拒绝。需要重跑时推一个空提交。
- **日志**：`gh run view --log-failed` 要等整次运行结束后才拿得到。运行还在进行时，用 `gh api .../actions/jobs/<id>` 查看是哪一步失败的。
