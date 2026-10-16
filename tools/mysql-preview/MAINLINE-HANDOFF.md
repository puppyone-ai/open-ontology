# MySQL MAINLINE HANDOFF

固定证据根：`/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1`。本报告记录作者验证，不替代 root 独立接收、正式 signoff、生产验收或全 Goal 退出；其他源保持暂停，正式源计数不晋级。


作者全部本卡出口已实跑，待root独立核收与最终安装。源码写权在四报告精确提交、完整包manifest固定后释放，释放事件/固定报告commit见私有 DELIVERY.json；四条长serve保持quiet，不把后台运行报成exit0。

## 默认安装精确映射

- `mysql57`：`/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/mysql57-operator-final-long-v1.json`；SHA `3018e7cf6992323499f03349a4e29f403f9e22880dc5756d9a7eda9969eada56`；ready `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/mysql57-native-isolated-v2/rust-current-gateway.json`；binding `ac32fda3-d275-4b58-a318-bc42e089fe2d`。
- `mysql8`：`/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/mysql8-operator-final-long-v1.json`；SHA `3bfdfc21dbb3d3e471f5d6133d8174f16b312aa1bfb614970e15e00869a195d1`；ready `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/mysql8-native-isolated-v2/rust-current-gateway.json`；binding `7eb8b32b-feae-4885-aa40-4667357db005`。
- `original-mysql57`：`/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/original-mysql57-operator-final-long-v1.json`；SHA `ccfbba18e4604ae10a5b4abe56c557b1cc82d1833d669a1a03f2dd8ed3ef700d`；ready `/tmp/oo-mysql-connection-whole/sol-20261016/mysql57/onboarding-native-tls-v2/rust-current-gateway.json`；binding `150a87a1-24d7-406c-bbd1-fc598759995e`。
- `original-mysql8`：`/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/original-mysql8-operator-final-long-v1.json`；SHA `50e8982b52870de8f8107d0a2f4046c769b5e371eaa93a3612b6ae0252ceb8fc`；ready `/tmp/oo-mysql-connection-whole/sol-20261015/onboarding-c1-complete-v2/rust-current-gateway.json`；binding `6e824358-6b96-4ddf-9f24-e02b1eaccbc5`。

仅 `original-mysql57` / `original-mysql8` 建议替换原默认native安装；输入分别 `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/original-mysql57-input-v1.json` 与 original-mysql8-input-v1.json。新 `mysql57` / `mysql8` 为隔离验收scope，输入分别 `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/mysql57-input-v2.json` 与 mysql8-input-v2.json，不替换客户/原演示source身份。

root在受信Linux容器的 `/etc/open-ontology/native/mysql-operators.json` 0600安装：

```json
{"protocol":"oo.mysql.native-operators/v1","operators":["/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/original-mysql57-operator-final-long-v1.json","/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/original-mysql8-operator-final-long-v1.json"]}
```

不增加env搜索/新registry框架；六字段精确匹配、未知/歧义/重定向拒绝，显式 --operator 兼容。全局正式默认安装、legacy GW停启、旧Python注册归档、台账/远端由root执行；本作者未改它们。root若选择保留隔离入口可单独注册其不同secret_ref的两operator，但不得混为原source。

producer f9、runtime00f198的完整SHA/文件位置见REPORT/mainline-runtime-elf-v3.json。原两 native schedule 和已有loader override均已沿官方协议改为f9；原manifest/pins原件不动。私有新manifest：original-*-feature-manifest-v1.json，实际生效override：original-*-feature-loader-override-v1.json。新两个manifest来自其最终prepared根官方capture。四source/GTID/Pack/PID/期限如下。

| scope | source / generation | GTID end | Pack | serve PID | UTC deadline |
|---|---|---|---|---|---|
| mysql57 | `mysql_48d47e1bc2688ce4ea9bfccf` / 4 | 100 | `44de92bcebec51beac953a97b8cf914e92ed4b06ab98bdf3408700ad5a61fa6f` | 92800 | 2026-10-17T12:59:46.322038+00:00 |
| mysql8 | `mysql_41e1122d356cca616281931c` / 4 | 46 | `68c69e44035cf30344eea2180d12af93ca69532b34cc090b9e6d0c8e9a72c418` | 92824 | 2026-10-17T12:59:46.478634+00:00 |
| original-mysql57 | `mysql_911437f094f3278dab47b513` / 9 | 100 | `88224546576062ee13679011e6e8e1e0d125dadcdf1145d01af3c196607d1e73` | 92812 | 2026-10-17T12:59:46.400570+00:00 |
| original-mysql8 | `mysql_67fff4e8ebff9f91bee8c75e` / 13 | 46 | `242c82b50b2c47360495545924f7fd8bd6c845804bac3b31e38b9d74ebada0b6` | 92836 | 2026-10-17T12:59:46.561252+00:00 |


## 独立复核与完整包

只读 `MAINLINE-VERIFY.py` SHA256 `c7a74829fc3ed07cb5214b8a8eeae71e15c98ac9c8f7f6bc697d6a2dec1041c1`：

```sh
python3 /tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/MAINLINE-VERIFY.py --out /tmp/ROOT_NEW_PRIVATE_MAINLINE_VERIFY
```

只写新的0700输出，0模型0DML；核固定archive/owned字节/ELF、四真实进程/当前state/typed43cols/ACK、当前CLI/Secret隔离/quiet，以及最终8事实、当前源gold、原QID WAL。作者原始exit0及输出目录 /tmp/oo-mysql-mainline-independent-verifier-author-v1 已留。根证据 final-runtime-observation-v1/runtime.json、final-physical-quiet-v2/proof.json、final-usage-v2/usage.json 和四 dialogue verdict。完整不可变私有包、manifest SHA/actual exit/资源最终清单见 `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/DELIVERY.json`（与本报告分别固定，避免自引用哈希）。

归档生成物补充：generated-cursor-provenance-v1.json 声明唯一非Git的219字节测试cursor；固定单测新空cwd实跑exit0、逐字节同SHA，非编译/产品输入。原archive SHA不改，完整包及独立补充共同固定在DELIVERY.json；不得将整个archive误述为全部Git成员。

最重要限制：g1源提交时间未知时严格freshness不ready；成功只绑定后续真实提交时间的最终cut，不代表任意客户/静态库首次接入签收。保留全部失败、unknown exit；55 paid responses/137055tokens，工程师总usage未知，正式源/parent计数不变。
