# medme-ehr · 多人电子健康档案服务

基于 [MedMe](https://github.com/Chesterguan/medme)（本地优先个人病历保险箱，Rust 内核）
二次开发的**服务端形态**：为家庭/团队存档多人的电子健康档案，通过 REST API
供 AI agent（SKILL/MCP）、Web、手机客户端查询与维护数据。

- 🧱 **数据真相层**：原件 sha256 CAS + 事件日志（append-only），派生索引可全量重建
- 🔎 **查询**：按人取时间线/报告/化验指标（跨报告趋势 + H/L 标记 + 证据回链），中文全文检索
- ✍️ **维护**：上传报告（txt/pdf）、建档；原件永存不失
- 🤖 **Agent 接入**：一个 SKILL（纯 markdown + curl）即可全功能，零注册、零守护进程
- 🐳 **部署**：docker compose 一键起，OpenAPI 自动生成

## 一键部署（免维护）

```bash
cd deploy
cp .env.sample .env            # 端口/镜像 tag 可改，不改也能跑
docker compose -f docker-compose.ghcr.yml up -d   # 引用 ghcr 镜像，免构建
# 或者从源码本地构建：
# docker compose -f docker-compose.local.yml up -d --build
curl -s http://127.0.0.1:8585/api/v1/healthz      # {"status":"ok",...}
```

数据落在 named volume `deploy_ehr-home`（重启不丢）。`docker compose ... down` 停止。

> 镜像 `ghcr.io/lbbit/medme_pro:<tag>` 由 CI 在打 `v*` tag 时自动构建推送
> （见 `.github/workflows/ci-image.yml`）。包当前为 private，自用需 `docker login ghcr.io`。

**自检（部署后必看）**：容器 stderr 有一行 `[证据] ehr-api v0.0.1 | mode=serve | EHR_HOME=/home/ehr | ADDR=0.0.0.0:8443`，
它与实际绑定地址同源；`docker compose -f <file> run --rm --no-deps ehr-api --print-openapi` 可离线自检契约。

## AI agent 接入（SKILL，免维护）

服务只暴露一套 REST API（OpenAPI 16 路径，base = `$EHR_API_BASE/api/v1`）。
**任何能执行 shell 的 agent（Copilot CLI / codex / QwenPaw / 手机端脚本）装上这个 skill 即得全功能**：

```bash
git clone https://github.com/lbbit/medme_pro.git
export EHR_API_BASE=http://127.0.0.1:8585      # API 地址走环境变量，升级/换机器不改文件
# token（服务端配置了 token 时必填；未配置=open 模式可留空）
export EHR_API_TOKEN=
export PATH="$PWD/medme_pro/skills/medme-ehr/scripts:$PATH"
ehr.sh persons
```

把 skill 放进 agent 的技能目录即可被发现：

| 宿主 | 放置位置 |
|---|---|
| Copilot CLI | `<项目>/.agents/skills/medme-ehr/`（或 `~/.copilot/skills/`、`copilot skill add <dir>`） |
| 其他 agent | 复制 `skills/medme-ehr/` 到其 skill 目录，或直接只用 `scripts/ehr.sh` |

- skill 目录：`skills/medme-ehr/`（`SKILL.md` + `references/rest-contracts.md` + `scripts/ehr.sh`）
- 拷贝到你的 agent skill 目录即用；**skill 内不写死地址与 token**
- 宿主若只吃 MCP，服务端另有 `POST /mcp`（JSON-RPC 2.0，读侧 6 + 写侧 2 工具），契约见
  `skills/medme-ehr/references/tool-contracts.md`

## 常用命令

```bash
ehr.sh persons                      # 人员列表（拿 person_id）
ehr.sh person-create 张三 --gender male --birthday 1990-01-02
ehr.sh upload <person_id> 化验单.txt
ehr.sh timeline <person_id>         # 时间线
ehr.sh search  <person_id> 白细胞    # 全文检索（片段带 [高亮]）
ehr.sh labs    <person_id>          # 指标趋势（H/L + docref 证据）
ehr.sh detail  <person_id> <sha256>  # 文档详情（OCR 文本层）
```

Web 管理端最小版随镜像提供：<http://127.0.0.1:8585/>（建档/时间线/上传/指标/检索）。
服务端配置了 token 时，页面右上角有 token 输入框（存 localStorage，或用 `?token=…` 打开）。

## API 鉴权（T4）

服务端用**静态 Bearer token**（env 注入），多 token 可按只读/读写分权：

```bash
# 部署侧（deploy/.env）：<token>[:ro|rw] 逗号分隔；或 <name>=<token>[:ro|rw]
EHR_API_TOKENS=mainrw:rw,agent:ro
# 客户端侧
export EHR_API_TOKEN=mainrw
```

- 未配置任何 token ⇒ **open 模式**（放行全部请求；启动 `[证据]` 行会标 `auth=open` 警示）。
- 401 = 缺/错 token；403 = 只读 token 试图写（读写操作需 rw token）。
- 例外（免鉴权）：`/api/v1/healthz`、`/openapi.json`、Web 首页。

## 派生索引重建（运维）

真相层 = CAS 原件 + 事件日志（append-only）；派生层 = 每人的 `medme.db`（SQLite/FTS 缓存）**可随时丢弃重建**。

```bash
# 先干跑：只列计划，不动任何文件
docker compose -f docker-compose.ghcr.yml run --rm --no-deps ehr-api --rebuild-index=dry-run
# 真重建：删派生库 → 从事件日志回放（全量重建），逐人打印 timeline/labs 计数
docker compose -f docker-compose.ghcr.yml run --rm --no-deps ehr-api --rebuild-index
# 只重建某人
docker compose -f docker-compose.ghcr.yml run --rm --no-deps ehr-api --rebuild-index=<person_id>
```

重建只碰派生层（`medme.db` + wal/shm），**不动真相层**（`log/`、`objects/`）；重建后 timeline/labs/检索计数应与重建前一致。

> ⚠️ `--rebuild-index` 需镜像含本命令：ghcr 上的 `v0.1.0` 镜像**尚不含**该 flag（CI 需打新 tag），
> 本地验证用 `docker compose -f deploy/docker-compose.local.yml build` 后的镜像，或直接跑仓库内的二进制。
> 重建是**离线动作**：重建期间本命令会持有跨进程互斥锁 `{EHR_HOME}/.rebuild.lock`，
> 此时**运行中的服务对业务路径一律返回 503**（`Retry-After: 5`），直到重建结束自动恢复；
> 并发发起第二个重建会被直接拒绝（报 `already held`）。若重建进程被 `kill -9`，
> 残留锁会被下一次重建/服务自动识别为陈旧并回收（不永久锁死）。

## 文档

| 路径 | 说明 |
|---|---|
| `PLAN.md` | 阶段计划与当前状态（开工先读） |
| `docs/DECISIONS.md` | 架构决策记录（ADR-lite，新决策在上） |
| `docs/API-DESIGN.md` | API 设计 |
| `docs/upstream-study.md` | MedMe 上游源码勘察报告 |
| `skills/medme-ehr/references/rest-contracts.md` | REST 字段契约（人类可读） |
| `skills/medme-ehr/references/tool-contracts.md` | MCP 工具契约 |
| `upstream-medme/` | 上游代码只读参考（不在版本库，版本由 `upstream.pin` 固定） |

## 状态

阶段 1~3 已交付（内核 POC / REST API 16 路径 / docker-compose + Web 最小版），
阶段 4（SKILL + MCP）已交付，阶段 5（token 鉴权 T4）已交付。详见 `PLAN.md`。

License: AGPL-3.0（跟随上游）。本项目不是医疗器械，不提供诊断结论本身。
