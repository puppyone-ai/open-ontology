# a108 preflight 限定签收

实际终态：Grok 原 handle87537 退出码0；不再轮询该 handle。源码 commit6cd955af38ce426bb1102a6e078e8580febed776 / tree463bbf6ada8ea58e676370c872bfd711e9b9c251 / parent3ca5357930a2c44c42d3061db5fabe577881269a。完整review、findings、HANDOFF已读取，参考applicator、退出聚合与raw SQLSTATE已核验。

决定run06f2ff6d-9e03-40b3-bd34-f4ccf1b9642a，Linux arm64 Rust1.94.0、PG17.10。summary实际154 PASS、0 FAIL、fails0；原console确认probe/fetch/package/Clippy/live均0，live 4.64秒。10项SQL/README/test/JSON哈希逐个与git show固定commit比较一致。1575份证据477172字节、无symlink、无private-key PEM标记，完整SHA索引a108-evidence-sha256.txt。上述扫描不等于完整秘密扫描或供应链批准。

批准W05-a1 preflight修正，关闭A099-1：空schema、缺relation、缺constraint均精确OO002且无version3 marker；旧0002缺relation的42P01死路径仍明确RED。新v1 supersede与既有v2 forward保留旧marker/stamp/receipt；外部实算文件hash与segment_preflight_ready合取；非法历史/foreignowner/NULL/有界锁等待/同backend replay/conflict有限回归通过。

生产src未调用test的route/apply_reference；该test路线与review自有apply都不是生产installer。17/163网格为静态计数，不冒称重跑180-cell PG或320-case矩阵。完整SP04-W05-a/W05/AT-PL-05、F2 lawful owner writer-GUC插入、F3无receipt的用户表DML、F5外部worker身份、Loader/COPY/物理映射/control mirror/ACK/CDC/100workers仍未签收。完整87工作项与16Phase仍未完成。

作者a104首轮失败stdout被覆盖的缺口保持公开，不补造原件；a108三次harness失败原始console/phase/probe分别保留。决定raw summary SHA7ba78f5af76f19c45d59b4b26e8e0342d9f4fc6be010af73e75e43f5ff3f8ee5，console SHA3e2ae9aa13a3a8575ce55c771517b8041a1650430d4dabf036f9ceb668a0bfed，Rust probe SHA25b1dc013032d356be127f951ecd1efd5b0c0fa5a63cff373a2b7f2ce51c51f8。

作者清理记录表明自有builder/PG/network/PGvolume已删除，两a108 cargo/target卷仍保留，未宣称全清理，亦未授予a106清理权。槽B在实际终态后移交a106，槽A仍a110。此修正复审不增加首次复审分母；累计9产品交付组件、8产品验收子项，首次返工5/7。goal active。
