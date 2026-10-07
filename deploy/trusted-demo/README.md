# 可信算子 PostgreSQL 开发演示

这是 OPS-D01 / SP11-W06 的开发演示，固定产品档案为 `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`。

本命令不发行软件，不表示认证或授权，不使用 runsc，不是共享多租户，也不构成 I01 或 I02 的签收。父项 SP11-W06 不因本演示被接受。它也不代替 Compose、管理运维页或完整部署。双架构矩阵留给后续集成节点。

## 入口

资源身份来自一份显式部署清单。脚本里没有默认 UUID。省略 `--manifest` 会退出 2，不会落到历史演示 `a305`。

在仓库根目录把下面这一行写成自己的清单。`owner` 必须是 `run_id` 的最后四位小写十六进制。`fixed_source` 与 `source_id` 不能改。

```bash
cat > /tmp/oo-demo-manifest.json <<'EOF'
{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"a312","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0a312","source_id":"public_shop"}
EOF
./deploy/trusted-demo/start.sh --manifest /tmp/oo-demo-manifest.json --public-fixture
```

换一台新克隆时，换成自己的 `owner` 和 `run_id`。不要复用别人的 UUID。历史演示只在清单里写明 `owner` 为 `a305`、`run_id` 为 `eef831aa-dba4-4af2-a78b-541bc9e0a305` 时才会使用原来的资源名。

该命令只做下面这些事：

1. 从固定提交的 `git archive` 编译已有的评审 CLI。驱动器 `src/drive.rs` 和本目录脚本来自这份演示树。不执行 `cargo update`。
2. 启动一台私有 PostgreSQL 17.10。源库 `oo_<owner>_src` 与目标库 `oo_<owner>_dst` 分属不同角色，共用这一台服务器。
3. 装入 `fixture/source.sql` 的公开合成数据。源登录 `oo_<owner>_reader` 只有两张 shop 表的 `SELECT`。
4. 运行 `pg-onboarding inspect`。源标识与项目名都是 `public_shop`。固定导入 CLI 的 SourceId 不接受连字符。这一阶段的成功状态是 `awaiting_review`。
5. 把 `fixture/declared-review.json` 里的声明决定绑定到捕获草稿中的指纹。夹具里没有指纹。审查者标记 `public-fixture` 是夹具声明，不是人类审批。
6. 用 `shop.customer.not_a_column` 证明坏决定在列 ACL、映射读者、analyze、角色包和可信查询之前停止。
7. 在原始 inspect 目录上运行 `pg-onboarding apply-reviewed`。
8. 核对北向角色 `oo_r_north` 的正例行，以及 `query-pg` 对隐藏列和写入的拒绝。
9. 写下回执、位置、摘要和已经执行过的查询命令。
10. 停止并再次启动同一数据卷，重复北向查询。角色包 SHA-256 必须保持不变。

`--smoke` 在回执状态已是 `ready_for_trusted_query` 时重复第 10 步。审计文件不能复用，重复查询会写一份新的审计路径，SQL、角色和策略标识不变。

```bash
./deploy/trusted-demo/start.sh --manifest /tmp/oo-demo-manifest.json --smoke
```

算子可见的命令写在证据目录 `live/work/operator-stage.txt`。

## 回执

`evidence/<run_id>/SP11/trusted-demo-identity/live/receipt.json`

协议 `oo.deploy.trusted-demo-receipt/v1`。其中 `natural_language` 与 `model_provider` 为 `unsupported`，`harness` 为 `not_performed`。本演示没有模型调用，也没有自然语言循环。

回执里的 `query-pg --request` 是已经执行过的命令。原样再跑会因为审计文件已存在而失败。

## 资源

- Rust：`docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`，`linux/arm64`，工具链 1.94.0。新构建器 6 GiB、1 CPU。
- PostgreSQL：`docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`，1 GiB、1 CPU。
- 网络、卷和容器名由清单派生。不发布主机端口，不挂 Docker 套接字，不挂宿主家目录，不使用特权模式，`--cap-drop ALL`。
- 口令、CA 和密钥只在 `/tmp/oo-<owner>-<run_id>/secrets`。它们不进入仓库，也不从清单或 argv 读入。
- 镜像不在本地时退出码为 1，脚本不拉取镜像。

PostgreSQL 的就绪只认 `pg_isready`。容器已经启动不算就绪。

四个标签必须同时成立：`oo.project=open-ontology`、`oo.owner`、`oo.session`（run id）、`oo.fixed`（固定档案）。少一个就算外部资源，命令拒绝并不改它。

## 退出码

| 码 | 含义 |
| --- | --- |
| 0 | 该阶段按合同完成 |
| 1 | 镜像缺失、数据库未就绪、编译失败或预言拒绝。此时不会写下就绪回执 |
| 2 | 用法错误、清单不合格、外部标签、容器名已占用，或已有运行目录与资源被保留、拒绝覆盖 |
| 3 | 本任务已有构建器、宿主已有 cargo/rustc/rustdoc/rustup 进程，或已有 10 个带标签的 Rust 构建器 |

失败尝试留在 `evidence/<run_id>/SP11/trusted-demo-identity/linux/`。已有 `live/run`、私有临时目录或同名卷时，`--public-fixture` 退出 2，不覆盖。

## 清理

`--stop` 只删除四个标签都匹配的容器。卷、私有口令和证据保留。

```bash
./deploy/trusted-demo/start.sh --manifest /tmp/oo-demo-manifest.json --stop
```

停掉之后再跑 `--public-fixture` 会因为卷和运行目录还在而退出 2。这是碰撞，不是覆盖。

`--clean` 只删除上述容器，以及这份清单的卷、网络和私有临时目录。不执行 `docker prune`，不清理其他所有者。

```bash
./deploy/trusted-demo/start.sh --manifest /tmp/oo-demo-manifest.json --clean
```

清单 `linux/public/manifest.sha256` 在容器内生成。容器退出后才写下的 `hostconfig.txt`、`mounts.txt`、`tmpfs.txt`、`invocation.txt`、`docker-run.exit` 和仍在增长的 `console.log` 可能不在这份清单里，或清单中的 `console.log` 不是最终字节。这些文件仍留在该次尝试目录中。
