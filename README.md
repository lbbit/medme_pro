# medme-ehr · 多人电子健康档案服务

基于 [MedMe](https://github.com/Chesterguan/medme)（本地优先个人病历保险箱，Rust 内核）
二次开发的**服务端形态**：为家庭/团队存档多人的电子健康档案，通过 REST API
供 AI agent（SKILL/MCP）、Web、手机客户端查询与维护数据。

## 特性规划

- 👥 多人档案：人员注册档 + 每人的事件溯源健康档案（原件永存 sha256 CAS）
- 🔎 查询接口：按人取时间线/报告/化验指标/影像，中文全文检索
- ✍️ 维护接口：上传报告(自动 OCR+结构化)、手写录入条目、修订与审计追踪
- 🤖 Agent 接入：MCP server + QwenPaw SKILL，"给 XX 做个健康回顾"一句话完成
- 🐳 docker-compose 一键部署（api + web），OpenAPI 文档自动生成

## 目录

| 路径 | 说明 |
|---|---|
| `PLAN.md` | 阶段计划与当前状态（开工先读） |
| `docs/DECISIONS.md` | 架构决策记录 |
| `docs/upstream-study.md` | MedMe 上游源码勘察报告 |
| `upstream-medme/` | 上游代码只读参考 |
| `server/` `web/` | （待建）我们的实现 |

License: AGPL-3.0（跟随上游）。本项目不是医疗器械，不提供诊断结论本身。
