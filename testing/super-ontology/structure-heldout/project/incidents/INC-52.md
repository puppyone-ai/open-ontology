---
reported: 2026-03-07
severity: sev1
service: billing-api
reporter: Ines Okafor
---

# INC-52 billing-api 故障

## 影响

billing-api 在晚高峰出现超时，约 17% 的请求失败。

## 根因

连接池配置在上次发布后被重置。

## 处置

回滚配置并增加告警。
