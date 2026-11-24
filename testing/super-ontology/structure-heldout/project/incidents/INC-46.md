---
reported: 2026-02-04
severity: sev1
service: auth
reporter: Tobias Wren
---

# INC-46 auth 故障

## 影响

auth 在晚高峰出现超时，约 11% 的请求失败。

## 根因

连接池配置在上次发布后被重置。

## 处置

回滚配置并增加告警。
