---
reported: 2026-02-01
severity: sev1
service: billing-api
reporter: Mara Lindqvist
---

# INC-40 billing-api 故障

## 影响

billing-api 在晚高峰出现超时，约 5% 的请求失败。

## 根因

连接池配置在上次发布后被重置。

## 处置

回滚配置并增加告警。
