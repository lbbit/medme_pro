# medme-ehr 项目守则（长期项目）

## 项目目标

以 MedMe(https://github.com/Chesterguan/medme, AGPL-3.0)为底座，构建
**多人电子健康档案(EHR)后端服务**：

1. 存档多人的健康档案（原件 + 结构化数据）
2. 提供清晰的查询接口：人员列表/详情、指定人员的任何健康信息
3. 维护接口：AI agent(SKILL/MCP)、Web 客户端、手机客户端均可通过 API 维护数据
4. docker-compose 一键部署（后端 + Web）
5. 完整 OpenAPI 说明文件

## 目录约定

- `upstream-medme/` —— 上游仓库**浅克隆**(HEAD@9c9251d)，只读参考、不在其中开发。
  同步配方（按 DECISIONS D-006 的通道优先级）:
  `git -C upstream-medme fetch --depth 1 origin main && git -C upstream-medme reset --hard FETCH_HEAD`
  （直连 GitHub 慢时用 remote=`https://ghfast.top/https://github.com/Chesterguan/medme.git`）。
  推送/PR 前需把本机公钥绑到 GitHub（见 D-006），fetch 不需要。
- `server/` —— 我们的 EHR 后端（基于上游内核加 HTTP API 层）
- `web/` —— Web 管理端
- `docs/DECISIONS.md` —— 关键决策及理由（改设计必须先记在这里）
- `docs/API.md` —— API 面向人类文档（OpenAPI 之外的叙述性说明）
- `PLAN.md` —— 阶段计划与当前状态

## 工程纪律（继承全局 D1–D7，此处补充）

- **许可证红线**：AGPL-3.0。我们所有新增代码同样开源(AGPL)，且要与上游可合并；
  若引入 GPL/AGPL 不兼容依赖，先停下汇报。
- **不与上游打架**：能提 PR 回上游的通用改进就提；本地私有改动集中在自己的
  crate/目录里，少改上游文件，降低 rebase 成本。
- **单一事实来源**：档案的唯一真相 = MedMe 事件日志 + CAS；任何数据库(SQLite/PG)
  都只是可重建的派生索引。禁止出现"只有 DB 里才有的数据"（人物主数据除外，见 DECISIONS）。
- 每次会话开工前先读 `PLAN.md` 的「当前状态」段。
