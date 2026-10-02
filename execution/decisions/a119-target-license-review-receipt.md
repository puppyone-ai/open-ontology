# a119 限定目标门禁接收

复审 evidence commit `e2730033628e33fba207eba9cd176942c6ad5722`，tree `19e275476f3d645796e400c885d03901f367d6e1`，parent `999b901360d335c373cf3d8d91350c6240cd9061`。实际恢复 handle63967 exit0，关闭；旧56789也已关闭。完整 REPORT/FINDINGS/HANDOFF 已读。

限定接收对象 `341ece0ea25ae50ff054a4581e8a3389ccb2dd66` 的 ci/deny.toml 四行双 GNU Linux target 列表。原始 canonical command 无 CLI target override，exit0/stdout四部分ok；debug日志实际 gathered259/filtered65，包含三项x86_64特有包与两个目标共有 rustversion、Arrow60、native-certs，排除 wasm-only CC0/CDLA 路径。协调者已读真实 debug marker 行、subtract输出、oracle完整集合差异摘要、Linux1.94身份/探针/driver/inside exits 与 own cleanup记录。wasm/unfiltered 控制均 exit4/two license errors。没有放宽 allow/ignore，也没有修改 lock 或产品源图。

Oracle漏 mio1.2.3，deny实际检查259而oracle258，差异deny_only=1/oracle_only=0；不能称独立oracle全集合匹配。预登记 rustversion 属x86_64-only 的假设被实测纠正：它也由aarch64 encoding_rs build edge引用。两次失败编译保留，不算产品返工。

HASHES.txt 是身份说明，非完整 shasum 清单，未宣称执行其全目录 -c。公开可检查的原始证据随固定 evidence 提交保存；报告写明 a117 sidecar路径误报、错误ID matcher 和不变源图依据。独立复审限定结论成立，不把 residual 掩盖为完全通过。

保留 a110 all-target RED、wasm/all-lock许可证失败；types、SP00、发行、双架构二进制、runsc、真实负责人/客户/受保护CI均未因此通过。advisories仅覆盖实际数据库checkout f8dee89e，不新增独立audit或持续freshness声明。

slot B 已释放。slot A a116 handle68913仍实际live。下一a120可用于独立R2-W驱动任务，无连接snapshot源码写冲突；激活必须冻结精确路径和输入，并遵守全球max2 builder。9产品组件/9验收子项/首轮返工6比9不变。
